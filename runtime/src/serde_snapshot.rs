use {
    crate::{
        accounts::Accounts,
        accounts_db::{AccountStorageEntry, AccountsDB, AppendVecId, BankHashInfo},
        accounts_index::{AccountIndex, Ancestors},
        append_vec::AppendVec,
        bank::{Bank, BankFieldsToDeserialize, BankRc, Builtins},
        blockhash_queue::BlockhashQueue,
        epoch_stakes::EpochStakes,
        message_processor::MessageProcessor,
        rent_collector::RentCollector,
        stakes::Stakes,
    },
    bincode,
    bincode::{config::Options, Error},
    fs_extra::dir::CopyOptions,
    log::{info, warn},
    rand::{thread_rng, Rng},
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
        io::{BufReader, BufWriter, Read, Write},
        path::{Path, PathBuf},
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
    NEWER,
}

const MAX_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, AbiExample)]
struct AccountsDbFields<T>(HashMap<Slot, Vec<T>>, u64, Slot, BankHashInfo);

trait TypeContext<'a> {
    type SerializableAccountStorageEntry: Serialize
        + DeserializeOwned
        + From<&'a AccountStorageEntry>
        + Into<AccountStorageEntry>;

    fn serialize_bank_and_storage<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankAndStorage<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized;

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDB<'a, Self>,
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
pub(crate) fn bank_from_stream<R, P>(
    serde_style: SerdeStyle,
    stream: &mut BufReader<R>,
    append_vecs_path: P,
    account_paths: &[PathBuf],
    genesis_config: &GenesisConfig,
    frozen_account_pubkeys: &[Pubkey],
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: HashSet<AccountIndex>,
    caching_enabled: bool,
) -> std::result::Result<Bank, Error>
where
    R: Read,
    P: AsRef<Path>,
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
                append_vecs_path,
                debug_keys,
                additional_builtins,
                account_indexes,
                caching_enabled,
            )?;
            Ok(bank)
        }};
    }
    match serde_style {
        SerdeStyle::NEWER => INTO!(TypeContextFuture),
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
    match serde_style {
        SerdeStyle::NEWER => INTO!(TypeContextFuture),
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

struct SerializableAccountsDB<'a, C> {
    accounts_db: &'a AccountsDB,
    slot: Slot,
    account_storage_entries: &'a [SnapshotStorage],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, C: TypeContext<'a>> Serialize for SerializableAccountsDB<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_accounts_db_fields(serializer, self)
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl<'a, C> IgnoreAsHelper for SerializableAccountsDB<'a, C> {}

#[allow(clippy::too_many_arguments)]
fn reconstruct_bank_from_fields<E, P>(
    bank_fields: BankFieldsToDeserialize,
    accounts_db_fields: AccountsDbFields<E>,
    genesis_config: &GenesisConfig,
    frozen_account_pubkeys: &[Pubkey],
    account_paths: &[PathBuf],
    append_vecs_path: P,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: HashSet<AccountIndex>,
    caching_enabled: bool,
) -> Result<Bank, Error>
where
    E: Into<AccountStorageEntry>,
    P: AsRef<Path>,
{
    let mut accounts_db = reconstruct_accountsdb_from_fields(
        accounts_db_fields,
        account_paths,
        append_vecs_path,
        &genesis_config.cluster_type,
        account_indexes,
        caching_enabled,
    )?;
    accounts_db.freeze_accounts(&bank_fields.ancestors, frozen_account_pubkeys);

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

fn reconstruct_accountsdb_from_fields<E, P>(
    accounts_db_fields: AccountsDbFields<E>,
    account_paths: &[PathBuf],
    stream_append_vecs_path: P,
    cluster_type: &ClusterType,
    account_indexes: HashSet<AccountIndex>,
    caching_enabled: bool,
) -> Result<AccountsDB, Error>
where
    E: Into<AccountStorageEntry>,
    P: AsRef<Path>,
{
    let mut accounts_db = AccountsDB::new_with_config(
        account_paths.to_vec(),
        cluster_type,
        account_indexes,
        caching_enabled,
    );
    let AccountsDbFields(storage, version, slot, bank_hash_info) = accounts_db_fields;

    // convert to two level map of slot -> id -> account storage entry
    let storage = {
        let mut map = HashMap::new();
        for (slot, entries) in storage.into_iter() {
            let sub_map = map.entry(slot).or_insert_with(HashMap::new);
            for entry in entries.into_iter() {
                let entry: AccountStorageEntry = entry.into();
                entry.slot.store(slot, Ordering::Relaxed);
                sub_map.insert(entry.append_vec_id(), Arc::new(entry));
            }
        }
        map
    };

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
            for (id, storage_entry) in slot_storage.drain() {
                let path_index = thread_rng().gen_range(0, accounts_db.paths.len());
                let local_dir = &accounts_db.paths[path_index];

                // Move the corresponding AppendVec from the snapshot into the directory pointed
                // at by `local_dir`
                let append_vec_relative_path =
                    AppendVec::new_relative_path(slot, storage_entry.append_vec_id());
                let append_vec_abs_path = stream_append_vecs_path
                    .as_ref()
                    .join(&append_vec_relative_path);
                let target = local_dir.join(append_vec_abs_path.file_name().unwrap());
                std::fs::rename(append_vec_abs_path.clone(), target).or_else(|_| {
                    let mut copy_options = CopyOptions::new();
                    copy_options.overwrite = true;
                    fs_extra::move_items(&vec![&append_vec_abs_path], &local_dir, &copy_options)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                        .and(Ok(()))
                })?;

                // Notify the AppendVec of the new file location
                let local_path = local_dir.join(append_vec_relative_path);
                let mut u_storage_entry = Arc::try_unwrap(storage_entry).unwrap();
                u_storage_entry.set_file(local_path)?;
                new_slot_storage.insert(id, Arc::new(u_storage_entry));
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
    accounts_db.generate_index();
    Ok(accounts_db)
}
