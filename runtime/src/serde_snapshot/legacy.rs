use {super::*, solana_measure::measure::Measure, std::cell::RefCell};

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct SerializableAccountStorageEntry {
    id: AppendVecId,
    accounts: SerializableAppendVec,
    count_and_status: (usize, AccountStorageStatus),
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id,
            accounts: SerializableAppendVec::from(&rhs.accounts),
            ..Self::default()
        }
    }
}

impl Into<AccountStorageEntry> for SerializableAccountStorageEntry {
    fn into(self) -> AccountStorageEntry {
        AccountStorageEntry::new_empty_map(self.id, self.accounts.current_len)
    }
}

// Serializable version of AppendVec for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SerializableAppendVec {
    current_len: usize,
}

impl From<&AppendVec> for SerializableAppendVec {
    fn from(rhs: &AppendVec) -> SerializableAppendVec {
        SerializableAppendVec {
            current_len: rhs.len(),
        }
    }
}

impl Into<AppendVec> for SerializableAppendVec {
    fn into(self) -> AppendVec {
        AppendVec::new_empty_map(self.current_len)
    }
}

// Serialization of AppendVec  requires serialization of u64 to
// eight byte vector which is then itself serialized to the stream
impl Serialize for SerializableAppendVec {
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

// Deserialization of AppendVec  requires deserialization
// of eight byte vector from which u64 is then deserialized
impl<'de> Deserialize<'de> for SerializableAppendVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        struct SerializableAppendVecVisitor;
        impl<'a> Visitor<'a> for SerializableAppendVecVisitor {
            type Value = SerializableAppendVec;
            fn expecting(&self, formatter: &mut Formatter) -> FormatResult {
                formatter.write_str("Expecting SerializableAppendVec")
            }
            fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
            where
                E: Error,
            {
                const LEN: u64 = std::mem::size_of::<usize>() as u64;
                let mut rd = Cursor::new(&data[..]);
                let current_len: usize = deserialize_from(&mut rd).map_err(Error::custom)?;
                if rd.position() != LEN {
                    Err(Error::custom("SerializableAppendVec: unexpected length"))
                } else {
                    Ok(SerializableAppendVec { current_len })
                }
            }
        }
        deserializer.deserialize_bytes(SerializableAppendVecVisitor)
    }
}

pub(super) struct Context {}
impl<'a> TypeContext<'a> for Context {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

    fn serialize_bank_rc_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankRc<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        // as there is no deserialize_bank_rc_fields(), do not emit the u64
        // size field here and have serialize_accounts_db_fields() emit two
        // u64 size fields instead
        SerializableAccountsDB::<'a, Self> {
            accounts_db: &*serializable_bank.bank_rc.accounts.accounts_db,
            slot: serializable_bank.bank_rc.slot,
            account_storage_entries: serializable_bank.snapshot_storages,
            phantom: std::marker::PhantomData::default(),
        }
        .serialize(serializer)
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

        // (1st of 3 elements) write the list of account storage entry lists out as a map
        let entry_count = RefCell::<usize>::new(0);
        let entries =
            serialize_iter_as_map(serializable_db.account_storage_entries.iter().map(|x| {
                *entry_count.borrow_mut() += x.len();
                (
                    x.first().unwrap().slot,
                    serialize_iter_as_seq(
                        x.iter()
                            .map(|x| Self::SerializableAccountStorageEntry::from(x.as_ref())),
                    ),
                )
            }));

        let slot_hash = (
            serializable_db.slot,
            serializable_db
                .accounts_db
                .bank_hashes
                .read()
                .unwrap()
                .get(&serializable_db.slot)
                .unwrap_or_else(|| panic!("No bank_hashes entry for slot {}", serializable_db.slot))
                .clone(),
        );

        let mut serialize_account_storage_timer = Measure::start("serialize_account_storage_ms");
        let result = (
            &MAX_ACCOUNTS_DB_STREAM_SIZE,
            &MAX_ACCOUNTS_DB_STREAM_SIZE,
            &entries,
            &version,
            &slot_hash,
        )
            .serialize(serializer);
        serialize_account_storage_timer.stop();
        datapoint_info!(
            "serialize_account_storage_ms",
            ("duration", serialize_account_storage_timer.as_ms(), i64),
            ("num_entries", *entry_count.borrow(), i64),
        );
        result
    }

    fn deserialize_accounts_db_fields<R>(
        mut stream: &mut BufReader<R>,
    ) -> Result<AccountDBFields<Self::SerializableAccountStorageEntry>, IoError>
    where
        R: Read,
    {
        // read and discard two u64 byte vector lengths
        let serialized_len = MAX_ACCOUNTS_DB_STREAM_SIZE;
        let serialized_len = min(
            serialized_len,
            deserialize_from(&mut stream).map_err(accountsdb_to_io_error)?,
        );
        let serialized_len = min(
            serialized_len,
            deserialize_from(&mut stream).map_err(accountsdb_to_io_error)?,
        );

        // (1st of 3 elements) read in map of slots to account storage entries
        let storage: HashMap<Slot, Vec<Self::SerializableAccountStorageEntry>> = bincode::config()
            .limit(serialized_len)
            .deserialize_from(&mut stream)
            .map_err(accountsdb_to_io_error)?;

        // (2nd of 3 elements) read in write version
        let version: u64 = deserialize_from(&mut stream)
            .map_err(|e| format!("write version deserialize error: {}", e.to_string()))
            .map_err(accountsdb_to_io_error)?;

        // (3rd of 3 elements) read in (slot, bank hashes) pair
        let (slot, bank_hash_info): (Slot, BankHashInfo) = deserialize_from(&mut stream)
            .map_err(|e| format!("bank hashes deserialize error: {}", e.to_string()))
            .map_err(accountsdb_to_io_error)?;

        Ok(AccountDBFields(storage, version, slot, bank_hash_info))
    }
}
