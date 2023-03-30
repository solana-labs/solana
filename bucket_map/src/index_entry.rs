#![allow(dead_code)]

use {
    crate::{
        bucket_storage::{BucketOccupied, BucketStorage},
        RefCount,
    },
    bv::BitVec,
    modular_bitfield::prelude::*,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{fmt::Debug, marker::PhantomData},
};

/// in use/occupied
const OCCUPIED_OCCUPIED: u64 = 1;
/// free, ie. not occupied
const OCCUPIED_FREE: u64 = 0;

/// header for elements in a bucket
/// needs to be multiple of size_of::<u64>()
#[repr(C)]
struct OccupiedHeader {
    /// OCCUPIED_OCCUPIED or OCCUPIED_FREE
    occupied: u64,
}

/// allocated in `contents` in a BucketStorage
pub struct BucketWithBitVec<T: 'static> {
    pub occupied: BitVec,
    _phantom: PhantomData<&'static T>,
}

impl<T> BucketOccupied for BucketWithBitVec<T> {
    fn occupy(&mut self, element: &mut [u8], ix: usize) {
        assert!(self.is_free(element, ix));
        self.occupied.set(ix as u64, true);
    }
    fn free(&mut self, element: &mut [u8], ix: usize) {
        assert!(!self.is_free(element, ix));
        self.occupied.set(ix as u64, false);
    }
    fn is_free(&self, _element: &[u8], ix: usize) -> bool {
        !self.occupied.get(ix as u64)
    }
    fn offset_to_first_data() -> usize {
        // no header, nothing stored in data stream
        0
    }
    fn new(num_elements: usize) -> Self {
        Self {
            occupied: BitVec::new_fill(false, num_elements as u64),
            _phantom: PhantomData,
        }
    }
}

pub type DataBucket = BucketWithBitVec<()>;
pub type IndexBucket<T> = BucketWithBitVec<T>;

/// contains the index of an entry in the index bucket.
/// This type allows us to call methods to interact with the index entry on this type.
pub struct IndexEntryPlaceInBucket<T: 'static> {
    pub ix: u64,
    _phantom: PhantomData<&'static T>,
}

#[repr(C)]
#[derive(Copy, Clone)]
// one instance of this per item in the index
// stored in the index bucket
pub struct IndexEntry<T: 'static> {
    pub key: Pubkey, // can this be smaller if we have reduced the keys into buckets already?
    ref_count: RefCount, // can this be smaller? Do we ever need more than 4B refcounts?
    multiple_slots: MultipleSlots,
    _phantom: PhantomData<&'static T>,
}

/// required fields when an index element references the data file
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub(crate) struct MultipleSlots {
    // if the bucket doubled, the index can be recomputed using storage_cap_and_offset.create_bucket_capacity_pow2
    storage_cap_and_offset: PackedStorage,
    /// num elements in the slot list
    num_slots: Slot, // can this be smaller? epoch size should ~ be the max len. this is the num elements in the slot list
}

impl MultipleSlots {
    pub(crate) fn set_storage_capacity_when_created_pow2(
        &mut self,
        storage_capacity_when_created_pow2: u8,
    ) {
        self.storage_cap_and_offset
            .set_capacity_when_created_pow2(storage_capacity_when_created_pow2)
    }

    pub(crate) fn set_storage_offset(&mut self, storage_offset: u64) {
        self.storage_cap_and_offset
            .set_offset_checked(storage_offset)
            .expect("New storage offset must fit into 7 bytes!")
    }

    fn storage_capacity_when_created_pow2(&self) -> u8 {
        self.storage_cap_and_offset.capacity_when_created_pow2()
    }

    fn storage_offset(&self) -> u64 {
        self.storage_cap_and_offset.offset()
    }

    pub(crate) fn num_slots(&self) -> Slot {
        self.num_slots
    }

    pub(crate) fn set_num_slots(&mut self, num_slots: Slot) {
        self.num_slots = num_slots;
    }

    pub(crate) fn data_bucket_ix(&self) -> u64 {
        Self::data_bucket_from_num_slots(self.num_slots())
    }

    /// return closest bucket index fit for the slot slice.
    /// Since bucket size is 2^index, the return value is
    ///     min index, such that 2^index >= num_slots
    ///     index = ceiling(log2(num_slots))
    /// special case, when slot slice empty, return 0th index.
    pub(crate) fn data_bucket_from_num_slots(num_slots: Slot) -> u64 {
        // Compute the ceiling of log2 for integer
        if num_slots == 0 {
            0
        } else {
            (Slot::BITS - (num_slots - 1).leading_zeros()) as u64
        }
    }
}

/// Pack the storage offset and capacity-when-crated-pow2 fields into a single u64
#[bitfield(bits = 64)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
struct PackedStorage {
    capacity_when_created_pow2: B8,
    offset: B56,
}

impl<T> IndexEntryPlaceInBucket<T> {
    pub fn init(&self, index_bucket: &mut BucketStorage<IndexBucket<T>>, pubkey: &Pubkey) {
        let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
        index_entry.key = *pubkey;
        index_entry.ref_count = 0;
        index_entry.multiple_slots = MultipleSlots::default();
    }

    pub fn set_storage_capacity_when_created_pow2(
        &self,
        index_bucket: &mut BucketStorage<IndexBucket<T>>,
        storage_capacity_when_created_pow2: u8,
    ) {
        self.get_multiple_slots_mut(index_bucket)
            .set_storage_capacity_when_created_pow2(storage_capacity_when_created_pow2);
    }

    pub fn set_storage_offset(
        &self,
        index_bucket: &mut BucketStorage<IndexBucket<T>>,
        storage_offset: u64,
    ) {
        self.get_multiple_slots_mut(index_bucket)
            .set_storage_offset(storage_offset);
    }

    pub(crate) fn get_multiple_slots<'a>(
        &self,
        index_bucket: &'a BucketStorage<IndexBucket<T>>,
    ) -> &'a MultipleSlots {
        &index_bucket.get::<IndexEntry<T>>(self.ix).multiple_slots
    }

    pub(crate) fn get_multiple_slots_mut<'a>(
        &self,
        index_bucket: &'a mut BucketStorage<IndexBucket<T>>,
    ) -> &'a mut MultipleSlots {
        &mut index_bucket
            .get_mut::<IndexEntry<T>>(self.ix)
            .multiple_slots
    }

    pub fn ref_count(&self, index_bucket: &BucketStorage<IndexBucket<T>>) -> RefCount {
        let index_entry = index_bucket.get::<IndexEntry<T>>(self.ix);
        index_entry.ref_count
    }

    /// This function maps the original data location into an index in the current bucket storage.
    /// This is coupled with how we resize bucket storages.
    pub fn data_loc(
        &self,
        index_bucket: &BucketStorage<IndexBucket<T>>,
        storage: &BucketStorage<DataBucket>,
    ) -> u64 {
        let multiple_slots = self.get_multiple_slots(index_bucket);
        multiple_slots.storage_offset()
            << (storage.capacity_pow2 - multiple_slots.storage_capacity_when_created_pow2())
    }

    pub fn read_value<'a>(
        &self,
        index_bucket: &BucketStorage<IndexBucket<T>>,
        data_buckets: &'a [BucketStorage<DataBucket>],
    ) -> Option<(&'a [T], RefCount)> {
        let multiple_slots = self.get_multiple_slots(index_bucket);
        let num_slots = multiple_slots.num_slots();
        let slice = if num_slots > 0 {
            let data_bucket_ix = multiple_slots.data_bucket_ix();
            let data_bucket = &data_buckets[data_bucket_ix as usize];
            let loc = self.data_loc(index_bucket, data_bucket);
            assert!(!data_bucket.is_free(loc));
            data_bucket.get_cell_slice(loc, num_slots)
        } else {
            // num_slots is 0. This means we don't have an actual allocation.
            &[]
        };
        Some((slice, self.ref_count(index_bucket)))
    }

    pub fn new(ix: u64) -> Self {
        Self {
            ix,
            _phantom: PhantomData,
        }
    }

    pub fn key<'a>(&self, index_bucket: &'a BucketStorage<IndexBucket<T>>) -> &'a Pubkey {
        let entry: &IndexEntry<T> = index_bucket.get(self.ix);
        &entry.key
    }

    pub fn set_ref_count(
        &self,
        index_bucket: &mut BucketStorage<IndexBucket<T>>,
        ref_count: RefCount,
    ) {
        let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
        index_entry.ref_count = ref_count;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{path::PathBuf, sync::Arc},
        tempfile::tempdir,
    };

    impl<T> IndexEntry<T> {
        pub fn new(key: Pubkey) -> Self {
            IndexEntry {
                key,
                ref_count: 0,
                multiple_slots: MultipleSlots::default(),
                _phantom: PhantomData,
            }
        }
    }

    /// verify that accessors for storage_offset and capacity_when_created are
    /// correct and independent
    #[test]
    fn test_api() {
        for offset in [0, 1, u32::MAX as u64] {
            let mut multiple_slots = MultipleSlots::default();

            if offset != 0 {
                multiple_slots.set_storage_offset(offset);
            }
            assert_eq!(multiple_slots.storage_offset(), offset);
            assert_eq!(multiple_slots.storage_capacity_when_created_pow2(), 0);
            for pow in [1, 255, 0] {
                multiple_slots.set_storage_capacity_when_created_pow2(pow);
                assert_eq!(multiple_slots.storage_offset(), offset);
                assert_eq!(multiple_slots.storage_capacity_when_created_pow2(), pow);
            }
        }
    }

    #[test]
    fn test_size() {
        assert_eq!(std::mem::size_of::<PackedStorage>(), 1 + 7);
        assert_eq!(std::mem::size_of::<IndexEntry<u64>>(), 32 + 8 + 8 + 8);
    }

    fn index_bucket_for_testing() -> BucketStorage<IndexBucket<u64>> {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());

        // `new` here creates a file in `tmpdir`. Once the file is created, `tmpdir` can be dropped without issue.
        BucketStorage::<IndexBucket<u64>>::new(
            Arc::new(paths),
            1,
            std::mem::size_of::<IndexEntry<u64>>() as u64,
            1,
            Arc::default(),
            Arc::default(),
        )
    }

    fn index_entry_for_testing() -> (
        BucketStorage<IndexBucket<u64>>,
        IndexEntryPlaceInBucket<u64>,
    ) {
        (index_bucket_for_testing(), IndexEntryPlaceInBucket::new(0))
    }

    #[test]
    #[should_panic(expected = "New storage offset must fit into 7 bytes!")]
    fn test_set_storage_offset_value_too_large() {
        let too_big = 1 << 56;
        let mut multiple_slots = MultipleSlots::default();
        multiple_slots.set_storage_offset(too_big);
    }

    #[test]
    fn test_data_bucket_from_num_slots() {
        for n in 0..512 {
            assert_eq!(
                MultipleSlots::data_bucket_from_num_slots(n),
                (n as f64).log2().ceil() as u64
            );
        }
        assert_eq!(
            MultipleSlots::data_bucket_from_num_slots(u32::MAX as u64),
            32
        );
        assert_eq!(
            MultipleSlots::data_bucket_from_num_slots(u32::MAX as u64 + 1),
            32
        );
        assert_eq!(
            MultipleSlots::data_bucket_from_num_slots(u32::MAX as u64 + 2),
            33
        );
    }
}
