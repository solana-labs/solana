use {
    crate::{
        accounts::Accounts,
        accounts_db::{
            AccountShrinkThreshold, AccountStorageEntry, AccountsDb, AppendVecId, BankHashInfo,
        },
        accounts_index::AccountSecondaryIndexes,
        ancestors::Ancestors,
        append_vec::{AppendVec, StoredMetaWriteVersion},
        bank::{Bank, BankFieldsToDeserialize, BankRc, Builtins},
        blockhash_queue::BlockhashQueue,
        epoch_stakes::EpochStakes,
        hardened_unpack::UnpackedAppendVecMap,
        message_processor::MessageProcessor,
        rent_collector::RentCollector,
        serde_snapshot::future::SerializableStorage,
        stakes::Stakes,
    },
    bincode,
    bincode::{config::Options, Error},
    log::*,
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    solana_sdk::{
        clock::{Epoch, Slot, UnixTimestamp},
        epoch_schedule::EpochSchedule,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        genesis_config::ClusterType,
        genesis_config::GenesisConfig,
        hard_forks::HardForks,
        hash::Hash,
        inflation::Inflation,
        pubkey::Pubkey,
    },
    std::{
        collections::{HashMap, HashSet},
        io::{self, BufReader, BufWriter, Read, Write},
        path::PathBuf,
        result::Result,
        sync::{atomic::Ordering, Arc, RwLock},
        time::Instant,
    },
};

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::IgnoreAsHelper;

mod common;
mod future;
mod tests;
mod utils;

use future::Context as TypeContextFuture;
#[allow(unused_imports)]
use utils::{serialize_iter_as_map, serialize_iter_as_seq, serialize_iter_as_tuple};

// a number of test cases in accounts_db use this
#[cfg(test)]
pub(crate) use self::tests::reconstruct_accounts_db_via_serialization;

pub(crate) use crate::accounts_db::{SnapshotStorage, SnapshotStorages};

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum SerdeStyle {
    Newer,
}

const MAX_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, AbiExample)]
struct AccountsDbFields<T>(
    HashMap<Slot, Vec<T>>,
    StoredMetaWriteVersion,
    Slot,
    BankHashInfo,
);

trait TypeContext<'a> {
    type SerializableAccountStorageEntry: Serialize
        + DeserializeOwned
        + From<&'a AccountStorageEntry>
        + SerializableStorage;

    fn serialize_bank_and_storage<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankAndStorage<'a, Self>,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn bank_from_stream<R>(
    serde_style: SerdeStyle,
    stream: &mut BufReader<R>,
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    genesis_config: &GenesisConfig,
    frozen_account_pubkeys: &[Pubkey],
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> std::result::Result<Bank, Error>
where
    R: Read,
{
    macro_rules! INTO {
        ($x:ident) => {{
            let (bank_fields, accounts_db_fields) = $x::deserialize_bank_fields(stream)?;

            let bank = reconstruct_bank_from_fields(
                bank_fields,
                accounts_db_fields,
                genesis_config,
                frozen_account_pubkeys,
                account_paths,
                unpacked_append_vec_map,
                debug_keys,
                additional_builtins,
                account_indexes,
                caching_enabled,
                limit_load_slot_count_from_snapshot,
                shrink_ratio,
            )?;
            Ok(bank)
        }};
    }
    match serde_style {
        SerdeStyle::Newer => INTO!(TypeContextFuture),
    }
    .map_err(|err| {
        warn!("bankrc_from_stream error: {:?}", err);
        err
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bank_from_stream_incremental<R>(
    serde_style: SerdeStyle,
    fss_stream: &mut BufReader<R>,
    iss_stream: &mut BufReader<R>,
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    genesis_config: &GenesisConfig,
    frozen_account_pubkeys: &[Pubkey],
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> std::result::Result<Bank, Error>
where
    R: Read,
{
    macro_rules! INTO {
        ($x:ident) => {{
            let (_, fss_accounts_db_fields) = $x::deserialize_bank_fields(fss_stream)?;
            let (bank_fields, iss_accounts_db_fields) = $x::deserialize_bank_fields(iss_stream)?;

            let bank = reconstruct_bank_from_fields_incremental(
                bank_fields,
                fss_accounts_db_fields,
                iss_accounts_db_fields,
                genesis_config,
                frozen_account_pubkeys,
                account_paths,
                unpacked_append_vec_map,
                debug_keys,
                additional_builtins,
                account_indexes,
                caching_enabled,
                limit_load_slot_count_from_snapshot,
                shrink_ratio,
            )?;
            Ok(bank)
        }};
    }
    match serde_style {
        SerdeStyle::Newer => INTO!(TypeContextFuture),
    }
    .map_err(|err| {
        warn!("bankrc_from_stream error: {:?}", err);
        err
    })
}

pub(crate) fn bank_to_stream<W>(
    serde_style: SerdeStyle,
    stream: &mut BufWriter<W>,
    bank: &Bank,
    snapshot_storages: &[SnapshotStorage],
) -> Result<(), Error>
where
    W: Write,
{
    macro_rules! INTO {
        ($x:ident) => {
            bincode::serialize_into(
                stream,
                &SerializableBankAndStorage::<$x> {
                    bank,
                    snapshot_storages,
                    phantom: std::marker::PhantomData::default(),
                },
            )
        };
    }

    debug!(
        "bprumo DEBUG: bank_to_stream(), snapshot_storages: {:?}",
        snapshot_storages
    );
    match serde_style {
        SerdeStyle::Newer => INTO!(TypeContextFuture),
    }
    .map_err(|err| {
        warn!("bankrc_to_stream error: {:?}", err);
        err
    })
}

struct SerializableBankAndStorage<'a, C> {
    bank: &'a Bank,
    snapshot_storages: &'a [SnapshotStorage],
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

struct SerializableAccountsDb<'a, C> {
    accounts_db: &'a AccountsDb,
    slot: Slot,
    account_storage_entries: &'a [SnapshotStorage],
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
impl<'a, C> IgnoreAsHelper for SerializableAccountsDb<'a, C> {}

#[allow(clippy::too_many_arguments)]
fn reconstruct_bank_from_fields<E>(
    bank_fields: BankFieldsToDeserialize,
    accounts_db_fields: AccountsDbFields<E>,
    genesis_config: &GenesisConfig,
    frozen_account_pubkeys: &[Pubkey],
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<Bank, Error>
where
    E: SerializableStorage,
{
    let mut accounts_db = reconstruct_accountsdb_from_fields(
        accounts_db_fields,
        account_paths,
        unpacked_append_vec_map,
        &genesis_config.cluster_type,
        account_indexes,
        caching_enabled,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
    )?;
    accounts_db.freeze_accounts(
        &Ancestors::from(&bank_fields.ancestors),
        frozen_account_pubkeys,
    );

    let bank_rc = BankRc::new(Accounts::new_empty(accounts_db), bank_fields.slot);
    let bank = Bank::new_from_fields(
        bank_rc,
        genesis_config,
        bank_fields,
        debug_keys,
        additional_builtins,
    );

    Ok(bank)
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_bank_from_fields_incremental<E>(
    bank_fields: BankFieldsToDeserialize,
    fss_accounts_db_fields: AccountsDbFields<E>,
    iss_accounts_db_fields: AccountsDbFields<E>,
    genesis_config: &GenesisConfig,
    frozen_account_pubkeys: &[Pubkey],
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<Bank, Error>
where
    E: SerializableStorage,
{
    let mut accounts_db = reconstruct_accountsdb_from_fields_incremental(
        fss_accounts_db_fields,
        iss_accounts_db_fields,
        account_paths,
        unpacked_append_vec_map,
        &genesis_config.cluster_type,
        account_indexes,
        caching_enabled,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
    )?;
    accounts_db.freeze_accounts(
        &Ancestors::from(&bank_fields.ancestors),
        frozen_account_pubkeys,
    );

    let bank_rc = BankRc::new(Accounts::new_empty(accounts_db), bank_fields.slot);
    let bank = Bank::new_from_fields(
        bank_rc,
        genesis_config,
        bank_fields,
        debug_keys,
        additional_builtins,
    );

    Ok(bank)
}

fn reconstruct_accountsdb_from_fields<E>(
    accounts_db_fields: AccountsDbFields<E>,
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    cluster_type: &ClusterType,
    account_indexes: AccountSecondaryIndexes,
    caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<AccountsDb, Error>
where
    E: SerializableStorage,
{
    let mut accounts_db = AccountsDb::new_with_config(
        account_paths.to_vec(),
        cluster_type,
        account_indexes,
        caching_enabled,
        shrink_ratio,
    );
    let AccountsDbFields(storage, version, slot, bank_hash_info) = accounts_db_fields;
    accounts_db.set_last_full_snapshot_slot(slot);

    // Ensure all account paths exist
    for path in &accounts_db.paths {
        std::fs::create_dir_all(path)
            .unwrap_or_else(|err| panic!("Failed to create directory {}: {}", path.display(), err));
    }

    let mut last_log_update = Instant::now();
    let mut remaining_slots_to_process = storage.len();

    // Remap the deserialized AppendVec paths to point to correct local paths
    let mut storage = storage
        .into_iter()
        .map(|(slot, mut slot_storage)| {
            let now = Instant::now();
            if now.duration_since(last_log_update).as_secs() >= 10 {
                info!("{} slots remaining...", remaining_slots_to_process);
                last_log_update = now;
            }
            remaining_slots_to_process -= 1;

            let mut new_slot_storage = HashMap::new();
            for storage_entry in slot_storage.drain(..) {
                let file_name = AppendVec::file_name(slot, storage_entry.id());
                debug!("bprumo DEBUG: reconstruct_accountsdb_from_fields(): slot: {}, storage entry id: {}, appendvec filename: {:?}", slot, storage_entry.id(), file_name);

                let append_vec_path = unpacked_append_vec_map.get(&file_name).ok_or_else(|| {
                    debug!("bprumo DEBUG: reconstruct_accountsdb_from_fields(): failed to get append vec path! file_name: {:?}", file_name);
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{} not found in unpacked append vecs", file_name),
                    )
                })?;

                let (accounts, num_accounts) =
                    AppendVec::new_from_file(append_vec_path, storage_entry.current_len())?;
                let u_storage_entry = AccountStorageEntry::new_existing(
                    slot,
                    storage_entry.id(),
                    accounts,
                    num_accounts,
                );
                debug!("bprumo DEBUG: reconstruct_accountsdb_from_fields(): append vec path: {:?}", append_vec_path);

                new_slot_storage.insert(storage_entry.id(), Arc::new(u_storage_entry));
            }
            Ok((slot, new_slot_storage))
        })
        .collect::<Result<HashMap<Slot, _>, Error>>()?;

    // discard any slots with no storage entries
    // this can happen if a non-root slot was serialized
    // but non-root stores should not be included in the snapshot
    storage.retain(|_slot, stores| !stores.is_empty());

    accounts_db
        .bank_hashes
        .write()
        .unwrap()
        .insert(slot, bank_hash_info);

    // Process deserialized data, set necessary fields in self
    let max_id: usize = *storage
        .values()
        .flat_map(HashMap::keys)
        .max()
        .expect("At least one storage entry must exist from deserializing stream");

    {
        accounts_db.storage.0.extend(
            storage.into_iter().map(|(slot, slot_storage_entry)| {
                (slot, Arc::new(RwLock::new(slot_storage_entry)))
            }),
        );
    }

    if max_id > AppendVecId::MAX / 2 {
        panic!("Storage id {} larger than allowed max", max_id);
    }

    accounts_db.next_id.store(max_id + 1, Ordering::Relaxed);
    accounts_db
        .write_version
        .fetch_add(version, Ordering::Relaxed);
    accounts_db.generate_index(limit_load_slot_count_from_snapshot);
    Ok(accounts_db)
}

fn reconstruct_accountsdb_from_fields_incremental<E>(
    fss_accounts_db_fields: AccountsDbFields<E>,
    iss_accounts_db_fields: AccountsDbFields<E>,
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    cluster_type: &ClusterType,
    account_indexes: AccountSecondaryIndexes,
    caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<AccountsDb, Error>
where
    E: SerializableStorage,
{
    let mut accounts_db = AccountsDb::new_with_config(
        account_paths.to_vec(),
        cluster_type,
        account_indexes,
        caching_enabled,
        shrink_ratio,
    );
    let AccountsDbFields(fss_storage, _, fss_slot, _) = fss_accounts_db_fields;
    let AccountsDbFields(iss_storage, iss_version, iss_slot, iss_bank_hash_info) =
        iss_accounts_db_fields;
    // bprumo TODO: what should be done if the versions are different between fss and iss? Just
    // ignore, or error out?

    accounts_db.set_last_full_snapshot_slot(fss_slot);

    // Ensure all account paths exist
    for path in &accounts_db.paths {
        std::fs::create_dir_all(path)
            .unwrap_or_else(|err| panic!("Failed to create directory {}: {}", path.display(), err));
    }

    let mut last_log_update = Instant::now();
    let mut remaining_slots_to_process = fss_storage.len() + iss_storage.len();

    // Remap the deserialized AppendVec paths to point to correct local paths
    let mut storage = fss_storage.into_iter().chain(iss_storage.into_iter())
        .map(|(slot, mut slot_storage)| {
            let now = Instant::now();
            if now.duration_since(last_log_update).as_secs() >= 10 {
                info!("{} slots remaining...", remaining_slots_to_process);
                last_log_update = now;
            }
            remaining_slots_to_process -= 1;

            let mut new_slot_storage = HashMap::new();
            for storage_entry in slot_storage.drain(..) {
                let file_name = AppendVec::file_name(slot, storage_entry.id());

                let append_vec_path = unpacked_append_vec_map.get(&file_name).ok_or_else(|| {
                    debug!("bprumo DEBUG: reconstruct_accountsdb_from_fields_incremental(): failed to get append vec path! file_name: {:?}", file_name);
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{} not found in unpacked append vecs", file_name),
                    )
                })?;

                let (accounts, num_accounts) =
                    AppendVec::new_from_file(append_vec_path, storage_entry.current_len())?;
                let u_storage_entry = AccountStorageEntry::new_existing(
                    slot,
                    storage_entry.id(),
                    accounts,
                    num_accounts,
                );
                debug!("bprumo DEBUG: reconstruct_accountsdb_from_fields_incremental(): slot: {}, storage entry id: {}, append vec filename: {:?}, append vec path: {:?}", slot, storage_entry.id(), file_name, append_vec_path);

                new_slot_storage.insert(storage_entry.id(), Arc::new(u_storage_entry));
            }
            Ok((slot, new_slot_storage))
        })
        .collect::<Result<HashMap<Slot, _>, Error>>()?;

    // discard any slots with no storage entries
    // this can happen if a non-root slot was serialized
    // but non-root stores should not be included in the snapshot
    storage.retain(|_slot, stores| !stores.is_empty());

    accounts_db
        .bank_hashes
        .write()
        .unwrap()
        .insert(iss_slot, iss_bank_hash_info);

    // Process deserialized data, set necessary fields in self
    let max_id: usize = *storage
        .values()
        .flat_map(HashMap::keys)
        .max()
        .expect("At least one storage entry must exist from deserializing stream");

    {
        accounts_db.storage.0.extend(
            storage.into_iter().map(|(slot, slot_storage_entry)| {
                (slot, Arc::new(RwLock::new(slot_storage_entry)))
            }),
        );
    }

    if max_id > AppendVecId::MAX / 2 {
        panic!("Storage id {} larger than allowed max", max_id);
    }

    accounts_db.next_id.store(max_id + 1, Ordering::Relaxed);
    accounts_db
        .write_version
        .fetch_add(iss_version, Ordering::Relaxed);
    accounts_db.generate_index(limit_load_slot_count_from_snapshot);
    Ok(accounts_db)
}
