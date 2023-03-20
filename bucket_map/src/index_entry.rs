#![allow(dead_code)]

use std::marker::PhantomData;

use {
    crate::{
        bucket::Bucket,
        bucket_storage::{BucketOccupied, BucketStorage},
        RefCount,
    },
    bv::BitVec,
    modular_bitfield::prelude::*,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::fmt::Debug,
};

/// header for elements in a bucket
#[repr(C)]
struct OccupiedHeader {
    /// non-zero if occupied, 0 if occupied
    occupied: u64,
}

/// allocated in `contents` in a BucketStorage
#[derive(Debug, Default)]
pub struct BucketWithBitVec {
    pub occupied: BitVec,
}

impl BucketOccupied for BucketWithBitVec {
    fn occupy(&mut self, _element: &mut [u8], ix: usize) {
        self.occupied.set(ix as u64, true);
    }
    fn free(&mut self, _element: &mut [u8], ix: usize) {
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
        }
    }
}

/// allocated in `contents` in a BucketStorage
#[derive(Debug, Default)]
pub struct IndexBucketUsingRefCountBits<T> {
    _phantom: PhantomData<T>,
}

impl<T: Copy + 'static> BucketOccupied for IndexBucketUsingRefCountBits<T> {
    fn occupy(&mut self, element: &mut [u8], _ix: usize) {
        let entry: &mut IndexEntry<T> =
            BucketStorage::<IndexBucketUsingRefCountBits<T>>::get_mut_from_parts(element);
        assert!(matches!(entry.get_slot_count_enum(), SlotCountEnum::Free));
        entry.set_slot_count_enum_value(SlotCountEnum::ZeroSlots);
    }
    fn free(&mut self, element: &mut [u8], _ix: usize) {
        let entry: &mut IndexEntry<T> =
            BucketStorage::<IndexBucketUsingRefCountBits<T>>::get_mut_from_parts(element);
        assert!(!matches!(entry.get_slot_count_enum(), SlotCountEnum::Free));
        entry.set_slot_count_enum_value(SlotCountEnum::Free);
    }
    fn is_free(&self, element: &[u8], _ix: usize) -> bool {
        let entry: &IndexEntry<T> =
            BucketStorage::<IndexBucketUsingRefCountBits<T>>::get_from_parts(element);
        matches!(entry.get_slot_count_enum(), SlotCountEnum::Free)
    }
    fn offset_to_first_data() -> usize {
        0
    }
    fn new(_num_elements: usize) -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

pub type DataBucket = BucketWithBitVec;
pub type IndexBucket<T> = IndexBucketUsingRefCountBits<T>;

#[repr(C)]
#[derive(Copy, Clone)]
// one instance of this per item in the index
// stored in the index bucket
pub struct IndexEntry<T: Clone + Copy> {
    pub(crate) key: Pubkey, // can this be smaller if we have reduced the keys into buckets already?
    packed_ref_count: PackedRefCount,
    /// depends on the contents of ref_count.slot_count_enum
    pub(crate) contents: SingleElementOrMultipleSlots<T>,
}

#[bitfield(bits = 64)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub(crate) struct PackedRefCount {
    /// tag for `SlotCountEnum`
    pub(crate) slot_count_enum: B2,
    /// ref_count of this entry. We don't need any where near 62 bits for this value
    pub(crate) ref_count: B62,
}

/// required fields when an index element references the data file
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub(crate) struct MultipleSlots {
    // if the bucket doubled, the index can be recomputed using storage_cap_and_offset.create_bucket_capacity_pow2
    pub(crate) storage_cap_and_offset: PackedStorage,
    /// num elements in the slot list
    pub num_slots: Slot,
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

    pub(crate) fn storage_capacity_when_created_pow2(&self) -> u8 {
        self.storage_cap_and_offset.capacity_when_created_pow2()
    }

    fn storage_offset(&self) -> u64 {
        self.storage_cap_and_offset.offset()
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) union SingleElementOrMultipleSlots<T: Clone + Copy> {
    /// the slot list contains a single element. No need for an entry in the data file.
    pub(crate) single_element: T,
    /// the slot list ocntains more than one element. This contains the reference to the data file.
    pub(crate) multiple_slots: MultipleSlots,
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum SlotCountEnum<'a, T> {
    /// this spot is not allocated
    Free = 0,
    /// zero slots in the slot list
    ZeroSlots = 1,
    /// one slot in the slot list, it is stored in the index
    OneSlotInIndex(&'a T) = 2,
    /// > 1 slots, slots are stored in data file
    MultipleSlots(&'a MultipleSlots) = 3,
}

/// Pack the storage offset and capacity-when-crated-pow2 fields into a single u64
#[bitfield(bits = 64)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub(crate) struct PackedStorage {
    capacity_when_created_pow2: B8,
    offset: B56,
}

impl<T: Clone + Copy + 'static> IndexEntry<T> {
    pub fn init(&mut self, pubkey: &Pubkey) {
        self.key = *pubkey;
        self.packed_ref_count.set_ref_count(0);
        self.set_slot_count_enum_value(SlotCountEnum::ZeroSlots);
    }

    pub(crate) fn set_ref_count(&mut self, ref_count: RefCount) {
        self.packed_ref_count
            .set_ref_count_checked(ref_count)
            .expect("ref count must fit into 62 bits!")
    }

    pub(crate) fn get_slot_count_enum(&self) -> SlotCountEnum<'_, T> {
        unsafe {
            match self.packed_ref_count.slot_count_enum() {
                0 => SlotCountEnum::Free,
                1 => SlotCountEnum::ZeroSlots,
                2 => SlotCountEnum::OneSlotInIndex(&self.contents.single_element),
                3 => SlotCountEnum::MultipleSlots(&self.contents.multiple_slots),
                _ => {
                    panic!("unexpected value");
                }
            }
        }
    }

    pub(crate) fn get_multiple_slots_mut(&mut self) -> Option<&mut MultipleSlots> {
        unsafe {
            match self.packed_ref_count.slot_count_enum() {
                3 => Some(&mut self.contents.multiple_slots),
                _ => None,
            }
        }
    }

    pub(crate) fn set_slot_count_enum_value<'a>(&'a mut self, value: SlotCountEnum<'a, T>) {
        self.packed_ref_count.set_slot_count_enum(match value {
            SlotCountEnum::Free => 0,
            SlotCountEnum::ZeroSlots => 1,
            SlotCountEnum::OneSlotInIndex(single_element) => {
                self.contents.single_element = *single_element;
                2
            }
            SlotCountEnum::MultipleSlots(multiple_slots) => {
                self.contents.multiple_slots = *multiple_slots;
                3
            }
        });
    }

    /// return closest bucket index fit for the slot slice.
    /// Since bucket size is 2^index, the return value is
    ///     min index, such that 2^index >= num_slots
    ///     index = ceiling(log2(num_slots))
    /// special case, when slot slice empty, return 0th index.
    pub fn data_bucket_from_num_slots(num_slots: Slot) -> u64 {
        // Compute the ceiling of log2 for integer
        if num_slots == 0 {
            0
        } else {
            (Slot::BITS - (num_slots - 1).leading_zeros()) as u64
        }
    }

    pub fn ref_count(&self) -> RefCount {
        self.packed_ref_count.ref_count()
    }

    // This function maps the original data location into an index in the current bucket storage.
    // This is coupled with how we resize bucket storages.
    pub(crate) fn data_loc(
        storage: &BucketStorage<DataBucket>,
        multiple_slots: &MultipleSlots,
    ) -> u64 {
        multiple_slots.storage_offset()
            << (storage.capacity_pow2 - multiple_slots.storage_capacity_when_created_pow2())
    }

    pub fn read_value<'a>(&'a self, bucket: &'a Bucket<T>) -> Option<(&'a [T], RefCount)> {
        Some((
            match self.get_slot_count_enum() {
                SlotCountEnum::ZeroSlots => {
                    // num_slots is 0. This means we don't have an actual allocation.
                    &[]
                }
                SlotCountEnum::OneSlotInIndex(single_element) => {
                    // only element is stored in the index entry
                    std::slice::from_ref(single_element)
                }
                SlotCountEnum::MultipleSlots(multiple_slots) => {
                    let data_bucket_ix = Self::data_bucket_from_num_slots(multiple_slots.num_slots);
                    let data_bucket = &bucket.data[data_bucket_ix as usize];
                    let loc = Self::data_loc(data_bucket, multiple_slots);
                    assert!(!data_bucket.is_free(loc));
                    data_bucket.get_cell_slice::<T>(loc, multiple_slots.num_slots)
                }
                _ => {
                    unimplemented!();
                }
            },
            self.ref_count(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<T: Clone + Copy + Debug> IndexEntry<T> {
        pub fn new(key: Pubkey) -> Self {
            IndexEntry {
                key,
                packed_ref_count: PackedRefCount::default(),
                contents: SingleElementOrMultipleSlots {
                    multiple_slots: MultipleSlots {
                        storage_cap_and_offset: PackedStorage::default(),
                        num_slots: 0,
                    },
                },
            }
        }
    }

    /// verify that accessors for storage_offset and capacity_when_created are
    /// correct and independent
    #[test]
    fn test_api() {
        for offset in [0, 1, u32::MAX as u64] {
            let mut index = MultipleSlots::default();
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

    #[test]
    fn test_size() {
        assert_eq!(std::mem::size_of::<PackedStorage>(), 1 + 7);
        assert_eq!(
            std::mem::size_of::<IndexEntry::<u64>>(),
            32 + 8 + (8 + 8).max(std::mem::size_of::<u64>())
        );
    }

    #[test]
    #[should_panic(expected = "New storage offset must fit into 7 bytes!")]
    fn test_set_storage_offset_value_too_large() {
        let too_big = 1 << 56;
        let mut multiple_slots = MultipleSlots::default();
        multiple_slots.set_storage_offset(too_big);
    }

    #[test]
    #[should_panic(expected = "ref count must fit into 62 bits!")]
    fn test_set_ref_count_too_large() {
        let too_big = 1 << 62;
        let mut index = IndexEntry::<u64>::new(Pubkey::default());
        index.set_ref_count(too_big);
    }

    #[test]
    fn test_data_bucket_from_num_slots() {
        for n in 0..512 {
            assert_eq!(
                IndexEntry::<u64>::data_bucket_from_num_slots(n),
                (n as f64).log2().ceil() as u64
            );
        }
        assert_eq!(
            IndexEntry::<u64>::data_bucket_from_num_slots(u32::MAX as u64),
            32
        );
        assert_eq!(
            IndexEntry::<u64>::data_bucket_from_num_slots(u32::MAX as u64 + 1),
            32
        );
        assert_eq!(
            IndexEntry::<u64>::data_bucket_from_num_slots(u32::MAX as u64 + 2),
            33
        );
    }
}
