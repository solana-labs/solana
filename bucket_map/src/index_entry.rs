#![allow(dead_code)]

use {
    crate::{
        bucket_storage::{BucketOccupied, BucketStorage, IncludeHeader},
        RefCount,
    },
    bv::BitVec,
    modular_bitfield::prelude::*,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{fmt::Debug, marker::PhantomData},
};

/// allocated in `contents` in a BucketStorage
pub struct BucketWithBitVec {
    pub occupied: BitVec,
}

impl BucketOccupied for BucketWithBitVec {
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
        }
    }
}

#[derive(Debug, Default)]
pub struct IndexBucketUsingRefCountBits<T: 'static> {
    _phantom: PhantomData<&'static T>,
}

impl<T: Copy> BucketOccupied for IndexBucketUsingRefCountBits<T> {
    fn occupy(&mut self, element: &mut [u8], ix: usize) {
        assert!(self.is_free(element, ix));
        let entry: &mut IndexEntry<T> =
            BucketStorage::<IndexBucketUsingRefCountBits<T>>::get_mut_from_parts(element);
        entry.set_slot_count_enum_value(OccupiedEnum::ZeroSlots);
    }
    fn free(&mut self, element: &mut [u8], ix: usize) {
        assert!(!self.is_free(element, ix));
        let entry: &mut IndexEntry<T> =
            BucketStorage::<IndexBucketUsingRefCountBits<T>>::get_mut_from_parts(element);
        entry.set_slot_count_enum_value(OccupiedEnum::Free);
    }
    fn is_free(&self, element: &[u8], _ix: usize) -> bool {
        let entry: &IndexEntry<T> =
            BucketStorage::<IndexBucketUsingRefCountBits<T>>::get_from_parts(element);
        matches!(entry.get_slot_count_enum(), OccupiedEnum::Free)
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

/// contains the index of an entry in the index bucket.
/// This type allows us to call methods to interact with the index entry on this type.
pub struct IndexEntryPlaceInBucket<T: 'static> {
    pub ix: u64,
    _phantom: PhantomData<&'static T>,
}

#[repr(C)]
#[derive(Copy, Clone)]
/// one instance of this per item in the index
/// stored in the index bucket
pub struct IndexEntry<T: Clone + Copy> {
    pub(crate) key: Pubkey, // can this be smaller if we have reduced the keys into buckets already?
    packed_ref_count: PackedRefCount,
    /// depends on the contents of ref_count.slot_count_enum
    contents: SingleElementOrMultipleSlots<T>,
}

/// 62 bits available for ref count
pub(crate) const MAX_LEGAL_REFCOUNT: RefCount = RefCount::MAX >> 2;

/// hold a big `RefCount` while leaving room for extra bits to be used for things like 'Occupied'
#[bitfield(bits = 64)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
struct PackedRefCount {
    /// tag for Enum
    slot_count_enum: B2,
    /// ref_count of this entry. We don't need any where near 62 bits for this value
    ref_count: B62,
}

/// required fields when an index element references the data file
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub(crate) struct MultipleSlots {
    // if the bucket doubled, the index can be recomputed using storage_cap_and_offset.create_bucket_capacity_pow2
    storage_cap_and_offset: PackedStorage,
    /// num elements in the slot list
    num_slots: Slot,
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

    /// This function maps the original data location into an index in the current bucket storage.
    /// This is coupled with how we resize bucket storages.
    pub(crate) fn data_loc(&self, storage: &BucketStorage<DataBucket>) -> u64 {
        self.storage_offset() << (storage.capacity_pow2 - self.storage_capacity_when_created_pow2())
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) union SingleElementOrMultipleSlots<T: Clone + Copy> {
    /// the slot list contains a single element. No need for an entry in the data file.
    /// The element itself is stored in place in the index entry
    pub(crate) single_element: T,
    /// the slot list ocntains more than one element. This contains the reference to the data file.
    pub(crate) multiple_slots: MultipleSlots,
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum OccupiedEnum<'a, T> {
    /// this spot is not occupied.
    /// ALL other enum values ARE occupied.
    Free = 0,
    /// zero slots in the slot list
    ZeroSlots = 1,
    /// one slot in the slot list, it is stored in the index
    OneSlotInIndex(&'a T) = 2,
    /// data is stored in data file
    MultipleSlots(&'a MultipleSlots) = 3,
}

impl<T: Copy> IndexEntry<T> {
    pub(crate) fn get_slot_count_enum(&self) -> OccupiedEnum<'_, T> {
        unsafe {
            match self.packed_ref_count.slot_count_enum() {
                0 => OccupiedEnum::Free,
                1 => OccupiedEnum::ZeroSlots,
                2 => OccupiedEnum::OneSlotInIndex(&self.contents.single_element),
                3 => OccupiedEnum::MultipleSlots(&self.contents.multiple_slots),
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

    pub(crate) fn set_slot_count_enum_value<'a>(&'a mut self, value: OccupiedEnum<'a, T>) {
        self.packed_ref_count.set_slot_count_enum(match value {
            OccupiedEnum::Free => 0,
            OccupiedEnum::ZeroSlots => 1,
            OccupiedEnum::OneSlotInIndex(single_element) => {
                self.contents.single_element = *single_element;
                2
            }
            OccupiedEnum::MultipleSlots(multiple_slots) => {
                self.contents.multiple_slots = *multiple_slots;
                3
            }
        });
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

impl<T: Copy> IndexEntryPlaceInBucket<T> {
    pub fn init(&self, index_bucket: &mut BucketStorage<IndexBucket<T>>, pubkey: &Pubkey) {
        self.set_slot_count_enum_value(index_bucket, OccupiedEnum::ZeroSlots);
        let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
        index_entry.key = *pubkey;
        index_entry.packed_ref_count.set_ref_count(0);
    }

    /// return Some(MultipleSlots) if this item's data is stored in the data file
    pub(crate) fn get_multiple_slots_mut<'a>(
        &self,
        index_bucket: &'a mut BucketStorage<IndexBucket<T>>,
    ) -> Option<&'a mut MultipleSlots> {
        let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
        index_entry.get_multiple_slots_mut()
    }

    pub(crate) fn get_slot_count_enum<'a>(
        &self,
        index_bucket: &'a BucketStorage<IndexBucket<T>>,
    ) -> OccupiedEnum<'a, T> {
        let index_entry = index_bucket.get::<IndexEntry<T>>(self.ix);
        index_entry.get_slot_count_enum()
    }

    /// make this index entry reflect `value`
    pub(crate) fn set_slot_count_enum_value<'a>(
        &self,
        index_bucket: &'a mut BucketStorage<IndexBucket<T>>,
        value: OccupiedEnum<'a, T>,
    ) {
        let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
        index_entry.set_slot_count_enum_value(value);
    }

    fn ref_count(&self, index_bucket: &BucketStorage<IndexBucket<T>>) -> RefCount {
        let index_entry = index_bucket.get::<IndexEntry<T>>(self.ix);
        index_entry.packed_ref_count.ref_count()
    }

    pub(crate) fn read_value<'a>(
        &self,
        index_bucket: &'a BucketStorage<IndexBucket<T>>,
        data_buckets: &'a [BucketStorage<DataBucket>],
    ) -> (&'a [T], RefCount) {
        (
            match self.get_slot_count_enum(index_bucket) {
                OccupiedEnum::ZeroSlots => {
                    // num_slots is 0. This means we don't have an actual allocation.
                    &[]
                }
                OccupiedEnum::OneSlotInIndex(single_element) => {
                    // only element is stored in the index entry
                    // Note that the lifetime comes from `index_bucket` here.
                    std::slice::from_ref(single_element)
                }
                OccupiedEnum::MultipleSlots(multiple_slots) => {
                    // data is in data file, so return a ref to that data
                    let data_bucket_ix = multiple_slots.data_bucket_ix();
                    let data_bucket = &data_buckets[data_bucket_ix as usize];
                    let loc = multiple_slots.data_loc(data_bucket);
                    assert!(!data_bucket.is_free(loc));
                    data_bucket.get_cell_slice::<T>(
                        loc,
                        multiple_slots.num_slots,
                        IncludeHeader::NoHeader,
                    )
                }
                _ => {
                    panic!("trying to read data from a free entry");
                }
            },
            self.ref_count(index_bucket),
        )
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
        index_entry
            .packed_ref_count
            .set_ref_count_checked(ref_count)
            .expect("ref count must fit into 62 bits!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
