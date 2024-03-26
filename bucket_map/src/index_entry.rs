#![allow(dead_code)]

use {
    crate::{
        bucket_storage::{BucketCapacity, BucketOccupied, BucketStorage, Capacity, IncludeHeader},
        RefCount,
    },
    bv::BitVec,
    modular_bitfield::prelude::*,
    num_enum::FromPrimitive,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{fmt::Debug, marker::PhantomData},
};

/// in use/occupied
const OCCUPIED_OCCUPIED: u8 = 1;
/// free, ie. not occupied
const OCCUPIED_FREE: u8 = 0;

impl BucketCapacity for BucketWithHeader {
    fn capacity(&self) -> u64 {
        self.capacity_pow2.capacity()
    }
    fn capacity_pow2(&self) -> u8 {
        self.capacity_pow2.capacity_pow2()
    }
}

/// header for elements in a bucket
/// needs to be multiple of size_of::<u64>()
#[derive(Copy, Clone)]
#[repr(C)]
struct DataBucketRefCountOccupiedHeader {
    /// stores `ref_count` and
    /// occupied = OCCUPIED_OCCUPIED or OCCUPIED_FREE
    packed_ref_count: PackedRefCount,
}

#[derive(Debug, PartialEq)]
pub enum OccupyIfMatches {
    /// this entry is occupied and contains the same pubkey but with a different value, so this entry could not be updated
    FoundDuplicate,
    /// this entry was free and contains this pubkey and either value matched or the value was written to match
    SuccessfulInit,
    /// this entry had a different pubkey
    PubkeyMismatch,
}

/// allocated in `contents` in a BucketStorage
#[derive(Copy, Clone)]
#[repr(C)]
pub struct BucketWithHeader {
    capacity_pow2: Capacity,
}

impl BucketOccupied for BucketWithHeader {
    fn occupy(&mut self, element: &mut [u8], ix: usize) {
        assert!(self.is_free(element, ix));
        let entry = get_mut_from_bytes::<DataBucketRefCountOccupiedHeader>(element);
        entry.packed_ref_count.set_occupied(OCCUPIED_OCCUPIED);
    }
    fn free(&mut self, element: &mut [u8], ix: usize) {
        assert!(!self.is_free(element, ix));
        let entry = get_mut_from_bytes::<DataBucketRefCountOccupiedHeader>(element);
        entry.packed_ref_count.set_occupied(OCCUPIED_FREE);
    }
    fn is_free(&self, element: &[u8], _ix: usize) -> bool {
        let entry = get_from_bytes::<DataBucketRefCountOccupiedHeader>(element);
        entry.packed_ref_count.occupied() == OCCUPIED_FREE
    }
    fn offset_to_first_data() -> usize {
        std::mem::size_of::<DataBucketRefCountOccupiedHeader>()
    }
    fn new(capacity: Capacity) -> Self {
        assert!(matches!(capacity, Capacity::Pow2(_)));
        Self {
            capacity_pow2: capacity,
        }
    }
}

/// allocated in `contents` in a BucketStorage
#[derive(Debug)]
pub struct IndexBucketUsingBitVecBits<T: PartialEq + 'static> {
    /// 2 bits per entry that represent a 4 state enum tag
    pub enum_tag: BitVec,
    /// number of elements allocated
    capacity: u64,
    _phantom: PhantomData<&'static T>,
}

impl<T: Copy + PartialEq + 'static> IndexBucketUsingBitVecBits<T> {
    /// set the 2 bits (first and second) in `enum_tag`
    fn set_bits(&mut self, ix: u64, first: bool, second: bool) {
        self.enum_tag.set(ix * 2, first);
        self.enum_tag.set(ix * 2 + 1, second);
    }
    /// get the 2 bits (first and second) in `enum_tag`
    fn get_bits(&self, ix: u64) -> (bool, bool) {
        (self.enum_tag.get(ix * 2), self.enum_tag.get(ix * 2 + 1))
    }
    /// turn the tag into bits and store them
    fn set_enum_tag(&mut self, ix: u64, value: OccupiedEnumTag) {
        let value = value as u8;
        self.set_bits(ix, (value & 2) == 2, (value & 1) == 1);
    }
    /// read the bits and convert them to an enum tag
    fn get_enum_tag(&self, ix: u64) -> OccupiedEnumTag {
        let (first, second) = self.get_bits(ix);
        let tag = (first as u8 * 2) + second as u8;
        num_enum::FromPrimitive::from_primitive(tag)
    }
}

impl<T: Copy + PartialEq + 'static> BucketOccupied for IndexBucketUsingBitVecBits<T> {
    fn occupy(&mut self, element: &mut [u8], ix: usize) {
        assert!(self.is_free(element, ix));
        self.set_enum_tag(ix as u64, OccupiedEnumTag::ZeroSlots);
    }
    fn free(&mut self, element: &mut [u8], ix: usize) {
        assert!(!self.is_free(element, ix));
        self.set_enum_tag(ix as u64, OccupiedEnumTag::Free);
    }
    fn is_free(&self, _element: &[u8], ix: usize) -> bool {
        self.get_enum_tag(ix as u64) == OccupiedEnumTag::Free
    }
    fn offset_to_first_data() -> usize {
        // no header, nothing stored in data stream
        0
    }
    fn new(capacity: Capacity) -> Self {
        Self {
            // note: twice as many bits allocated as `num_elements` because we store 2 bits per element
            enum_tag: BitVec::new_fill(false, capacity.capacity() * 2),
            capacity: capacity.capacity(),
            _phantom: PhantomData,
        }
    }
    /// in this impl, the enum tag is stored in in-memory bit vec and there is more information than
    /// a single 'occupied' bit. So, this enum_tag needs to be copied over.
    fn copying_entry(
        &mut self,
        _element_new: &mut [u8],
        ix_new: usize,
        other: &Self,
        _element_old: &[u8],
        ix_old: usize,
    ) {
        self.set_enum_tag(ix_new as u64, other.get_enum_tag(ix_old as u64));
    }
}

impl<T: PartialEq> BucketCapacity for IndexBucketUsingBitVecBits<T> {
    fn capacity(&self) -> u64 {
        self.capacity
    }
}

pub type DataBucket = BucketWithHeader;
pub type IndexBucket<T> = IndexBucketUsingBitVecBits<T>;

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
    /// depends on the contents of ref_count.slot_count_enum
    contents: SingleElementOrMultipleSlots<T>,
}

/// 63 bits available for ref count
pub(crate) const MAX_LEGAL_REFCOUNT: RefCount = RefCount::MAX >> 1;

/// hold a big `RefCount` while leaving room for extra bits to be used for things like 'Occupied'
#[bitfield(bits = 64)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub(crate) struct PackedRefCount {
    /// whether this entry in the data file is occupied or not
    pub(crate) occupied: B1,
    /// ref_count of this entry. We don't need any where near 63 bits for this value
    pub(crate) ref_count: B63,
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
        self.storage_offset()
            << (storage.contents.capacity_pow2() - self.storage_capacity_when_created_pow2())
    }

    /// ref_count is stored in the header per cell, in `packed_ref_count`
    pub fn set_ref_count(
        data_bucket: &mut BucketStorage<DataBucket>,
        data_ix: u64,
        ref_count: RefCount,
    ) {
        data_bucket
            .get_header_mut::<DataBucketRefCountOccupiedHeader>(data_ix)
            .packed_ref_count
            .set_ref_count(ref_count);
    }

    /// ref_count is stored in the header per cell, in `packed_ref_count`
    pub fn ref_count(data_bucket: &BucketStorage<DataBucket>, data_ix: u64) -> RefCount {
        data_bucket
            .get_header::<DataBucketRefCountOccupiedHeader>(data_ix)
            .packed_ref_count
            .ref_count()
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) union SingleElementOrMultipleSlots<T: Clone + Copy> {
    /// the slot list contains a single element. No need for an entry in the data file.
    /// The element itself is stored in place in the index entry
    pub(crate) single_element: T,
    /// the slot list contains more than one element. This contains the reference to the data file.
    pub(crate) multiple_slots: MultipleSlots,
}

/// just the values for `OccupiedEnum`
/// This excludes the contents of any enum value.
#[derive(PartialEq, FromPrimitive, Debug)]
#[repr(u8)]
enum OccupiedEnumTag {
    #[default]
    Free = 0,
    ZeroSlots = 1,
    OneSlotInIndex = 2,
    MultipleSlots = 3,
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum OccupiedEnum<'a, T> {
    /// this spot is not occupied.
    /// ALL other enum values ARE occupied.
    Free = OccupiedEnumTag::Free as u8,
    /// zero slots in the slot list
    ZeroSlots = OccupiedEnumTag::ZeroSlots as u8,
    /// one slot in the slot list, it is stored in the index
    OneSlotInIndex(&'a T) = OccupiedEnumTag::OneSlotInIndex as u8,
    /// data is stored in data file
    MultipleSlots(&'a MultipleSlots) = OccupiedEnumTag::MultipleSlots as u8,
}

/// Pack the storage offset and capacity-when-created-pow2 fields into a single u64
#[bitfield(bits = 64)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
struct PackedStorage {
    capacity_when_created_pow2: B8,
    offset: B56,
}

impl<T: Copy + PartialEq + 'static> IndexEntryPlaceInBucket<T> {
    pub(crate) fn get_slot_count_enum<'a>(
        &self,
        index_bucket: &'a BucketStorage<IndexBucket<T>>,
    ) -> OccupiedEnum<'a, T> {
        let enum_tag = index_bucket.contents.get_enum_tag(self.ix);
        let index_entry = index_bucket.get::<IndexEntry<T>>(self.ix);
        match enum_tag {
            OccupiedEnumTag::Free => OccupiedEnum::Free,
            OccupiedEnumTag::ZeroSlots => OccupiedEnum::ZeroSlots,
            OccupiedEnumTag::OneSlotInIndex => unsafe {
                OccupiedEnum::OneSlotInIndex(&index_entry.contents.single_element)
            },
            OccupiedEnumTag::MultipleSlots => unsafe {
                OccupiedEnum::MultipleSlots(&index_entry.contents.multiple_slots)
            },
        }
    }

    /// return Some(MultipleSlots) if this item's data is stored in the data file
    pub(crate) fn get_multiple_slots_mut<'a>(
        &self,
        index_bucket: &'a mut BucketStorage<IndexBucket<T>>,
    ) -> Option<&'a mut MultipleSlots> {
        let enum_tag = index_bucket.contents.get_enum_tag(self.ix);
        unsafe {
            match enum_tag {
                OccupiedEnumTag::MultipleSlots => {
                    let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
                    Some(&mut index_entry.contents.multiple_slots)
                }
                _ => None,
            }
        }
    }

    /// make this index entry reflect `value`
    pub(crate) fn set_slot_count_enum_value<'a>(
        &self,
        index_bucket: &'a mut BucketStorage<IndexBucket<T>>,
        value: OccupiedEnum<'a, T>,
    ) {
        let tag = match value {
            OccupiedEnum::Free => OccupiedEnumTag::Free,
            OccupiedEnum::ZeroSlots => OccupiedEnumTag::ZeroSlots,
            OccupiedEnum::OneSlotInIndex(single_element) => {
                let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
                index_entry.contents.single_element = *single_element;
                OccupiedEnumTag::OneSlotInIndex
            }
            OccupiedEnum::MultipleSlots(multiple_slots) => {
                let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
                index_entry.contents.multiple_slots = *multiple_slots;
                OccupiedEnumTag::MultipleSlots
            }
        };
        index_bucket.contents.set_enum_tag(self.ix, tag);
    }

    pub fn init(&self, index_bucket: &mut BucketStorage<IndexBucket<T>>, pubkey: &Pubkey) {
        self.set_slot_count_enum_value(index_bucket, OccupiedEnum::ZeroSlots);
        let index_entry = index_bucket.get_mut::<IndexEntry<T>>(self.ix);
        index_entry.key = *pubkey;
    }

    /// If the entry matches the pubkey and is unoccupied, then store `data` here and occupy the entry.
    pub(crate) fn occupy_if_matches(
        &self,
        index_bucket: &mut BucketStorage<IndexBucket<T>>,
        data: &T,
        k: &Pubkey,
    ) -> OccupyIfMatches {
        let index_entry = index_bucket.get::<IndexEntry<T>>(self.ix);
        if &index_entry.key == k {
            let enum_tag = index_bucket.contents.get_enum_tag(self.ix);
            if unsafe { &index_entry.contents.single_element } == data {
                assert_eq!(
                    enum_tag,
                    OccupiedEnumTag::Free,
                    "index asked to insert the same data twice"
                );
                index_bucket
                    .contents
                    .set_enum_tag(self.ix, OccupiedEnumTag::OneSlotInIndex);
                OccupyIfMatches::SuccessfulInit
            } else if enum_tag == OccupiedEnumTag::Free {
                // pubkey is same, but value is different, so update value
                self.set_slot_count_enum_value(index_bucket, OccupiedEnum::OneSlotInIndex(data));
                OccupyIfMatches::SuccessfulInit
            } else {
                // found occupied duplicate of this pubkey
                OccupyIfMatches::FoundDuplicate
            }
        } else {
            OccupyIfMatches::PubkeyMismatch
        }
    }

    pub(crate) fn read_value<'a>(
        &self,
        index_bucket: &'a BucketStorage<IndexBucket<T>>,
        data_buckets: &'a [BucketStorage<DataBucket>],
    ) -> (&'a [T], RefCount) {
        let mut ref_count = 1;
        let slot_list = match self.get_slot_count_enum(index_bucket) {
            OccupiedEnum::ZeroSlots => {
                // num_slots is 0. This means empty slot list and ref_count=1
                &[]
            }
            OccupiedEnum::OneSlotInIndex(single_element) => {
                // only element is stored in the index entry
                std::slice::from_ref(single_element)
            }
            OccupiedEnum::MultipleSlots(multiple_slots) => {
                // slot list and ref_count are in data file
                let data_bucket_ix =
                    MultipleSlots::data_bucket_from_num_slots(multiple_slots.num_slots);
                let data_bucket = &data_buckets[data_bucket_ix as usize];
                let loc = multiple_slots.data_loc(data_bucket);
                assert!(!data_bucket.is_free(loc));

                ref_count = MultipleSlots::ref_count(data_bucket, loc);
                data_bucket.get_slice::<T>(loc, multiple_slots.num_slots, IncludeHeader::NoHeader)
            }
            _ => {
                panic!("trying to read data from a free entry");
            }
        };
        (slot_list, ref_count)
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
}

fn get_from_bytes<T>(item_slice: &[u8]) -> &T {
    debug_assert!(std::mem::size_of::<T>() <= item_slice.len());
    let item = item_slice.as_ptr() as *const T;
    debug_assert!(item as usize % std::mem::align_of::<T>() == 0);
    unsafe { &*item }
}

fn get_mut_from_bytes<T>(item_slice: &mut [u8]) -> &mut T {
    debug_assert!(std::mem::size_of::<T>() <= item_slice.len());
    let item = item_slice.as_mut_ptr() as *mut T;
    debug_assert!(item as usize % std::mem::align_of::<T>() == 0);
    unsafe { &mut *item }
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
        assert_eq!(std::mem::size_of::<IndexEntry<u64>>(), 32 + 8 + 8);
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
