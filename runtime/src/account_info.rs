use crate::{
    accounts_db::{AppendVecId, CACHE_VIRTUAL_OFFSET, CACHE_VIRTUAL_STORAGE_ID},
    accounts_index::{IsCached, ZeroLamport},
};

/// offset within an append vec to account data
pub type Offset = usize;

/// specify where account data is located
pub enum StorageLocation {
    AppendVec(AppendVecId, Offset),
    Cached,
}

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub struct AccountInfo {
    /// index identifying the append storage
    pub store_id: AppendVecId,

    /// offset into the storage
    offset: Offset,

    /// needed to track shrink candidacy in bytes. Used to update the number
    /// of alive bytes in an AppendVec as newer slots purge outdated entries
    stored_size: usize,

    /// lamports in the account used when squashing kept for optimization
    /// purposes to remove accounts with zero balance.
    lamports: u64,
}

impl ZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.lamports == 0
    }
}

impl IsCached for AccountInfo {
    fn is_cached(&self) -> bool {
        self.store_id == CACHE_VIRTUAL_STORAGE_ID
    }
}

impl AccountInfo {
    pub fn new(storage_location: StorageLocation, stored_size: usize, lamports: u64) -> Self {
        let (store_id, offset) = match storage_location {
            StorageLocation::AppendVec(store_id, offset) => (store_id, offset),
            StorageLocation::Cached => (CACHE_VIRTUAL_STORAGE_ID, CACHE_VIRTUAL_OFFSET),
        };
        Self {
            store_id,
            offset,
            stored_size,
            lamports,
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn stored_size(&self) -> usize {
        self.stored_size
    }
}
