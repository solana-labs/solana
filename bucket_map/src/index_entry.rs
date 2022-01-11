use {
    crate::{
        bucket::Bucket,
        bucket_storage::{BucketStorage, Uid},
        RefCount,
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::hash_map::DefaultHasher,
        fmt::Debug,
        hash::{Hash, Hasher},
    },
};

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
// one instance of this per item in the index
// stored in the index bucket
pub struct IndexEntry {
    pub key: Pubkey, // can this be smaller if we have reduced the keys into buckets already?
    pub ref_count: RefCount, // can this be smaller? Do we ever need more than 4B refcounts?
    // storage_offset_mask contains both storage_offset and storage_capacity_when_created_pow2
    // see _MASK_ constants below
    storage_offset_mask: u64, // smaller? since these are variably sized, this could get tricky. well, actually accountinfo is not variable sized...
    // if the bucket doubled, the index can be recomputed using create_bucket_capacity_pow2
    pub num_slots: Slot, // can this be smaller? epoch size should ~ be the max len. this is the num elements in the slot list
}

/// how many bits to shift the capacity value in the mask
const STORAGE_OFFSET_MASK_CAPACITY_SHIFT: u64 = (u64::BITS - u8::BITS) as u64;
/// mask to use on 'storage_offset_mask' to get the 'storage_offset' portion
const STORAGE_OFFSET_MASK_STORAGE_OFFSET: u64 = (1 << STORAGE_OFFSET_MASK_CAPACITY_SHIFT) - 1;
impl IndexEntry {
    pub fn init(&mut self, pubkey: &Pubkey) {
        self.key = *pubkey;
        self.ref_count = 0;
        self.storage_offset_mask = 0;
        self.num_slots = 0;
    }
    pub fn set_storage_capacity_when_created_pow2(
        &mut self,
        storage_capacity_when_created_pow2: u8,
    ) {
        self.storage_offset_mask = self.storage_offset()
            | ((storage_capacity_when_created_pow2 as u64) << STORAGE_OFFSET_MASK_CAPACITY_SHIFT)
    }

    pub fn set_storage_offset(&mut self, storage_offset: u64) {
        let offset_mask = storage_offset & STORAGE_OFFSET_MASK_STORAGE_OFFSET;
        assert_eq!(storage_offset, offset_mask, "offset too large");
        self.storage_offset_mask = ((self.storage_capacity_when_created_pow2() as u64)
            << STORAGE_OFFSET_MASK_CAPACITY_SHIFT)
            | offset_mask;
    }

    pub fn data_bucket_from_num_slots(num_slots: Slot) -> u64 {
        (num_slots as f64).log2().ceil() as u64 // use int log here?
    }

    pub fn data_bucket_ix(&self) -> u64 {
        Self::data_bucket_from_num_slots(self.num_slots)
    }

    pub fn ref_count(&self) -> RefCount {
        self.ref_count
    }

    fn storage_offset(&self) -> u64 {
        self.storage_offset_mask & STORAGE_OFFSET_MASK_STORAGE_OFFSET
    }
    fn storage_capacity_when_created_pow2(&self) -> u8 {
        (self.storage_offset_mask >> STORAGE_OFFSET_MASK_CAPACITY_SHIFT) as u8
    }

    // This function maps the original data location into an index in the current bucket storage.
    // This is coupled with how we resize bucket storages.
    pub fn data_loc(&self, storage: &BucketStorage) -> u64 {
        self.storage_offset() << (storage.capacity_pow2 - self.storage_capacity_when_created_pow2())
    }

    pub fn read_value<'a, T>(&self, bucket: &'a Bucket<T>) -> Option<(&'a [T], RefCount)> {
        let data_bucket_ix = self.data_bucket_ix();
        let data_bucket = &bucket.data[data_bucket_ix as usize];
        let slice = if self.num_slots > 0 {
            let loc = self.data_loc(data_bucket);
            let uid = Self::key_uid(&self.key);
            assert_eq!(Some(uid), bucket.data[data_bucket_ix as usize].uid(loc));
            bucket.data[data_bucket_ix as usize].get_cell_slice(loc, self.num_slots)
        } else {
            // num_slots is 0. This means we don't have an actual allocation.
            // can we trust that the data_bucket is even safe?
            bucket.data[data_bucket_ix as usize].get_empty_cell_slice()
        };
        Some((slice, self.ref_count))
    }
    pub fn key_uid(key: &Pubkey) -> Uid {
        let mut s = DefaultHasher::new();
        key.hash(&mut s);
        s.finish().max(1u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl IndexEntry {
        pub fn new(key: Pubkey) -> Self {
            IndexEntry {
                key,
                ref_count: 0,
                storage_offset_mask: 0,
                num_slots: 0,
            }
        }
    }

    /// verify that accessors for storage_offset and capacity_when_created are
    /// correct and independent
    #[test]
    fn test_api() {
        for offset in [0, 1, u32::MAX as u64] {
            let mut index = IndexEntry::new(solana_sdk::pubkey::new_rand());
            if offset != 0 {
                index.set_storage_offset(offset);
            }
            assert_eq!(index.storage_offset(), offset);
            assert_eq!(index.storage_capacity_when_created_pow2(), 0);
            for pow in [1, 255, 0] {
                index.set_storage_capacity_when_created_pow2(pow);
                assert_eq!(index.storage_offset(), offset);
                assert_eq!(index.storage_capacity_when_created_pow2(), pow);
            }
        }
    }
}
