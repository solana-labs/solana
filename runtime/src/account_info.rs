//! AccountInfo represents a reference to AccountSharedData in either an AppendVec or the write cache.
//! AccountInfo is not persisted anywhere between program runs.
//! AccountInfo is purely runtime state.
//! Note that AccountInfo is saved to disk buckets during runtime, but disk buckets are recreated at startup.
use {
    crate::{
        accounts_db::AppendVecId,
        accounts_file::ALIGN_BOUNDARY_OFFSET,
        accounts_index::{IsCached, ZeroLamport},
    },
    modular_bitfield::prelude::*,
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

/// This is an illegal value for 'offset'.
/// Account size on disk would have to be pointing to the very last 8 byte value in the max sized append vec.
/// That would mean there was a maximum size of 8 bytes for the last entry in the append vec.
/// A pubkey alone is 32 bytes, so there is no way for a valid offset to be this high of a value.
/// Realistically, a max offset is (1<<31 - 156) bytes or so for an account with zero data length. Of course, this
/// depends on the layout on disk, compression, etc. But, 8 bytes per account will never be possible.
/// So, we use this last value as a sentinel to say that the account info refers to an entry in the write cache.
const CACHED_OFFSET: OffsetReduced = (1 << (OffsetReduced::BITS - 1)) - 1;

#[bitfield(bits = 32)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct PackedOffsetAndFlags {
    /// this provides 2^31 bits, which when multipled by 8 (sizeof(u64)) = 16G, which is the maximum size of an append vec
    offset_reduced: B31,
    /// use 1 bit to specify that the entry is zero lamport
    is_zero_lamport: bool,
}

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct AccountInfo {
    /// index identifying the append storage
    store_id: AppendVecId,

    account_offset_and_flags: AccountOffsetAndFlags,
}

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct AccountOffsetAndFlags {
    /// offset = 'packed_offset_and_flags.offset_reduced()' * ALIGN_BOUNDARY_OFFSET into the storage
    /// Note this is a smaller type than 'Offset'
    packed_offset_and_flags: PackedOffsetAndFlags,
}

impl ZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.account_offset_and_flags
            .packed_offset_and_flags
            .is_zero_lamport()
    }
}

impl IsCached for AccountInfo {
    fn is_cached(&self) -> bool {
        self.account_offset_and_flags
            .packed_offset_and_flags
            .offset_reduced()
            == CACHED_OFFSET
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
    pub fn new(storage_location: StorageLocation, lamports: u64) -> Self {
        let mut packed_offset_and_flags = PackedOffsetAndFlags::default();
        let store_id = match storage_location {
            StorageLocation::AppendVec(store_id, offset) => {
                let reduced_offset = Self::get_reduced_offset(offset);
                assert_ne!(
                    CACHED_OFFSET, reduced_offset,
                    "illegal offset for non-cached item"
                );
                packed_offset_and_flags.set_offset_reduced(Self::get_reduced_offset(offset));
                assert_eq!(
                    Self::reduced_offset_to_offset(packed_offset_and_flags.offset_reduced()),
                    offset,
                    "illegal offset"
                );
                store_id
            }
            StorageLocation::Cached => {
                packed_offset_and_flags.set_offset_reduced(CACHED_OFFSET);
                CACHE_VIRTUAL_STORAGE_ID
            }
        };
        packed_offset_and_flags.set_is_zero_lamport(lamports == 0);
        let account_offset_and_flags = AccountOffsetAndFlags {
            packed_offset_and_flags,
        };
        Self {
            store_id,
            account_offset_and_flags,
        }
    }

    fn get_reduced_offset(offset: usize) -> OffsetReduced {
        (offset / ALIGN_BOUNDARY_OFFSET) as OffsetReduced
    }

    pub fn store_id(&self) -> AppendVecId {
        // if the account is in a cached store, the store_id is meaningless
        assert!(!self.is_cached());
        self.store_id
    }

    pub fn offset(&self) -> Offset {
        Self::reduced_offset_to_offset(
            self.account_offset_and_flags
                .packed_offset_and_flags
                .offset_reduced(),
        )
    }

    fn reduced_offset_to_offset(reduced_offset: OffsetReduced) -> Offset {
        (reduced_offset as Offset) * ALIGN_BOUNDARY_OFFSET
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
            // MAXIMUM_APPEND_VEC_FILE_SIZE is too big. That would be an offset at the first invalid byte in the max file size.
            // MAXIMUM_APPEND_VEC_FILE_SIZE - 8 bytes would reference the very last 8 bytes in the file size. It makes no sense to reference that since element sizes are always more than 8.
            // MAXIMUM_APPEND_VEC_FILE_SIZE - 16 bytes would reference the second to last 8 bytes in the max file size. This is still likely meaningless, but it is 'valid' as far as the index
            // is concerned.
            (MAXIMUM_APPEND_VEC_FILE_SIZE - 2 * (ALIGN_BOUNDARY_OFFSET as u64)) as Offset,
            0,
            ALIGN_BOUNDARY_OFFSET,
            4 * ALIGN_BOUNDARY_OFFSET,
        ] {
            let info = AccountInfo::new(StorageLocation::AppendVec(0, offset), 0);
            assert!(info.offset() == offset);
        }
    }

    #[test]
    #[should_panic(expected = "illegal offset")]
    fn test_illegal_offset() {
        let offset = (MAXIMUM_APPEND_VEC_FILE_SIZE - (ALIGN_BOUNDARY_OFFSET as u64)) as Offset;
        AccountInfo::new(StorageLocation::AppendVec(0, offset), 0);
    }

    #[test]
    #[should_panic(expected = "illegal offset")]
    fn test_alignment() {
        let offset = 1; // not aligned
        AccountInfo::new(StorageLocation::AppendVec(0, offset), 0);
    }
}
