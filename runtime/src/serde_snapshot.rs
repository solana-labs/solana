use {
    crate::{
        accounts::Accounts,
        accounts_db::{
            AccountStorageEntry, AccountStorageStatus, AccountsDB, AppendVecId, BankHashInfo,
        },
        append_vec::AppendVec,
        bank::BankRc,
    },
    bincode::{deserialize_from, serialize_into},
    fs_extra::dir::CopyOptions,
    log::{info, warn},
    rand::{thread_rng, Rng},
    serde::{
        de::{DeserializeOwned, Visitor},
        Deserialize, Deserializer, Serialize, Serializer,
    },
    solana_sdk::clock::Slot,
    std::{
        cmp::min,
        collections::HashMap,
        fmt::{Formatter, Result as FormatResult},
        io::{
            BufReader, BufWriter, Cursor, Error as IoError, ErrorKind as IoErrorKind, Read, Write,
        },
        path::{Path, PathBuf},
        result::Result,
        sync::{atomic::Ordering, Arc},
    },
};

mod future;
mod legacy;
mod tests;
mod utils;

use future::Context as TypeContextFuture;
use legacy::Context as TypeContextLegacy;
#[allow(unused_imports)]
use utils::{serialize_iter_as_map, serialize_iter_as_seq, serialize_iter_as_tuple};

// a number of test cases in accounts_db use this
#[cfg(test)]
pub(crate) use self::tests::reconstruct_accounts_db_via_serialization;

pub use crate::accounts_db::{SnapshotStorage, SnapshotStorages};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SerdeStyle {
    NEWER,
    OLDER,
}

const MAX_ACCOUNTS_DB_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccountDBFields<T>(HashMap<Slot, Vec<T>>, u64, Slot, BankHashInfo);

pub trait TypeContext<'a> {
    type SerializableAccountStorageEntry: Serialize
        + DeserializeOwned
        + From<&'a AccountStorageEntry>
        + Into<AccountStorageEntry>;

    fn serialize_bank_rc_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankRc<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized;

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDB<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized;

    fn deserialize_accounts_db_fields<R>(
        stream: &mut BufReader<R>,
    ) -> Result<AccountDBFields<Self::SerializableAccountStorageEntry>, IoError>
    where
        R: Read;

    // we might define fn (de)serialize_bank(...) -> Result<Bank,...> for versionized bank serialization in the future
}

fn bankrc_to_io_error<T: ToString>(error: T) -> IoError {
    let msg = error.to_string();
    warn!("BankRc error: {:?}", msg);
    IoError::new(IoErrorKind::Other, msg)
}

fn accountsdb_to_io_error<T: ToString>(error: T) -> IoError {
    let msg = error.to_string();
    warn!("AccountsDB error: {:?}", msg);
    IoError::new(IoErrorKind::Other, msg)
}

pub fn bankrc_from_stream<R, P>(
    serde_style: SerdeStyle,
    account_paths: &[PathBuf],
    slot: Slot,
    stream: &mut BufReader<R>,
    stream_append_vecs_path: P,
) -> std::result::Result<BankRc, IoError>
where
    R: Read,
    P: AsRef<Path>,
{
    macro_rules! INTO {
        ($x:ident) => {
            Ok(BankRc::new(
                Accounts::new_empty(context_accountsdb_from_fields::<$x, P>(
                    $x::deserialize_accounts_db_fields(stream)?,
                    account_paths,
                    stream_append_vecs_path,
                )?),
                slot,
            ))
        };
    }
    match serde_style {
        SerdeStyle::NEWER => INTO!(TypeContextFuture),
        SerdeStyle::OLDER => INTO!(TypeContextLegacy),
    }
}

pub fn bankrc_to_stream<W>(
    serde_style: SerdeStyle,
    stream: &mut BufWriter<W>,
    bank_rc: &BankRc,
    snapshot_storages: &[SnapshotStorage],
) -> Result<(), IoError>
where
    W: Write,
{
    macro_rules! INTO {
        ($x:ident) => {
            serialize_into(
                stream,
                &SerializableBankRc::<$x> {
                    bank_rc,
                    snapshot_storages,
                    phantom: std::marker::PhantomData::default(),
                },
            )
            .map_err(bankrc_to_io_error)
        };
    }
    match serde_style {
        SerdeStyle::NEWER => INTO!(TypeContextFuture),
        SerdeStyle::OLDER => INTO!(TypeContextLegacy),
    }
}

pub struct SerializableBankRc<'a, C> {
    bank_rc: &'a BankRc,
    snapshot_storages: &'a [SnapshotStorage],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, C: TypeContext<'a>> Serialize for SerializableBankRc<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_bank_rc_fields(serializer, self)
    }
}

pub struct SerializableAccountsDB<'a, C> {
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

fn context_accountsdb_from_fields<'a, C, P>(
    account_db_fields: AccountDBFields<C::SerializableAccountStorageEntry>,
    account_paths: &[PathBuf],
    stream_append_vecs_path: P,
) -> Result<AccountsDB, IoError>
where
    C: TypeContext<'a>,
    P: AsRef<Path>,
{
    let accounts_db = AccountsDB::new(account_paths.to_vec());

    let AccountDBFields(storage, version, slot, bank_hash_info) = account_db_fields;

    // convert to two level map of slot -> id -> account storage entry
    let storage = {
        let mut map = HashMap::new();
        for (slot, entries) in storage.into_iter() {
            let sub_map = map.entry(slot).or_insert_with(HashMap::new);
            for entry in entries.into_iter() {
                let mut entry: AccountStorageEntry = entry.into();
                entry.slot = slot;
                sub_map.insert(entry.id, Arc::new(entry));
            }
        }
        map
    };

    // Remap the deserialized AppendVec paths to point to correct local paths
    let mut storage = storage
        .into_iter()
        .map(|(slot, mut slot_storage)| {
            let mut new_slot_storage = HashMap::new();
            for (id, storage_entry) in slot_storage.drain() {
                let path_index = thread_rng().gen_range(0, accounts_db.paths.len());
                let local_dir = &accounts_db.paths[path_index];

                std::fs::create_dir_all(local_dir).expect("Create directory failed");

                // Move the corresponding AppendVec from the snapshot into the directory pointed
                // at by `local_dir`
                let append_vec_relative_path = AppendVec::new_relative_path(slot, storage_entry.id);
                let append_vec_abs_path = stream_append_vecs_path
                    .as_ref()
                    .join(&append_vec_relative_path);
                let target = local_dir.join(append_vec_abs_path.file_name().unwrap());
                if std::fs::rename(append_vec_abs_path.clone(), target).is_err() {
                    let mut copy_options = CopyOptions::new();
                    copy_options.overwrite = true;
                    let e = fs_extra::move_items(
                        &vec![&append_vec_abs_path],
                        &local_dir,
                        &copy_options,
                    )
                    .map_err(|e| {
                        format!(
                            "unable to move {:?} to {:?}: {}",
                            append_vec_abs_path, local_dir, e
                        )
                    })
                    .map_err(accountsdb_to_io_error);
                    if e.is_err() {
                        info!("{:?}", e);
                        continue;
                    }
                };

                // Notify the AppendVec of the new file location
                let local_path = local_dir.join(append_vec_relative_path);
                let mut u_storage_entry = Arc::try_unwrap(storage_entry).unwrap();
                u_storage_entry
                    .set_file(local_path)
                    .map_err(accountsdb_to_io_error)?;
                new_slot_storage.insert(id, Arc::new(u_storage_entry));
            }
            Ok((slot, new_slot_storage))
        })
        .collect::<Result<HashMap<Slot, _>, IoError>>()?;

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
        let mut stores = accounts_db.storage.write().unwrap();
        stores.0.extend(storage);
    }

    accounts_db.next_id.store(max_id + 1, Ordering::Relaxed);
    accounts_db
        .write_version
        .fetch_add(version, Ordering::Relaxed);
    accounts_db.generate_index();
    Ok(accounts_db)
}
