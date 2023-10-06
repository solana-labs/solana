use {
    crate::{
        bank::{Bank, BankFieldsToDeserialize, BankRc},
        builtins::BuiltinPrototype,
        epoch_stakes::EpochStakes,
        runtime_config::RuntimeConfig,
        serde_snapshot::storage::SerializableAccountStorageEntry,
        snapshot_utils::{
            self, SnapshotError, StorageAndNextAppendVecId, BANK_SNAPSHOT_PRE_FILENAME_EXTENSION,
        },
        stakes::Stakes,
    },
    bincode::{self, config::Options, Error},
    log::*,
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    solana_accounts_db::{
        account_storage::meta::StoredMetaWriteVersion,
        accounts::Accounts,
        accounts_db::{
            AccountShrinkThreshold, AccountStorageEntry, AccountsDb, AccountsDbConfig, AppendVecId,
            AtomicAppendVecId, BankHashStats, IndexGenerationInfo,
        },
        accounts_file::AccountsFile,
        accounts_hash::AccountsHash,
        accounts_index::AccountSecondaryIndexes,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        blockhash_queue::BlockhashQueue,
        epoch_accounts_hash::EpochAccountsHash,
        rent_collector::RentCollector,
    },
    solana_measure::measure::Measure,
    solana_sdk::{
        clock::{Epoch, Slot, UnixTimestamp},
        deserialize_utils::default_on_eof,
        epoch_schedule::EpochSchedule,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        genesis_config::GenesisConfig,
        hard_forks::HardForks,
        hash::Hash,
        inflation::Inflation,
        pubkey::Pubkey,
    },
    std::{
        collections::{HashMap, HashSet},
        io::{self, BufReader, BufWriter, Read, Write},
        path::{Path, PathBuf},
        result::Result,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread::Builder,
    },
    storage::SerializableStorage,
};

mod newer;
mod storage;
mod tests;
mod utils;

pub(crate) use {
    solana_accounts_db::accounts_hash::{
        SerdeAccountsDeltaHash, SerdeAccountsHash, SerdeIncrementalAccountsHash,
    },
    storage::SerializedAppendVecId,
};

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum SerdeStyle {
    Newer,
}

const MAX_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, AbiExample, PartialEq, Eq)]
pub struct AccountsDbFields<T>(
    HashMap<Slot, Vec<T>>,
    StoredMetaWriteVersion,
    Slot,
    BankHashInfo,
    /// all slots that were roots within the last epoch
    #[serde(deserialize_with = "default_on_eof")]
    Vec<Slot>,
    /// slots that were roots within the last epoch for which we care about the hash value
    #[serde(deserialize_with = "default_on_eof")]
    Vec<(Slot, Hash)>,
);

/// Incremental snapshots only calculate their accounts hash based on the
/// account changes WITHIN the incremental slot range. So, we need to keep track
/// of the full snapshot expected accounts hash results. We also need to keep
/// track of the hash and capitalization specific to the incremental snapshot
/// slot range. The capitalization we calculate for the incremental slot will
/// NOT be consistent with the bank's capitalization. It is not feasible to
/// calculate a capitalization delta that is correct given just incremental
/// slots account data and the full snapshot's capitalization.
#[derive(Serialize, Deserialize, AbiExample, Clone, Debug, Default, PartialEq, Eq)]
pub struct BankIncrementalSnapshotPersistence {
    /// slot of full snapshot
    pub full_slot: Slot,
    /// accounts hash from the full snapshot
    pub full_hash: SerdeAccountsHash,
    /// capitalization from the full snapshot
    pub full_capitalization: u64,
    /// hash of the accounts in the incremental snapshot slot range, including zero-lamport accounts
    pub incremental_hash: SerdeIncrementalAccountsHash,
    /// capitalization of the accounts in the incremental snapshot slot range
    pub incremental_capitalization: u64,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, AbiExample)]
struct BankHashInfo {
    accounts_delta_hash: SerdeAccountsDeltaHash,
    accounts_hash: SerdeAccountsHash,
    stats: BankHashStats,
}

/// Helper type to wrap BufReader streams when deserializing and reconstructing from either just a
/// full snapshot, or both a full and incremental snapshot
pub struct SnapshotStreams<'a, R> {
    pub full_snapshot_stream: &'a mut BufReader<R>,
    pub incremental_snapshot_stream: Option<&'a mut BufReader<R>>,
}

/// Helper type to wrap BankFields when reconstructing Bank from either just a full
/// snapshot, or both a full and incremental snapshot
#[derive(Debug)]
pub struct SnapshotBankFields {
    full: BankFieldsToDeserialize,
    incremental: Option<BankFieldsToDeserialize>,
}

impl SnapshotBankFields {
    /// Collapse the SnapshotBankFields into a single (the latest) BankFieldsToDeserialize.
    pub fn collapse_into(self) -> BankFieldsToDeserialize {
        self.incremental.unwrap_or(self.full)
    }
}

/// Helper type to wrap AccountsDbFields when reconstructing AccountsDb from either just a full
/// snapshot, or both a full and incremental snapshot
#[derive(Debug)]
pub struct SnapshotAccountsDbFields<T> {
    full_snapshot_accounts_db_fields: AccountsDbFields<T>,
    incremental_snapshot_accounts_db_fields: Option<AccountsDbFields<T>>,
}

impl<T> SnapshotAccountsDbFields<T> {
    /// Collapse the SnapshotAccountsDbFields into a single AccountsDbFields.  If there is no
    /// incremental snapshot, this returns the AccountsDbFields from the full snapshot.
    /// Otherwise, use the AccountsDbFields from the incremental snapshot, and a combination
    /// of the storages from both the full and incremental snapshots.
    fn collapse_into(self) -> Result<AccountsDbFields<T>, Error> {
        match self.incremental_snapshot_accounts_db_fields {
            None => Ok(self.full_snapshot_accounts_db_fields),
            Some(AccountsDbFields(
                mut incremental_snapshot_storages,
                incremental_snapshot_version,
                incremental_snapshot_slot,
                incremental_snapshot_bank_hash_info,
                incremental_snapshot_historical_roots,
                incremental_snapshot_historical_roots_with_hash,
            )) => {
                let full_snapshot_storages = self.full_snapshot_accounts_db_fields.0;
                let full_snapshot_slot = self.full_snapshot_accounts_db_fields.2;

                // filter out incremental snapshot storages with slot <= full snapshot slot
                incremental_snapshot_storages.retain(|slot, _| *slot > full_snapshot_slot);

                // There must not be any overlap in the slots of storages between the full snapshot and the incremental snapshot
                incremental_snapshot_storages
                    .iter()
                    .all(|storage_entry| !full_snapshot_storages.contains_key(storage_entry.0)).then_some(()).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "Snapshots are incompatible: There are storages for the same slot in both the full snapshot and the incremental snapshot!")
                    })?;

                let mut combined_storages = full_snapshot_storages;
                combined_storages.extend(incremental_snapshot_storages);

                Ok(AccountsDbFields(
                    combined_storages,
                    incremental_snapshot_version,
                    incremental_snapshot_slot,
                    incremental_snapshot_bank_hash_info,
                    incremental_snapshot_historical_roots,
                    incremental_snapshot_historical_roots_with_hash,
                ))
            }
        }
    }
}

trait TypeContext<'a>: PartialEq {
    type SerializableAccountStorageEntry: Serialize
        + DeserializeOwned
        + From<&'a AccountStorageEntry>
        + SerializableStorage
        + Sync;

    fn serialize_bank_and_storage<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankAndStorage<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized;

    #[cfg(test)]
    fn serialize_bank_and_storage_without_extra_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankAndStorageNoExtra<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized;

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDb<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized;

    fn deserialize_bank_fields<R>(
        stream: &mut BufReader<R>,
    ) -> Result<
        (
            BankFieldsToDeserialize,
            AccountsDbFields<Self::SerializableAccountStorageEntry>,
        ),
        Error,
    >
    where
        R: Read;

    fn deserialize_accounts_db_fields<R>(
        stream: &mut BufReader<R>,
    ) -> Result<AccountsDbFields<Self::SerializableAccountStorageEntry>, Error>
    where
        R: Read;

    /// deserialize the bank from 'stream_reader'
    /// modify the accounts_hash
    /// reserialize the bank to 'stream_writer'
    fn reserialize_bank_fields_with_hash<R, W>(
        stream_reader: &mut BufReader<R>,
        stream_writer: &mut BufWriter<W>,
        accounts_hash: &AccountsHash,
        incremental_snapshot_persistence: Option<&BankIncrementalSnapshotPersistence>,
    ) -> std::result::Result<(), Box<bincode::ErrorKind>>
    where
        R: Read,
        W: Write;
}

fn deserialize_from<R, T>(reader: R) -> bincode::Result<T>
where
    R: Read,
    T: DeserializeOwned,
{
    bincode::options()
        .with_limit(MAX_STREAM_SIZE)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize_from::<R, T>(reader)
}

/// used by tests to compare contents of serialized bank fields
/// serialized format is not deterministic - likely due to randomness in structs like hashmaps
#[cfg(feature = "dev-context-only-utils")]
pub(crate) fn compare_two_serialized_banks(
    path1: impl AsRef<Path>,
    path2: impl AsRef<Path>,
) -> std::result::Result<bool, Error> {
    use std::fs::File;
    let file1 = File::open(path1)?;
    let mut stream1 = BufReader::new(file1);
    let file2 = File::open(path2)?;
    let mut stream2 = BufReader::new(file2);

    let fields1 = newer::Context::deserialize_bank_fields(&mut stream1)?;
    let fields2 = newer::Context::deserialize_bank_fields(&mut stream2)?;
    Ok(fields1 == fields2)
}

/// Get snapshot storage lengths from accounts_db_fields
pub(crate) fn snapshot_storage_lengths_from_fields(
    accounts_db_fields: &AccountsDbFields<SerializableAccountStorageEntry>,
) -> HashMap<Slot, HashMap<SerializedAppendVecId, usize>> {
    let AccountsDbFields(snapshot_storage, ..) = &accounts_db_fields;
    snapshot_storage
        .iter()
        .map(|(slot, slot_storage)| {
            (
                *slot,
                slot_storage
                    .iter()
                    .map(|storage_entry| (storage_entry.id(), storage_entry.current_len()))
                    .collect(),
            )
        })
        .collect()
}

pub(crate) fn fields_from_stream<R: Read>(
    serde_style: SerdeStyle,
    snapshot_stream: &mut BufReader<R>,
) -> std::result::Result<
    (
        BankFieldsToDeserialize,
        AccountsDbFields<SerializableAccountStorageEntry>,
    ),
    Error,
> {
    match serde_style {
        SerdeStyle::Newer => newer::Context::deserialize_bank_fields(snapshot_stream),
    }
}

pub(crate) fn fields_from_streams(
    serde_style: SerdeStyle,
    snapshot_streams: &mut SnapshotStreams<impl Read>,
) -> std::result::Result<
    (
        SnapshotBankFields,
        SnapshotAccountsDbFields<SerializableAccountStorageEntry>,
    ),
    Error,
> {
    let (full_snapshot_bank_fields, full_snapshot_accounts_db_fields) =
        fields_from_stream(serde_style, snapshot_streams.full_snapshot_stream)?;
    let (incremental_snapshot_bank_fields, incremental_snapshot_accounts_db_fields) =
        snapshot_streams
            .incremental_snapshot_stream
            .as_mut()
            .map(|stream| fields_from_stream(serde_style, stream))
            .transpose()?
            .unzip();

    let snapshot_bank_fields = SnapshotBankFields {
        full: full_snapshot_bank_fields,
        incremental: incremental_snapshot_bank_fields,
    };
    let snapshot_accounts_db_fields = SnapshotAccountsDbFields {
        full_snapshot_accounts_db_fields,
        incremental_snapshot_accounts_db_fields,
    };
    Ok((snapshot_bank_fields, snapshot_accounts_db_fields))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bank_from_streams<R>(
    serde_style: SerdeStyle,
    snapshot_streams: &mut SnapshotStreams<R>,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> std::result::Result<Bank, Error>
where
    R: Read,
{
    let (bank_fields, accounts_db_fields) = fields_from_streams(serde_style, snapshot_streams)?;
    reconstruct_bank_from_fields(
        bank_fields,
        accounts_db_fields,
        genesis_config,
        runtime_config,
        account_paths,
        storage_and_next_append_vec_id,
        debug_keys,
        additional_builtins,
        account_secondary_indexes,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    )
}

pub(crate) fn bank_to_stream<W>(
    serde_style: SerdeStyle,
    stream: &mut BufWriter<W>,
    bank: &Bank,
    snapshot_storages: &[Vec<Arc<AccountStorageEntry>>],
) -> Result<(), Error>
where
    W: Write,
{
    match serde_style {
        SerdeStyle::Newer => bincode::serialize_into(
            stream,
            &SerializableBankAndStorage::<newer::Context> {
                bank,
                snapshot_storages,
                phantom: std::marker::PhantomData,
            },
        ),
    }
}

#[cfg(test)]
pub(crate) fn bank_to_stream_no_extra_fields<W>(
    serde_style: SerdeStyle,
    stream: &mut BufWriter<W>,
    bank: &Bank,
    snapshot_storages: &[Vec<Arc<AccountStorageEntry>>],
) -> Result<(), Error>
where
    W: Write,
{
    match serde_style {
        SerdeStyle::Newer => bincode::serialize_into(
            stream,
            &SerializableBankAndStorageNoExtra::<newer::Context> {
                bank,
                snapshot_storages,
                phantom: std::marker::PhantomData,
            },
        ),
    }
}

/// deserialize the bank from 'stream_reader'
/// modify the accounts_hash
/// reserialize the bank to 'stream_writer'
fn reserialize_bank_fields_with_new_hash<W, R>(
    stream_reader: &mut BufReader<R>,
    stream_writer: &mut BufWriter<W>,
    accounts_hash: &AccountsHash,
    incremental_snapshot_persistence: Option<&BankIncrementalSnapshotPersistence>,
) -> Result<(), Error>
where
    W: Write,
    R: Read,
{
    newer::Context::reserialize_bank_fields_with_hash(
        stream_reader,
        stream_writer,
        accounts_hash,
        incremental_snapshot_persistence,
    )
}

/// effectively updates the accounts hash in the serialized bank file on disk
/// read serialized bank from pre file
/// update accounts_hash
/// write serialized bank to post file
/// return true if pre file found
pub fn reserialize_bank_with_new_accounts_hash(
    bank_snapshot_dir: impl AsRef<Path>,
    slot: Slot,
    accounts_hash: &AccountsHash,
    incremental_snapshot_persistence: Option<&BankIncrementalSnapshotPersistence>,
) -> bool {
    let bank_post = bank_snapshot_dir
        .as_ref()
        .join(snapshot_utils::get_snapshot_file_name(slot));
    let bank_pre = bank_post.with_extension(BANK_SNAPSHOT_PRE_FILENAME_EXTENSION);

    let mut found = false;
    {
        let file = std::fs::File::open(&bank_pre);
        // some tests don't create the file
        if let Ok(file) = file {
            found = true;
            let file_out = std::fs::File::create(bank_post).unwrap();
            reserialize_bank_fields_with_new_hash(
                &mut BufReader::new(file),
                &mut BufWriter::new(file_out),
                accounts_hash,
                incremental_snapshot_persistence,
            )
            .unwrap();
        }
    }
    if found {
        std::fs::remove_file(bank_pre).unwrap();
    }
    found
}

struct SerializableBankAndStorage<'a, C> {
    bank: &'a Bank,
    snapshot_storages: &'a [Vec<Arc<AccountStorageEntry>>],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, C: TypeContext<'a>> Serialize for SerializableBankAndStorage<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_bank_and_storage(serializer, self)
    }
}

#[cfg(test)]
pub fn serialize_test_bank_and_storage<S>(
    bank: &Bank,
    storage: &[Vec<Arc<AccountStorageEntry>>],
    s: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    (SerializableBankAndStorage::<newer::Context> {
        bank,
        snapshot_storages: storage,
        phantom: std::marker::PhantomData,
    })
    .serialize(s)
}

#[cfg(test)]
struct SerializableBankAndStorageNoExtra<'a, C> {
    bank: &'a Bank,
    snapshot_storages: &'a [Vec<Arc<AccountStorageEntry>>],
    phantom: std::marker::PhantomData<C>,
}

#[cfg(test)]
impl<'a, C: TypeContext<'a>> Serialize for SerializableBankAndStorageNoExtra<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_bank_and_storage_without_extra_fields(serializer, self)
    }
}

#[cfg(test)]
impl<'a, C> From<SerializableBankAndStorageNoExtra<'a, C>> for SerializableBankAndStorage<'a, C> {
    fn from(s: SerializableBankAndStorageNoExtra<'a, C>) -> SerializableBankAndStorage<'a, C> {
        let SerializableBankAndStorageNoExtra {
            bank,
            snapshot_storages,
            phantom,
        } = s;
        SerializableBankAndStorage {
            bank,
            snapshot_storages,
            phantom,
        }
    }
}

struct SerializableAccountsDb<'a, C> {
    accounts_db: &'a AccountsDb,
    slot: Slot,
    account_storage_entries: &'a [Vec<Arc<AccountStorageEntry>>],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, C: TypeContext<'a>> Serialize for SerializableAccountsDb<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_accounts_db_fields(serializer, self)
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl<'a, C> solana_frozen_abi::abi_example::IgnoreAsHelper for SerializableAccountsDb<'a, C> {}

#[allow(clippy::too_many_arguments)]
fn reconstruct_bank_from_fields<E>(
    bank_fields: SnapshotBankFields,
    snapshot_accounts_db_fields: SnapshotAccountsDbFields<E>,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> Result<Bank, Error>
where
    E: SerializableStorage + std::marker::Sync,
{
    let capitalizations = (
        bank_fields.full.capitalization,
        bank_fields
            .incremental
            .as_ref()
            .map(|bank_fields| bank_fields.capitalization),
    );
    let bank_fields = bank_fields.collapse_into();
    let (accounts_db, reconstructed_accounts_db_info) = reconstruct_accountsdb_from_fields(
        snapshot_accounts_db_fields,
        account_paths,
        storage_and_next_append_vec_id,
        genesis_config,
        account_secondary_indexes,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
        bank_fields.epoch_accounts_hash,
        capitalizations,
        bank_fields.incremental_snapshot_persistence.as_ref(),
    )?;

    let bank_rc = BankRc::new(Accounts::new_empty(accounts_db), bank_fields.slot);
    let runtime_config = Arc::new(runtime_config.clone());

    // if limit_load_slot_count_from_snapshot is set, then we need to side-step some correctness checks beneath this call
    let debug_do_not_add_builtins = limit_load_slot_count_from_snapshot.is_some();
    let bank = Bank::new_from_fields(
        bank_rc,
        genesis_config,
        runtime_config,
        bank_fields,
        debug_keys,
        additional_builtins,
        debug_do_not_add_builtins,
        reconstructed_accounts_db_info.accounts_data_len,
    );

    info!("rent_collector: {:?}", bank.rent_collector());

    Ok(bank)
}

pub(crate) fn reconstruct_single_storage(
    slot: &Slot,
    append_vec_path: &Path,
    current_len: usize,
    append_vec_id: AppendVecId,
) -> Result<Arc<AccountStorageEntry>, SnapshotError> {
    let (accounts_file, num_accounts) = AccountsFile::new_from_file(append_vec_path, current_len)?;
    Ok(Arc::new(AccountStorageEntry::new_existing(
        *slot,
        append_vec_id,
        accounts_file,
        num_accounts,
    )))
}

fn remap_append_vec_file(
    slot: Slot,
    old_append_vec_id: SerializedAppendVecId,
    append_vec_path: &Path,
    next_append_vec_id: &AtomicAppendVecId,
    num_collisions: &AtomicUsize,
) -> io::Result<(AppendVecId, PathBuf)> {
    // Remap the AppendVec ID to handle any duplicate IDs that may previously existed
    // due to full snapshots and incremental snapshots generated from different nodes
    let (remapped_append_vec_id, remapped_append_vec_path) = loop {
        let remapped_append_vec_id = next_append_vec_id.fetch_add(1, Ordering::AcqRel);
        let remapped_file_name = AccountsFile::file_name(slot, remapped_append_vec_id);
        let remapped_append_vec_path = append_vec_path.parent().unwrap().join(remapped_file_name);

        // Break out of the loop in the following situations:
        // 1. The new ID is the same as the original ID.  This means we do not need to
        //    rename the file, since the ID is the "correct" one already.
        // 2. There is not a file already at the new path.  This means it is safe to
        //    rename the file to this new path.
        //    **DEVELOPER NOTE:**  Keep this check last so that it can short-circuit if
        //    possible.
        if old_append_vec_id == remapped_append_vec_id as SerializedAppendVecId
            || std::fs::metadata(&remapped_append_vec_path).is_err()
        {
            break (remapped_append_vec_id, remapped_append_vec_path);
        }

        // If we made it this far, a file exists at the new path.  Record the collision
        // and try again.
        num_collisions.fetch_add(1, Ordering::Relaxed);
    };
    // Only rename the file if the new ID is actually different from the original.
    if old_append_vec_id != remapped_append_vec_id as SerializedAppendVecId {
        std::fs::rename(append_vec_path, &remapped_append_vec_path)?;
    }

    Ok((remapped_append_vec_id, remapped_append_vec_path))
}

pub(crate) fn remap_and_reconstruct_single_storage(
    slot: Slot,
    old_append_vec_id: SerializedAppendVecId,
    current_len: usize,
    append_vec_path: &Path,
    next_append_vec_id: &AtomicAppendVecId,
    num_collisions: &AtomicUsize,
) -> Result<Arc<AccountStorageEntry>, SnapshotError> {
    let (remapped_append_vec_id, remapped_append_vec_path) = remap_append_vec_file(
        slot,
        old_append_vec_id,
        append_vec_path,
        next_append_vec_id,
        num_collisions,
    )?;
    let storage = reconstruct_single_storage(
        &slot,
        &remapped_append_vec_path,
        current_len,
        remapped_append_vec_id,
    )?;
    Ok(storage)
}

/// This struct contains side-info while reconstructing the accounts DB from fields.
#[derive(Debug, Default, Copy, Clone)]
struct ReconstructedAccountsDbInfo {
    accounts_data_len: u64,
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_accountsdb_from_fields<E>(
    snapshot_accounts_db_fields: SnapshotAccountsDbFields<E>,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    genesis_config: &GenesisConfig,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
    epoch_accounts_hash: Option<Hash>,
    capitalizations: (u64, Option<u64>),
    incremental_snapshot_persistence: Option<&BankIncrementalSnapshotPersistence>,
) -> Result<(AccountsDb, ReconstructedAccountsDbInfo), Error>
where
    E: SerializableStorage + std::marker::Sync,
{
    let mut accounts_db = AccountsDb::new_with_config(
        account_paths.to_vec(),
        &genesis_config.cluster_type,
        account_secondary_indexes,
        shrink_ratio,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    );

    if let Some(epoch_accounts_hash) = epoch_accounts_hash {
        accounts_db
            .epoch_accounts_hash_manager
            .set_valid(EpochAccountsHash::new(epoch_accounts_hash), 0);
    }

    // Store the accounts hash & capitalization, from the full snapshot, in the new AccountsDb
    {
        let AccountsDbFields(_, _, slot, bank_hash_info, _, _) =
            &snapshot_accounts_db_fields.full_snapshot_accounts_db_fields;

        if let Some(incremental_snapshot_persistence) = incremental_snapshot_persistence {
            // If we've booted from local state that was originally intended to be an incremental
            // snapshot, then we will use the incremental snapshot persistence field to set the
            // initial accounts hashes in accounts db.
            let old_accounts_hash = accounts_db.set_accounts_hash_from_snapshot(
                incremental_snapshot_persistence.full_slot,
                incremental_snapshot_persistence.full_hash.clone(),
                incremental_snapshot_persistence.full_capitalization,
            );
            assert!(
                old_accounts_hash.is_none(),
                "There should not already be an AccountsHash at slot {slot}: {old_accounts_hash:?}",
            );
            let old_incremental_accounts_hash = accounts_db
                .set_incremental_accounts_hash_from_snapshot(
                    *slot,
                    incremental_snapshot_persistence.incremental_hash.clone(),
                    incremental_snapshot_persistence.incremental_capitalization,
                );
            assert!(
                old_incremental_accounts_hash.is_none(),
                "There should not already be an IncrementalAccountsHash at slot {slot}: {old_incremental_accounts_hash:?}",
            );
        } else {
            // Otherwise, we've booted from a snapshot archive, or from local state that was *not*
            // intended to be an incremental snapshot.
            let old_accounts_hash = accounts_db.set_accounts_hash_from_snapshot(
                *slot,
                bank_hash_info.accounts_hash.clone(),
                capitalizations.0,
            );
            assert!(
                old_accounts_hash.is_none(),
                "There should not already be an AccountsHash at slot {slot}: {old_accounts_hash:?}",
            );
        }
    }

    // Store the accounts hash & capitalization, from the incremental snapshot, in the new AccountsDb
    {
        if let Some(AccountsDbFields(_, _, slot, bank_hash_info, _, _)) =
            snapshot_accounts_db_fields
                .incremental_snapshot_accounts_db_fields
                .as_ref()
        {
            if let Some(incremental_snapshot_persistence) = incremental_snapshot_persistence {
                // Use the presence of a BankIncrementalSnapshotPersistence to indicate the
                // Incremental Accounts Hash feature is enabled, and use its accounts hashes
                // instead of `BankHashInfo`'s.
                let AccountsDbFields(_, _, full_slot, full_bank_hash_info, _, _) =
                    &snapshot_accounts_db_fields.full_snapshot_accounts_db_fields;
                let full_accounts_hash = &full_bank_hash_info.accounts_hash;
                assert_eq!(
                    incremental_snapshot_persistence.full_slot, *full_slot,
                    "The incremental snapshot's base slot ({}) must match the full snapshot's slot ({full_slot})!",
                    incremental_snapshot_persistence.full_slot,
                );
                assert_eq!(
                    &incremental_snapshot_persistence.full_hash, full_accounts_hash,
                    "The incremental snapshot's base accounts hash ({}) must match the full snapshot's accounts hash ({})!",
                    &incremental_snapshot_persistence.full_hash.0, full_accounts_hash.0,
                );
                assert_eq!(
                    incremental_snapshot_persistence.full_capitalization, capitalizations.0,
                    "The incremental snapshot's base capitalization ({}) must match the full snapshot's capitalization ({})!",
                    incremental_snapshot_persistence.full_capitalization, capitalizations.0,
                );
                let old_incremental_accounts_hash = accounts_db
                    .set_incremental_accounts_hash_from_snapshot(
                        *slot,
                        incremental_snapshot_persistence.incremental_hash.clone(),
                        incremental_snapshot_persistence.incremental_capitalization,
                    );
                assert!(
                    old_incremental_accounts_hash.is_none(),
                    "There should not already be an IncrementalAccountsHash at slot {slot}: {old_incremental_accounts_hash:?}",
                );
            } else {
                // ..and without a BankIncrementalSnapshotPersistence then the Incremental Accounts
                // Hash feature is disabled; the accounts hash in `BankHashInfo` is valid.
                let old_accounts_hash = accounts_db.set_accounts_hash_from_snapshot(
                    *slot,
                    bank_hash_info.accounts_hash.clone(),
                    capitalizations
                        .1
                        .expect("capitalization from incremental snapshot"),
                );
                assert!(
                    old_accounts_hash.is_none(),
                    "There should not already be an AccountsHash at slot {slot}: {old_accounts_hash:?}",
                );
            };
        }
    }

    let AccountsDbFields(
        _snapshot_storages,
        snapshot_version,
        snapshot_slot,
        snapshot_bank_hash_info,
        _snapshot_historical_roots,
        _snapshot_historical_roots_with_hash,
    ) = snapshot_accounts_db_fields.collapse_into()?;

    // Ensure all account paths exist
    for path in &accounts_db.paths {
        std::fs::create_dir_all(path)
            .unwrap_or_else(|err| panic!("Failed to create directory {}: {}", path.display(), err));
    }

    let StorageAndNextAppendVecId {
        storage,
        next_append_vec_id,
    } = storage_and_next_append_vec_id;

    assert!(
        !storage.is_empty(),
        "At least one storage entry must exist from deserializing stream"
    );

    let next_append_vec_id = next_append_vec_id.load(Ordering::Acquire);
    let max_append_vec_id = next_append_vec_id - 1;
    assert!(
        max_append_vec_id <= AppendVecId::MAX / 2,
        "Storage id {max_append_vec_id} larger than allowed max"
    );

    // Process deserialized data, set necessary fields in self
    let old_accounts_delta_hash = accounts_db.set_accounts_delta_hash_from_snapshot(
        snapshot_slot,
        snapshot_bank_hash_info.accounts_delta_hash,
    );
    assert!(
        old_accounts_delta_hash.is_none(),
        "There should not already be an AccountsDeltaHash at slot {snapshot_slot}: {old_accounts_delta_hash:?}",
        );
    let old_stats = accounts_db
        .update_bank_hash_stats_from_snapshot(snapshot_slot, snapshot_bank_hash_info.stats);
    assert!(
        old_stats.is_none(),
        "There should not already be a BankHashStats at slot {snapshot_slot}: {old_stats:?}",
    );
    accounts_db.storage.initialize(storage);
    accounts_db
        .next_id
        .store(next_append_vec_id, Ordering::Release);
    accounts_db
        .write_version
        .fetch_add(snapshot_version, Ordering::Release);

    let mut measure_notify = Measure::start("accounts_notify");

    let accounts_db = Arc::new(accounts_db);
    let accounts_db_clone = accounts_db.clone();
    let handle = Builder::new()
        .name("solNfyAccRestor".to_string())
        .spawn(move || {
            accounts_db_clone.notify_account_restore_from_snapshot();
        })
        .unwrap();

    let IndexGenerationInfo {
        accounts_data_len,
        rent_paying_accounts_by_partition,
    } = accounts_db.generate_index(
        limit_load_slot_count_from_snapshot,
        verify_index,
        genesis_config,
    );
    accounts_db
        .accounts_index
        .rent_paying_accounts_by_partition
        .set(rent_paying_accounts_by_partition)
        .unwrap();

    accounts_db.maybe_add_filler_accounts(&genesis_config.epoch_schedule, snapshot_slot);

    handle.join().unwrap();
    measure_notify.stop();

    datapoint_info!(
        "reconstruct_accountsdb_from_fields()",
        ("accountsdb-notify-at-start-us", measure_notify.as_us(), i64),
    );

    Ok((
        Arc::try_unwrap(accounts_db).unwrap(),
        ReconstructedAccountsDbInfo { accounts_data_len },
    ))
}
