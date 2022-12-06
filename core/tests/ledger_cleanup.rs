#![allow(clippy::integer_arithmetic)]
// Long-running ledger_cleanup tests

#[cfg(test)]
mod tests {
    use {
        crossbeam_channel::unbounded,
        log::*,
        solana_core::ledger_cleanup_service::LedgerCleanupService,
        solana_ledger::{
            blockstore::{make_many_slot_shreds, Blockstore},
            blockstore_options::{
                BlockstoreOptions, BlockstoreRocksFifoOptions, LedgerColumnOptions,
                ShredStorageType,
            },
            get_tmp_ledger_path,
        },
        solana_measure::measure::Measure,
        std::{
            collections::VecDeque,
            str::FromStr,
            sync::{
                atomic::{AtomicBool, AtomicU64, Ordering},
                Arc, Mutex, RwLock,
            },
            thread::{self, Builder, JoinHandle},
            time::{Duration, Instant},
        },
        systemstat::{CPULoad, Platform, System},
    };

    const DEFAULT_BENCHMARK_SLOTS: u64 = 50;
    const DEFAULT_BATCH_SIZE_SLOTS: u64 = 1;
    const DEFAULT_MAX_LEDGER_SHREDS: u64 = 50;
    const DEFAULT_SHREDS_PER_SLOT: u64 = 25;
    const DEFAULT_STOP_SIZE_BYTES: u64 = 0;
    const DEFAULT_STOP_SIZE_ITERATIONS: u64 = 0;
    const DEFAULT_STOP_SIZE_CF_DATA_BYTES: u64 = 0;
    const DEFAULT_SHRED_DATA_CF_SIZE_BYTES: u64 = 125 * 1024 * 1024 * 1024;

    #[derive(Debug)]
    struct BenchmarkConfig {
        benchmark_slots: u64,
        batch_size_slots: u64,
        max_ledger_shreds: u64,
        shreds_per_slot: u64,
        stop_size_bytes: u64,
        stop_size_iterations: u64,
        stop_size_cf_data_bytes: u64,
        pre_generate_data: bool,
        cleanup_blockstore: bool,
        num_writers: u64,
        cleanup_service: bool,
        fifo_compaction: bool,
        shred_data_cf_size: u64,
    }

    #[derive(Clone, Copy, Debug)]
    struct CpuStatsInner {
        cpu_user: f32,
        cpu_system: f32,
        cpu_idle: f32,
    }

    impl From<CPULoad> for CpuStatsInner {
        fn from(cpu: CPULoad) -> Self {
            Self {
                cpu_user: cpu.user * 100.0,
                cpu_system: cpu.system * 100.0,
                cpu_idle: cpu.idle * 100.0,
            }
        }
    }

    impl Default for CpuStatsInner {
        fn default() -> Self {
            Self {
                cpu_user: 0.0,
                cpu_system: 0.0,
                cpu_idle: 0.0,
            }
        }
    }

    struct CpuStats {
        stats: RwLock<CpuStatsInner>,
        sys: System,
    }

    impl Default for CpuStats {
        fn default() -> Self {
            Self {
                stats: RwLock::new(CpuStatsInner::default()),
                sys: System::new(),
            }
        }
    }

    impl CpuStats {
        fn update(&self) {
            if let Ok(cpu) = self.sys.cpu_load_aggregate() {
                std::thread::sleep(Duration::from_millis(400));
                let cpu_new = CpuStatsInner::from(cpu.done().unwrap());
                *self.stats.write().unwrap() = cpu_new;
            }
        }

        fn get_stats(&self) -> CpuStatsInner {
            *self.stats.read().unwrap()
        }
    }

    struct CpuStatsUpdater {
        cpu_stats: Arc<CpuStats>,
        t_cleanup: JoinHandle<()>,
    }

    impl CpuStatsUpdater {
        pub fn new(exit: &Arc<AtomicBool>) -> Self {
            let exit = exit.clone();
            let cpu_stats = Arc::new(CpuStats::default());
            let cpu_stats_clone = cpu_stats.clone();

            let t_cleanup = Builder::new()
                .name("cpu_info".to_string())
                .spawn(move || loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    cpu_stats_clone.update();
                })
                .unwrap();

            Self {
                cpu_stats,
                t_cleanup,
            }
        }

        pub fn get_stats(&self) -> CpuStatsInner {
            self.cpu_stats.get_stats()
        }

        pub fn join(self) -> std::thread::Result<()> {
            self.t_cleanup.join()
        }
    }

    fn read_env<T>(key: &str, default: T) -> T
    where
        T: FromStr,
    {
        match std::env::var(key) {
            Ok(val) => val.parse().unwrap_or(default),
            Err(_e) => default,
        }
    }

    /// Obtains the benchmark config from the following environmental arguments:
    ///
    /// Basic benchmark settings:
    /// - `BENCHMARK_SLOTS`: the number of slots in the benchmark.
    /// - `BATCH_SIZE`: the number of slots in each write batch.
    /// - `SHREDS_PER_SLOT`: the number of shreds in each slot.  Together with
    ///   the `BATCH_SIZE` and `BENCHMARK_SLOTS`, it means:
    ///    - the number of shreds in one write batch is `BATCH_SIZE` * `SHREDS_PER_SLOT`.
    ///    - the total number of batches is `BENCHMARK_SLOTS` / `BATCH_SIZE`.
    ///    - the total number of shreds is `BENCHMARK_SLOTS` * `SHREDS_PER_SLOT`.
    /// - `NUM_WRITERS`: controls the number of concurrent threads performing
    ///   shred insertion.  Default: 1.
    ///
    /// Advanced benchmark settings:
    /// - `STOP_SIZE_BYTES`: if specified, the benchmark will count how
    ///   many times the ledger store size exceeds the specified threshold.
    /// - `STOP_SIZE_CF_DATA_BYTES`: if specified, the benchmark will count how
    ///   many times the storage size of `cf::ShredData` which stores data shred
    ///   exceeds the specified threshold.
    /// - `STOP_SIZE_ITERATIONS`: when any of the stop size is specified, the
    ///   benchmark will stop immediately when the number of consecutive times
    ///   where the ledger store size exceeds the configured `STOP_SIZE_BYTES`.
    ///   These configs are used to make sure the benchmark runs successfully
    ///   under the storage limitation.
    /// - `CLEANUP_BLOCKSTORE`: if true, the ledger store created in the current
    ///   benchmark run will be deleted.  Default: true.
    ///
    /// Cleanup-service related settings:
    /// - `MAX_LEDGER_SHREDS`: when the clean-up service is on, the service will
    ///   clean up the ledger store when the number of shreds exceeds this value.
    /// - `CLEANUP_SERVICE`: whether to enable the background cleanup service.
    ///   If set to false, the ledger store in the benchmark will be purely relied
    ///   on RocksDB's compaction.  Default: true.
    ///
    /// Fifo-compaction settings:
    /// - `FIFO_COMPACTION`: if true, then RocksDB's Fifo compaction will be
    ///   used for storing data shreds.  Default: false.
    /// - `SHRED_DATA_CF_SIZE_BYTES`: the maximum size of the data-shred column family.
    ///   Default: 125 * 1024 * 1024 * 1024.
    fn get_benchmark_config() -> BenchmarkConfig {
        let benchmark_slots = read_env("BENCHMARK_SLOTS", DEFAULT_BENCHMARK_SLOTS);
        let batch_size_slots = read_env("BATCH_SIZE", DEFAULT_BATCH_SIZE_SLOTS);
        let max_ledger_shreds = read_env("MAX_LEDGER_SHREDS", DEFAULT_MAX_LEDGER_SHREDS);
        let shreds_per_slot = read_env("SHREDS_PER_SLOT", DEFAULT_SHREDS_PER_SLOT);
        let stop_size_bytes = read_env("STOP_SIZE_BYTES", DEFAULT_STOP_SIZE_BYTES);
        let stop_size_iterations = read_env("STOP_SIZE_ITERATIONS", DEFAULT_STOP_SIZE_ITERATIONS);
        let stop_size_cf_data_bytes =
            read_env("STOP_SIZE_CF_DATA_BYTES", DEFAULT_STOP_SIZE_CF_DATA_BYTES);
        let pre_generate_data = read_env("PRE_GENERATE_DATA", false);
        let cleanup_blockstore = read_env("CLEANUP_BLOCKSTORE", true);
        let num_writers = read_env("NUM_WRITERS", 1);
        // A flag indicating whether to have a background clean-up service.
        // If set to false, the ledger store will purely rely on RocksDB's
        // compaction to perform the clean-up.
        let cleanup_service = read_env("CLEANUP_SERVICE", true);
        let fifo_compaction = read_env("FIFO_COMPACTION", false);
        let shred_data_cf_size =
            read_env("SHRED_DATA_CF_SIZE_BYTES", DEFAULT_SHRED_DATA_CF_SIZE_BYTES);

        BenchmarkConfig {
            benchmark_slots,
            batch_size_slots,
            max_ledger_shreds,
            shreds_per_slot,
            stop_size_bytes,
            stop_size_iterations,
            stop_size_cf_data_bytes,
            pre_generate_data,
            cleanup_blockstore,
            num_writers,
            cleanup_service,
            fifo_compaction,
            shred_data_cf_size,
        }
    }

    fn emit_header() {
        println!("TIME_MS,DELTA_MS,START_SLOT,BATCH_SIZE,SHREDS,MAX,SIZE,DELTA_SIZE,DATA_SHRED_SIZE,DATA_SHRED_SIZE_DELTA,CPU_USER,CPU_SYSTEM,CPU_IDLE");
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_stats(
        time_initial: Instant,
        time_previous: &mut Instant,
        storage_previous: &mut u64,
        data_shred_storage_previous: &mut u64,
        start_slot: u64,
        batch_size: u64,
        num_shreds: u64,
        max_shreds: i64,
        blockstore: &Blockstore,
        cpu: &CpuStatsInner,
    ) {
        let time_now = Instant::now();
        let storage_now = blockstore.storage_size().unwrap_or(0);
        let data_shred_storage_now = blockstore.total_data_shred_storage_size().unwrap();
        let (cpu_user, cpu_system, cpu_idle) = (cpu.cpu_user, cpu.cpu_system, cpu.cpu_idle);

        info!(
            "{},{},{},{},{},{},{},{},{},{},{:.2},{:.2},{:.2}",
            time_now.duration_since(time_initial).as_millis(),
            time_now.duration_since(*time_previous).as_millis(),
            start_slot,
            batch_size,
            num_shreds,
            max_shreds,
            storage_now,
            storage_now as i64 - *storage_previous as i64,
            data_shred_storage_now,
            data_shred_storage_now - *data_shred_storage_previous as i64,
            cpu_user,
            cpu_system,
            cpu_idle,
        );

        *time_previous = time_now;
        *storage_previous = storage_now;
        *data_shred_storage_previous = data_shred_storage_now.try_into().unwrap();
    }

    /// Helper function of the benchmark `test_ledger_cleanup_compaction` which
    /// returns true if the benchmark fails the size limitation check.
    fn is_exceeded_stop_size_iterations(
        storage_size: u64,
        stop_size: u64,
        exceeded_iterations: &mut u64,
        iteration_limit: u64,
        storage_desc: &str,
    ) -> bool {
        if stop_size > 0 {
            if storage_size >= stop_size {
                *exceeded_iterations += 1;
                warn!(
                    "{} size {} exceeds the stop size {} for {} times!",
                    storage_desc, storage_size, stop_size, exceeded_iterations
                );
            } else {
                *exceeded_iterations = 0;
            }

            if *exceeded_iterations >= iteration_limit {
                error!(
                    "{} size exceeds the configured limit {} for {} times",
                    storage_desc, stop_size, exceeded_iterations,
                );
                return true;
            }
        }
        false
    }

    /// The ledger cleanup  test which can also be used as a benchmark
    /// measuring shred insertion performance of the blockstore.
    ///
    /// The benchmark is controlled by several environmental arguments.
    /// Check [`get_benchmark_config`] for the full list of arguments.
    ///
    /// Example command:
    /// BENCHMARK_SLOTS=1000000 BATCH_SIZE=1 SHREDS_PER_SLOT=25 NUM_WRITERS=8 \
    /// PRE_GENERATE_DATA=false cargo test --release tests::test_ledger_cleanup \
    /// -- --exact --nocapture
    #[test]
    fn test_ledger_cleanup() {
        solana_logger::setup_with("error,ledger_cleanup::tests=info");

        let ledger_path = get_tmp_ledger_path!();
        let config = get_benchmark_config();
        let blockstore = Blockstore::open_with_options(
            &ledger_path,
            if config.fifo_compaction {
                BlockstoreOptions {
                    column_options: LedgerColumnOptions {
                        shred_storage_type: ShredStorageType::RocksFifo(
                            BlockstoreRocksFifoOptions {
                                shred_data_cf_size: config.shred_data_cf_size,
                                shred_code_cf_size: config.shred_data_cf_size,
                            },
                        ),
                        ..LedgerColumnOptions::default()
                    },
                    ..BlockstoreOptions::default()
                }
            } else {
                BlockstoreOptions::default()
            },
        )
        .unwrap();
        let blockstore = Arc::new(blockstore);

        info!("Benchmark configuration: {:#?}", config);
        info!("Ledger path: {:?}", &ledger_path);

        let benchmark_slots = config.benchmark_slots;
        let batch_size_slots = config.batch_size_slots;
        let max_ledger_shreds = config.max_ledger_shreds;
        let shreds_per_slot = config.shreds_per_slot;
        let stop_size_bytes = config.stop_size_bytes;
        let stop_size_iterations = config.stop_size_iterations;
        let stop_size_cf_data_bytes = config.stop_size_cf_data_bytes;
        let pre_generate_data = config.pre_generate_data;
        let num_writers = config.num_writers;
        let cleanup_service = config.cleanup_service;

        let num_batches = benchmark_slots / batch_size_slots;
        let num_shreds_total = benchmark_slots * shreds_per_slot;

        let (sender, receiver) = unbounded();
        let exit = Arc::new(AtomicBool::new(false));

        let cleaner = if cleanup_service {
            Some(LedgerCleanupService::new(
                receiver,
                blockstore.clone(),
                max_ledger_shreds,
                &exit,
            ))
        } else {
            None
        };

        let exit_cpu = Arc::new(AtomicBool::new(false));
        let sys = CpuStatsUpdater::new(&exit_cpu);

        let mut shreds = VecDeque::new();

        if pre_generate_data {
            let mut pre_generate_data_timer = Measure::start("Pre-generate data");
            info!("Pre-generate data ... this may take a while");
            for i in 0..num_batches {
                let start_slot = i * batch_size_slots;
                let (new_shreds, _) =
                    make_many_slot_shreds(start_slot, batch_size_slots, shreds_per_slot);
                shreds.push_back(new_shreds);
            }
            pre_generate_data_timer.stop();
            info!("{}", pre_generate_data_timer);
        }
        let shreds = Arc::new(Mutex::new(shreds));

        info!(
            "Bench info num_batches: {}, batch size (slots): {}, shreds_per_slot: {}, num_shreds_total: {}",
            num_batches,
            batch_size_slots,
            shreds_per_slot,
            num_shreds_total
        );

        let time_initial = Instant::now();
        let mut time_previous = time_initial;
        let mut storage_previous = 0;
        let mut data_shred_storage_previous = 0;
        let mut stop_size_bytes_exceeded_iterations = 0;
        let mut stop_size_cf_data_exceeded_iterations = 0;

        emit_header();
        emit_stats(
            time_initial,
            &mut time_previous,
            &mut storage_previous,
            &mut data_shred_storage_previous,
            0,
            0,
            0,
            0,
            &blockstore,
            &sys.get_stats(),
        );

        let mut insert_threads = vec![];
        let insert_exit = Arc::new(AtomicBool::new(false));

        info!("Begin inserting shreds ...");
        let mut insert_timer = Measure::start("Shred insertion");
        let current_batch_id = Arc::new(AtomicU64::new(0));
        let finished_batch_count = Arc::new(AtomicU64::new(0));

        for i in 0..num_writers {
            let cloned_insert_exit = insert_exit.clone();
            let cloned_blockstore = blockstore.clone();
            let cloned_shreds = shreds.clone();
            let shared_batch_id = current_batch_id.clone();
            let shared_finished_count = finished_batch_count.clone();
            let insert_thread = Builder::new()
                .name(format!("insert_shreds-{i}"))
                .spawn(move || {
                    let start = Instant::now();
                    let mut now = Instant::now();
                    let mut total = 0;
                    let mut total_batches = 0;
                    let mut total_inserted_shreds = 0;
                    let mut num_shreds = 0;
                    let mut max_speed = 0f32;
                    let mut min_speed = f32::MAX;
                    let (first_shreds, _) = make_many_slot_shreds(
                        0, batch_size_slots, shreds_per_slot);
                    loop {
                        let batch_id = shared_batch_id.fetch_add(1, Ordering::Relaxed);
                        let start_slot = batch_id * batch_size_slots;
                        if start_slot >= benchmark_slots {
                            break;
                        }
                        let len = batch_id;

                        // No duplicates being generated, so all shreds
                        // being passed to insert() are getting inserted
                        let num_shred_inserted = if pre_generate_data {
                            let mut sl = cloned_shreds.lock().unwrap();
                            if let Some(shreds_from_queue) = sl.pop_front() {
                                let num_shreds = shreds_from_queue.len();
                                total += num_shreds;
                                cloned_blockstore.insert_shreds(
                                    shreds_from_queue, None, false).unwrap();
                                num_shreds
                            } else {
                                // If the queue is empty, we're done!
                                break;
                            }
                        } else {
                            let slot_id = start_slot;
                            if slot_id > 0 {
                                let (shreds_with_parent, _) = make_many_slot_shreds(
                                    slot_id, batch_size_slots, shreds_per_slot);
                                let num_shreds = shreds_with_parent.len();
                                total += num_shreds;
                                cloned_blockstore.insert_shreds(
                                    shreds_with_parent.clone(), None, false).unwrap();
                                num_shreds
                            } else {
                                let num_shreds = first_shreds.len();
                                total += num_shreds;
                                cloned_blockstore.insert_shreds(
                                    first_shreds.clone(), None, false).unwrap();
                                num_shreds
                            }
                        };

                        total_batches += 1;
                        total_inserted_shreds += num_shred_inserted;
                        num_shreds += num_shred_inserted;
                        shared_finished_count.fetch_add(1, Ordering::Relaxed);

                        // as_secs() returns whole number of seconds, so this runs every second
                        if now.elapsed().as_secs() > 0 {
                            let shreds_per_second = num_shreds as f32 / now.elapsed().as_secs() as f32;
                            warn!(
                                "insert-{} tried: {} inserted: {} batches: {} len: {} shreds_per_second: {}",
                                i, total, total_inserted_shreds, total_batches, len, shreds_per_second,
                            );
                            let average_speed =
                                total_inserted_shreds as f32 / start.elapsed().as_secs() as f32;
                            max_speed = max_speed.max(shreds_per_second);
                            min_speed = min_speed.min(shreds_per_second);
                            warn!(
                                "highest: {} lowest: {} avg: {}",
                                max_speed, min_speed, average_speed
                            );
                            now = Instant::now();
                            num_shreds = 0;
                        }

                        if cloned_insert_exit.load(Ordering::Relaxed) {
                            if max_speed > 0.0 {
                                info!(
                                    "insert-{} exiting highest shreds/s: {}, lowest shreds/s: {}",
                                    i, max_speed, min_speed
                                );
                            } else {
                                // Not enough time elapsed to sample
                                info!(
                                    "insert-{} exiting",
                                    i
                                );
                            }
                            break;
                        }
                    }
                })
                .unwrap();
            insert_threads.push(insert_thread);
        }

        loop {
            let finished_batch = finished_batch_count.load(Ordering::Relaxed);
            let finished_slot = (finished_batch + 1) * batch_size_slots - 1;

            if cleanup_service {
                sender.send(finished_slot).unwrap();
            }

            emit_stats(
                time_initial,
                &mut time_previous,
                &mut storage_previous,
                &mut data_shred_storage_previous,
                finished_slot,
                batch_size_slots,
                shreds_per_slot,
                max_ledger_shreds as i64,
                &blockstore,
                &sys.get_stats(),
            );

            if is_exceeded_stop_size_iterations(
                storage_previous,
                stop_size_bytes,
                &mut stop_size_bytes_exceeded_iterations,
                stop_size_iterations,
                "Storage",
            ) {
                break;
            }

            if is_exceeded_stop_size_iterations(
                data_shred_storage_previous,
                stop_size_cf_data_bytes,
                &mut stop_size_cf_data_exceeded_iterations,
                stop_size_iterations,
                "cf::ShredData",
            ) {
                break;
            }

            if finished_batch >= num_batches {
                break;
            } else {
                thread::sleep(Duration::from_millis(500));
            }
        }
        // Send exit signal to stop all the writer threads.
        insert_exit.store(true, Ordering::Relaxed);

        while let Some(thread) = insert_threads.pop() {
            thread.join().unwrap();
        }
        insert_timer.stop();

        info!(
            "Done inserting shreds: {}, {} shreds/s",
            insert_timer,
            num_shreds_total as f32 / insert_timer.as_s(),
        );

        exit.store(true, Ordering::SeqCst);
        if cleanup_service {
            cleaner.unwrap().join().unwrap();
        }

        exit_cpu.store(true, Ordering::SeqCst);
        sys.join().unwrap();

        if config.cleanup_blockstore {
            drop(blockstore);
            Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
        }
    }
}
