//! AccountInfo represents a reference to AccountSharedData in either an AppendVec or the write cache.
//! AccountInfo is not persisted anywhere between program runs.
//! AccountInfo is purely runtime state.
//! Note that AccountInfo is saved to disk buckets during runtime, but disk buckets are recreated at startup.
use crate::{
    accounts_db::{AppendVecId, CACHE_VIRTUAL_OFFSET, CACHE_VIRTUAL_STORAGE_ID},
    accounts_index::{IsCached, ZeroLamport},
};

/// offset within an append vec to account data
pub type Offset = usize;

/// specify where account data is located
#[derive(Debug)]
pub enum StorageLocation {
    AppendVec(AppendVecId, Offset),
    Cached,
}

impl StorageLocation {
    pub fn is_offset_equal(&self, other: &StorageLocation) -> bool {
        match self {
            StorageLocation::Cached => {
                matches!(other, StorageLocation::Cached) // technically, 2 cached entries match in offset
            }
            StorageLocation::AppendVec(_, offset) => {
                match other {
                    StorageLocation::Cached => {
                        false // 1 cached, 1 not
                    }
                    StorageLocation::AppendVec(_, other_offset) => other_offset == offset,
                }
            }
        }
    }
    pub fn is_store_id_equal(&self, other: &StorageLocation) -> bool {
        match self {
            StorageLocation::Cached => {
                matches!(other, StorageLocation::Cached) // 2 cached entries are same store id
            }
            StorageLocation::AppendVec(store_id, _) => {
                match other {
                    StorageLocation::Cached => {
                        false // 1 cached, 1 not
                    }
                    StorageLocation::AppendVec(other_store_id, _) => other_store_id == store_id,
                }
            }
        }
    }
}

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub struct AccountInfo {
    /// index identifying the append storage
    store_id: AppendVecId,

    /// offset into the storage
    offset: Offset,

    /// needed to track shrink candidacy in bytes. Used to update the number
    /// of alive bytes in an AppendVec as newer slots purge outdated entries
    /// Note that highest bit is used for ZERO_LAMPORT_BIT
    stored_size: usize,
}

/// presence of this bit in stored_size indicates this account info references an account with zero lamports
const ZERO_LAMPORT_BIT: usize = 1 << (usize::BITS - 1);

impl ZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.stored_size & ZERO_LAMPORT_BIT == ZERO_LAMPORT_BIT
    }
}

impl IsCached for AccountInfo {
    fn is_cached(&self) -> bool {
        self.store_id == CACHE_VIRTUAL_STORAGE_ID
    }
}

impl IsCached for StorageLocation {
    fn is_cached(&self) -> bool {
        matches!(self, StorageLocation::Cached)
    }
}

impl AccountInfo {
    pub fn new(storage_location: StorageLocation, mut stored_size: usize, lamports: u64) -> Self {
        let (store_id, offset) = match storage_location {
            StorageLocation::AppendVec(store_id, offset) => (store_id, offset),
            StorageLocation::Cached => (CACHE_VIRTUAL_STORAGE_ID, CACHE_VIRTUAL_OFFSET),
        };
        assert!(stored_size < ZERO_LAMPORT_BIT);
        if lamports == 0 {
            stored_size |= ZERO_LAMPORT_BIT;
        }
        Self {
            store_id,
            offset,
            stored_size,
        }
    }

    pub fn store_id(&self) -> usize {
        self.store_id
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn stored_size(&self) -> usize {
        // elminate the special bit that indicates the info references an account with zero lamports
        self.stored_size & !ZERO_LAMPORT_BIT
    }

    pub fn storage_location(&self) -> StorageLocation {
        if self.is_cached() {
            StorageLocation::Cached
        } else {
            StorageLocation::AppendVec(self.store_id, self.offset)
        }
    }
}
