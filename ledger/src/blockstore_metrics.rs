use {
    rocksdb::{
        perf::{set_perf_stats, PerfMetric, PerfStatsLevel},
        PerfContext,
    },
    std::{
        cell::RefCell,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    },
};

// Thread local instance of RocksDB's PerfContext.
thread_local! {static PER_THREAD_ROCKS_PERF_CONTEXT: RefCell<PerfContext> = RefCell::new(PerfContext::default());}

/// The function enables RocksDB PerfContext once for every `sample_interval`.
///
/// PerfContext is a thread-local struct defined in RocksDB for collecting
/// per-thread read / write performance metrics.
///
/// When this function enables PerfContext, the function will return true,
/// and the PerfContext of the ubsequent RocksDB operation will be collected.
pub(crate) fn maybe_enable_rocksdb_perf(
    sample_interval: usize,
    perf_samples_counter: &Arc<AtomicUsize>,
) -> bool {
    if sample_interval == 0 {
        return false;
    }

    if perf_samples_counter.fetch_add(1, Ordering::Relaxed) < sample_interval {
        return false;
    }
    // Ideally, fetch_sub(*sample_interval) should be used to keep it
    // super precise.  However, since we do not use Mutex to protect the
    // above check and the below operation, we simply reset it to 0.
    perf_samples_counter.store(0, Ordering::Relaxed);

    set_perf_stats(PerfStatsLevel::EnableTime);
    PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context| {
        perf_context.borrow_mut().reset();
    });
    true
}

/// Reports the collected PerfContext and disables the PerfContext after
/// reporting.
pub(crate) fn report_rocksdb_read_perf(metric_header: &'static str) {
    PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context_cell| {
        set_perf_stats(PerfStatsLevel::Disable);
        let perf_context = perf_context_cell.borrow();
        datapoint_info!(
            metric_header,
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
            (
                "env_file_exists_nanos",
                perf_context.metric(PerfMetric::EnvFileExistsNanos) as i64,
                i64
            ),
            (
                "env_get_children_nanos",
                perf_context.metric(PerfMetric::EnvGetChildrenNanos) as i64,
                i64
            ),
            (
                "env_lock_file_nanos",
                perf_context.metric(PerfMetric::EnvLockFileNanos) as i64,
                i64
            ),
            (
                "env_unlock_file_nanos",
                perf_context.metric(PerfMetric::EnvUnlockFileNanos) as i64,
                i64
            ),
            (
                "total_metric_count",
                perf_context.metric(PerfMetric::TotalMetricCount) as i64,
                i64
            ),
        );
    });
}
/// Reports the collected PerfContext and disables the PerfContext after
/// reporting.
pub(crate) fn report_rocksdb_write_perf(metric_header: &'static str) {
    PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context_cell| {
        set_perf_stats(PerfStatsLevel::Disable);
        let perf_context = perf_context_cell.borrow();
        datapoint_info!(
            metric_header,
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
