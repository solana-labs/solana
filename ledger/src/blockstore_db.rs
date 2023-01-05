pub use rocksdb::Direction as IteratorDirection;
use {
    crate::{
        blockstore_meta,
        blockstore_metrics::{
            maybe_enable_rocksdb_perf, report_rocksdb_read_perf, report_rocksdb_write_perf,
            BlockstoreRocksDbColumnFamilyMetrics, PerfSamplingStatus, PERF_METRIC_OP_NAME_GET,
            PERF_METRIC_OP_NAME_MULTI_GET, PERF_METRIC_OP_NAME_PUT,
            PERF_METRIC_OP_NAME_WRITE_BATCH,
        },
        blockstore_options::{
            AccessType, BlockstoreOptions, LedgerColumnOptions, ShredStorageType,
        },
    },
    bincode::{deserialize, serialize},
    byteorder::{BigEndian, ByteOrder},
    log::*,
    prost::Message,
    rocksdb::{
        self,
        compaction_filter::CompactionFilter,
        compaction_filter_factory::{CompactionFilterContext, CompactionFilterFactory},
        properties as RocksProperties, ColumnFamily, ColumnFamilyDescriptor, CompactionDecision,
        DBCompactionStyle, DBIterator, DBPinnableSlice, DBRawIterator, FifoCompactOptions,
        IteratorMode as RocksIteratorMode, LiveFile, Options, WriteBatch as RWriteBatch, DB,
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

const BLOCKSTORE_METRICS_ERROR: i64 = -1;

const MAX_WRITE_BUFFER_SIZE: u64 = 256 * 1024 * 1024; // 256MB
const FIFO_WRITE_BUFFER_SIZE: u64 = 2 * MAX_WRITE_BUFFER_SIZE;

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
    /// The slot metadata column.
    ///
    /// This column family tracks the status of the received shred data for a
    /// given slot.  Tracking the progress as the slot fills up allows us to
    /// know if the slot (or pieces of the slot) are ready to be replayed.
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::SlotMeta`
    pub struct SlotMeta;

    #[derive(Debug)]
    /// The orphans column.
    ///
    /// This column family tracks whether a slot has a parent.  Slots without a
    /// parent are by definition orphan slots.  Orphans will have an entry in
    /// this column family with true value.  Once an orphan slot has a parent,
    /// its entry in this column will be deleted.
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: bool
    pub struct Orphans;

    #[derive(Debug)]
    /// The dead slots column.
    /// This column family tracks whether a slot is dead.
    ///
    /// A slot is marked as dead if the validator thinks it will never be able
    /// to successfully replay this slot.  Example scenarios include errors
    /// during the replay of a slot, or the validator believes it will never
    /// receive all the shreds of a slot.
    ///
    /// If a slot has been mistakenly marked as dead, the ledger-tool's
    /// --remove-dead-slot can unmark a dead slot.
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: bool
    pub struct DeadSlots;

    #[derive(Debug)]
    /// The duplicate slots column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::DuplicateSlotProof`
    pub struct DuplicateSlots;

    #[derive(Debug)]
    /// The erasure meta column.
    ///
    /// This column family stores ErasureMeta which includes metadata about
    /// dropped network packets (or erasures) that can be used to recover
    /// missing data shreds.
    ///
    /// Its index type is ErasureSetId, which consists of a Slot ID
    /// and a FEC (Forward Error Correction) set index.
    ///
    /// index type: `ErasureSetId` (Slot, fec_set_index: u64)
    /// value type: `blockstore_meta::ErasureMeta`
    pub struct ErasureMeta;

    #[derive(Debug)]
    /// The bank hash column.
    ///
    /// This column family persists the bank hash of a given slot.  Note that
    /// not every slot has a bank hash (e.g., a dead slot.)
    ///
    /// The bank hash of a slot is derived from hashing the delta state of all
    /// the accounts in a slot combined with the bank hash of its parent slot.
    /// A bank hash of a slot essentially represents all the account states at
    /// that slot.
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::FrozenHashVersioned`
    pub struct BankHash;

    #[derive(Debug)]
    /// The root column.
    ///
    /// This column family persists whether a slot is a root.  Slots on the
    /// main fork will be inserted into this column when they are finalized.
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: bool
    pub struct Root;

    #[derive(Debug)]
    /// The index column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::Index`
    pub struct Index;

    #[derive(Debug)]
    /// The shred data column
    ///
    /// index type: (u64, u64)
    /// value type: Vec<u8>
    pub struct ShredData;

    #[derive(Debug)]
    /// The shred erasure code column
    ///
    /// index type: (u64, u64)
    /// value type: Vec<u8>
    pub struct ShredCode;

    #[derive(Debug)]
    /// The transaction status column
    ///
    /// index type: (u64, Signature, Slot)
    /// value type: `generated::TransactionStatusMeta`
    pub struct TransactionStatus;

    #[derive(Debug)]
    /// The address signatures column
    ///
    /// index type: (u64, Pubkey, Slot, Signature)
    /// value type: `blockstore_meta::AddressSignatureMeta`
    pub struct AddressSignatures;

    #[derive(Debug)]
    /// The transaction memos column
    ///
    /// index type: Signature
    /// value type: String
    pub struct TransactionMemos;

    #[derive(Debug)]
    /// The transaction status index column.
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::TransactionStatusIndexMeta`
    pub struct TransactionStatusIndex;

    #[derive(Debug)]
    /// The rewards column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `generated::Rewards`
    pub struct Rewards;

    #[derive(Debug)]
    /// The blocktime column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `UnixTimestamp`
    pub struct Blocktime;

    #[derive(Debug)]
    /// The performance samples column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::PerfSample`
    pub struct PerfSamples;

    #[derive(Debug)]
    /// The block height column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: u64
    pub struct BlockHeight;

    #[derive(Debug)]
    /// The program costs column
    ///
    /// index type: `Pubkey`
    /// value type: `blockstore_meta::ProgramCost`
    pub struct ProgramCosts;

    #[derive(Debug)]
    /// The optimistic slot column
    ///
    /// index type: u64 (see `SlotColumn`)
    /// value type: `blockstore_meta::OptimisticSlotMetaVersioned`
    pub struct OptimisticSlots;

    // When adding a new column ...
    // - Add struct below and implement `Column` and `ColumnName` traits
    // - Add descriptor in Rocks::cf_descriptors() and name in Rocks::columns()
    // - Account for column in both `run_purge_with_stats()` and
    //   `compact_storage()` in ledger/src/blockstore/blockstore_purge.rs !!
    // - Account for column in `analyze_storage()` in ledger-tool/src/main.rs
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
struct Rocks {
    db: rocksdb::DB,
    access_type: AccessType,
    oldest_slot: OldestSlot,
    column_options: LedgerColumnOptions,
    write_batch_perf_status: PerfSamplingStatus,
}

impl Rocks {
    fn open(path: &Path, options: BlockstoreOptions) -> Result<Rocks> {
        let access_type = options.access_type.clone();
        let recovery_mode = options.recovery_mode.clone();

        fs::create_dir_all(path)?;

        // Use default database options
        if should_disable_auto_compactions(&access_type) {
            info!("Disabling rocksdb's automatic compactions...");
        }
        let mut db_options = get_db_options(&access_type);
        if let Some(recovery_mode) = recovery_mode {
            db_options.set_wal_recovery_mode(recovery_mode.into());
        }
        let oldest_slot = OldestSlot::default();
        let column_options = options.column_options.clone();

        // Open the database
        let db = match access_type {
            AccessType::Primary | AccessType::PrimaryForMaintenance => Rocks {
                db: DB::open_cf_descriptors(
                    &db_options,
                    path,
                    Self::cf_descriptors(&options, &oldest_slot),
                )?,
                access_type,
                oldest_slot,
                column_options,
                write_batch_perf_status: PerfSamplingStatus::default(),
            },
            AccessType::Secondary => {
                let secondary_path = path.join("solana-secondary");

                info!(
                    "Opening Rocks with secondary (read only) access at: {:?}",
                    secondary_path
                );
                info!("This secondary access could temporarily degrade other accesses, such as by solana-validator");

                Rocks {
                    db: DB::open_cf_descriptors_as_secondary(
                        &db_options,
                        path,
                        &secondary_path,
                        Self::cf_descriptors(&options, &oldest_slot),
                    )?,
                    access_type,
                    oldest_slot,
                    column_options,
                    write_batch_perf_status: PerfSamplingStatus::default(),
                }
            }
        };

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
        self.db
            .cf_handle(cf)
            .expect("should never get an unknown column")
    }

    fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let opt = self.db.get_cf(cf, key)?;
        Ok(opt)
    }

    fn get_pinned_cf(&self, cf: &ColumnFamily, key: &[u8]) -> Result<Option<DBPinnableSlice>> {
        let opt = self.db.get_pinned_cf(cf, key)?;
        Ok(opt)
    }

    fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.put_cf(cf, key, value)?;
        Ok(())
    }

    fn multi_get_cf(
        &self,
        cf: &ColumnFamily,
        keys: Vec<&[u8]>,
    ) -> Vec<Result<Option<DBPinnableSlice>>> {
        let values = self
            .db
            .batched_multi_get_cf(cf, keys, false)
            .into_iter()
            .map(|result| match result {
                Ok(opt) => Ok(opt),
                Err(e) => Err(BlockstoreError::RocksDb(e)),
            })
            .collect::<Vec<_>>();
        values
    }

    fn delete_cf(&self, cf: &ColumnFamily, key: &[u8]) -> Result<()> {
        self.db.delete_cf(cf, key)?;
        Ok(())
    }

    /// Delete files whose slot range is within \[`from`, `to`\].
    fn delete_file_in_range_cf(
        &self,
        cf: &ColumnFamily,
        from_key: &[u8],
        to_key: &[u8],
    ) -> Result<()> {
        self.db.delete_file_in_range_cf(cf, from_key, to_key)?;
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
        self.db.iterator_cf(cf, iterator_mode)
    }

    fn raw_iterator_cf(&self, cf: &ColumnFamily) -> DBRawIterator {
        self.db.raw_iterator_cf(cf)
    }

    fn batch(&self) -> RWriteBatch {
        RWriteBatch::default()
    }

    fn write(&self, batch: RWriteBatch) -> Result<()> {
        let op_start_instant = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.write_batch_perf_status,
        );
        let result = self.db.write(batch);
        if let Some(op_start_instant) = op_start_instant {
            report_rocksdb_write_perf(
                PERF_METRIC_OP_NAME_WRITE_BATCH, // We use write_batch as cf_name for write batch.
                PERF_METRIC_OP_NAME_WRITE_BATCH, // op_name
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(BlockstoreError::RocksDb(e)),
        }
    }

    fn is_primary_access(&self) -> bool {
        self.access_type == AccessType::Primary
            || self.access_type == AccessType::PrimaryForMaintenance
    }

    /// Retrieves the specified RocksDB integer property of the current
    /// column family.
    ///
    /// Full list of properties that return int values could be found
    /// [here](https://github.com/facebook/rocksdb/blob/08809f5e6cd9cc4bc3958dd4d59457ae78c76660/include/rocksdb/db.h#L654-L689).
    fn get_int_property_cf(&self, cf: &ColumnFamily, name: &'static std::ffi::CStr) -> Result<i64> {
        match self.db.property_int_value_cf(cf, name) {
            Ok(Some(value)) => Ok(value.try_into().unwrap()),
            Ok(None) => Ok(0),
            Err(e) => Err(BlockstoreError::RocksDb(e)),
        }
    }

    fn live_files_metadata(&self) -> Result<Vec<LiveFile>> {
        match self.db.live_files() {
            Ok(live_files) => Ok(live_files),
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

/// SlotColumn is a trait for slot-based column families.  Its index is
/// essentially Slot (or more generally speaking, has a 1:1 mapping to Slot).
///
/// The clean-up of any LedgerColumn that implements SlotColumn is managed by
/// [`LedgerCleanupService`], which will periodically deprecate and purge
/// oldest entries that are older than the latest root in order to maintain the
/// configured --limit-ledger-size under the validator argument.
pub trait SlotColumn<Index = u64> {}

impl<T: SlotColumn> Column for T {
    type Index = u64;

    /// Converts a u64 Index to its RocksDB key.
    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    /// Converts a RocksDB key to its u64 Index.
    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    /// Obtains the primary index from the specified index.
    fn primary_index(index: u64) -> Slot {
        index
    }

    #[allow(clippy::wrong_self_convention)]
    /// Converts a Slot to its u64 Index.
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

#[derive(Debug)]
pub struct Database {
    backend: Arc<Rocks>,
    path: Arc<Path>,
    column_options: Arc<LedgerColumnOptions>,
}

#[derive(Debug)]
pub struct LedgerColumn<C>
where
    C: Column + ColumnName,
{
    backend: Arc<Rocks>,
    column: PhantomData<C>,
    pub column_options: Arc<LedgerColumnOptions>,
    read_perf_status: PerfSamplingStatus,
    write_perf_status: PerfSamplingStatus,
}

impl<C: Column + ColumnName> LedgerColumn<C> {
    pub fn submit_rocksdb_cf_metrics(&self) {
        let cf_rocksdb_metrics = BlockstoreRocksDbColumnFamilyMetrics {
            total_sst_files_size: self
                .get_int_property(RocksProperties::TOTAL_SST_FILES_SIZE)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            size_all_mem_tables: self
                .get_int_property(RocksProperties::SIZE_ALL_MEM_TABLES)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            num_snapshots: self
                .get_int_property(RocksProperties::NUM_SNAPSHOTS)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            oldest_snapshot_time: self
                .get_int_property(RocksProperties::OLDEST_SNAPSHOT_TIME)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            actual_delayed_write_rate: self
                .get_int_property(RocksProperties::ACTUAL_DELAYED_WRITE_RATE)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            is_write_stopped: self
                .get_int_property(RocksProperties::IS_WRITE_STOPPED)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            block_cache_capacity: self
                .get_int_property(RocksProperties::BLOCK_CACHE_CAPACITY)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            block_cache_usage: self
                .get_int_property(RocksProperties::BLOCK_CACHE_USAGE)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            block_cache_pinned_usage: self
                .get_int_property(RocksProperties::BLOCK_CACHE_PINNED_USAGE)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            estimate_table_readers_mem: self
                .get_int_property(RocksProperties::ESTIMATE_TABLE_READERS_MEM)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            mem_table_flush_pending: self
                .get_int_property(RocksProperties::MEM_TABLE_FLUSH_PENDING)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            compaction_pending: self
                .get_int_property(RocksProperties::COMPACTION_PENDING)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            num_running_compactions: self
                .get_int_property(RocksProperties::NUM_RUNNING_COMPACTIONS)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            num_running_flushes: self
                .get_int_property(RocksProperties::NUM_RUNNING_FLUSHES)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            estimate_oldest_key_time: self
                .get_int_property(RocksProperties::ESTIMATE_OLDEST_KEY_TIME)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
            background_errors: self
                .get_int_property(RocksProperties::BACKGROUND_ERRORS)
                .unwrap_or(BLOCKSTORE_METRICS_ERROR),
        };
        cf_rocksdb_metrics.report_metrics(C::NAME, &self.column_options);
    }
}

pub struct WriteBatch<'a> {
    write_batch: RWriteBatch,
    map: HashMap<&'static str, &'a ColumnFamily>,
}

impl Database {
    pub fn open(path: &Path, options: BlockstoreOptions) -> Result<Self> {
        let column_options = Arc::new(options.column_options.clone());
        let backend = Arc::new(Rocks::open(path, options)?);

        Ok(Database {
            backend,
            path: Arc::from(path),
            column_options,
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
        if let Some(pinnable_slice) = self
            .backend
            .get_pinned_cf(self.cf_handle::<C>(), &C::key(key))?
        {
            let value = deserialize(pinnable_slice.as_ref())?;
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
            column_options: Arc::clone(&self.column_options),
            read_perf_status: PerfSamplingStatus::default(),
            write_perf_status: PerfSamplingStatus::default(),
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

    /// Adds a \[`from`, `to`\] range that deletes all entries between the `from` slot
    /// and `to` slot inclusively.  If `from` slot and `to` slot are the same, then all
    /// entries in that slot will be removed.
    ///
    pub fn delete_range_cf<C>(&self, batch: &mut WriteBatch, from: Slot, to: Slot) -> Result<()>
    where
        C: Column + ColumnName,
    {
        let cf = self.cf_handle::<C>();
        // Note that the default behavior of rocksdb's delete_range_cf deletes
        // files within [from, to), while our purge logic applies to [from, to].
        //
        // For consistency, we make our delete_range_cf works for [from, to] by
        // adjusting the `to` slot range by 1.
        let from_index = C::as_index(from);
        let to_index = C::as_index(to.saturating_add(1));
        batch.delete_range_cf::<C>(cf, from_index, to_index)
    }

    /// Delete files whose slot range is within \[`from`, `to`\].
    pub fn delete_file_in_range_cf<C>(&self, from: Slot, to: Slot) -> Result<()>
    where
        C: Column + ColumnName,
    {
        self.backend.delete_file_in_range_cf(
            self.cf_handle::<C>(),
            &C::key(C::as_index(from)),
            &C::key(C::as_index(to)),
        )
    }

    pub fn is_primary_access(&self) -> bool {
        self.backend.is_primary_access()
    }

    pub fn set_oldest_slot(&self, oldest_slot: Slot) {
        self.backend.oldest_slot.set(oldest_slot);
    }

    pub fn live_files_metadata(&self) -> Result<Vec<LiveFile>> {
        self.backend.live_files_metadata()
    }
}

impl<C> LedgerColumn<C>
where
    C: Column + ColumnName,
{
    pub fn get_bytes(&self, key: C::Index) -> Result<Option<Vec<u8>>> {
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.read_perf_status,
        );
        let result = self.backend.get_cf(self.handle(), &C::key(key));
        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_read_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_GET,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }
        result
    }

    pub fn multi_get_bytes(&self, keys: Vec<C::Index>) -> Vec<Result<Option<Vec<u8>>>> {
        let rocks_keys: Vec<_> = keys.into_iter().map(|key| C::key(key)).collect();
        {
            let ref_rocks_keys: Vec<_> = rocks_keys.iter().map(|k| &k[..]).collect();
            let is_perf_enabled = maybe_enable_rocksdb_perf(
                self.column_options.rocks_perf_sample_interval,
                &self.read_perf_status,
            );
            let result = self
                .backend
                .multi_get_cf(self.handle(), ref_rocks_keys)
                .into_iter()
                .map(|r| match r {
                    Ok(opt) => match opt {
                        Some(pinnable_slice) => Ok(Some(pinnable_slice.as_ref().to_vec())),
                        None => Ok(None),
                    },
                    Err(e) => Err(e),
                })
                .collect::<Vec<Result<Option<_>>>>();
            if let Some(op_start_instant) = is_perf_enabled {
                // use multi-get instead
                report_rocksdb_read_perf(
                    C::NAME,
                    PERF_METRIC_OP_NAME_MULTI_GET,
                    &op_start_instant.elapsed(),
                    &self.column_options,
                );
            }

            result
        }
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
        self.backend.db.compact_range_cf(cf, from, to);
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
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.write_perf_status,
        );
        let result = self.backend.put_cf(self.handle(), &C::key(key), value);
        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_write_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_PUT,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }
        result
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
    pub fn multi_get(&self, keys: Vec<C::Index>) -> Vec<Result<Option<C::Type>>> {
        let rocks_keys: Vec<_> = keys.into_iter().map(|key| C::key(key)).collect();
        {
            let ref_rocks_keys: Vec<_> = rocks_keys.iter().map(|k| &k[..]).collect();
            let is_perf_enabled = maybe_enable_rocksdb_perf(
                self.column_options.rocks_perf_sample_interval,
                &self.read_perf_status,
            );
            let result = self
                .backend
                .multi_get_cf(self.handle(), ref_rocks_keys)
                .into_iter()
                .map(|r| match r {
                    Ok(opt) => match opt {
                        Some(pinnable_slice) => Ok(Some(deserialize(pinnable_slice.as_ref())?)),
                        None => Ok(None),
                    },
                    Err(e) => Err(e),
                })
                .collect::<Vec<Result<Option<_>>>>();
            if let Some(op_start_instant) = is_perf_enabled {
                // use multi-get instead
                report_rocksdb_read_perf(
                    C::NAME,
                    PERF_METRIC_OP_NAME_MULTI_GET,
                    &op_start_instant.elapsed(),
                    &self.column_options,
                );
            }

            result
        }
    }

    pub fn get(&self, key: C::Index) -> Result<Option<C::Type>> {
        let mut result = Ok(None);
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.read_perf_status,
        );
        if let Some(pinnable_slice) = self.backend.get_pinned_cf(self.handle(), &C::key(key))? {
            let value = deserialize(pinnable_slice.as_ref())?;
            result = Ok(Some(value))
        }

        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_read_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_GET,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }
        result
    }

    pub fn put(&self, key: C::Index, value: &C::Type) -> Result<()> {
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.write_perf_status,
        );
        let serialized_value = serialize(value)?;

        let result = self
            .backend
            .put_cf(self.handle(), &C::key(key), &serialized_value);

        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_write_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_PUT,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }
        result
    }

    pub fn delete(&self, key: C::Index) -> Result<()> {
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.write_perf_status,
        );
        let result = self.backend.delete_cf(self.handle(), &C::key(key));
        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_write_perf(
                C::NAME,
                "delete",
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }
        result
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
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.read_perf_status,
        );
        let result = self.backend.get_pinned_cf(self.handle(), &C::key(key));
        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_read_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_GET,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }

        if let Some(pinnable_slice) = result? {
            let value = match C::Type::decode(pinnable_slice.as_ref()) {
                Ok(value) => value,
                Err(_) => deserialize::<T>(pinnable_slice.as_ref())?.into(),
            };
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn get_protobuf(&self, key: C::Index) -> Result<Option<C::Type>> {
        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.read_perf_status,
        );
        let result = self.backend.get_pinned_cf(self.handle(), &C::key(key));
        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_read_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_GET,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }

        if let Some(pinnable_slice) = result? {
            Ok(Some(C::Type::decode(pinnable_slice.as_ref())?))
        } else {
            Ok(None)
        }
    }

    pub fn put_protobuf(&self, key: C::Index, value: &C::Type) -> Result<()> {
        let mut buf = Vec::with_capacity(value.encoded_len());
        value.encode(&mut buf)?;

        let is_perf_enabled = maybe_enable_rocksdb_perf(
            self.column_options.rocks_perf_sample_interval,
            &self.write_perf_status,
        );
        let result = self.backend.put_cf(self.handle(), &C::key(key), &buf);
        if let Some(op_start_instant) = is_perf_enabled {
            report_rocksdb_write_perf(
                C::NAME,
                PERF_METRIC_OP_NAME_PUT,
                &op_start_instant.elapsed(),
                &self.column_options,
            );
        }

        result
    }
}

impl<'a> WriteBatch<'a> {
    pub fn put_bytes<C: Column + ColumnName>(&mut self, key: C::Index, bytes: &[u8]) -> Result<()> {
        self.write_batch
            .put_cf(self.get_cf::<C>(), C::key(key), bytes);
        Ok(())
    }

    pub fn delete<C: Column + ColumnName>(&mut self, key: C::Index) -> Result<()> {
        self.write_batch.delete_cf(self.get_cf::<C>(), C::key(key));
        Ok(())
    }

    pub fn put<C: TypedColumn + ColumnName>(
        &mut self,
        key: C::Index,
        value: &C::Type,
    ) -> Result<()> {
        let serialized_value = serialize(&value)?;
        self.write_batch
            .put_cf(self.get_cf::<C>(), C::key(key), serialized_value);
        Ok(())
    }

    #[inline]
    fn get_cf<C: Column + ColumnName>(&self) -> &'a ColumnFamily {
        self.map[C::NAME]
    }

    /// Adds a \[`from`, `to`) range deletion entry to the batch.
    ///
    /// Note that the \[`from`, `to`) deletion range of WriteBatch::delete_range_cf
    /// is different from \[`from`, `to`\] of Database::delete_range_cf as we makes
    /// the semantics of Database::delete_range_cf matches the blockstore purge
    /// logic.
    fn delete_range_cf<C: Column>(
        &mut self,
        cf: &ColumnFamily,
        from: C::Index,
        to: C::Index, // exclusive
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

    process_cf_options_advanced::<C>(&mut cf_options, &options.column_options);

    cf_options
}

fn process_cf_options_advanced<C: 'static + Column + ColumnName>(
    cf_options: &mut Options,
    column_options: &LedgerColumnOptions,
) {
    if should_enable_compression::<C>() {
        cf_options.set_compression_type(
            column_options
                .compression_type
                .to_rocksdb_compression_type(),
        );
    }
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
            new_cf_descriptor_fifo::<D>(&fifo_options.shred_data_cf_size, &options.column_options),
            new_cf_descriptor_fifo::<C>(&fifo_options.shred_code_cf_size, &options.column_options),
        ),
    }
}

fn new_cf_descriptor_fifo<C: 'static + Column + ColumnName>(
    max_cf_size: &u64,
    column_options: &LedgerColumnOptions,
) -> ColumnFamilyDescriptor {
    if *max_cf_size > FIFO_WRITE_BUFFER_SIZE {
        ColumnFamilyDescriptor::new(
            C::NAME,
            get_cf_options_fifo::<C>(max_cf_size, column_options),
        )
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
fn get_cf_options_fifo<C: 'static + Column + ColumnName>(
    max_cf_size: &u64,
    column_options: &LedgerColumnOptions,
) -> Options {
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

    process_cf_options_advanced::<C>(&mut options, column_options);

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
    // Leave automatic compactions enabled (do not disable) in Primary mode;
    // disable in all other modes to prevent accidental cleaning
    !matches!(access_type, AccessType::Primary)
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

// Returns true if the column family enables compression.
fn should_enable_compression<C: 'static + Column + ColumnName>() -> bool {
    C::NAME == columns::TransactionStatus::NAME
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
    fn test_should_disable_auto_compactions() {
        assert!(!should_disable_auto_compactions(&AccessType::Primary));
        assert!(should_disable_auto_compactions(
            &AccessType::PrimaryForMaintenance
        ));
        assert!(should_disable_auto_compactions(&AccessType::Secondary));
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
