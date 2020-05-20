use {
    crate::{
        accounts::Accounts,
        accounts_db::{
            AccountStorage, AccountStorageEntry, AccountStorageStatus, AccountsDB, AppendVecId,
            BankHashInfo, SlotStores,
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
        ser::SerializeTuple,
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

pub use crate::accounts_db::{SnapshotStorage, SnapshotStorages};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SerdeStyle {
    NEWER,
    OLDER,
}

trait TypeContext<'a> {
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

struct TypeContextLegacy {}
impl<'a> TypeContext<'a> for TypeContextLegacy {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntryLegacy;

    fn legacy_or_zero<T: Default>(x: T) -> T {
        x
    }

    fn legacy_serialize_byte_length<S>(serializer: &mut S, x: u64) -> Result<(), S::Error>
    where
        S: SerializeTuple,
    {
        serializer.serialize_element(&x)
    }

    fn legacy_deserialize_byte_length<R: Read>(stream: &mut R) -> Result<u64, bincode::Error> {
        deserialize_from(stream)
    }
}

struct TypeContextFuture {}
impl<'a> TypeContext<'a> for TypeContextFuture {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntryFuture;

    fn legacy_or_zero<T: Default>(_x: T) -> T {
        T::default()
    }

    fn legacy_serialize_byte_length<S>(_serializer: &mut S, _x: u64) -> Result<(), S::Error>
    where
        S: SerializeTuple,
    {
        Ok(())
    }

    fn legacy_deserialize_byte_length<R: Read>(_stream: &mut R) -> Result<u64, bincode::Error> {
        Ok(MAX_ACCOUNTS_DB_STREAM_SIZE)
    }
}

const MAX_ACCOUNTS_DB_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024;

// consumes an iterator and returns an object that will serialize as a serde seq
fn serialize_iter_as_seq<I>(iter: I) -> impl Serialize
where
    I: IntoIterator,
    <I as IntoIterator>::Item: Serialize,
{
    struct SerializableSequencedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    impl<I> Serialize for SerializableSequencedIterator<I>
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.collect_seq(self.iter.borrow_mut().take().unwrap())
        }
    }

    SerializableSequencedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
}

// consumes a 2-tuple iterator and returns an object that will serialize as a serde map
fn serialize_iter_as_map<K, V, I>(iter: I) -> impl Serialize
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

            // Possibly write out a dummy vector length for backward compatibility
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

    // read map of slots to account storage entries
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

    // Remap the deserialized AppendVec paths to point to correct local paths
    let new_storage_map: Result<HashMap<Slot, SlotStores>, IoError> = storage
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

    let new_storage_map = new_storage_map?;
    let mut storage = AccountStorage(new_storage_map);

    // discard any slots with no storage entries
    // this can happen if a non-root slot was serialized
    // but non-root stores should not be included in the snapshot
    storage.0.retain(|_slot, stores| !stores.is_empty());

    let version: u64 = deserialize_from(&mut stream)
        .map_err(|e| format!("write version deserialize error: {}", e.to_string()))
        .map_err(accountsdb_to_io_error)?;

    let (slot, bank_hash): (Slot, BankHashInfo) = deserialize_from(&mut stream)
        .map_err(|e| format!("bank hashes deserialize error: {}", e.to_string()))
        .map_err(accountsdb_to_io_error)?;
    accounts_db
        .bank_hashes
        .write()
        .unwrap()
        .insert(slot, bank_hash);

    // Process deserialized data, set necessary fields in self
    let max_id: usize = *storage
        .0
        .values()
        .flat_map(HashMap::keys)
        .max()
        .expect("At least one storage entry must exist from deserializing stream");

    {
        let mut stores = accounts_db.storage.write().unwrap();
        stores.0.extend(storage.0);
    }

    accounts_db.next_id.store(max_id + 1, Ordering::Relaxed);
    accounts_db
        .write_version
        .fetch_add(version, Ordering::Relaxed);
    accounts_db.generate_index();
    Ok(accounts_db)
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

        let mut seq = serializer.serialize_tuple(C::legacy_or_zero(1) + 3)?;

        // Possibly write out a dummy vector length for backward compatibility
        C::legacy_serialize_byte_length(&mut seq, MAX_ACCOUNTS_DB_STREAM_SIZE)?;

        // write the list of account storage entry lists out as a map
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

        // write the current write version sampled before the account
        // storage entries were written out
        seq.serialize_element(&version)?;

        // write out bank hashes
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

// Serializable version of AccountStorageEntry for snapshot format Legacy
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SerializableAccountStorageEntryLegacy {
    id: AppendVecId,
    accounts: SerializableAppendVecLegacy,
    count_and_status: (usize, AccountStorageStatus),
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntryLegacy {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id,
            accounts: SerializableAppendVecLegacy::from(&rhs.accounts),
            ..Self::default()
        }
    }
}

impl Into<AccountStorageEntry> for SerializableAccountStorageEntryLegacy {
    fn into(self) -> AccountStorageEntry {
        AccountStorageEntry::new_empty_map(self.id, self.accounts.current_len)
    }
}

// Serializable version of AppendVec for snapshot format Legacy
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SerializableAppendVecLegacy {
    current_len: usize,
}

impl From<&AppendVec> for SerializableAppendVecLegacy {
    fn from(rhs: &AppendVec) -> SerializableAppendVecLegacy {
        SerializableAppendVecLegacy {
            current_len: rhs.len(),
        }
    }
}

impl Into<AppendVec> for SerializableAppendVecLegacy {
    fn into(self) -> AppendVec {
        AppendVec::new_empty_map(self.current_len)
    }
}

// Serialization of AppendVec Legacy requires serialization of u64 to
// eight byte vector which is then itself serialized to the stream
impl Serialize for SerializableAppendVecLegacy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        const LEN: usize = std::mem::size_of::<usize>();
        let mut buf = [0u8; LEN];
        serialize_into(Cursor::new(&mut buf[..]), &(self.current_len as u64))
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_bytes(&buf)
    }
}

// Deserialization of AppendVec Legacy requires deserialization
// of eight byte vector from which u64 is then deserialized
impl<'de> Deserialize<'de> for SerializableAppendVecLegacy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        struct SerializableAppendVecLegacyVisitor;
        impl<'a> Visitor<'a> for SerializableAppendVecLegacyVisitor {
            type Value = SerializableAppendVecLegacy;
            fn expecting(&self, formatter: &mut Formatter) -> FormatResult {
                formatter.write_str("Expecting SerializableAppendVecLegacy")
            }
            fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
            where
                E: Error,
            {
                const LEN: u64 = std::mem::size_of::<usize>() as u64;
                let mut rd = Cursor::new(&data[..]);
                let current_len: usize = deserialize_from(&mut rd).map_err(Error::custom)?;
                if rd.position() != LEN {
                    Err(Error::custom(
                        "SerializableAppendVecLegacy: unexpected length",
                    ))
                } else {
                    Ok(SerializableAppendVecLegacy { current_len })
                }
            }
        }
        deserializer.deserialize_bytes(SerializableAppendVecLegacyVisitor)
    }
}

// Serializable version of AccountStorageEntry for snapshot format Future
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SerializableAccountStorageEntryFuture {
    id: AppendVecId,
    accounts_current_len: usize,
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntryFuture {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id,
            accounts_current_len: rhs.accounts.len(),
        }
    }
}

impl Into<AccountStorageEntry> for SerializableAccountStorageEntryFuture {
    fn into(self) -> AccountStorageEntry {
        AccountStorageEntry::new_empty_map(self.id, self.accounts_current_len)
    }
}

// a number of test cases in accounts_db use this
#[cfg(test)]
pub(crate) use self::tests::reconstruct_accounts_db_via_serialization;

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            accounts::{create_test_accounts, Accounts},
            accounts_db::get_temp_accounts_paths,
            bank::{Bank, StatusCacheRc},
        },
        rand::{thread_rng, Rng},
        solana_sdk::{
            account::Account,
            genesis_config::create_genesis_config,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
        },
        std::io::{BufReader, Cursor},
        tempfile::TempDir,
    };

    fn copy_append_vecs<P: AsRef<Path>>(
        accounts_db: &AccountsDB,
        output_dir: P,
    ) -> std::io::Result<()> {
        let storage_entries = accounts_db.get_snapshot_storages(Slot::max_value());
        for storage in storage_entries.iter().flatten() {
            let storage_path = storage.get_path();
            let output_path = output_dir.as_ref().join(
                storage_path
                    .file_name()
                    .expect("Invalid AppendVec file path"),
            );

            std::fs::copy(storage_path, output_path)?;
        }

        Ok(())
    }

    fn check_accounts(accounts: &Accounts, pubkeys: &[Pubkey], num: usize) {
        for _ in 1..num {
            let idx = thread_rng().gen_range(0, num - 1);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]);
            let account1 = Some((
                Account::new((idx + 1) as u64, 0, &Account::default().owner),
                0,
            ));
            assert_eq!(account, account1);
        }
    }

    fn accountsdb_from_stream<R, P>(
        serde_style: SerdeStyle,
        stream: &mut BufReader<R>,
        account_paths: &[PathBuf],
        stream_append_vecs_path: P,
    ) -> Result<AccountsDB, IoError>
    where
        R: Read,
        P: AsRef<Path>,
    {
        match serde_style {
            SerdeStyle::NEWER => context_accountsdb_from_stream::<TypeContextFuture, R, P>(
                stream,
                account_paths,
                stream_append_vecs_path,
            ),
            SerdeStyle::OLDER => context_accountsdb_from_stream::<TypeContextLegacy, R, P>(
                stream,
                account_paths,
                stream_append_vecs_path,
            ),
        }
    }

    fn accountsdb_to_stream<W>(
        serde_style: SerdeStyle,
        stream: &mut W,
        accounts_db: &AccountsDB,
        slot: Slot,
        account_storage_entries: &[SnapshotStorage],
    ) -> Result<(), IoError>
    where
        W: Write,
    {
        match serde_style {
            SerdeStyle::NEWER => serialize_into(
                stream,
                &SerializableAccountsDatabaseUnversioned::<TypeContextFuture> {
                    accounts_db,
                    slot,
                    account_storage_entries,
                    phantom: std::marker::PhantomData::default(),
                },
            )
            .map_err(bankrc_to_io_error),
            SerdeStyle::OLDER => serialize_into(
                stream,
                &SerializableAccountsDatabaseUnversioned::<TypeContextLegacy> {
                    accounts_db,
                    slot,
                    account_storage_entries,
                    phantom: std::marker::PhantomData::default(),
                },
            )
            .map_err(bankrc_to_io_error),
        }
    }

    fn test_accounts_serialize_style(serde_style: SerdeStyle) {
        solana_logger::setup();
        let (_accounts_dir, paths) = get_temp_accounts_paths(4).unwrap();
        let accounts = Accounts::new(paths);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_test_accounts(&accounts, &mut pubkeys, 100, 0);
        check_accounts(&accounts, &pubkeys, 100);
        accounts.add_root(0);

        let mut writer = Cursor::new(vec![]);
        accountsdb_to_stream(
            serde_style,
            &mut writer,
            &*accounts.accounts_db,
            0,
            &accounts.accounts_db.get_snapshot_storages(0),
        )
        .unwrap();

        let copied_accounts = TempDir::new().unwrap();

        // Simulate obtaining a copy of the AppendVecs from a tarball
        copy_append_vecs(&accounts.accounts_db, copied_accounts.path()).unwrap();

        let buf = writer.into_inner();
        let mut reader = BufReader::new(&buf[..]);
        let (_accounts_dir, daccounts_paths) = get_temp_accounts_paths(2).unwrap();
        let daccounts = Accounts::new_empty(
            accountsdb_from_stream(
                serde_style,
                &mut reader,
                &daccounts_paths,
                copied_accounts.path(),
            )
            .unwrap(),
        );
        check_accounts(&daccounts, &pubkeys, 100);
        assert_eq!(accounts.bank_hash_at(0), daccounts.bank_hash_at(0));
    }

    fn test_bank_serialize_style(serde_style: SerdeStyle) {
        solana_logger::setup();
        let (genesis_config, _) = create_genesis_config(500);
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank0.squash();

        // Create an account on a non-root fork
        let key1 = Keypair::new();
        bank1.deposit(&key1.pubkey(), 5);

        let bank2 = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);

        // Test new account
        let key2 = Keypair::new();
        bank2.deposit(&key2.pubkey(), 10);
        assert_eq!(bank2.get_balance(&key2.pubkey()), 10);

        let key3 = Keypair::new();
        bank2.deposit(&key3.pubkey(), 0);

        bank2.squash();

        let snapshot_storages = bank2.get_snapshot_storages();
        let mut buf = vec![];
        let mut writer = Cursor::new(&mut buf);
        serialize_into(&mut writer, &bank2).unwrap();
        crate::serde_snapshot::bankrc_to_stream(
            serde_style,
            &mut std::io::BufWriter::new(&mut writer),
            &bank2.rc,
            &snapshot_storages,
        )
        .unwrap();

        let mut rdr = Cursor::new(&buf[..]);
        let mut dbank: Bank = bincode::deserialize_from(&mut rdr).unwrap();
        let mut reader = std::io::BufReader::new(&buf[rdr.position() as usize..]);

        // Create a new set of directories for this bank's accounts
        let (_accounts_dir, dbank_paths) = get_temp_accounts_paths(4).unwrap();
        let ref_sc = StatusCacheRc::default();
        ref_sc.status_cache.write().unwrap().add_root(2);
        // Create a directory to simulate AppendVecs unpackaged from a snapshot tar
        let copied_accounts = TempDir::new().unwrap();
        copy_append_vecs(&bank2.rc.accounts.accounts_db, copied_accounts.path()).unwrap();
        dbank.set_bank_rc(
            crate::serde_snapshot::bankrc_from_stream(
                serde_style,
                &dbank_paths,
                dbank.slot(),
                &mut reader,
                copied_accounts.path(),
            )
            .unwrap(),
            ref_sc,
        );
        assert_eq!(dbank.get_balance(&key1.pubkey()), 0);
        assert_eq!(dbank.get_balance(&key2.pubkey()), 10);
        assert_eq!(dbank.get_balance(&key3.pubkey()), 0);
        bank2.compare_bank(&dbank);
    }

    pub(crate) fn reconstruct_accounts_db_via_serialization(
        accounts: &AccountsDB,
        slot: Slot,
    ) -> AccountsDB {
        let mut writer = Cursor::new(vec![]);
        let snapshot_storages = accounts.get_snapshot_storages(slot);
        accountsdb_to_stream(
            SerdeStyle::NEWER,
            &mut writer,
            &accounts,
            slot,
            &snapshot_storages,
        )
        .unwrap();

        let buf = writer.into_inner();
        let mut reader = BufReader::new(&buf[..]);
        let copied_accounts = TempDir::new().unwrap();
        // Simulate obtaining a copy of the AppendVecs from a tarball
        copy_append_vecs(&accounts, copied_accounts.path()).unwrap();
        accountsdb_from_stream(SerdeStyle::NEWER, &mut reader, &[], copied_accounts.path()).unwrap()
    }

    #[test]
    fn test_accounts_serialize_newer() {
        test_accounts_serialize_style(SerdeStyle::NEWER)
    }

    #[test]
    fn test_accounts_serialize_older() {
        test_accounts_serialize_style(SerdeStyle::OLDER)
    }

    #[test]
    fn test_bank_serialize_newer() {
        test_bank_serialize_style(SerdeStyle::NEWER)
    }

    #[test]
    fn test_bank_serialize_older() {
        test_bank_serialize_style(SerdeStyle::OLDER)
    }
}
