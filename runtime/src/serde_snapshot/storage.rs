use {
    crate::accounts_db::{AccountStorageEntry, AppendVecId},
    serde::{Deserialize, Serialize},
};

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct SerializableAccountStorageEntry {
    id: AppendVecId,
    accounts_current_len: usize,
}

pub(super) trait SerializableStorage {
    fn id(&self) -> AppendVecId;
    fn current_len(&self) -> usize;
}

impl SerializableStorage for SerializableAccountStorageEntry {
    fn id(&self) -> AppendVecId {
        self.id
    }
    fn current_len(&self) -> usize {
        self.accounts_current_len
    }
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.append_vec_id(),
            accounts_current_len: rhs.accounts.len(),
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl solana_frozen_abi::abi_example::IgnoreAsHelper for SerializableAccountStorageEntry {}
