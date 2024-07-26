use {
    serde::{Deserialize, Serialize},
    solana_accounts_db::accounts_db::AccountStorageEntry,
};

/// The serialized AccountsFileId type is fixed as usize
pub(crate) type SerializedAccountsFileId = usize;

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SerializableAccountStorageEntry {
    id: SerializedAccountsFileId,
    accounts_current_len: usize,
}

pub(super) trait SerializableStorage {
    fn id(&self) -> SerializedAccountsFileId;
    fn current_len(&self) -> usize;
}

impl SerializableStorage for SerializableAccountStorageEntry {
    fn id(&self) -> SerializedAccountsFileId {
        self.id
    }
    fn current_len(&self) -> usize {
        self.accounts_current_len
    }
}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id() as SerializedAccountsFileId,
            accounts_current_len: rhs.accounts.len(),
        }
    }
}

#[cfg(all(RUSTC_WITH_SPECIALIZATION, feature = "frozen-abi"))]
impl solana_frozen_abi::abi_example::TransparentAsHelper for SerializableAccountStorageEntry {}
