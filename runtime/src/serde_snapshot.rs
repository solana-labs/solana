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
        ser::{SerializeSeq, SerializeTuple},
        Deserialize, Deserializer, Serialize, Serializer,
    },
    solana_measure::measure::Measure,
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
        sync::{atomic::Ordering, Arc, RwLock},
    },
};

mod legacy;
use legacy::{
    Context as TypeContextLegacy,
};

mod future;
use future::{
    Context as TypeContextFuture,
};

pub(super) trait TypeContext<'a> {
    type SerializableAccountStorageEntry: Serialize
        + DeserializeOwned
        + From<&'a AccountStorageEntry>
        + Into<AccountStorageEntry>;

    fn legacy_or_zero<T: Default>(x: T) -> T;

    fn legacy_serialize_byte_length<S>(serializer: &mut S, x: u64) -> Result<(), S::Error>
    where
        S: SerializeTuple;

    fn legacy_deserialize_byte_length<R: Read>(stream: &mut R) -> Result<u64, bincode::Error>;
}

pub use crate::accounts_db::{SnapshotStorage, SnapshotStorages};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SerdeStyle {
    NEWER,
    OLDER,
}

const MAX_ACCOUNTS_DB_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024;

// consumes an iterator and returns an object that will serialize as a serde seq
pub fn serialize_iter_as_seq<I>(iter: I) -> impl Serialize
where
    I: IntoIterator,
    <I as IntoIterator>::Item: Serialize,
    <I as IntoIterator>::IntoIter: ExactSizeIterator,
{
    struct SerializableSequencedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    impl<I> Serialize for SerializableSequencedIterator<I>
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Serialize,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
	    let iter = self.iter.borrow_mut().take().unwrap().into_iter();
	    let mut seq = serializer.serialize_seq(Some(iter.len()))?;
	    for item in iter {
		seq.serialize_element(&item)?;
	    }
	    seq.end()
        }
    }

    SerializableSequencedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
}

// consumes an iterator and returns an object that will serialize as a serde tuple
pub fn serialize_iter_as_tuple<I>(iter: I) -> impl Serialize
where
    I: IntoIterator,
    <I as IntoIterator>::Item: Serialize,
    <I as IntoIterator>::IntoIter: ExactSizeIterator,
{
    struct SerializableSequencedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    impl<I> Serialize for SerializableSequencedIterator<I>
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Serialize,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
	    let iter = self.iter.borrow_mut().take().unwrap().into_iter();
	    let mut tup = serializer.serialize_tuple(iter.len())?;
	    for item in iter {
		tup.serialize_element(&item)?;
	    }
	    tup.end()
        }
    }

    SerializableSequencedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
}

// consumes a 2-tuple iterator and returns an object that will serialize as a serde map
pub fn serialize_iter_as_map<K, V, I>(iter: I) -> impl Serialize
where
    K: Serialize,
    V: Serialize,
    I: IntoIterator<Item = (K, V)>,
{
    struct SerializableMappedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    impl<K, V, I> Serialize for SerializableMappedIterator<I>
    where
        K: Serialize,
        V: Serialize,
        I: IntoIterator<Item = (K, V)>,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.collect_map(self.iter.borrow_mut().take().unwrap())
        }
    }

    SerializableMappedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
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
    match serde_style {
        SerdeStyle::NEWER => context_bankrc_from_stream::<TypeContextFuture, R, P>(
            account_paths,
            slot,
            stream,
            stream_append_vecs_path,
        ),
        SerdeStyle::OLDER => context_bankrc_from_stream::<TypeContextLegacy, R, P>(
            account_paths,
            slot,
            stream,
            stream_append_vecs_path,
        ),
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
    match serde_style {
        SerdeStyle::NEWER => {
            context_bankrc_to_stream::<TypeContextFuture, W>(stream, bank_rc, snapshot_storages)
        }
        SerdeStyle::OLDER => {
            context_bankrc_to_stream::<TypeContextLegacy, W>(stream, bank_rc, snapshot_storages)
        }
    }
}

pub fn context_bankrc_from_stream_2<'a, C, R, P>(
    account_paths: &[PathBuf],
    slot: Slot,
    stream: &mut BufReader<R>,
    stream_append_vecs_path: P,
) -> std::result::Result<BankRc, IoError>
where
    C: SerdeContext<'a>,
    R: Read,
    P: AsRef<Path>,
{
    // read and deserialise the accounts database directly from the stream
    let accounts = Accounts::new_empty(context_accountsdb_from_fields::<C, P>(
        C::deserialize_accounts_db_fields(stream)?,
        account_paths,
        stream_append_vecs_path,
    )?);

    Ok(BankRc {
        accounts: Arc::new(accounts),
        parent: RwLock::new(None),
        slot,
    })
}

fn context_bankrc_from_stream<'a, C, R, P>(
    account_paths: &[PathBuf],
    slot: Slot,
    mut stream: &mut BufReader<R>,
    stream_append_vecs_path: P,
) -> std::result::Result<BankRc, IoError>
where
    C: TypeContext<'a>,
    R: Read,
    P: AsRef<Path>,
{
    // Possibly read and discard the prepended serialized byte vector length
    let _serialized_len: u64 =
        C::legacy_deserialize_byte_length(&mut stream).map_err(accountsdb_to_io_error)?;

    // read and deserialise the accounts database directly from the stream
    let accounts = Accounts::new_empty(context_accountsdb_from_stream::<C, R, P>(
        stream,
        account_paths,
        stream_append_vecs_path,
    )?);

    Ok(BankRc {
        accounts: Arc::new(accounts),
        parent: RwLock::new(None),
        slot,
    })
}

fn context_bankrc_to_stream<'a, 'b, C, W>(
    stream: &'b mut BufWriter<W>,
    bank_rc: &'a BankRc,
    snapshot_storages: &'a [SnapshotStorage],
) -> Result<(), IoError>
where
    C: TypeContext<'a>,
    W: Write,
{
    struct BankRcSerialize<'a, C> {
        bank_rc: &'a BankRc,
        snapshot_storages: &'a [SnapshotStorage],
        phantom: std::marker::PhantomData<C>,
    }

    impl<'a, C: TypeContext<'a>> Serialize for BankRcSerialize<'a, C> {
        fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
        where
            S: serde::ser::Serializer,
        {
            let mut seq = serializer.serialize_tuple(C::legacy_or_zero(1) + 1)?;

            // possibly write out a dummy vector length for backward compatibility
            C::legacy_serialize_byte_length(&mut seq, MAX_ACCOUNTS_DB_STREAM_SIZE)?;

            seq.serialize_element(&SerializableAccountsDatabaseUnversioned::<'a, 'a, C> {
                accounts_db: &*self.bank_rc.accounts.accounts_db,
                slot: self.bank_rc.slot,
                account_storage_entries: self.snapshot_storages,
                phantom: std::marker::PhantomData::default(),
            })?;

            seq.end()
        }
    }

    serialize_into(
        stream,
        &BankRcSerialize::<C> {
            bank_rc,
            snapshot_storages,
            phantom: std::marker::PhantomData::default(),
        },
    )
    .map_err(bankrc_to_io_error)
}

pub struct SerializableBankRc<'a, C> {
    bank_rc: &'a BankRc,
    snapshot_storages: &'a [SnapshotStorage],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, C: SerdeContext<'a>> Serialize for SerializableBankRc<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_bank_rc_fields(serializer, self)
    }
}

pub fn context_bankrc_to_stream_2<'a, 'b, C, W>(
    stream: &'b mut BufWriter<W>,
    bank_rc: &'a BankRc,
    snapshot_storages: &'a [SnapshotStorage],
) -> Result<(), IoError>
where
    C: SerdeContext<'a>,
    W: Write,
{
    serialize_into(
        stream,
        &SerializableBankRc::<C> {
            bank_rc,
            snapshot_storages,
            phantom: std::marker::PhantomData::default(),
        },
    )
    .map_err(bankrc_to_io_error)
}

fn context_accountsdb_from_stream<'a, C, R, P>(
    mut stream: &mut BufReader<R>,
    account_paths: &[PathBuf],
    stream_append_vecs_path: P,
) -> Result<AccountsDB, IoError>
where
    C: TypeContext<'a>,
    R: Read,
    P: AsRef<Path>,
{
    let accounts_db = AccountsDB::new(account_paths.to_vec());

    // read and discard u64 byte vector length
    // (artifact from accountsdb_to_stream serializing first
    // into byte vector and then into stream)
    let serialized_len: u64 =
        C::legacy_deserialize_byte_length(&mut stream).map_err(accountsdb_to_io_error)?;

    // (1st of 3 elements) read in map of slots to account storage entries
    let storage: HashMap<Slot, Vec<C::SerializableAccountStorageEntry>> = bincode::config()
        .limit(min(serialized_len, MAX_ACCOUNTS_DB_STREAM_SIZE))
        .deserialize_from(&mut stream)
        .map_err(accountsdb_to_io_error)?;

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

    // construct a new HashMap<Slot, SlotStores> and remap the deserialized
    // AppendVec paths to point to correct local paths
    let mut storage = storage
        .into_iter()
        .map(|(slot, mut slot_storage)| {
            let mut new_slot_storage = HashMap::new();
            for (id, storage_entry) in slot_storage.drain() {
                let path_index = thread_rng().gen_range(0, accounts_db.paths.len());
                let local_dir = &accounts_db.paths[path_index];

                std::fs::create_dir_all(local_dir).expect("Create directory failed");

                // move the corresponding AppendVec from the snapshot into the directory pointed
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

                // notify the AppendVec of the new file location
                let local_path = local_dir.join(append_vec_relative_path);
                let mut u_storage_entry = Arc::try_unwrap(storage_entry).unwrap();
                u_storage_entry
                    .set_file(local_path)
                    .map_err(accountsdb_to_io_error)?;
                new_slot_storage.insert(id, Arc::new(u_storage_entry));
            }
            Ok((slot, new_slot_storage))
        })
        .collect::<Result<HashMap<_, _>, IoError>>()?;

    // discard any slots with no storage entries
    // this can happen if a non-root slot was serialized
    // but non-root stores should not be included in the snapshot
    storage.retain(|_slot, stores| !stores.is_empty());

    // set next entry id to 1 + maximum of all remaining storage entry ids
    accounts_db.next_id.store(
        1 + *storage
            .values()
            .flat_map(HashMap::keys)
            .max()
            .expect("At least one storage entry must exist from deserializing stream"),
        Ordering::Relaxed,
    );

    // move storage entries into accounts database
    accounts_db.storage.write().unwrap().0.extend(storage);

    // (2nd of 3 elements) read in write version
    let version: u64 = deserialize_from(&mut stream)
        .map_err(|e| format!("write version deserialize error: {}", e.to_string()))
        .map_err(accountsdb_to_io_error)?;
    accounts_db
        .write_version
        .fetch_add(version, Ordering::Relaxed);

    // (3rd of 3 elements) read in (slot, bank hashes) pair
    let (slot, bank_hash): (Slot, BankHashInfo) = deserialize_from(&mut stream)
        .map_err(|e| format!("bank hashes deserialize error: {}", e.to_string()))
        .map_err(accountsdb_to_io_error)?;
    accounts_db
        .bank_hashes
        .write()
        .unwrap()
        .insert(slot, bank_hash);

    // lastly generate accounts database index
    accounts_db.generate_index();

    Ok(accounts_db)
}

pub struct SerializableAccountsDB<'a, C> {
    accounts_db: &'a AccountsDB,
    slot: Slot,
    account_storage_entries: &'a [SnapshotStorage],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, C: SerdeContext<'a>> Serialize for SerializableAccountsDB<'a, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        C::serialize_accounts_db_fields(serializer, self)
    }
}

struct SerializableAccountsDatabaseUnversioned<'a, 'b, C> {
    accounts_db: &'a AccountsDB,
    slot: Slot,
    account_storage_entries: &'b [SnapshotStorage],
    phantom: std::marker::PhantomData<C>,
}

impl<'a, 'b, C: TypeContext<'b>> Serialize for SerializableAccountsDatabaseUnversioned<'a, 'b, C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        // sample write version before serializing storage entries
        let version = self.accounts_db.write_version.load(Ordering::Relaxed);

        // accounts database serializes as a 3-tuple
        let mut seq = serializer.serialize_tuple(C::legacy_or_zero(1) + 3)?;

        // possibly write out a dummy vector length for backward compatibility
        C::legacy_serialize_byte_length(&mut seq, MAX_ACCOUNTS_DB_STREAM_SIZE)?;

        // (1st of 3 elements) write the list of account storage entry lists out as a map
        {
            let mut serialize_account_storage_timer =
                Measure::start("serialize_account_storage_ms");
            let mut entry_count = 0;
            seq.serialize_element(&serialize_iter_as_map(
                self.account_storage_entries.iter().map(|x| {
                    entry_count += x.len();
                    (
                        x.first().unwrap().slot,
                        serialize_iter_as_seq(
                            x.iter()
                                .map(|x| C::SerializableAccountStorageEntry::from(x.as_ref())),
                        ),
                    )
                }),
            ))?;
            serialize_account_storage_timer.stop();
            datapoint_info!(
                "serialize_account_storage_ms",
                ("duration", serialize_account_storage_timer.as_ms(), i64),
                ("num_entries", entry_count, i64),
            );
        }

        // (2nd of 3 elements) write the current write version sampled before
        // the account storage entries were written out
        seq.serialize_element(&version)?;

        // (3rd of 3 elements) write out (slot, bank hashes) pair
        seq.serialize_element(&(
            self.slot,
            &*self
                .accounts_db
                .bank_hashes
                .read()
                .unwrap()
                .get(&self.slot)
                .unwrap_or_else(|| panic!("No bank_hashes entry for slot {}", self.slot)),
        ))?;

        seq.end()
    }
}

// a number of test cases in accounts_db use this
#[cfg(test)]
pub(crate) use self::tests::reconstruct_accounts_db_via_serialization;


















pub trait SerdeContext<'a> {
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
    ) -> Result<
        (
            HashMap<Slot, Vec<Self::SerializableAccountStorageEntry>>,
            u64,
            Slot,
            BankHashInfo,
        ),
        IoError,
    >
    where
        R: Read;

    // we might define fn (de)serialize_bank(...) -> Result<Bank,...> for versionized bank serialization in the future

    // this fn can now be removed
    fn legacy_or_zero<T: Default>(x: T) -> T;

    // this fn can now be removed
    fn legacy_serialize_byte_length<S>(serializer: &mut S, x: u64) -> Result<(), S::Error>
    where
        S: SerializeTuple;

    // this fn can now be removed
    fn legacy_deserialize_byte_length<R: Read>(stream: &mut R) -> Result<u64, bincode::Error>;
}

fn context_accountsdb_from_fields<'a, C, P>(
    (storage, version, slot, bank_hash_info): (
        HashMap<Slot, Vec<C::SerializableAccountStorageEntry>>,
        u64,
        Slot,
        BankHashInfo,
    ),
    account_paths: &[PathBuf],
    stream_append_vecs_path: P,
) -> Result<AccountsDB, IoError>
where
    C: SerdeContext<'a>,
    P: AsRef<Path>,
{
    let accounts_db = AccountsDB::new(account_paths.to_vec());

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
    let new_storage_map: Result<HashMap<Slot, _>, IoError> = storage
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
        .collect();

    let mut storage = new_storage_map?;

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
