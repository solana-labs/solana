//! AccountInfo represents a reference to AccountSharedData in either an AppendVec or the write cache.
//! AccountInfo is not persisted anywhere between program runs.
//! AccountInfo is purely runtime state.
//! Note that AccountInfo is saved to disk buckets during runtime, but disk buckets are recreated at startup.
use crate::{
    accounts_db::{AppendVecId, CACHE_VIRTUAL_OFFSET},
    accounts_index::{IsCached, ZeroLamport},
    append_vec::ALIGN_BOUNDARY_OFFSET,
};

/// offset within an append vec to account data
pub type Offset = usize;

/// bytes used to store this account in append vec
/// Note this max needs to be big enough to handle max data len of 10MB, which is a const
pub type StoredSize = u32;

/// specify where account data is located
#[derive(Debug, PartialEq, Eq)]
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

/// how large the offset we store in AccountInfo is
/// Note this is a smaller datatype than 'Offset'
/// AppendVecs store accounts aligned to u64, so offset is always a multiple of 8 (sizeof(u64))
pub type OffsetReduced = u32;

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct AccountInfo {
    /// index identifying the append storage
    store_id: AppendVecId,

    /// offset = 'reduced_offset' * ALIGN_BOUNDARY_OFFSET into the storage
    /// Note this is a smaller type than 'Offset'
    reduced_offset: OffsetReduced,

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

/// We have to have SOME value for store_id when we are cached
const CACHE_VIRTUAL_STORAGE_ID: AppendVecId = AppendVecId::MAX;

impl AccountInfo {
    pub fn new(storage_location: StorageLocation, stored_size: StoredSize, lamports: u64) -> Self {
        assert_eq!(stored_size & ALL_FLAGS, 0);
        let mut stored_size_mask = stored_size;
        let (store_id, raw_offset) = match storage_location {
            StorageLocation::AppendVec(store_id, offset) => (store_id, offset),
            StorageLocation::Cached => {
                stored_size_mask |= IS_CACHED_STORE_ID_FLAG;
                (CACHE_VIRTUAL_STORAGE_ID, CACHE_VIRTUAL_OFFSET)
            }
        };
        if lamports == 0 {
            stored_size_mask |= IS_ZERO_LAMPORT_FLAG;
        }
        let reduced_offset: OffsetReduced = (raw_offset / ALIGN_BOUNDARY_OFFSET) as OffsetReduced;
        let result = Self {
            store_id,
            reduced_offset,
            stored_size_mask,
        };
        assert_eq!(result.offset(), raw_offset, "illegal offset");
        result
    }

    pub fn store_id(&self) -> AppendVecId {
        // if the account is in a cached store, the store_id is meaningless
        assert!(!self.is_cached());
        self.store_id
    }

    pub fn offset(&self) -> Offset {
        (self.reduced_offset as Offset) * ALIGN_BOUNDARY_OFFSET
    }

    pub fn stored_size(&self) -> StoredSize {
        // elminate the special bit that indicates the info references an account with zero lamports
        self.stored_size_mask & !ALL_FLAGS
    }

    /// true iff store_id and offset match self AND self is not cached
    /// If self is cached, then store_id and offset are meaningless.
    pub fn matches_storage_location(&self, store_id: AppendVecId, offset: Offset) -> bool {
        // order is set for best short circuit
        self.store_id == store_id && self.offset() == offset && !self.is_cached()
    }

    pub fn storage_location(&self) -> StorageLocation {
        if self.is_cached() {
            StorageLocation::Cached
        } else {
            StorageLocation::AppendVec(self.store_id, self.offset())
        }
    }
}
#[cfg(test)]
mod test {
    use {super::*, crate::append_vec::MAXIMUM_APPEND_VEC_FILE_SIZE};

    #[test]
    fn test_limits() {
        for offset in [
            MAXIMUM_APPEND_VEC_FILE_SIZE as Offset,
            0,
            ALIGN_BOUNDARY_OFFSET,
            4 * ALIGN_BOUNDARY_OFFSET,
        ] {
            let info = AccountInfo::new(StorageLocation::AppendVec(0, offset), 0, 0);
            assert!(info.offset() == offset);
        }
    }

    #[test]
    #[should_panic(expected = "illegal offset")]
    fn test_alignment() {
        let offset = 1; // not aligned
        AccountInfo::new(StorageLocation::AppendVec(0, offset), 0, 0);
    }

    #[test]
    fn test_matches_storage_location() {
        let offset = 0;
        let id = 0;
        let info = AccountInfo::new(StorageLocation::AppendVec(id, offset), 0, 0);
        assert!(info.matches_storage_location(id, offset));

        // wrong offset
        let offset = ALIGN_BOUNDARY_OFFSET;
        assert!(!info.matches_storage_location(id, offset));

        // wrong id
        let offset = 0;
        let id = 1;
        assert!(!info.matches_storage_location(id, offset));

        // is cached
        let id = CACHE_VIRTUAL_STORAGE_ID;
        let info = AccountInfo::new(StorageLocation::Cached, 0, 0);
        assert!(!info.matches_storage_location(id, offset));
    }
}
