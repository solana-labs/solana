use {
    serde::{Deserialize, Serialize},
    solana_accounts_db::accounts_db::AccountStorageEntry,
};

/// The serialized AppendVecId type is fixed as usize
pub(crate) type SerializedAppendVecId = usize;

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SerializableAccountStorageEntry {
    id: SerializedAppendVecId,
    accounts_current_len: usize,
}

pub(super) trait SerializableStorage {
    fn id(&self) -> SerializedAppendVecId;
    fn current_len(&self) -> usize;
}

impl SerializableStorage for SerializableAccountStorageEntry {
    fn id(&self) -> SerializedAppendVecId {
        self.id
    }
    fn current_len(&self) -> usize {
        self.accounts_current_len
    }
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.append_vec_id() as SerializedAppendVecId,
            accounts_current_len: rhs.accounts.len(),
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl solana_frozen_abi::abi_example::IgnoreAsHelper for SerializableAccountStorageEntry {}
