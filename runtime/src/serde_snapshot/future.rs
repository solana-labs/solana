use super::*;

pub(super) struct Context {}
impl<'a> TypeContext<'a> for Context {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

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

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct SerializableAccountStorageEntry {
    id: AppendVecId,
    accounts_current_len: usize,
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id,
            accounts_current_len: rhs.accounts.len(),
        }
    }
}

impl Into<AccountStorageEntry> for SerializableAccountStorageEntry {
    fn into(self) -> AccountStorageEntry {
        AccountStorageEntry::new_empty_map(self.id, self.accounts_current_len)
    }
}

pub(super) struct Context2 {}
impl<'a> SerdeContext<'a> for Context2 {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

    fn serialize_bank_rc_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankRc<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        let accounts_db_serialize = SerializableAccountsDB::<'a, Self> {
            accounts_db: &*serializable_bank.bank_rc.accounts.accounts_db,
            slot: serializable_bank.bank_rc.slot,
            account_storage_entries: serializable_bank.snapshot_storages,
            phantom: std::marker::PhantomData::default(),
        };

        accounts_db_serialize.serialize(serializer)
    }

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDB<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        // sample write version before serializing storage entries
        let version = serializable_db
            .accounts_db
            .write_version
            .load(Ordering::Relaxed);

        let entries = serializable_db
            .account_storage_entries
            .iter()
            .map(|x| {
                (
                    x.first().unwrap().slot,
                    x.iter()
                        .map(|x| Self::SerializableAccountStorageEntry::from(x.as_ref()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<HashMap<Slot, _>>();

        let bank_hashes = serializable_db.accounts_db.bank_hashes.read().unwrap();

        (
            entries,
            version,
            serializable_db.slot,
            &*bank_hashes.get(&serializable_db.slot).unwrap_or_else(|| {
                panic!("No bank_hashes entry for slot {}", serializable_db.slot)
            }),
        )
            .serialize(serializer)
    }

    fn deserialize_accounts_db_fields<R>(
        mut stream: &mut BufReader<R>,
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
        R: Read,
    {
        deserialize_from(&mut stream).map_err(accountsdb_to_io_error)
    }

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
