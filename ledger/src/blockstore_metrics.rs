use {
    crate::blockstore_db::{
        columns, BlockstoreCompressionType, BlockstoreRocksDbColumnFamilyMetrics,
        LedgerColumnOptions, ShredStorageType,
    },
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
