use super::*;

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

pub(super) struct TypeContextFuture {}
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

pub(super) struct TypeContextLegacy {}
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
