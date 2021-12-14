//! AccountInfo represents a reference to AccountSharedData in either an AppendVec or the write cache.
//! AccountInfo is not persisted anywhere between program runs.
//! AccountInfo is purely runtime state.
//! Note that AccountInfo is saved to disk buckets during runtime, but disk buckets are recreated at startup.
use crate::{
    accounts_db::{AppendVecId, CACHE_VIRTUAL_OFFSET},
    accounts_index::{IsCached, ZeroLamport},
};

/// offset within an append vec to account data
pub type Offset = usize;

/// bytes used to store this account in append vec
/// Note this max needs to be big enough to handle max data len of 10MB, which is a const
pub type StoredSize = u32;

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
    /// Note that highest bits are used by ALL_FLAGS
    /// So, 'stored_size' is 'stored_size_mask' with ALL_FLAGS masked out.
    stored_size_mask: StoredSize,
}

/// These flags can be present in stored_size_mask to indicate additional info about the AccountInfo

/// presence of this flag in stored_size_mask indicates this account info references an account with zero lamports
const IS_ZERO_LAMPORT_FLAG: StoredSize = 1 << (StoredSize::BITS - 1);
/// presence of this flag in stored_size_mask indicates this account info references an account stored in the cache
const IS_CACHED_STORE_ID_FLAG: StoredSize = 1 << (StoredSize::BITS - 2);
const ALL_FLAGS: StoredSize = IS_ZERO_LAMPORT_FLAG | IS_CACHED_STORE_ID_FLAG;

impl ZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.stored_size_mask & IS_ZERO_LAMPORT_FLAG == IS_ZERO_LAMPORT_FLAG
    }
}

impl IsCached for AccountInfo {
    fn is_cached(&self) -> bool {
        self.stored_size_mask & IS_CACHED_STORE_ID_FLAG == IS_CACHED_STORE_ID_FLAG
    }
}

impl IsCached for StorageLocation {
    fn is_cached(&self) -> bool {
        matches!(self, StorageLocation::Cached)
    }
}

impl AccountInfo {
    pub fn new(storage_location: StorageLocation, stored_size: StoredSize, lamports: u64) -> Self {
        assert_eq!(stored_size & ALL_FLAGS, 0);
        let mut stored_size_mask = stored_size;
        let (store_id, offset) = match storage_location {
            StorageLocation::AppendVec(store_id, offset) => (store_id, offset),
            StorageLocation::Cached => {
                stored_size_mask |= IS_CACHED_STORE_ID_FLAG;
                // We have to have SOME value for store_id when we are cached
                const CACHE_VIRTUAL_STORAGE_ID: AppendVecId = AppendVecId::MAX;
                (CACHE_VIRTUAL_STORAGE_ID, CACHE_VIRTUAL_OFFSET)
            }
        };
        if lamports == 0 {
            stored_size_mask |= IS_ZERO_LAMPORT_FLAG;
        }
        Self {
            store_id,
            offset,
            stored_size_mask,
        }
    }

    pub fn store_id(&self) -> AppendVecId {
        assert!(!self.is_cached());
        self.store_id
    }

    pub fn offset(&self) -> Offset {
        self.offset
    }

    pub fn stored_size(&self) -> StoredSize {
        // elminate the special bit that indicates the info references an account with zero lamports
        self.stored_size_mask & !ALL_FLAGS
    }

    pub fn storage_location(&self) -> StorageLocation {
        if self.is_cached() {
            StorageLocation::Cached
        } else {
            StorageLocation::AppendVec(self.store_id, self.offset)
        }
    }
}
