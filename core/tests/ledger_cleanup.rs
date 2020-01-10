// Long-running ledger_cleanup tests

#[cfg(test)]
mod tests {
    use solana_core::ledger_cleanup_service::LedgerCleanupService;
    use solana_ledger::blockstore::{make_many_slot_entries, Blockstore};
    use solana_ledger::get_tmp_ledger_path;
    use solana_ledger::shred::Shred;
    use std::collections::VecDeque;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::{Builder, JoinHandle};
    use std::time::{Duration, Instant};
    use systemstat::{CPULoad, Platform, System};

    const DEFAULT_BENCHMARK_SLOTS: u64 = 50;
    const DEFAULT_BATCH_SIZE: u64 = 1;
    const DEFAULT_MAX_LEDGER_SLOTS: u64 = 50;
    const DEFAULT_ENTRIES_PER_SLOT: u64 = 500;
    const DEFAULT_STOP_SIZE_BYTES: u64 = 0;
    const DEFAULT_STOP_SIZE_ITERATIONS: u64 = 0;

    const ROCKSDB_FLUSH_GRACE_PERIOD_SECS: u64 = 20;

    #[derive(Debug)]
    struct BenchmarkConfig {
        pub benchmark_slots: u64,
        pub batch_size: u64,
        pub max_ledger_slots: u64,
        pub entries_per_slot: u64,
        pub stop_size_bytes: u64,
        pub stop_size_iterations: u64,
        pub pre_generate_data: bool,
        pub cleanup_blockstore: bool,
        pub emit_cpu_info: bool,
        pub assert_compaction: bool,
    }

    #[derive(Clone, Copy, Debug)]
    struct CpuStatsInner {
        pub cpu_user: f32,
        pub cpu_system: f32,
        pub cpu_idle: f32,
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
            match self.sys.cpu_load_aggregate() {
                Ok(cpu) => {
                    std::thread::sleep(Duration::from_millis(400));
                    let cpu_new = CpuStatsInner::from(cpu.done().unwrap());
                    *self.stats.write().unwrap() = cpu_new;
                }
                _ => (),
            }
        }

        fn get_stats(&self) -> CpuStatsInner {
            self.stats.read().unwrap().clone()
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
                cpu_stats: cpu_stats.clone(),
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

    fn get_benchmark_config() -> BenchmarkConfig {
        let benchmark_slots = read_env("BENCHMARK_SLOTS", DEFAULT_BENCHMARK_SLOTS);
        let batch_size = read_env("BATCH_SIZE", DEFAULT_BATCH_SIZE);
        let max_ledger_slots = read_env("MAX_LEDGER_SLOTS", DEFAULT_MAX_LEDGER_SLOTS);
        let entries_per_slot = read_env("ENTRIES_PER_SLOT", DEFAULT_ENTRIES_PER_SLOT);
        let stop_size_bytes = read_env("STOP_SIZE_BYTES", DEFAULT_STOP_SIZE_BYTES);
        let stop_size_iterations = read_env("STOP_SIZE_ITERATIONS", DEFAULT_STOP_SIZE_ITERATIONS);
        let pre_generate_data = read_env("PRE_GENERATE_DATA", false);
        let cleanup_blockstore = read_env("CLEANUP_BLOCKSTORE", true);
        let emit_cpu_info = read_env("EMIT_CPU_INFO", true);
        // set default to `true` once compaction is merged
        let assert_compaction = read_env("ASSERT_COMPACTION", false);

        BenchmarkConfig {
            benchmark_slots,
            batch_size,
            max_ledger_slots,
            entries_per_slot,
            stop_size_bytes,
            stop_size_iterations,
            pre_generate_data,
            cleanup_blockstore,
            emit_cpu_info,
            assert_compaction,
        }
    }

    fn emit_header() {
        println!("TIME_MS,DELTA_MS,START_SLOT,BATCH_SIZE,ENTRIES,MAX,SIZE,DELTA_SIZE,CPU_USER,CPU_SYSTEM,CPU_IDLE");
    }

    fn emit_stats(
        time_initial: &Instant,
        time_previous: &mut Instant,
        storage_previous: &mut u64,
        start_slot: u64,
        batch_size: u64,
        entries: u64,
        max_slots: i64,
        blockstore: &Blockstore,
        cpu: &CpuStatsInner,
    ) {
        let time_now = Instant::now();
        let storage_now = blockstore.storage_size().unwrap_or(0);
        let (cpu_user, cpu_system, cpu_idle) = (cpu.cpu_user, cpu.cpu_system, cpu.cpu_idle);

        println!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            time_now.duration_since(*time_initial).as_millis(),
            time_now.duration_since(*time_previous).as_millis(),
            start_slot,
            batch_size,
            entries,
            max_slots,
            storage_now,
            storage_now as i64 - *storage_previous as i64,
            cpu_user,
            cpu_system,
            cpu_idle,
        );

        *time_previous = time_now;
        *storage_previous = storage_now;
    }

    #[test]
    fn test_ledger_cleanup_compaction() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&blockstore_path).unwrap());
        let config = get_benchmark_config();
        eprintln!("BENCHMARK CONFIG: {:?}", config);
        eprintln!("LEDGER_PATH: {:?}", &blockstore_path);

        let benchmark_slots = config.benchmark_slots;
        let batch_size = config.batch_size;
        let max_ledger_slots = config.max_ledger_slots;
        let entries_per_slot = config.entries_per_slot;
        let stop_size_bytes = config.stop_size_bytes;
        let stop_size_iterations = config.stop_size_iterations;
        let pre_generate_data = config.pre_generate_data;
        let batches = benchmark_slots / batch_size;

        let (sender, receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let cleaner =
            LedgerCleanupService::new(receiver, blockstore.clone(), max_ledger_slots, &exit);

        let exit_cpu = Arc::new(AtomicBool::new(false));
        let sys = CpuStatsUpdater::new(&exit_cpu);

        let mut generated_batches = VecDeque::<Vec<Shred>>::new();

        if pre_generate_data {
            let t0 = Instant::now();
            eprintln!("PRE_GENERATE_DATA: (this may take a while)");
            for i in 0..batches {
                let x = i * batch_size;
                let (shreds, _) = make_many_slot_entries(x, batch_size, entries_per_slot);
                generated_batches.push_back(shreds);
            }
            eprintln!("PRE_GENERATE_DATA: took {} ms", t0.elapsed().as_millis());
        };

        let time_initial = Instant::now();
        let mut time_previous = time_initial;
        let mut storage_previous = 0;
        let mut stop_size_bytes_exceeded_iterations = 0;

        emit_header();
        emit_stats(
            &time_initial,
            &mut time_previous,
            &mut storage_previous,
            0,
            0,
            0,
            0,
            &blockstore,
            &sys.get_stats(),
        );

        for i in 0..batches {
            let x = i * batch_size;

            let shreds = if pre_generate_data {
                generated_batches.pop_front().unwrap()
            } else {
                make_many_slot_entries(x, batch_size, entries_per_slot).0
            };

            blockstore.insert_shreds(shreds, None, false).unwrap();
            sender.send(x).unwrap();

            emit_stats(
                &time_initial,
                &mut time_previous,
                &mut storage_previous,
                x,
                batch_size,
                batch_size,
                max_ledger_slots as i64,
                &blockstore,
                &sys.get_stats(),
            );

            if stop_size_bytes > 0 {
                if storage_previous >= stop_size_bytes {
                    stop_size_bytes_exceeded_iterations += 1;
                } else {
                    stop_size_bytes_exceeded_iterations = 0;
                }

                if stop_size_bytes_exceeded_iterations > stop_size_iterations {
                    break;
                }
            }
        }

        let u1 = storage_previous;

        // send final `ledger_cleanup` notification (since iterations above are zero-based)
        sender.send(benchmark_slots).unwrap();

        emit_stats(
            &time_initial,
            &mut time_previous,
            &mut storage_previous,
            benchmark_slots,
            0,
            0,
            max_ledger_slots as i64,
            &blockstore,
            &sys.get_stats(),
        );

        // Poll on some compaction happening
        let start_poll = Instant::now();
        while blockstore.storage_size().unwrap_or(0) >= u1 {
            if start_poll.elapsed().as_secs() > ROCKSDB_FLUSH_GRACE_PERIOD_SECS {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        emit_stats(
            &time_initial,
            &mut time_previous,
            &mut storage_previous,
            benchmark_slots,
            0,
            0,
            max_ledger_slots as i64,
            &blockstore,
            &sys.get_stats(),
        );

        let u2 = storage_previous;

        exit.store(true, Ordering::SeqCst);
        cleaner.join().unwrap();

        exit_cpu.store(true, Ordering::SeqCst);
        sys.join().unwrap();

        if config.assert_compaction {
            assert!(u2 < u1, "expected compaction! pre={},post={}", u1, u2);
        }

        if config.cleanup_blockstore {
            drop(blockstore);
            Blockstore::destroy(&blockstore_path)
                .expect("Expected successful database destruction");
        }
    }
}
