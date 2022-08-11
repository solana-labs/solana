pub use rocksdb::Direction as IteratorDirection;
use {
    crate::blockstore_meta,
    bincode::{deserialize, serialize},
    byteorder::{BigEndian, ByteOrder},
    log::*,
    prost::Message,
    rocksdb::{
        self,
        compaction_filter::CompactionFilter,
        compaction_filter_factory::{CompactionFilterContext, CompactionFilterFactory},
        ColumnFamily, ColumnFamilyDescriptor, CompactionDecision, DBCompactionStyle, DBIterator,
        DBRawIterator, DBRecoveryMode, FifoCompactOptions, IteratorMode as RocksIteratorMode,
        Options, WriteBatch as RWriteBatch, DB,
    },
    serde::{de::DeserializeOwned, Serialize},
    solana_runtime::hardened_unpack::UnpackError,
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        pubkey::Pubkey,
        signature::Signature,
    },
    solana_storage_proto::convert::generated,
    std::{
        collections::{HashMap, HashSet},
        ffi::{CStr, CString},
        fs,
        marker::PhantomData,
        path::Path,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    },
    thiserror::Error,
};

// The default storage size for storing shreds when `rocksdb-shred-compaction`
// is set to `fifo` in the validator arguments.  This amount of storage size
// in bytes will equally allocated to both data shreds and coding shreds.
pub const DEFAULT_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES: u64 = 250 * 1024 * 1024 * 1024;

const MAX_WRITE_BUFFER_SIZE: u64 = 256 * 1024 * 1024; // 256MB
const FIFO_WRITE_BUFFER_SIZE: u64 = 2 * MAX_WRITE_BUFFER_SIZE;
// Maximum size of cf::DataShred.  Used when `shred_storage_type`
// is set to ShredStorageType::RocksFifo.  The default value is set
// to 125GB, assuming 500GB total storage for ledger and 25% is
// used by data shreds.
const DEFAULT_FIFO_COMPACTION_DATA_CF_SIZE: u64 = 125 * 1024 * 1024 * 1024;
// Maximum size of cf::CodeShred.  Used when `shred_storage_type`
// is set to ShredStorageType::RocksFifo.  The default value is set
// to 100GB, assuming 500GB total storage for ledger and 20% is
// used by coding shreds.
const DEFAULT_FIFO_COMPACTION_CODING_CF_SIZE: u64 = 100 * 1024 * 1024 * 1024;

// Column family for metadata about a leader slot
const META_CF: &str = "meta";
// Column family for slots that have been marked as dead
const DEAD_SLOTS_CF: &str = "dead_slots";
// Column family for storing proof that there were multiple
// versions of a slot
const DUPLICATE_SLOTS_CF: &str = "duplicate_slots";
// Column family storing erasure metadata for a slot
const ERASURE_META_CF: &str = "erasure_meta";
// Column family for orphans data
const ORPHANS_CF: &str = "orphans";
/// Column family for bank hashes
const BANK_HASH_CF: &str = "bank_hashes";
// Column family for root data
const ROOT_CF: &str = "root";
/// Column family for indexes
const INDEX_CF: &str = "index";
/// Column family for Data Shreds
const DATA_SHRED_CF: &str = "data_shred";
/// Column family for Code Shreds
const CODE_SHRED_CF: &str = "code_shred";
/// Column family for Transaction Status
const TRANSACTION_STATUS_CF: &str = "transaction_status";
/// Column family for Address Signatures
const ADDRESS_SIGNATURES_CF: &str = "address_signatures";
/// Column family for TransactionMemos
const TRANSACTION_MEMOS_CF: &str = "transaction_memos";
/// Column family for the Transaction Status Index.
/// This column family is used for tracking the active primary index for columns that for
/// query performance reasons should not be indexed by Slot.
const TRANSACTION_STATUS_INDEX_CF: &str = "transaction_status_index";
/// Column family for Rewards
const REWARDS_CF: &str = "rewards";
/// Column family for Blocktime
const BLOCKTIME_CF: &str = "blocktime";
/// Column family for Performance Samples
const PERF_SAMPLES_CF: &str = "perf_samples";
/// Column family for BlockHeight
const BLOCK_HEIGHT_CF: &str = "block_height";
/// Column family for ProgramCosts
const PROGRAM_COSTS_CF: &str = "program_costs";
/// Column family for optimistic slots
const OPTIMISTIC_SLOTS_CF: &str = "optimistic_slots";

// 1 day is chosen for the same reasoning of DEFAULT_COMPACTION_SLOT_INTERVAL
const PERIODIC_COMPACTION_SECONDS: u64 = 60 * 60 * 24;

#[derive(Error, Debug)]
pub enum BlockstoreError {
    ShredForIndexExists,
    InvalidShredData(Box<bincode::ErrorKind>),
    RocksDb(#[from] rocksdb::Error),
    SlotNotRooted,
    DeadSlot,
    Io(#[from] std::io::Error),
    Serialize(#[from] Box<bincode::ErrorKind>),
    FsExtraError(#[from] fs_extra::error::Error),
    SlotCleanedUp,
    UnpackError(#[from] UnpackError),
    UnableToSetOpenFileDescriptorLimit,
    TransactionStatusSlotMismatch,
    EmptyEpochStakes,
    NoVoteTimestampsInRange,
    ProtobufEncodeError(#[from] prost::EncodeError),
    ProtobufDecodeError(#[from] prost::DecodeError),
    ParentEntriesUnavailable,
    SlotUnavailable,
    UnsupportedTransactionVersion,
    MissingTransactionMetadata,
}
pub type Result<T> = std::result::Result<T, BlockstoreError>;

impl std::fmt::Display for BlockstoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "blockstore error")
    }
}

pub enum IteratorMode<Index> {
    Start,
    End,
    From(Index, IteratorDirection),
}

pub mod columns {
    #[derive(Debug)]
    /// The slot metadata column
    pub struct SlotMeta;

    #[derive(Debug)]
    /// The orphans column
    pub struct Orphans;

    #[derive(Debug)]
    /// The dead slots column
    pub struct DeadSlots;

    #[derive(Debug)]
    /// The duplicate slots column
    pub struct DuplicateSlots;

    #[derive(Debug)]
    /// The erasure meta column
    pub struct ErasureMeta;

    #[derive(Debug)]
    /// The bank hash column
    pub struct BankHash;

    #[derive(Debug)]
    /// The root column
    pub struct Root;

    #[derive(Debug)]
    /// The index column
    pub struct Index;

    #[derive(Debug)]
    /// The shred data column
    pub struct ShredData;

    #[derive(Debug)]
    /// The shred erasure code column
    pub struct ShredCode;

    #[derive(Debug)]
    /// The transaction status column
    pub struct TransactionStatus;

    #[derive(Debug)]
    /// The address signatures column
    pub struct AddressSignatures;

    #[derive(Debug)]
    /// The transaction memos column
    pub struct TransactionMemos;

    #[derive(Debug)]
    /// The transaction status index column
    pub struct TransactionStatusIndex;

    #[derive(Debug)]
    /// The rewards column
    pub struct Rewards;

    #[derive(Debug)]
    /// The blocktime column
    pub struct Blocktime;

    #[derive(Debug)]
    /// The performance samples column
    pub struct PerfSamples;

    #[derive(Debug)]
    /// The block height column
    pub struct BlockHeight;

    #[derive(Debug)]
    /// The program costs column
    pub struct ProgramCosts;

    #[derive(Debug)]
    /// The optimistic slot column
    pub struct OptimisticSlots;

    // When adding a new column ...
    // - Add struct below and implement `Column` and `ColumnName` traits
    // - Add descriptor in Rocks::cf_descriptors() and name in Rocks::columns()
    // - Account for column in both `run_purge_with_stats()` and
    //   `compact_storage()` in ledger/src/blockstore/blockstore_purge.rs !!
    // - Account for column in `analyze_storage()` in ledger-tool/src/main.rs
}

pub enum AccessType {
    PrimaryOnly,
    PrimaryOnlyForMaintenance, // this indicates no compaction
    TryPrimaryThenSecondary,
}

#[derive(Debug, PartialEq)]
pub enum ActualAccessType {
    Primary,
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
            bad_mode => panic!("Invalid recovery mode: {}", bad_mode),
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

#[derive(Default, Clone, Debug)]
struct OldestSlot(Arc<AtomicU64>);

impl OldestSlot {
    pub fn set(&self, oldest_slot: Slot) {
        // this is independently used for compaction_filter without any data dependency.
        // also, compaction_filters are created via its factories, creating short-lived copies of
        // this atomic value for the single job of compaction. So, Relaxed store can be justified
        // in total
        self.0.store(oldest_slot, Ordering::Relaxed);
    }

    pub fn get(&self) -> Slot {
        // copy from the AtomicU64 as a general precaution so that the oldest_slot can not mutate
        // across single run of compaction for simpler reasoning although this isn't strict
        // requirement at the moment
        // also eventual propagation (very Relaxed) load is Ok, because compaction by nature doesn't
        // require strictly synchronized semantics in this regard
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
struct Rocks(rocksdb::DB, ActualAccessType, OldestSlot);

impl Rocks {
    fn open(path: &Path, options: BlockstoreOptions) -> Result<Rocks> {
        let access_type = &options.access_type;
        let recovery_mode = options.recovery_mode.clone();

        fs::create_dir_all(&path)?;

        // Use default database options
        if should_disable_auto_compactions(access_type) {
            warn!("Disabling rocksdb's auto compaction for maintenance bulk ledger update...");
        }
        let mut db_options = get_db_options(access_type);
        if let Some(recovery_mode) = recovery_mode {
            db_options.set_wal_recovery_mode(recovery_mode.into());
        }

        let oldest_slot = OldestSlot::default();
        let cf_descriptors = Self::cf_descriptors(&options, &oldest_slot);
        let cf_names = Self::columns();

        // Open the database
        let db = match access_type {
            AccessType::PrimaryOnly | AccessType::PrimaryOnlyForMaintenance => Rocks(
                DB::open_cf_descriptors(&db_options, path, cf_descriptors)?,
                ActualAccessType::Primary,
                oldest_slot,
            ),
            AccessType::TryPrimaryThenSecondary => {
                match DB::open_cf_descriptors(&db_options, path, cf_descriptors) {
                    Ok(db) => Rocks(db, ActualAccessType::Primary, oldest_slot),
                    Err(err) => {
                        let secondary_path = path.join("solana-secondary");

                        warn!("Error when opening as primary: {}", err);
                        warn!("Trying as secondary at : {:?}", secondary_path);
                        warn!("This active secondary db use may temporarily cause the performance of another db use (like by validator) to degrade");

                        Rocks(
                            DB::open_cf_as_secondary(
                                &db_options,
                                path,
                                &secondary_path,
                                cf_names.clone(),
                            )?,
                            ActualAccessType::Secondary,
                            oldest_slot,
                        )
                    }
                }
            }
        };
        // this is only needed for LedgerCleanupService. so guard with PrimaryOnly (i.e. running solana-validator)
        if matches!(access_type, AccessType::PrimaryOnly) {
            for cf_name in cf_names {
                // these special column families must be excluded from LedgerCleanupService's rocksdb
                // compactions
                if should_exclude_from_compaction(cf_name) {
                    continue;
                }

                // This is the crux of our write-stall-free storage cleaning strategy with consistent
                // state view for higher-layers
                //
                // For the consistent view, we commit delete_range on pruned slot range by LedgerCleanupService.
                // simple story here.
                //
                // For actual storage cleaning, we employ RocksDB compaction. But default RocksDB compaction
                // settings don't work well for us. That's because we're using it rather like a really big
                // (100 GBs) ring-buffer. RocksDB is basically assuming uniform data write over the key space for
                // efficient compaction, which isn't true for our use as a ring buffer.
                //
                // So, we customize the compaction strategy with 2 combined tweaks:
                // (1) compaction_filter and (2) shortening its periodic cycles.
                //
                // Via the compaction_filter, we finally reclaim previously delete_range()-ed storage occupied
                // by pruned slots. When compaction_filter is set, each SST files are re-compacted periodically
                // to hunt for keys newly expired by the compaction_filter re-evaluation. But RocksDb's default
                // `periodic_compaction_seconds` is 30 days, which is too long for our case. So, we
                // shorten it to a day (24 hours).
                //
                // As we write newer SST files over time at rather consistent rate of speed, this
                // effectively makes each newly-created sets be re-compacted for the filter at
                // well-dispersed different timings.
                // As a whole, we rewrite the whole dataset at every PERIODIC_COMPACTION_SECONDS,
                // slowly over the duration of PERIODIC_COMPACTION_SECONDS. So, this results in
                // amortization.
                // So, there is a bit inefficiency here because we'll rewrite not-so-old SST files
                // too. But longer period would introduce higher variance of ledger storage sizes over
                // the long period. And it's much better than the daily IO spike caused by compact_range() by
                // previous implementation.
                //
                // `ttl` and `compact_range`(`ManualCompaction`), doesn't work nicely. That's
                // because its original intention is delete_range()s to reclaim disk space. So it tries to merge
                // them with N+1 SST files all way down to the bottommost SSTs, often leading to vastly large amount
                // (= all) of invalidated SST files, when combined with newer writes happening at the opposite
                // edge of the key space. This causes a long and heavy disk IOs and possible write
                // stall and ultimately, the deadly Replay/Banking stage stall at higher layers.
                db.0.set_options_cf(
                    db.cf_handle(cf_name),
                    &[(
                        "periodic_compaction_seconds",
                        &format!("{}", PERIODIC_COMPACTION_SECONDS),
                    )],
                )
                .unwrap();
            }
        }

        Ok(db)
    }

    fn cf_descriptors(
        options: &BlockstoreOptions,
        oldest_slot: &OldestSlot,
    ) -> Vec<ColumnFamilyDescriptor> {
        use columns::*;

        let (cf_descriptor_shred_data, cf_descriptor_shred_code) =
            new_cf_descriptor_pair_shreds::<ShredData, ShredCode>(options, oldest_slot);
        vec![
            new_cf_descriptor::<SlotMeta>(options, oldest_slot),
            new_cf_descriptor::<DeadSlots>(options, oldest_slot),
            new_cf_descriptor::<DuplicateSlots>(options, oldest_slot),
            new_cf_descriptor::<ErasureMeta>(options, oldest_slot),
            new_cf_descriptor::<Orphans>(options, oldest_slot),
            new_cf_descriptor::<BankHash>(options, oldest_slot),
            new_cf_descriptor::<Root>(options, oldest_slot),
            new_cf_descriptor::<Index>(options, oldest_slot),
            cf_descriptor_shred_data,
            cf_descriptor_shred_code,
            new_cf_descriptor::<TransactionStatus>(options, oldest_slot),
            new_cf_descriptor::<AddressSignatures>(options, oldest_slot),
            new_cf_descriptor::<TransactionMemos>(options, oldest_slot),
            new_cf_descriptor::<TransactionStatusIndex>(options, oldest_slot),
            new_cf_descriptor::<Rewards>(options, oldest_slot),
            new_cf_descriptor::<Blocktime>(options, oldest_slot),
            new_cf_descriptor::<PerfSamples>(options, oldest_slot),
            new_cf_descriptor::<BlockHeight>(options, oldest_slot),
            new_cf_descriptor::<ProgramCosts>(options, oldest_slot),
            new_cf_descriptor::<OptimisticSlots>(options, oldest_slot),
        ]
    }

    fn columns() -> Vec<&'static str> {
        use columns::*;

        vec![
            ErasureMeta::NAME,
            DeadSlots::NAME,
            DuplicateSlots::NAME,
            Index::NAME,
            Orphans::NAME,
            BankHash::NAME,
            Root::NAME,
            SlotMeta::NAME,
            ShredData::NAME,
            ShredCode::NAME,
            TransactionStatus::NAME,
            AddressSignatures::NAME,
            TransactionMemos::NAME,
            TransactionStatusIndex::NAME,
            Rewards::NAME,
            Blocktime::NAME,
            PerfSamples::NAME,
            BlockHeight::NAME,
            ProgramCosts::NAME,
            OptimisticSlots::NAME,
        ]
    }

    fn destroy(path: &Path) -> Result<()> {
        DB::destroy(&Options::default(), path)?;

        Ok(())
    }

    fn cf_handle(&self, cf: &str) -> &ColumnFamily {
        self.0
            .cf_handle(cf)
            .expect("should never get an unknown column")
    }

    fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let opt = self.0.get_cf(cf, key)?;
        Ok(opt)
    }

    fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        self.0.put_cf(cf, key, value)?;
        Ok(())
    }

    fn delete_cf(&self, cf: &ColumnFamily, key: &[u8]) -> Result<()> {
        self.0.delete_cf(cf, key)?;
        Ok(())
    }

    fn iterator_cf<C>(&self, cf: &ColumnFamily, iterator_mode: IteratorMode<C::Index>) -> DBIterator
    where
        C: Column,
    {
        let start_key;
        let iterator_mode = match iterator_mode {
            IteratorMode::From(start_from, direction) => {
                start_key = C::key(start_from);
                RocksIteratorMode::From(&start_key, direction)
            }
            IteratorMode::Start => RocksIteratorMode::Start,
            IteratorMode::End => RocksIteratorMode::End,
        };
        self.0.iterator_cf(cf, iterator_mode)
    }

    fn raw_iterator_cf(&self, cf: &ColumnFamily) -> DBRawIterator {
        self.0.raw_iterator_cf(cf)
    }

    fn batch(&self) -> RWriteBatch {
        RWriteBatch::default()
    }

    fn write(&self, batch: RWriteBatch) -> Result<()> {
        self.0.write(batch)?;
        Ok(())
    }

    fn is_primary_access(&self) -> bool {
        self.1 == ActualAccessType::Primary
    }

    /// Retrieves the specified RocksDB integer property of the current
    /// column family.
    ///
    /// Full list of properties that return int values could be found
    /// [here](https://github.com/facebook/rocksdb/blob/08809f5e6cd9cc4bc3958dd4d59457ae78c76660/include/rocksdb/db.h#L654-L689).
    fn get_int_property_cf(&self, cf: &ColumnFamily, name: &'static std::ffi::CStr) -> Result<i64> {
        match self.0.property_int_value_cf(cf, name) {
            Ok(Some(value)) => Ok(value.try_into().unwrap()),
            Ok(None) => Ok(0),
            Err(e) => Err(BlockstoreError::RocksDb(e)),
        }
    }
}

pub trait Column {
    type Index;

    fn key_size() -> usize {
        std::mem::size_of::<Self::Index>()
    }

    fn key(index: Self::Index) -> Vec<u8>;
    fn index(key: &[u8]) -> Self::Index;
    // this return Slot or some u64
    fn primary_index(index: Self::Index) -> u64;
    #[allow(clippy::wrong_self_convention)]
    fn as_index(slot: Slot) -> Self::Index;
    fn slot(index: Self::Index) -> Slot {
        Self::primary_index(index)
    }
}

pub trait ColumnName {
    const NAME: &'static str;
}

pub trait TypedColumn: Column {
    type Type: Serialize + DeserializeOwned;
}

impl TypedColumn for columns::AddressSignatures {
    type Type = blockstore_meta::AddressSignatureMeta;
}

impl TypedColumn for columns::TransactionMemos {
    type Type = String;
}

impl TypedColumn for columns::TransactionStatusIndex {
    type Type = blockstore_meta::TransactionStatusIndexMeta;
}

pub trait ProtobufColumn: Column {
    type Type: prost::Message + Default;
}

pub trait SlotColumn<Index = u64> {}

impl<T: SlotColumn> Column for T {
    type Index = u64;

    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    fn primary_index(index: u64) -> Slot {
        index
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(slot: Slot) -> u64 {
        slot
    }
}

impl Column for columns::TransactionStatus {
    type Index = (u64, Signature, Slot);

    fn key((index, signature, slot): (u64, Signature, Slot)) -> Vec<u8> {
        let mut key = vec![0; 8 + 64 + 8]; // size_of u64 + size_of Signature + size_of Slot
        BigEndian::write_u64(&mut key[0..8], index);
        key[8..72].clone_from_slice(&signature.as_ref()[0..64]);
        BigEndian::write_u64(&mut key[72..80], slot);
        key
    }

    fn index(key: &[u8]) -> (u64, Signature, Slot) {
        if key.len() != 80 {
            Self::as_index(0)
        } else {
            let index = BigEndian::read_u64(&key[0..8]);
            let signature = Signature::new(&key[8..72]);
            let slot = BigEndian::read_u64(&key[72..80]);
            (index, signature, slot)
        }
    }

    fn primary_index(index: Self::Index) -> u64 {
        index.0
    }

    fn slot(index: Self::Index) -> Slot {
        index.2
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(index: u64) -> Self::Index {
        (index, Signature::default(), 0)
    }
}

impl ColumnName for columns::TransactionStatus {
    const NAME: &'static str = TRANSACTION_STATUS_CF;
}
impl ProtobufColumn for columns::TransactionStatus {
    type Type = generated::TransactionStatusMeta;
}

impl Column for columns::AddressSignatures {
    type Index = (u64, Pubkey, Slot, Signature);

    fn key((index, pubkey, slot, signature): (u64, Pubkey, Slot, Signature)) -> Vec<u8> {
        let mut key = vec![0; 8 + 32 + 8 + 64]; // size_of u64 + size_of Pubkey + size_of Slot + size_of Signature
        BigEndian::write_u64(&mut key[0..8], index);
        key[8..40].clone_from_slice(&pubkey.as_ref()[0..32]);
        BigEndian::write_u64(&mut key[40..48], slot);
        key[48..112].clone_from_slice(&signature.as_ref()[0..64]);
        key
    }

    fn index(key: &[u8]) -> (u64, Pubkey, Slot, Signature) {
        let index = BigEndian::read_u64(&key[0..8]);
        let pubkey = Pubkey::new(&key[8..40]);
        let slot = BigEndian::read_u64(&key[40..48]);
        let signature = Signature::new(&key[48..112]);
        (index, pubkey, slot, signature)
    }

    fn primary_index(index: Self::Index) -> u64 {
        index.0
    }

    fn slot(index: Self::Index) -> Slot {
        index.2
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(index: u64) -> Self::Index {
        (index, Pubkey::default(), 0, Signature::default())
    }
}

impl ColumnName for columns::AddressSignatures {
    const NAME: &'static str = ADDRESS_SIGNATURES_CF;
}

impl Column for columns::TransactionMemos {
    type Index = Signature;

    fn key(signature: Signature) -> Vec<u8> {
        let mut key = vec![0; 64]; // size_of Signature
        key[0..64].clone_from_slice(&signature.as_ref()[0..64]);
        key
    }

    fn index(key: &[u8]) -> Signature {
        Signature::new(&key[0..64])
    }

    fn primary_index(_index: Self::Index) -> u64 {
        unimplemented!()
    }

    fn slot(_index: Self::Index) -> Slot {
        unimplemented!()
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(_index: u64) -> Self::Index {
        Signature::default()
    }
}

impl ColumnName for columns::TransactionMemos {
    const NAME: &'static str = TRANSACTION_MEMOS_CF;
}

impl Column for columns::TransactionStatusIndex {
    type Index = u64;

    fn key(index: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], index);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    fn primary_index(index: u64) -> u64 {
        index
    }

    fn slot(_index: Self::Index) -> Slot {
        unimplemented!()
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(slot: u64) -> u64 {
        slot
    }
}

impl ColumnName for columns::TransactionStatusIndex {
    const NAME: &'static str = TRANSACTION_STATUS_INDEX_CF;
}

impl SlotColumn for columns::Rewards {}
impl ColumnName for columns::Rewards {
    const NAME: &'static str = REWARDS_CF;
}
impl ProtobufColumn for columns::Rewards {
    type Type = generated::Rewards;
}

impl SlotColumn for columns::Blocktime {}
impl ColumnName for columns::Blocktime {
    const NAME: &'static str = BLOCKTIME_CF;
}
impl TypedColumn for columns::Blocktime {
    type Type = UnixTimestamp;
}

impl SlotColumn for columns::PerfSamples {}
impl ColumnName for columns::PerfSamples {
    const NAME: &'static str = PERF_SAMPLES_CF;
}
impl TypedColumn for columns::PerfSamples {
    type Type = blockstore_meta::PerfSample;
}

impl SlotColumn for columns::BlockHeight {}
impl ColumnName for columns::BlockHeight {
    const NAME: &'static str = BLOCK_HEIGHT_CF;
}
impl TypedColumn for columns::BlockHeight {
    type Type = u64;
}

impl ColumnName for columns::ProgramCosts {
    const NAME: &'static str = PROGRAM_COSTS_CF;
}
impl TypedColumn for columns::ProgramCosts {
    type Type = blockstore_meta::ProgramCost;
}
impl Column for columns::ProgramCosts {
    type Index = Pubkey;

    fn key(pubkey: Pubkey) -> Vec<u8> {
        let mut key = vec![0; 32]; // size_of Pubkey
        key[0..32].clone_from_slice(&pubkey.as_ref()[0..32]);
        key
    }

    fn index(key: &[u8]) -> Self::Index {
        Pubkey::new(&key[0..32])
    }

    fn primary_index(_index: Self::Index) -> u64 {
        unimplemented!()
    }

    fn slot(_index: Self::Index) -> Slot {
        unimplemented!()
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(_index: u64) -> Self::Index {
        Pubkey::default()
    }
}

impl Column for columns::ShredCode {
    type Index = (u64, u64);

    fn key(index: (u64, u64)) -> Vec<u8> {
        columns::ShredData::key(index)
    }

    fn index(key: &[u8]) -> (u64, u64) {
        columns::ShredData::index(key)
    }

    fn primary_index(index: Self::Index) -> Slot {
        index.0
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(slot: Slot) -> Self::Index {
        (slot, 0)
    }
}

impl ColumnName for columns::ShredCode {
    const NAME: &'static str = CODE_SHRED_CF;
}

impl Column for columns::ShredData {
    type Index = (u64, u64);

    fn key((slot, index): (u64, u64)) -> Vec<u8> {
        let mut key = vec![0; 16];
        BigEndian::write_u64(&mut key[..8], slot);
        BigEndian::write_u64(&mut key[8..16], index);
        key
    }

    fn index(key: &[u8]) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key[..8]);
        let index = BigEndian::read_u64(&key[8..16]);
        (slot, index)
    }

    fn primary_index(index: Self::Index) -> Slot {
        index.0
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(slot: Slot) -> Self::Index {
        (slot, 0)
    }
}

impl ColumnName for columns::ShredData {
    const NAME: &'static str = DATA_SHRED_CF;
}

impl SlotColumn for columns::Index {}
impl ColumnName for columns::Index {
    const NAME: &'static str = INDEX_CF;
}
impl TypedColumn for columns::Index {
    type Type = blockstore_meta::Index;
}

impl SlotColumn for columns::DeadSlots {}
impl ColumnName for columns::DeadSlots {
    const NAME: &'static str = DEAD_SLOTS_CF;
}
impl TypedColumn for columns::DeadSlots {
    type Type = bool;
}

impl SlotColumn for columns::DuplicateSlots {}
impl ColumnName for columns::DuplicateSlots {
    const NAME: &'static str = DUPLICATE_SLOTS_CF;
}
impl TypedColumn for columns::DuplicateSlots {
    type Type = blockstore_meta::DuplicateSlotProof;
}

impl SlotColumn for columns::Orphans {}
impl ColumnName for columns::Orphans {
    const NAME: &'static str = ORPHANS_CF;
}
impl TypedColumn for columns::Orphans {
    type Type = bool;
}

impl SlotColumn for columns::BankHash {}
impl ColumnName for columns::BankHash {
    const NAME: &'static str = BANK_HASH_CF;
}
impl TypedColumn for columns::BankHash {
    type Type = blockstore_meta::FrozenHashVersioned;
}

impl SlotColumn for columns::Root {}
impl ColumnName for columns::Root {
    const NAME: &'static str = ROOT_CF;
}
impl TypedColumn for columns::Root {
    type Type = bool;
}

impl SlotColumn for columns::SlotMeta {}
impl ColumnName for columns::SlotMeta {
    const NAME: &'static str = META_CF;
}
impl TypedColumn for columns::SlotMeta {
    type Type = blockstore_meta::SlotMeta;
}

impl Column for columns::ErasureMeta {
    type Index = (u64, u64);

    fn index(key: &[u8]) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key[..8]);
        let set_index = BigEndian::read_u64(&key[8..]);

        (slot, set_index)
    }

    fn key((slot, set_index): (u64, u64)) -> Vec<u8> {
        let mut key = vec![0; 16];
        BigEndian::write_u64(&mut key[..8], slot);
        BigEndian::write_u64(&mut key[8..], set_index);
        key
    }

    fn primary_index(index: Self::Index) -> Slot {
        index.0
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_index(slot: Slot) -> Self::Index {
        (slot, 0)
    }
}
impl ColumnName for columns::ErasureMeta {
    const NAME: &'static str = ERASURE_META_CF;
}
impl TypedColumn for columns::ErasureMeta {
    type Type = blockstore_meta::ErasureMeta;
}

impl SlotColumn for columns::OptimisticSlots {}
impl ColumnName for columns::OptimisticSlots {
    const NAME: &'static str = OPTIMISTIC_SLOTS_CF;
}
impl TypedColumn for columns::OptimisticSlots {
    type Type = blockstore_meta::OptimisticSlotMetaVersioned;
}

#[derive(Debug, Clone)]
pub struct Database {
    backend: Arc<Rocks>,
    path: Arc<Path>,
}

#[derive(Debug, Clone)]
pub struct LedgerColumn<C>
where
    C: Column,
{
    backend: Arc<Rocks>,
    column: PhantomData<C>,
}

pub struct WriteBatch<'a> {
    write_batch: RWriteBatch,
    map: HashMap<&'static str, &'a ColumnFamily>,
}

#[derive(Clone)]
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

/// Options for LedgerColumn.
/// Each field might also be used as a tag that supports group-by operation when
/// reporting metrics.
#[derive(Clone)]
pub struct LedgerColumnOptions {
    // Determine how to store both data and coding shreds. Default: RocksLevel.
    pub shred_storage_type: ShredStorageType,
}

impl Default for LedgerColumnOptions {
    fn default() -> Self {
        Self {
            shred_storage_type: ShredStorageType::RocksLevel,
        }
    }
}

pub struct BlockstoreOptions {
    // The access type of blockstore. Default: PrimaryOnly
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
            access_type: AccessType::PrimaryOnly,
            recovery_mode: None,
            enforce_ulimit_nofile: true,
            column_options: LedgerColumnOptions::default(),
        }
    }
}

#[derive(Clone)]
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

impl Default for BlockstoreRocksFifoOptions {
    fn default() -> Self {
        Self {
            // Maximum size of cf::ShredData.
            shred_data_cf_size: DEFAULT_FIFO_COMPACTION_DATA_CF_SIZE,
            // Maximum size of cf::ShredCode.
            shred_code_cf_size: DEFAULT_FIFO_COMPACTION_CODING_CF_SIZE,
        }
    }
}

impl Database {
    pub fn open(path: &Path, options: BlockstoreOptions) -> Result<Self> {
        let backend = Arc::new(Rocks::open(path, options)?);

        Ok(Database {
            backend,
            path: Arc::from(path),
        })
    }

    pub fn destroy(path: &Path) -> Result<()> {
        Rocks::destroy(path)?;

        Ok(())
    }

    pub fn get<C>(&self, key: C::Index) -> Result<Option<C::Type>>
    where
        C: TypedColumn + ColumnName,
    {
        if let Some(serialized_value) = self.backend.get_cf(self.cf_handle::<C>(), &C::key(key))? {
            let value = deserialize(&serialized_value)?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn iter<C>(
        &self,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)> + '_>
    where
        C: Column + ColumnName,
    {
        let cf = self.cf_handle::<C>();
        let iter = self.backend.iterator_cf::<C>(cf, iterator_mode);
        Ok(iter.map(|pair| {
            let (key, value) = pair.unwrap();
            (C::index(&key), value)
        }))
    }

    #[inline]
    pub fn cf_handle<C: ColumnName>(&self) -> &ColumnFamily
    where
        C: Column + ColumnName,
    {
        self.backend.cf_handle(C::NAME)
    }

    pub fn column<C>(&self) -> LedgerColumn<C>
    where
        C: Column + ColumnName,
    {
        LedgerColumn {
            backend: Arc::clone(&self.backend),
            column: PhantomData,
        }
    }

    #[inline]
    pub fn raw_iterator_cf(&self, cf: &ColumnFamily) -> Result<DBRawIterator> {
        Ok(self.backend.raw_iterator_cf(cf))
    }

    pub fn batch(&self) -> Result<WriteBatch> {
        let write_batch = self.backend.batch();
        let map = Rocks::columns()
            .into_iter()
            .map(|desc| (desc, self.backend.cf_handle(desc)))
            .collect();

        Ok(WriteBatch { write_batch, map })
    }

    pub fn write(&self, batch: WriteBatch) -> Result<()> {
        self.backend.write(batch.write_batch)
    }

    pub fn storage_size(&self) -> Result<u64> {
        Ok(fs_extra::dir::get_size(&self.path)?)
    }

    // Adds a range to delete to the given write batch
    pub fn delete_range_cf<C>(&self, batch: &mut WriteBatch, from: Slot, to: Slot) -> Result<()>
    where
        C: Column + ColumnName,
    {
        let cf = self.cf_handle::<C>();
        let from_index = C::as_index(from);
        let to_index = C::as_index(to);
        batch.delete_range_cf::<C>(cf, from_index, to_index)
    }

    pub fn is_primary_access(&self) -> bool {
        self.backend.is_primary_access()
    }

    pub fn set_oldest_slot(&self, oldest_slot: Slot) {
        self.backend.2.set(oldest_slot);
    }
}

impl<C> LedgerColumn<C>
where
    C: Column + ColumnName,
{
    pub fn get_bytes(&self, key: C::Index) -> Result<Option<Vec<u8>>> {
        self.backend.get_cf(self.handle(), &C::key(key))
    }

    pub fn iter(
        &self,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)> + '_> {
        let cf = self.handle();
        let iter = self.backend.iterator_cf::<C>(cf, iterator_mode);
        Ok(iter.map(|pair| {
            let (key, value) = pair.unwrap();
            (C::index(&key), value)
        }))
    }

    pub fn delete_slot(
        &self,
        batch: &mut WriteBatch,
        from: Option<Slot>,
        to: Option<Slot>,
    ) -> Result<bool>
    where
        C::Index: PartialOrd + Copy + ColumnName,
    {
        let mut end = true;
        let iter_config = match from {
            Some(s) => IteratorMode::From(C::as_index(s), IteratorDirection::Forward),
            None => IteratorMode::Start,
        };
        let iter = self.iter(iter_config)?;
        for (index, _) in iter {
            if let Some(to) = to {
                if C::primary_index(index) > to {
                    end = false;
                    break;
                }
            };
            if let Err(e) = batch.delete::<C>(index) {
                error!(
                    "Error: {:?} while adding delete from_slot {:?} to batch {:?}",
                    e,
                    from,
                    C::NAME
                )
            }
        }
        Ok(end)
    }

    pub fn compact_range(&self, from: Slot, to: Slot) -> Result<bool>
    where
        C::Index: PartialOrd + Copy,
    {
        let cf = self.handle();
        let from = Some(C::key(C::as_index(from)));
        let to = Some(C::key(C::as_index(to)));
        self.backend.0.compact_range_cf(cf, from, to);
        Ok(true)
    }

    #[inline]
    pub fn handle(&self) -> &ColumnFamily {
        self.backend.cf_handle(C::NAME)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> Result<bool> {
        let mut iter = self.backend.raw_iterator_cf(self.handle());
        iter.seek_to_first();
        Ok(!iter.valid())
    }

    pub fn put_bytes(&self, key: C::Index, value: &[u8]) -> Result<()> {
        self.backend.put_cf(self.handle(), &C::key(key), value)
    }

    /// Retrieves the specified RocksDB integer property of the current
    /// column family.
    ///
    /// Full list of properties that return int values could be found
    /// [here](https://github.com/facebook/rocksdb/blob/08809f5e6cd9cc4bc3958dd4d59457ae78c76660/include/rocksdb/db.h#L654-L689).
    pub fn get_int_property(&self, name: &'static std::ffi::CStr) -> Result<i64> {
        self.backend.get_int_property_cf(self.handle(), name)
    }
}

impl<C> LedgerColumn<C>
where
    C: TypedColumn + ColumnName,
{
    pub fn get(&self, key: C::Index) -> Result<Option<C::Type>> {
        if let Some(serialized_value) = self.backend.get_cf(self.handle(), &C::key(key))? {
            let value = deserialize(&serialized_value)?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn put(&self, key: C::Index, value: &C::Type) -> Result<()> {
        let serialized_value = serialize(value)?;

        self.backend
            .put_cf(self.handle(), &C::key(key), &serialized_value)
    }

    pub fn delete(&self, key: C::Index) -> Result<()> {
        self.backend.delete_cf(self.handle(), &C::key(key))
    }
}

impl<C> LedgerColumn<C>
where
    C: ProtobufColumn + ColumnName,
{
    pub fn get_protobuf_or_bincode<T: DeserializeOwned + Into<C::Type>>(
        &self,
        key: C::Index,
    ) -> Result<Option<C::Type>> {
        if let Some(serialized_value) = self.backend.get_cf(self.handle(), &C::key(key))? {
            let value = match C::Type::decode(&serialized_value[..]) {
                Ok(value) => value,
                Err(_) => deserialize::<T>(&serialized_value)?.into(),
            };
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn get_protobuf(&self, key: C::Index) -> Result<Option<C::Type>> {
        if let Some(serialized_value) = self.backend.get_cf(self.handle(), &C::key(key))? {
            Ok(Some(C::Type::decode(&serialized_value[..])?))
        } else {
            Ok(None)
        }
    }

    pub fn put_protobuf(&self, key: C::Index, value: &C::Type) -> Result<()> {
        let mut buf = Vec::with_capacity(value.encoded_len());
        value.encode(&mut buf)?;
        self.backend.put_cf(self.handle(), &C::key(key), &buf)
    }
}

impl<'a> WriteBatch<'a> {
    pub fn put_bytes<C: Column + ColumnName>(&mut self, key: C::Index, bytes: &[u8]) -> Result<()> {
        self.write_batch
            .put_cf(self.get_cf::<C>(), &C::key(key), bytes);
        Ok(())
    }

    pub fn delete<C: Column + ColumnName>(&mut self, key: C::Index) -> Result<()> {
        self.write_batch.delete_cf(self.get_cf::<C>(), &C::key(key));
        Ok(())
    }

    pub fn put<C: TypedColumn + ColumnName>(
        &mut self,
        key: C::Index,
        value: &C::Type,
    ) -> Result<()> {
        let serialized_value = serialize(&value)?;
        self.write_batch
            .put_cf(self.get_cf::<C>(), &C::key(key), &serialized_value);
        Ok(())
    }

    #[inline]
    fn get_cf<C: Column + ColumnName>(&self) -> &'a ColumnFamily {
        self.map[C::NAME]
    }

    pub fn delete_range_cf<C: Column>(
        &mut self,
        cf: &ColumnFamily,
        from: C::Index,
        to: C::Index,
    ) -> Result<()> {
        self.write_batch
            .delete_range_cf(cf, C::key(from), C::key(to));
        Ok(())
    }
}

struct PurgedSlotFilter<C: Column + ColumnName> {
    oldest_slot: Slot,
    name: CString,
    _phantom: PhantomData<C>,
}

impl<C: Column + ColumnName> CompactionFilter for PurgedSlotFilter<C> {
    fn filter(&mut self, _level: u32, key: &[u8], _value: &[u8]) -> CompactionDecision {
        use rocksdb::CompactionDecision::*;

        let slot_in_key = C::slot(C::index(key));
        // Refer to a comment about periodic_compaction_seconds, especially regarding implicit
        // periodic execution of compaction_filters
        if slot_in_key >= self.oldest_slot {
            Keep
        } else {
            Remove
        }
    }

    fn name(&self) -> &CStr {
        &self.name
    }
}

struct PurgedSlotFilterFactory<C: Column + ColumnName> {
    oldest_slot: OldestSlot,
    name: CString,
    _phantom: PhantomData<C>,
}

impl<C: Column + ColumnName> CompactionFilterFactory for PurgedSlotFilterFactory<C> {
    type Filter = PurgedSlotFilter<C>;

    fn create(&mut self, _context: CompactionFilterContext) -> Self::Filter {
        let copied_oldest_slot = self.oldest_slot.get();
        PurgedSlotFilter::<C> {
            oldest_slot: copied_oldest_slot,
            name: CString::new(format!(
                "purged_slot_filter({}, {:?})",
                C::NAME,
                copied_oldest_slot
            ))
            .unwrap(),
            _phantom: PhantomData::default(),
        }
    }

    fn name(&self) -> &CStr {
        &self.name
    }
}

fn new_cf_descriptor<C: 'static + Column + ColumnName>(
    options: &BlockstoreOptions,
    oldest_slot: &OldestSlot,
) -> ColumnFamilyDescriptor {
    ColumnFamilyDescriptor::new(C::NAME, get_cf_options::<C>(options, oldest_slot))
}

fn get_cf_options<C: 'static + Column + ColumnName>(
    options: &BlockstoreOptions,
    oldest_slot: &OldestSlot,
) -> Options {
    let mut cf_options = Options::default();
    // 256 * 8 = 2GB. 6 of these columns should take at most 12GB of RAM
    cf_options.set_max_write_buffer_number(8);
    cf_options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE as usize);
    let file_num_compaction_trigger = 4;
    // Recommend that this be around the size of level 0. Level 0 estimated size in stable state is
    // write_buffer_size * min_write_buffer_number_to_merge * level0_file_num_compaction_trigger
    // Source: https://docs.rs/rocksdb/0.6.0/rocksdb/struct.Options.html#method.set_level_zero_file_num_compaction_trigger
    let total_size_base = MAX_WRITE_BUFFER_SIZE * file_num_compaction_trigger;
    let file_size_base = total_size_base / 10;
    cf_options.set_level_zero_file_num_compaction_trigger(file_num_compaction_trigger as i32);
    cf_options.set_max_bytes_for_level_base(total_size_base);
    cf_options.set_target_file_size_base(file_size_base);

    let disable_auto_compactions = should_disable_auto_compactions(&options.access_type);
    if disable_auto_compactions {
        cf_options.set_disable_auto_compactions(true);
    }

    if !disable_auto_compactions && !should_exclude_from_compaction(C::NAME) {
        cf_options.set_compaction_filter_factory(PurgedSlotFilterFactory::<C> {
            oldest_slot: oldest_slot.clone(),
            name: CString::new(format!("purged_slot_filter_factory({})", C::NAME)).unwrap(),
            _phantom: PhantomData::default(),
        });
    }

    cf_options
}

/// Creates and returns the column family descriptors for both data shreds and
/// coding shreds column families.
///
/// @return a pair of ColumnFamilyDescriptor where the first / second elements
/// are associated to the first / second template class respectively.
fn new_cf_descriptor_pair_shreds<
    D: 'static + Column + ColumnName, // Column Family for Data Shred
    C: 'static + Column + ColumnName, // Column Family for Coding Shred
>(
    options: &BlockstoreOptions,
    oldest_slot: &OldestSlot,
) -> (ColumnFamilyDescriptor, ColumnFamilyDescriptor) {
    match &options.column_options.shred_storage_type {
        ShredStorageType::RocksLevel => (
            new_cf_descriptor::<D>(options, oldest_slot),
            new_cf_descriptor::<C>(options, oldest_slot),
        ),
        ShredStorageType::RocksFifo(fifo_options) => (
            new_cf_descriptor_fifo::<D>(&fifo_options.shred_data_cf_size),
            new_cf_descriptor_fifo::<C>(&fifo_options.shred_code_cf_size),
        ),
    }
}

fn new_cf_descriptor_fifo<C: 'static + Column + ColumnName>(
    max_cf_size: &u64,
) -> ColumnFamilyDescriptor {
    if *max_cf_size > FIFO_WRITE_BUFFER_SIZE {
        ColumnFamilyDescriptor::new(C::NAME, get_cf_options_fifo::<C>(max_cf_size))
    } else {
        panic!(
            "{} cf_size must be greater than write buffer size {} when using ShredStorageType::RocksFifo.",
            C::NAME, FIFO_WRITE_BUFFER_SIZE
        );
    }
}

/// Returns the RocksDB Column Family Options which use FIFO Compaction.
///
/// Note that this CF options is optimized for workloads which write-keys
/// are mostly monotonically increasing over time.  For workloads where
/// write-keys do not follow any order in general should use get_cf_options
/// instead.
///
/// - [`max_cf_size`]: the maximum allowed column family size.  Note that
/// rocksdb will start deleting the oldest SST file when the column family
/// size reaches `max_cf_size` - `FIFO_WRITE_BUFFER_SIZE` to strictly
/// maintain the size limit.
fn get_cf_options_fifo<C: 'static + Column + ColumnName>(max_cf_size: &u64) -> Options {
    let mut options = Options::default();

    options.set_max_write_buffer_number(8);
    options.set_write_buffer_size(FIFO_WRITE_BUFFER_SIZE as usize);
    // FIFO always has its files in L0 so we only have one level.
    options.set_num_levels(1);
    // Since FIFO puts all its file in L0, it is suggested to have unlimited
    // number of open files.  The actual total number of open files will
    // be close to max_cf_size / write_buffer_size.
    options.set_max_open_files(-1);

    let mut fifo_compact_options = FifoCompactOptions::default();

    // Note that the following actually specifies size trigger for deleting
    // the oldest SST file instead of specifying the size limit as its name
    // might suggest.  As a result, we should trigger the file deletion when
    // the size reaches `max_cf_size - write_buffer_size` in order to correctly
    // maintain the storage size limit.
    fifo_compact_options
        .set_max_table_files_size((*max_cf_size).saturating_sub(FIFO_WRITE_BUFFER_SIZE));

    options.set_compaction_style(DBCompactionStyle::Fifo);
    options.set_fifo_compaction_options(&fifo_compact_options);

    options
}

fn get_db_options(access_type: &AccessType) -> Options {
    let mut options = Options::default();

    // Create missing items to support a clean start
    options.create_if_missing(true);
    options.create_missing_column_families(true);

    // Per the docs, a good value for this is the number of cores on the machine
    options.increase_parallelism(num_cpus::get() as i32);

    let mut env = rocksdb::Env::default().unwrap();
    // While a compaction is ongoing, all the background threads
    // could be used by the compaction. This can stall writes which
    // need to flush the memtable. Add some high-priority background threads
    // which can service these writes.
    env.set_high_priority_background_threads(4);
    options.set_env(&env);

    // Set max total wal size to 4G.
    options.set_max_total_wal_size(4 * 1024 * 1024 * 1024);

    if should_disable_auto_compactions(access_type) {
        options.set_disable_auto_compactions(true);
    }

    // Allow Rocks to open/keep open as many files as it needs for performance;
    // however, this is also explicitly required for a secondary instance.
    // See https://github.com/facebook/rocksdb/wiki/Secondary-instance
    options.set_max_open_files(-1);

    options
}

// Returns whether automatic compactions should be disabled based upon access type
fn should_disable_auto_compactions(access_type: &AccessType) -> bool {
    // Disable automatic compactions in maintenance mode to prevent accidental cleaning
    matches!(access_type, AccessType::PrimaryOnlyForMaintenance)
}

// Returns whether the supplied column (name) should be excluded from compaction
fn should_exclude_from_compaction(cf_name: &str) -> bool {
    // List of column families to be excluded from compactions
    let no_compaction_cfs: HashSet<&'static str> = vec![
        columns::TransactionStatusIndex::NAME,
        columns::ProgramCosts::NAME,
        columns::TransactionMemos::NAME,
    ]
    .into_iter()
    .collect();

    no_compaction_cfs.get(cf_name).is_some()
}

#[cfg(test)]
pub mod tests {
    use {super::*, crate::blockstore_db::columns::ShredData};

    #[test]
    fn test_compaction_filter() {
        // this doesn't implement Clone...
        let dummy_compaction_filter_context = || CompactionFilterContext {
            is_full_compaction: true,
            is_manual_compaction: true,
        };
        let oldest_slot = OldestSlot::default();

        let mut factory = PurgedSlotFilterFactory::<ShredData> {
            oldest_slot: oldest_slot.clone(),
            name: CString::new("test compaction filter").unwrap(),
            _phantom: PhantomData::default(),
        };
        let mut compaction_filter = factory.create(dummy_compaction_filter_context());

        let dummy_level = 0;
        let key = ShredData::key(ShredData::as_index(0));
        let dummy_value = vec![];

        // we can't use assert_matches! because CompactionDecision doesn't implement Debug
        assert!(matches!(
            compaction_filter.filter(dummy_level, &key, &dummy_value),
            CompactionDecision::Keep
        ));

        // mutating oldest_slot doesn't affect existing compaction filters...
        oldest_slot.set(1);
        assert!(matches!(
            compaction_filter.filter(dummy_level, &key, &dummy_value),
            CompactionDecision::Keep
        ));

        // recreating compaction filter starts to expire the key
        let mut compaction_filter = factory.create(dummy_compaction_filter_context());
        assert!(matches!(
            compaction_filter.filter(dummy_level, &key, &dummy_value),
            CompactionDecision::Remove
        ));

        // newer key shouldn't be removed
        let key = ShredData::key(ShredData::as_index(1));
        matches!(
            compaction_filter.filter(dummy_level, &key, &dummy_value),
            CompactionDecision::Keep
        );
    }

    #[test]
    fn test_cf_names_and_descriptors_equal_length() {
        let options = BlockstoreOptions::default();
        let oldest_slot = OldestSlot::default();
        // The names and descriptors don't need to be in the same order for our use cases;
        // however, there should be the same number of each. For example, adding a new column
        // should update both lists.
        assert_eq!(
            Rocks::columns().len(),
            Rocks::cf_descriptors(&options, &oldest_slot).len()
        );
    }

    #[test]
    fn test_should_exclude_from_compaction() {
        // currently there are three CFs excluded from compaction:
        assert!(should_exclude_from_compaction(
            columns::TransactionStatusIndex::NAME
        ));
        assert!(should_exclude_from_compaction(columns::ProgramCosts::NAME));
        assert!(should_exclude_from_compaction(
            columns::TransactionMemos::NAME
        ));
        assert!(!should_exclude_from_compaction("something else"));
    }
}
