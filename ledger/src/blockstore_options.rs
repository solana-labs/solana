use {
    rocksdb::{DBCompressionType as RocksCompressionType, DBRecoveryMode},
    std::path::Path,
};

pub struct BlockstoreOptions {
    // The access type of blockstore. Default: Primary
    pub access_type: AccessType,
    // Whether to open a blockstore under a recovery mode. Default: None.
    pub recovery_mode: Option<BlockstoreRecoveryMode>,
    // Whether to allow unlimited number of open files. Default: true.
    pub enforce_ulimit_nofile: bool,
    pub column_options: LedgerColumnOptions,
}

impl Default for BlockstoreOptions {
    /// The default options are the values used by [`Blockstore::open`].
    ///
    /// [`Blockstore::open`]: crate::blockstore::Blockstore::open
    fn default() -> Self {
        Self {
            access_type: AccessType::Primary,
            recovery_mode: None,
            enforce_ulimit_nofile: true,
            column_options: LedgerColumnOptions::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessType {
    /// Primary (read/write) access; only one process can have Primary access.
    Primary,
    /// Primary (read/write) access with RocksDB automatic compaction disabled.
    PrimaryForMaintenance,
    /// Secondary (read) access; multiple processes can have Secondary access.
    /// Additionally, Secondary access can be obtained while another process
    /// already has Primary access.
    Secondary,
}

#[derive(Debug, Clone)]
pub enum BlockstoreRecoveryMode {
    TolerateCorruptedTailRecords,
    AbsoluteConsistency,
    PointInTime,
    SkipAnyCorruptedRecord,
}

impl From<&str> for BlockstoreRecoveryMode {
    fn from(string: &str) -> Self {
        match string {
            "tolerate_corrupted_tail_records" => {
                BlockstoreRecoveryMode::TolerateCorruptedTailRecords
            }
            "absolute_consistency" => BlockstoreRecoveryMode::AbsoluteConsistency,
            "point_in_time" => BlockstoreRecoveryMode::PointInTime,
            "skip_any_corrupted_record" => BlockstoreRecoveryMode::SkipAnyCorruptedRecord,
            bad_mode => panic!("Invalid recovery mode: {bad_mode}"),
        }
    }
}

impl From<BlockstoreRecoveryMode> for DBRecoveryMode {
    fn from(brm: BlockstoreRecoveryMode) -> Self {
        match brm {
            BlockstoreRecoveryMode::TolerateCorruptedTailRecords => {
                DBRecoveryMode::TolerateCorruptedTailRecords
            }
            BlockstoreRecoveryMode::AbsoluteConsistency => DBRecoveryMode::AbsoluteConsistency,
            BlockstoreRecoveryMode::PointInTime => DBRecoveryMode::PointInTime,
            BlockstoreRecoveryMode::SkipAnyCorruptedRecord => {
                DBRecoveryMode::SkipAnyCorruptedRecord
            }
        }
    }
}

/// Options for LedgerColumn.
/// Each field might also be used as a tag that supports group-by operation when
/// reporting metrics.
#[derive(Debug, Clone)]
pub struct LedgerColumnOptions {
    // Determine how to store both data and coding shreds. Default: RocksLevel.
    pub shred_storage_type: ShredStorageType,

    // Determine the way to compress column families which are eligible for
    // compression.
    pub compression_type: BlockstoreCompressionType,

    // Control how often RocksDB read/write performance samples are collected.
    // If the value is greater than 0, then RocksDB read/write perf sample
    // will be collected once for every `rocks_perf_sample_interval` ops.
    pub rocks_perf_sample_interval: usize,
}

impl Default for LedgerColumnOptions {
    fn default() -> Self {
        Self {
            shred_storage_type: ShredStorageType::RocksLevel,
            compression_type: BlockstoreCompressionType::default(),
            rocks_perf_sample_interval: 0,
        }
    }
}

impl LedgerColumnOptions {
    pub fn get_storage_type_string(&self) -> &'static str {
        match self.shred_storage_type {
            ShredStorageType::RocksLevel => "rocks_level",
            ShredStorageType::RocksFifo(_) => "rocks_fifo",
        }
    }

    pub fn get_compression_type_string(&self) -> &'static str {
        match self.compression_type {
            BlockstoreCompressionType::None => "None",
            BlockstoreCompressionType::Snappy => "Snappy",
            BlockstoreCompressionType::Lz4 => "Lz4",
            BlockstoreCompressionType::Zlib => "Zlib",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ShredStorageType {
    // Stores shreds under RocksDB's default compaction (level).
    RocksLevel,
    // (Experimental) Stores shreds under RocksDB's FIFO compaction which
    // allows ledger store to reclaim storage more efficiently with
    // lower I/O overhead.
    RocksFifo(BlockstoreRocksFifoOptions),
}

impl Default for ShredStorageType {
    fn default() -> Self {
        Self::RocksLevel
    }
}

pub const BLOCKSTORE_DIRECTORY_ROCKS_LEVEL: &str = "rocksdb";
pub const BLOCKSTORE_DIRECTORY_ROCKS_FIFO: &str = "rocksdb_fifo";

impl ShredStorageType {
    /// Returns a ShredStorageType::RocksFifo, see BlockstoreRocksFifoOptions
    /// for more details on how `max_shred_storage_size` is interpreted.
    pub fn rocks_fifo(max_shred_storage_size: Option<u64>) -> ShredStorageType {
        ShredStorageType::RocksFifo(BlockstoreRocksFifoOptions::new(max_shred_storage_size))
    }

    /// The directory under `ledger_path` to the underlying blockstore.
    pub fn blockstore_directory(&self) -> &str {
        match self {
            ShredStorageType::RocksLevel => BLOCKSTORE_DIRECTORY_ROCKS_LEVEL,
            ShredStorageType::RocksFifo(_) => BLOCKSTORE_DIRECTORY_ROCKS_FIFO,
        }
    }

    /// Returns the ShredStorageType that is used under the specified
    /// ledger_path.
    ///
    /// None will be returned if the ShredStorageType cannot be inferred.
    pub fn from_ledger_path(
        ledger_path: &Path,
        max_fifo_shred_storage_size: Option<u64>,
    ) -> Option<ShredStorageType> {
        let mut result: Option<ShredStorageType> = None;

        if Path::new(ledger_path)
            .join(BLOCKSTORE_DIRECTORY_ROCKS_LEVEL)
            .exists()
        {
            result = Some(ShredStorageType::RocksLevel);
        }

        if Path::new(ledger_path)
            .join(BLOCKSTORE_DIRECTORY_ROCKS_FIFO)
            .exists()
        {
            if result.is_none() {
                result = Some(ShredStorageType::RocksFifo(
                    BlockstoreRocksFifoOptions::new(max_fifo_shred_storage_size),
                ));
            } else {
                result = None;
            }
        }
        result
    }
}

#[derive(Debug, Clone)]
pub struct BlockstoreRocksFifoOptions {
    // The maximum storage size for storing data shreds in column family
    // [`cf::DataShred`].  Typically, data shreds contribute around 25% of the
    // ledger store storage size if the RPC service is enabled, or 50% if RPC
    // service is not enabled.
    //
    // Note that this number must be greater than FIFO_WRITE_BUFFER_SIZE
    // otherwise we won't be able to write any file.  If not, the blockstore
    // will panic.
    pub shred_data_cf_size: u64,
    // The maximum storage size for storing coding shreds in column family
    // [`cf::CodeShred`].  Typically, coding shreds contribute around 20% of the
    // ledger store storage size if the RPC service is enabled, or 40% if RPC
    // service is not enabled.
    //
    // Note that this number must be greater than FIFO_WRITE_BUFFER_SIZE
    // otherwise we won't be able to write any file.  If not, the blockstore
    // will panic.
    pub shred_code_cf_size: u64,
}

pub const MAX_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES: u64 = std::u64::MAX;

impl BlockstoreRocksFifoOptions {
    /// Returns a BlockstoreRocksFifoOptions where the specified
    /// `max_shred_storage_size` is equally split between shred_data_cf_size
    /// and shred_code_cf_size. A `None` value for `max_shred_storage_size`
    /// will (functionally) allow unbounded growth in these two columns. Once
    /// a column's total size exceeds the configured value, the oldest file(s)
    /// will be purged to get back within the limit.
    fn new(max_shred_storage_size: Option<u64>) -> Self {
        match max_shred_storage_size {
            Some(size) => Self {
                shred_data_cf_size: size / 2,
                shred_code_cf_size: size / 2,
            },
            None => Self {
                shred_data_cf_size: MAX_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES,
                shred_code_cf_size: MAX_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES,
            },
        }
    }

    pub fn new_for_tests() -> Self {
        Self {
            shred_data_cf_size: 150_000_000_000,
            shred_code_cf_size: 150_000_000_000,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BlockstoreCompressionType {
    None,
    Snappy,
    Lz4,
    Zlib,
}

impl Default for BlockstoreCompressionType {
    fn default() -> Self {
        Self::None
    }
}

impl BlockstoreCompressionType {
    pub(crate) fn to_rocksdb_compression_type(&self) -> RocksCompressionType {
        match self {
            Self::None => RocksCompressionType::None,
            Self::Snappy => RocksCompressionType::Snappy,
            Self::Lz4 => RocksCompressionType::Lz4,
            Self::Zlib => RocksCompressionType::Zlib,
        }
    }
}

#[test]
fn test_rocksdb_directory() {
    assert_eq!(
        ShredStorageType::RocksLevel.blockstore_directory(),
        BLOCKSTORE_DIRECTORY_ROCKS_LEVEL
    );
    assert_eq!(
        ShredStorageType::RocksFifo(BlockstoreRocksFifoOptions {
            shred_code_cf_size: 0,
            shred_data_cf_size: 0
        })
        .blockstore_directory(),
        BLOCKSTORE_DIRECTORY_ROCKS_FIFO
    );
}
