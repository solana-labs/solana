use {
    crate::blockstore_options::LedgerColumnOptions,
    rocksdb::{
        perf::{set_perf_stats, PerfMetric, PerfStatsLevel},
        PerfContext,
    },
    solana_metrics::datapoint_info,
    solana_sdk::timing::timestamp,
    std::{
        cell::RefCell,
        fmt::Debug,
        sync::atomic::{AtomicU64, AtomicUsize, Ordering},
        time::{Duration, Instant},
    },
};

#[derive(Default)]
pub struct BlockstoreInsertionMetrics {
    pub insert_lock_elapsed_us: u64,
    pub insert_shreds_elapsed_us: u64,
    pub shred_recovery_elapsed_us: u64,
    pub chaining_elapsed_us: u64,
    pub commit_working_sets_elapsed_us: u64,
    pub write_batch_elapsed_us: u64,
    pub total_elapsed_us: u64,
    pub index_meta_time_us: u64,
    pub num_shreds: usize,
    pub num_inserted: u64,
    pub num_repair: u64,
    pub num_recovered: usize,
    pub num_recovered_blockstore_error: usize,
    pub num_recovered_inserted: usize,
    pub num_recovered_failed_sig: usize,
    pub num_recovered_failed_invalid: usize,
    pub num_recovered_exists: usize,
    pub num_repaired_data_shreds_exists: usize,
    pub num_turbine_data_shreds_exists: usize,
    pub num_data_shreds_invalid: usize,
    pub num_data_shreds_blockstore_error: usize,
    pub num_coding_shreds_exists: usize,
    pub num_coding_shreds_invalid: usize,
    pub num_coding_shreds_invalid_erasure_config: usize,
    pub num_coding_shreds_inserted: usize,
}

impl BlockstoreInsertionMetrics {
    pub fn report_metrics(&self, metric_name: &'static str) {
        datapoint_info!(
            metric_name,
            ("num_shreds", self.num_shreds as i64, i64),
            ("total_elapsed_us", self.total_elapsed_us as i64, i64),
            (
                "insert_lock_elapsed_us",
                self.insert_lock_elapsed_us as i64,
                i64
            ),
            (
                "insert_shreds_elapsed_us",
                self.insert_shreds_elapsed_us as i64,
                i64
            ),
            (
                "shred_recovery_elapsed_us",
                self.shred_recovery_elapsed_us as i64,
                i64
            ),
            ("chaining_elapsed_us", self.chaining_elapsed_us as i64, i64),
            (
                "commit_working_sets_elapsed_us",
                self.commit_working_sets_elapsed_us as i64,
                i64
            ),
            (
                "write_batch_elapsed_us",
                self.write_batch_elapsed_us as i64,
                i64
            ),
            ("num_inserted", self.num_inserted as i64, i64),
            ("num_repair", self.num_repair as i64, i64),
            ("num_recovered", self.num_recovered as i64, i64),
            (
                "num_recovered_inserted",
                self.num_recovered_inserted as i64,
                i64
            ),
            (
                "num_recovered_failed_sig",
                self.num_recovered_failed_sig as i64,
                i64
            ),
            (
                "num_recovered_failed_invalid",
                self.num_recovered_failed_invalid as i64,
                i64
            ),
            (
                "num_recovered_exists",
                self.num_recovered_exists as i64,
                i64
            ),
            (
                "num_recovered_blockstore_error",
                self.num_recovered_blockstore_error,
                i64
            ),
            (
                "num_repaired_data_shreds_exists",
                self.num_repaired_data_shreds_exists,
                i64
            ),
            (
                "num_turbine_data_shreds_exists",
                self.num_turbine_data_shreds_exists,
                i64
            ),
            ("num_data_shreds_invalid", self.num_data_shreds_invalid, i64),
            (
                "num_data_shreds_blockstore_error",
                self.num_data_shreds_blockstore_error,
                i64
            ),
            (
                "num_coding_shreds_exists",
                self.num_coding_shreds_exists,
                i64
            ),
            (
                "num_coding_shreds_invalid",
                self.num_coding_shreds_invalid,
                i64
            ),
            (
                "num_coding_shreds_invalid_erasure_config",
                self.num_coding_shreds_invalid_erasure_config,
                i64
            ),
            (
                "num_coding_shreds_inserted",
                self.num_coding_shreds_inserted,
                i64
            ),
        );
    }
}

/// A metrics struct that exposes RocksDB's column family properties.
///
/// Here we only expose a subset of all the internal properties which are
/// relevant to the ledger store performance.
///
/// The list of completed RocksDB internal properties can be found
/// [here](https://github.com/facebook/rocksdb/blob/08809f5e6cd9cc4bc3958dd4d59457ae78c76660/include/rocksdb/db.h#L654-L689).
#[derive(Default)]
pub struct BlockstoreRocksDbColumnFamilyMetrics {
    // Size related

    // The storage size occupied by the column family.
    // RocksDB's internal property key: "rocksdb.total-sst-files-size"
    pub total_sst_files_size: i64,
    // The memory size occupied by the column family's in-memory buffer.
    // RocksDB's internal property key: "rocksdb.size-all-mem-tables"
    pub size_all_mem_tables: i64,

    // Snapshot related

    // Number of snapshots hold for the column family.
    // RocksDB's internal property key: "rocksdb.num-snapshots"
    pub num_snapshots: i64,
    // Unit timestamp of the oldest unreleased snapshot.
    // RocksDB's internal property key: "rocksdb.oldest-snapshot-time"
    pub oldest_snapshot_time: i64,

    // Write related

    // The current actual delayed write rate. 0 means no delay.
    // RocksDB's internal property key: "rocksdb.actual-delayed-write-rate"
    pub actual_delayed_write_rate: i64,
    // A flag indicating whether writes are stopped on this column family.
    // 1 indicates writes have been stopped.
    // RocksDB's internal property key: "rocksdb.is-write-stopped"
    pub is_write_stopped: i64,

    // Memory / block cache related

    // The block cache capacity of the column family.
    // RocksDB's internal property key: "rocksdb.block-cache-capacity"
    pub block_cache_capacity: i64,
    // The memory size used by the column family in the block cache.
    // RocksDB's internal property key: "rocksdb.block-cache-usage"
    pub block_cache_usage: i64,
    // The memory size used by the column family in the block cache where
    // entries are pinned.
    // RocksDB's internal property key: "rocksdb.block-cache-pinned-usage"
    pub block_cache_pinned_usage: i64,

    // The estimated memory size used for reading SST tables in this column
    // family such as filters and index blocks. Note that this number does not
    // include the memory used in block cache.
    // RocksDB's internal property key: "rocksdb.estimate-table-readers-mem"
    pub estimate_table_readers_mem: i64,

    // Flush and compaction

    // A 1 or 0 flag indicating whether a memtable flush is pending.
    // If this number is 1, it means a memtable is waiting for being flushed,
    // but there might be too many L0 files that prevents it from being flushed.
    // RocksDB's internal property key: "rocksdb.mem-table-flush-pending"
    pub mem_table_flush_pending: i64,

    // A 1 or 0 flag indicating whether a compaction job is pending.
    // If this number is 1, it means some part of the column family requires
    // compaction in order to maintain shape of LSM tree, but the compaction
    // is pending because the desired compaction job is either waiting for
    // other dependnent compactions to be finished or waiting for an available
    // compaction thread.
    // RocksDB's internal property key: "rocksdb.compaction-pending"
    pub compaction_pending: i64,

    // The number of compactions that are currently running for the column family.
    // RocksDB's internal property key: "rocksdb.num-running-compactions"
    pub num_running_compactions: i64,

    // The number of flushes that are currently running for the column family.
    // RocksDB's internal property key: "rocksdb.num-running-flushes"
    pub num_running_flushes: i64,

    // FIFO Compaction related

    // returns an estimation of the oldest key timestamp in the DB. Only vailable
    // for FIFO compaction with compaction_options_fifo.allow_compaction = false.
    // RocksDB's internal property key: "rocksdb.estimate-oldest-key-time"
    pub estimate_oldest_key_time: i64,

    // Misc

    // The accumulated number of RocksDB background errors.
    // RocksDB's internal property key: "rocksdb.background-errors"
    pub background_errors: i64,
}

impl BlockstoreRocksDbColumnFamilyMetrics {
    /// Report metrics with the specified metric name and column family tag.
    /// The metric name and the column family tag is embedded in the parameter
    /// `metric_name_and_cf_tag` with the following format.
    ///
    /// For example, "blockstore_rocksdb_cfs,cf_name=shred_data".
    pub fn report_metrics(&self, cf_name: &'static str, column_options: &LedgerColumnOptions) {
        datapoint_info!(
            "blockstore_rocksdb_cfs",
            // tags that support group-by operations
            "cf_name" => cf_name,
            "storage" => column_options.get_storage_type_string(),
            "compression" => column_options.get_compression_type_string(),
            // Size related
            (
                "total_sst_files_size",
                self.total_sst_files_size,
                i64
            ),
            ("size_all_mem_tables", self.size_all_mem_tables, i64),
            // Snapshot related
            ("num_snapshots", self.num_snapshots, i64),
            (
                "oldest_snapshot_time",
                self.oldest_snapshot_time,
                i64
            ),
            // Write related
            (
                "actual_delayed_write_rate",
                self.actual_delayed_write_rate,
                i64
            ),
            ("is_write_stopped", self.is_write_stopped, i64),
            // Memory / block cache related
            (
                "block_cache_capacity",
                self.block_cache_capacity,
                i64
            ),
            ("block_cache_usage", self.block_cache_usage, i64),
            (
                "block_cache_pinned_usage",
                self.block_cache_pinned_usage,
                i64
            ),
            (
                "estimate_table_readers_mem",
                self.estimate_table_readers_mem,
                i64
            ),
            // Flush and compaction
            (
                "mem_table_flush_pending",
                self.mem_table_flush_pending,
                i64
            ),
            ("compaction_pending", self.compaction_pending, i64),
            (
                "num_running_compactions",
                self.num_running_compactions,
                i64
            ),
            ("num_running_flushes", self.num_running_flushes, i64),
            // FIFO Compaction related
            (
                "estimate_oldest_key_time",
                self.estimate_oldest_key_time,
                i64
            ),
            // Misc
            ("background_errors", self.background_errors, i64),
        );
    }
}

// Thread local instance of RocksDB's PerfContext.
thread_local! {static PER_THREAD_ROCKS_PERF_CONTEXT: RefCell<PerfContext> = RefCell::new(PerfContext::default());}

// The minimum time duration between two RocksDB perf samples of the same operation.
const PERF_SAMPLING_MIN_DURATION: Duration = Duration::from_secs(1);
pub(crate) const PERF_METRIC_OP_NAME_GET: &str = "get";
pub(crate) const PERF_METRIC_OP_NAME_MULTI_GET: &str = "multi_get";
pub(crate) const PERF_METRIC_OP_NAME_PUT: &str = "put";
pub(crate) const PERF_METRIC_OP_NAME_WRITE_BATCH: &str = "write_batch";

/// The function enables RocksDB PerfContext once for every `sample_interval`.
///
/// PerfContext is a thread-local struct defined in RocksDB for collecting
/// per-thread read / write performance metrics.
///
/// When this function enables PerfContext, the function will return true,
/// and the PerfContext of the ubsequent RocksDB operation will be collected.
pub(crate) fn maybe_enable_rocksdb_perf(
    sample_interval: usize,
    perf_status: &PerfSamplingStatus,
) -> Option<Instant> {
    if perf_status.should_sample(sample_interval) {
        set_perf_stats(PerfStatsLevel::EnableTime);
        PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context| {
            perf_context.borrow_mut().reset();
        });
        return Some(Instant::now());
    }
    None
}

/// Reports the collected PerfContext and disables the PerfContext after
/// reporting.
pub(crate) fn report_rocksdb_read_perf(
    cf_name: &'static str,
    op_name: &'static str,
    total_op_duration: &Duration,
    column_options: &LedgerColumnOptions,
) {
    PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context_cell| {
        set_perf_stats(PerfStatsLevel::Disable);
        let perf_context = perf_context_cell.borrow();
        datapoint_info!(
            "blockstore_rocksdb_read_perf",
            // tags that support group-by operations
            "op" => op_name,
            "cf_name" => cf_name,
            "storage" => column_options.get_storage_type_string(),
            "compression" => column_options.get_compression_type_string(),
            // total nanos spent on the entire operation.
            ("total_op_nanos", total_op_duration.as_nanos() as i64, i64),
            (
                "user_key_comparison_count",
                perf_context.metric(PerfMetric::UserKeyComparisonCount) as i64,
                i64
            ),
            (
                "block_cache_hit_count",
                perf_context.metric(PerfMetric::BlockCacheHitCount) as i64,
                i64
            ),
            (
                "block_read_count",
                perf_context.metric(PerfMetric::BlockReadCount) as i64,
                i64
            ),
            (
                "block_read_byte",
                perf_context.metric(PerfMetric::BlockReadByte) as i64,
                i64
            ),
            (
                "block_read_nanos",
                perf_context.metric(PerfMetric::BlockReadTime) as i64,
                i64
            ),
            (
                "block_checksum_nanos",
                perf_context.metric(PerfMetric::BlockChecksumTime) as i64,
                i64
            ),
            (
                "block_decompress_nanos",
                perf_context.metric(PerfMetric::BlockDecompressTime) as i64,
                i64
            ),
            (
                "get_read_bytes",
                perf_context.metric(PerfMetric::GetReadBytes) as i64,
                i64
            ),
            (
                "multiget_read_bytes",
                perf_context.metric(PerfMetric::MultigetReadBytes) as i64,
                i64
            ),
            (
                "get_snapshot_nanos",
                perf_context.metric(PerfMetric::GetSnapshotTime) as i64,
                i64
            ),
            (
                "get_from_memtable_nanos",
                perf_context.metric(PerfMetric::GetFromMemtableTime) as i64,
                i64
            ),
            (
                "get_from_memtable_count",
                perf_context.metric(PerfMetric::GetFromMemtableCount) as i64,
                i64
            ),
            (
                // total nanos spent after Get() finds a key
                "get_post_process_nanos",
                perf_context.metric(PerfMetric::GetPostProcessTime) as i64,
                i64
            ),
            (
                // total nanos reading from output files
                "get_from_output_files_nanos",
                perf_context.metric(PerfMetric::GetFromOutputFilesTime) as i64,
                i64
            ),
            (
                // time spent on acquiring DB mutex
                "db_mutex_lock_nanos",
                perf_context.metric(PerfMetric::DbMutexLockNanos) as i64,
                i64
            ),
            (
                // time spent on waiting with a condition variable created with DB mutex.
                "db_condition_wait_nanos",
                perf_context.metric(PerfMetric::DbConditionWaitNanos) as i64,
                i64
            ),
            (
                "merge_operator_nanos",
                perf_context.metric(PerfMetric::MergeOperatorTimeNanos) as i64,
                i64
            ),
            (
                "read_index_block_nanos",
                perf_context.metric(PerfMetric::ReadIndexBlockNanos) as i64,
                i64
            ),
            (
                "read_filter_block_nanos",
                perf_context.metric(PerfMetric::ReadFilterBlockNanos) as i64,
                i64
            ),
            (
                "new_table_block_iter_nanos",
                perf_context.metric(PerfMetric::NewTableBlockIterNanos) as i64,
                i64
            ),
            (
                "block_seek_nanos",
                perf_context.metric(PerfMetric::BlockSeekNanos) as i64,
                i64
            ),
            (
                "find_table_nanos",
                perf_context.metric(PerfMetric::FindTableNanos) as i64,
                i64
            ),
            (
                "bloom_memtable_hit_count",
                perf_context.metric(PerfMetric::BloomMemtableHitCount) as i64,
                i64
            ),
            (
                "bloom_memtable_miss_count",
                perf_context.metric(PerfMetric::BloomMemtableMissCount) as i64,
                i64
            ),
            (
                "bloom_sst_hit_count",
                perf_context.metric(PerfMetric::BloomSstHitCount) as i64,
                i64
            ),
            (
                "bloom_sst_miss_count",
                perf_context.metric(PerfMetric::BloomSstMissCount) as i64,
                i64
            ),
            (
                "key_lock_wait_time",
                perf_context.metric(PerfMetric::KeyLockWaitTime) as i64,
                i64
            ),
            (
                "key_lock_wait_count",
                perf_context.metric(PerfMetric::KeyLockWaitCount) as i64,
                i64
            ),
            // nanos spent on file/directory operations.
            (
                "env_file_ops_nanos",
                (perf_context.metric(PerfMetric::EnvFileExistsNanos)
                    + perf_context.metric(PerfMetric::EnvGetChildrenNanos)
                    + perf_context.metric(PerfMetric::EnvLockFileNanos)
                    + perf_context.metric(PerfMetric::EnvUnlockFileNanos)) as i64,
                i64
            ),
        );
    });
}
/// Reports the collected PerfContext and disables the PerfContext after
/// reporting.
pub(crate) fn report_rocksdb_write_perf(
    cf_name: &'static str,
    op_name: &'static str,
    total_op_duration: &Duration,
    column_options: &LedgerColumnOptions,
) {
    PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context_cell| {
        set_perf_stats(PerfStatsLevel::Disable);
        let perf_context = perf_context_cell.borrow();
        datapoint_info!(
            "blockstore_rocksdb_write_perf",
            // tags that support group-by operations
            "op" => op_name,
            "cf_name" => cf_name,
            "storage" => column_options.get_storage_type_string(),
            "compression" => column_options.get_compression_type_string(),
            // total nanos spent on the entire operation.
            ("total_op_nanos", total_op_duration.as_nanos() as i64, i64),
            // total nanos spent on writing to WAL
            (
                "write_wal_nanos",
                perf_context.metric(PerfMetric::WriteWalTime) as i64,
                i64
            ),
            // total nanos spent on writing to mem tables
            (
                "write_memtable_nanos",
                perf_context.metric(PerfMetric::WriteMemtableTime) as i64,
                i64
            ),
            // total nanos spent on delaying or throttling write
            (
                "write_delay_nanos",
                perf_context.metric(PerfMetric::WriteDelayTime) as i64,
                i64
            ),
            // total nanos spent on writing a record, excluding the above four things
            (
                "write_pre_and_post_process_nanos",
                perf_context.metric(PerfMetric::WritePreAndPostProcessTime) as i64,
                i64
            ),
            // time spent on acquiring DB mutex.
            (
                "db_mutex_lock_nanos",
                perf_context.metric(PerfMetric::DbMutexLockNanos) as i64,
                i64
            ),
            // Time spent on waiting with a condition variable created with DB mutex.
            (
                "db_condition_wait_nanos",
                perf_context.metric(PerfMetric::DbConditionWaitNanos) as i64,
                i64
            ),
            // Time spent on merge operator.
            (
                "merge_operator_nanos_nanos",
                perf_context.metric(PerfMetric::MergeOperatorTimeNanos) as i64,
                i64
            ),
            // Time spent waiting on key locks in transaction lock manager.
            (
                "key_lock_wait_nanos",
                perf_context.metric(PerfMetric::KeyLockWaitTime) as i64,
                i64
            ),
            // number of times acquiring a lock was blocked by another transaction.
            (
                "key_lock_wait_count",
                perf_context.metric(PerfMetric::KeyLockWaitCount) as i64,
                i64
            ),
        );
    });
}

#[derive(Debug, Default)]
/// A struct that holds the current status of RocksDB perf sampling.
pub struct PerfSamplingStatus {
    // The number of RocksDB operations since the last perf sample.
    op_count: AtomicUsize,
    // The timestamp of the latest operation with perf stats collection.
    last_sample_time_ms: AtomicU64,
}

impl PerfSamplingStatus {
    fn should_sample(&self, sample_count_interval: usize) -> bool {
        if sample_count_interval == 0 {
            return false;
        }

        // Rate-limiting based on the number of samples.
        if self.op_count.fetch_add(1, Ordering::Relaxed) < sample_count_interval {
            return false;
        }
        self.op_count.store(0, Ordering::Relaxed);

        // Rate-limiting based on the time duration.
        let current_time_ms = timestamp();
        let old_time_ms = self.last_sample_time_ms.load(Ordering::Relaxed);
        if old_time_ms + (PERF_SAMPLING_MIN_DURATION.as_millis() as u64) > current_time_ms {
            return false;
        }

        // If the `last_sample_time_ms` has a different value than `old_time_ms`,
        // it means some other thread has performed the sampling and updated
        // the last sample time.  In this case, the current thread will skip
        // the current sample.
        self.last_sample_time_ms
            .compare_exchange_weak(
                old_time_ms,
                current_time_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
    }
}
