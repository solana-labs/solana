use {
    crate::blockstore_db::{
        columns, BlockstoreCompressionType, LedgerColumnOptions, ShredStorageType,
    },
    rocksdb::{
        perf::{set_perf_stats, PerfMetric, PerfStatsLevel},
        PerfContext,
    },
    solana_metrics::datapoint_info,
    solana_sdk::timing::timestamp,
    std::{
        cell::RefCell,
        fmt::Debug,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
};

#[macro_export]
macro_rules! rocksdb_metric_header {
    ($metric_name:literal, $cf_name:literal, $column_options:expr) => {
        match $column_options.shred_storage_type {
            ShredStorageType::RocksLevel =>
                rocksdb_metric_header!(@compression_type $metric_name, $cf_name, $column_options, "rocks_level"),
            ShredStorageType::RocksFifo(_) =>
                rocksdb_metric_header!(@compression_type $metric_name, $cf_name, $column_options, "rocks_fifo"),
        }
    };

    (@compression_type $metric_name:literal, $cf_name:literal, $column_options:expr, $storage_type:literal) => {
        match $column_options.compression_type {
            BlockstoreCompressionType::None => rocksdb_metric_header!(@all_fields
                $metric_name,
                $cf_name,
                $storage_type,
                "None"
            ),
            BlockstoreCompressionType::Snappy => rocksdb_metric_header!(@all_fields
                $metric_name,
                $cf_name,
                $storage_type,
                "Snappy"
            ),
            BlockstoreCompressionType::Lz4 => rocksdb_metric_header!(@all_fields
                $metric_name,
                $cf_name,
                $storage_type,
                "Lz4"
            ),
            BlockstoreCompressionType::Zlib => rocksdb_metric_header!(@all_fields
                $metric_name,
                $cf_name,
                $storage_type,
                "Zlib"
            ),
        }
    };

    (@all_fields $metric_name:literal, $cf_name:literal, $storage_type:literal, $compression_type:literal) => {
        concat!($metric_name,
            ",cf_name=", $cf_name,
            ",storage=", $storage_type,
            ",compression=", $compression_type,
        )
    };
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
    /// The metric name and the column family tag is embeded in the parameter
    /// `metric_name_and_cf_tag` with the following format.
    ///
    /// For example, "blockstore_rocksdb_cfs,cf_name=shred_data".
    pub fn report_metrics(&self, metric_name_and_cf_tag: &'static str) {
        datapoint_info!(
            metric_name_and_cf_tag,
            // Size related
            (
                "total_sst_files_size",
                self.total_sst_files_size as i64,
                i64
            ),
            ("size_all_mem_tables", self.size_all_mem_tables as i64, i64),
            // Snapshot related
            ("num_snapshots", self.num_snapshots as i64, i64),
            (
                "oldest_snapshot_time",
                self.oldest_snapshot_time as i64,
                i64
            ),
            // Write related
            (
                "actual_delayed_write_rate",
                self.actual_delayed_write_rate as i64,
                i64
            ),
            ("is_write_stopped", self.is_write_stopped as i64, i64),
            // Memory / block cache related
            (
                "block_cache_capacity",
                self.block_cache_capacity as i64,
                i64
            ),
            ("block_cache_usage", self.block_cache_usage as i64, i64),
            (
                "block_cache_pinned_usage",
                self.block_cache_pinned_usage as i64,
                i64
            ),
            (
                "estimate_table_readers_mem",
                self.estimate_table_readers_mem as i64,
                i64
            ),
            // Flush and compaction
            (
                "mem_table_flush_pending",
                self.mem_table_flush_pending as i64,
                i64
            ),
            ("compaction_pending", self.compaction_pending as i64, i64),
            (
                "num_running_compactions",
                self.num_running_compactions as i64,
                i64
            ),
            ("num_running_flushes", self.num_running_flushes as i64, i64),
            // FIFO Compaction related
            (
                "estimate_oldest_key_time",
                self.estimate_oldest_key_time as i64,
                i64
            ),
            // Misc
            ("background_errors", self.background_errors as i64, i64),
        );
    }
}

// Thread local instance of RocksDB's PerfContext.
thread_local! {static PER_THREAD_ROCKS_PERF_CONTEXT: RefCell<PerfContext> = RefCell::new(PerfContext::default());}

// The minimum time duration between two RocksDB perf samples of the same operation.
const PERF_SAMPLING_MIN_DURATION: Duration = Duration::from_secs(1);

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
) -> bool {
    if perf_status.should_sample(sample_interval) {
        set_perf_stats(PerfStatsLevel::EnableTime);
        PER_THREAD_ROCKS_PERF_CONTEXT.with(|perf_context| {
            perf_context.borrow_mut().reset();
        });
        return true;
    }
    false
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

pub trait ColumnMetrics {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    );
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str;
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str;
    fn rocksdb_delete_perf_metric_header(column_options: &Arc<LedgerColumnOptions>)
        -> &'static str;
}

impl ColumnMetrics for columns::TransactionStatus {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "transaction_status",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "transaction_status",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "transaction_status",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "transaction_status",
            column_options
        )
    }
}

impl ColumnMetrics for columns::AddressSignatures {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "address_signatures",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "address_signatures",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "address_signatures",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "address_signatures",
            column_options
        )
    }
}

impl ColumnMetrics for columns::TransactionMemos {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "transaction_memos",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "transaction_memos",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "transaction_memos",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "transaction_memos",
            column_options
        )
    }
}

impl ColumnMetrics for columns::TransactionStatusIndex {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "transaction_status_index",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "transaction_status_index",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "transaction_status_index",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "transaction_status_index",
            column_options
        )
    }
}

impl ColumnMetrics for columns::Rewards {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "rewards",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "rewards",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "rewards",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "rewards",
            column_options
        )
    }
}

impl ColumnMetrics for columns::Blocktime {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "blocktime",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "blocktime",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "blocktime",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "blocktime",
            column_options
        )
    }
}

impl ColumnMetrics for columns::PerfSamples {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "perf_samples",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "perf_samples",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "perf_samples",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "perf_samples",
            column_options
        )
    }
}

impl ColumnMetrics for columns::BlockHeight {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "block_height",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "block_height",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "block_height",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "block_height",
            column_options
        )
    }
}

impl ColumnMetrics for columns::ProgramCosts {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "program_costs",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "program_costs",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "program_costs",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "program_costs",
            column_options
        )
    }
}

impl ColumnMetrics for columns::ShredCode {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "shred_code",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "shred_code",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "shred_code",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "shred_code",
            column_options
        )
    }
}

impl ColumnMetrics for columns::ShredData {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "shred_data",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "shred_data",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "shred_data",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "shred_data",
            column_options
        )
    }
}

impl ColumnMetrics for columns::Index {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "index",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "index",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "index",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "index",
            column_options
        )
    }
}

impl ColumnMetrics for columns::DeadSlots {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "dead_slots",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "dead_slots",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "dead_slots",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "dead_slots",
            column_options
        )
    }
}

impl ColumnMetrics for columns::DuplicateSlots {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "duplicate_slots",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "duplicate_slots",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "duplicate_slots",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "duplicate_slots",
            column_options
        )
    }
}

impl ColumnMetrics for columns::Orphans {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "orphans",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "orphans",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "orphans",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "orphans",
            column_options
        )
    }
}

impl ColumnMetrics for columns::BankHash {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "bank_hash",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "bank_hash",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "bank_hash",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "bank_hash",
            column_options
        )
    }
}

impl ColumnMetrics for columns::OptimisticSlots {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "optimistic_slots",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "optimistic_slots",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "optimistic_slots",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "optimistic_slots",
            column_options
        )
    }
}

impl ColumnMetrics for columns::Root {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "root",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "root",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "root",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "root",
            column_options
        )
    }
}

impl ColumnMetrics for columns::SlotMeta {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "slot_meta",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "slot_meta",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "slot_meta",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "slot_meta",
            column_options
        )
    }
}

impl ColumnMetrics for columns::ErasureMeta {
    fn report_cf_metrics(
        cf_metrics: BlockstoreRocksDbColumnFamilyMetrics,
        column_options: &Arc<LedgerColumnOptions>,
    ) {
        cf_metrics.report_metrics(rocksdb_metric_header!(
            "blockstore_rocksdb_cfs",
            "erasure_meta",
            column_options
        ));
    }
    fn rocksdb_get_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_read_perf,op=get",
            "erasure_meta",
            column_options
        )
    }
    fn rocksdb_put_perf_metric_header(column_options: &Arc<LedgerColumnOptions>) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=put",
            "erasure_meta",
            column_options
        )
    }
    fn rocksdb_delete_perf_metric_header(
        column_options: &Arc<LedgerColumnOptions>,
    ) -> &'static str {
        rocksdb_metric_header!(
            "blockstore_rocksdb_write_perf,op=delete",
            "erasure_meta",
            column_options
        )
    }
}
