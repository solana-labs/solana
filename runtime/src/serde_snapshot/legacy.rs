use super::*;

pub(super) struct Context {}
impl<'a> TypeContext<'a> for Context {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

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
                    Err(Error::custom(
                        "SerializableAppendVec: unexpected length",
                    ))
                } else {
                    Ok(SerializableAppendVec { current_len })
                }
            }
        }
        deserializer.deserialize_bytes(SerializableAppendVecVisitor)
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
        // let's preserve the exact behavior for maximum compatibility
        // there is no gurantee how bincode encode things by using serde constructs (like
        // serialize_tuple())
        use serde::ser::Error;
        let mut wr = Cursor::new(Vec::new());
        let accounts_db_serialize = SerializableAccountsDB::<'a, Self> {
            accounts_db: &*serializable_bank.bank_rc.accounts.accounts_db,
            slot: serializable_bank.bank_rc.slot,
            account_storage_entries: serializable_bank.snapshot_storages,
            phantom: std::marker::PhantomData::default(),
        };
        serialize_into(&mut wr, &accounts_db_serialize).map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDB<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        // let's preserve the exact behavior for maximum compatibility
        // there is no gurantee how bincode encode things by using serde constructs (like
        // serialize_tuple())
        use serde::ser::Error;
        let mut wr = Cursor::new(vec![]);

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

        // write the list of account storage entry lists out as a map
        serialize_into(&mut wr, &entries).map_err(Error::custom)?;
        // write the current write version sampled before the account
        // storage entries were written out
        serialize_into(&mut wr, &version).map_err(Error::custom)?;

        serialize_into(
            &mut wr,
            &(
                serializable_db.slot,
                &*bank_hashes.get(&serializable_db.slot).unwrap_or_else(|| {
                    panic!("No bank_hashes entry for slot {}", serializable_db.slot)
                }),
            ),
        )
        .map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
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
        // Read and discard the prepended serialized byte vector length
        let _serialized_len: u64 = deserialize_from(&mut stream).map_err(accountsdb_to_io_error)?;

        // read and discard u64 byte vector length
        // (artifact from accountsdb_to_stream serializing first
        // into byte vector and then into stream)
        let serialized_len: u64 = deserialize_from(&mut stream).map_err(accountsdb_to_io_error)?;

        // read map of slots to account storage entries
        let storage: HashMap<Slot, Vec<Self::SerializableAccountStorageEntry>> = bincode::config()
            .limit(min(serialized_len, MAX_ACCOUNTS_DB_STREAM_SIZE))
            .deserialize_from(&mut stream)
            .map_err(accountsdb_to_io_error)?;
        let version: u64 = deserialize_from(&mut stream)
            .map_err(|e| format!("write version deserialize error: {}", e.to_string()))
            .map_err(accountsdb_to_io_error)?;

        let (slot, bank_hash_info): (Slot, BankHashInfo) = deserialize_from(&mut stream)
            .map_err(|e| format!("bank hashes deserialize error: {}", e.to_string()))
            .map_err(accountsdb_to_io_error)?;

        Ok((storage, version, slot, bank_hash_info))
    }

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
