use {
    crate::{
        cluster_info::MAX_CRDS_OBJECT_SIZE,
        crds_value::{self, MAX_SLOT, MAX_WALLCLOCK},
    },
    bincode::serialized_size,
    bv::BitVec,
    flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress},
    solana_sanitize::{Sanitize, SanitizeError},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
};

pub const MAX_SLOTS_PER_ENTRY: usize = 2048 * 8;
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Uncompressed {
    pub first_slot: Slot,
    pub num: usize,
    pub slots: BitVec<u8>,
}

impl Sanitize for Uncompressed {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.first_slot >= MAX_SLOT {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        if self.num >= MAX_SLOTS_PER_ENTRY {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        if self.slots.len() % 8 != 0 {
            // Uncompressed::new() ensures the length is always a multiple of 8
            return Err(SanitizeError::ValueOutOfBounds);
        }
        if self.slots.len() != self.slots.capacity() {
            // A BitVec<u8> with a length that's a multiple of 8 will always have len() equal to
            // capacity(), assuming no bit manipulation
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Flate2 {
    pub first_slot: Slot,
    pub num: usize,
    pub compressed: Vec<u8>,
}

impl Sanitize for Flate2 {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.first_slot >= MAX_SLOT {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        if self.num >= MAX_SLOTS_PER_ENTRY {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    CompressError,
    DecompressError,
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::convert::From<flate2::CompressError> for Error {
    fn from(_e: flate2::CompressError) -> Error {
        Error::CompressError
    }
}
impl std::convert::From<flate2::DecompressError> for Error {
    fn from(_e: flate2::DecompressError) -> Error {
        Error::DecompressError
    }
}

impl Flate2 {
    fn deflate(mut unc: Uncompressed) -> Result<Self> {
        let mut compressed = Vec::with_capacity(unc.slots.block_capacity());
        let mut compressor = Compress::new(Compression::best(), false);
        let first_slot = unc.first_slot;
        let num = unc.num;
        unc.slots.shrink_to_fit();
        let bits = unc.slots.into_boxed_slice();
        compressor.compress_vec(&bits, &mut compressed, FlushCompress::Finish)?;
        let rv = Self {
            first_slot,
            num,
            compressed,
        };
        let _ = rv.inflate()?;
        Ok(rv)
    }
    pub fn inflate(&self) -> Result<Uncompressed> {
        //add some head room for the decompressor which might spill more bits
        let mut uncompressed = Vec::with_capacity(32 + (self.num + 4) / 8);
        let mut decompress = Decompress::new(false);
        decompress.decompress_vec(&self.compressed, &mut uncompressed, FlushDecompress::Finish)?;
        Ok(Uncompressed {
            first_slot: self.first_slot,
            num: self.num,
            slots: BitVec::from_bits(&uncompressed),
        })
    }
}

impl Uncompressed {
    pub fn new(max_size: usize) -> Self {
        Self {
            num: 0,
            first_slot: 0,
            slots: BitVec::new_fill(false, 8 * max_size as u64),
        }
    }
    pub fn to_slots(&self, min_slot: Slot) -> Vec<Slot> {
        let mut rv = vec![];
        let start = if min_slot < self.first_slot {
            0
        } else {
            (min_slot - self.first_slot) as usize
        };
        for i in start..self.num {
            if i >= self.slots.len() as usize {
                break;
            }
            if self.slots.get(i as u64) {
                rv.push(self.first_slot + i as Slot);
            }
        }
        rv
    }
    pub fn add(&mut self, slots: &[Slot]) -> usize {
        for (i, s) in slots.iter().enumerate() {
            if self.num == 0 {
                self.first_slot = *s;
            }
            if self.num >= MAX_SLOTS_PER_ENTRY {
                return i;
            }
            if *s < self.first_slot {
                return i;
            }
            if *s - self.first_slot >= self.slots.len() {
                return i;
            }
            self.slots.set(*s - self.first_slot, true);
            self.num = std::cmp::max(self.num, 1 + (*s - self.first_slot) as usize);
        }
        slots.len()
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample, AbiEnumVisitor))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum CompressedSlots {
    Flate2(Flate2),
    Uncompressed(Uncompressed),
}

impl Sanitize for CompressedSlots {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        match self {
            CompressedSlots::Uncompressed(a) => a.sanitize(),
            CompressedSlots::Flate2(b) => b.sanitize(),
        }
    }
}

impl Default for CompressedSlots {
    fn default() -> Self {
        CompressedSlots::new(0)
    }
}

impl CompressedSlots {
    pub(crate) fn new(max_size: usize) -> Self {
        CompressedSlots::Uncompressed(Uncompressed::new(max_size))
    }

    pub fn first_slot(&self) -> Slot {
        match self {
            CompressedSlots::Uncompressed(a) => a.first_slot,
            CompressedSlots::Flate2(b) => b.first_slot,
        }
    }

    pub fn num_slots(&self) -> usize {
        match self {
            CompressedSlots::Uncompressed(a) => a.num,
            CompressedSlots::Flate2(b) => b.num,
        }
    }

    pub fn add(&mut self, slots: &[Slot]) -> usize {
        match self {
            CompressedSlots::Uncompressed(vals) => vals.add(slots),
            CompressedSlots::Flate2(_) => 0,
        }
    }
    pub fn to_slots(&self, min_slot: Slot) -> Result<Vec<Slot>> {
        match self {
            CompressedSlots::Uncompressed(vals) => Ok(vals.to_slots(min_slot)),
            CompressedSlots::Flate2(vals) => {
                let unc = vals.inflate()?;
                Ok(unc.to_slots(min_slot))
            }
        }
    }
    pub fn deflate(&mut self) -> Result<()> {
        match self {
            CompressedSlots::Uncompressed(vals) => {
                let unc = vals.clone();
                let compressed = Flate2::deflate(unc)?;
                *self = CompressedSlots::Flate2(compressed);
                Ok(())
            }
            CompressedSlots::Flate2(_) => Ok(()),
        }
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct EpochSlots {
    pub from: Pubkey,
    pub slots: Vec<CompressedSlots>,
    pub wallclock: u64,
}

impl Sanitize for EpochSlots {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        self.from.sanitize()?;
        self.slots.sanitize()
    }
}

use std::fmt;
impl fmt::Debug for EpochSlots {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num_slots: usize = self.slots.iter().map(|s| s.num_slots()).sum();
        let lowest_slot = self
            .slots
            .iter()
            .map(|s| s.first_slot())
            .fold(0, std::cmp::min);
        write!(
            f,
            "EpochSlots {{ from: {} num_slots: {} lowest_slot: {} wallclock: {} }}",
            self.from, num_slots, lowest_slot, self.wallclock
        )
    }
}

impl EpochSlots {
    pub fn new(from: Pubkey, now: u64) -> Self {
        Self {
            from,
            wallclock: now,
            slots: vec![],
        }
    }
    pub fn fill(&mut self, slots: &[Slot], now: u64) -> usize {
        let mut num = 0;
        self.wallclock = std::cmp::max(now, self.wallclock + 1);
        while num < slots.len() {
            num += self.add(&slots[num..]);
            if num < slots.len() {
                if self.deflate().is_err() {
                    return num;
                }
                let space = self.max_compressed_slot_size();
                if space > 0 {
                    let cslot = CompressedSlots::new(space as usize);
                    self.slots.push(cslot);
                } else {
                    return num;
                }
            }
        }
        num
    }
    pub fn add(&mut self, slots: &[Slot]) -> usize {
        let mut num = 0;
        for s in &mut self.slots {
            num += s.add(&slots[num..]);
            if num >= slots.len() {
                break;
            }
        }
        num
    }
    pub fn deflate(&mut self) -> Result<()> {
        for s in self.slots.iter_mut() {
            s.deflate()?;
        }
        Ok(())
    }
    pub fn max_compressed_slot_size(&self) -> isize {
        let len_header = serialized_size(self).unwrap();
        let len_slot = serialized_size(&CompressedSlots::default()).unwrap();
        MAX_CRDS_OBJECT_SIZE as isize - (len_header + len_slot) as isize
    }

    pub fn first_slot(&self) -> Option<Slot> {
        self.slots.iter().map(|s| s.first_slot()).min()
    }

    pub fn to_slots(&self, min_slot: Slot) -> Vec<Slot> {
        self.slots
            .iter()
            .filter(|s| min_slot < s.first_slot() + s.num_slots() as u64)
            .filter_map(|s| s.to_slots(min_slot).ok())
            .flatten()
            .collect()
    }

    /// New random EpochSlots for tests and simulations.
    pub(crate) fn new_rand<R: rand::Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        let now = crds_value::new_rand_timestamp(rng);
        let pubkey = pubkey.unwrap_or_else(solana_sdk::pubkey::new_rand);
        let mut epoch_slots = Self::new(pubkey, now);
        let num_slots = rng.gen_range(0..20);
        let slots: Vec<_> = std::iter::repeat_with(|| 47825632 + rng.gen_range(0..512))
            .take(num_slots)
            .collect();
        epoch_slots.add(&slots);
        epoch_slots
    }
}

#[cfg(test)]
mod tests {
    use {super::*, rand::Rng, std::iter::repeat_with};

    #[test]
    fn test_epoch_slots_max_size() {
        let epoch_slots = EpochSlots::default();
        assert!(epoch_slots.max_compressed_slot_size() > 0);
    }

    #[test]
    fn test_epoch_slots_uncompressed_add_1() {
        let mut slots = Uncompressed::new(1);
        assert_eq!(slots.slots.capacity(), 8);
        assert_eq!(slots.add(&[1]), 1);
        assert_eq!(slots.to_slots(1), vec![1]);
        assert!(slots.to_slots(2).is_empty());
    }

    #[test]
    fn test_epoch_slots_to_slots_overflow() {
        let mut slots = Uncompressed::new(1);
        slots.num = 100;
        assert!(slots.to_slots(0).is_empty());
    }

    #[test]
    fn test_epoch_slots_uncompressed_add_2() {
        let mut slots = Uncompressed::new(1);
        assert_eq!(slots.add(&[1, 2]), 2);
        assert_eq!(slots.to_slots(1), vec![1, 2]);
    }
    #[test]
    fn test_epoch_slots_uncompressed_add_3a() {
        let mut slots = Uncompressed::new(1);
        assert_eq!(slots.add(&[1, 3, 2]), 3);
        assert_eq!(slots.to_slots(1), vec![1, 2, 3]);
    }

    #[test]
    fn test_epoch_slots_uncompressed_add_3b() {
        let mut slots = Uncompressed::new(1);
        assert_eq!(slots.add(&[1, 10, 2]), 1);
        assert_eq!(slots.to_slots(1), vec![1]);
    }

    #[test]
    fn test_epoch_slots_uncompressed_add_3c() {
        let mut slots = Uncompressed::new(2);
        assert_eq!(slots.add(&[1, 10, 2]), 3);
        assert_eq!(slots.to_slots(1), vec![1, 2, 10]);
        assert_eq!(slots.to_slots(2), vec![2, 10]);
        assert_eq!(slots.to_slots(3), vec![10]);
        assert!(slots.to_slots(11).is_empty());
    }
    #[test]
    fn test_epoch_slots_compressed() {
        let mut slots = Uncompressed::new(100);
        slots.add(&[1, 701, 2]);
        assert_eq!(slots.num, 701);
        let compressed = Flate2::deflate(slots).unwrap();
        assert_eq!(compressed.first_slot, 1);
        assert_eq!(compressed.num, 701);
        assert!(compressed.compressed.len() < 32);
        let slots = compressed.inflate().unwrap();
        assert_eq!(slots.first_slot, 1);
        assert_eq!(slots.num, 701);
        assert_eq!(slots.to_slots(1), vec![1, 2, 701]);
    }

    #[test]
    fn test_epoch_slots_sanitize() {
        let mut slots = Uncompressed::new(100);
        slots.add(&[1, 701, 2]);
        assert_eq!(slots.num, 701);
        assert!(slots.sanitize().is_ok());

        let mut o = slots.clone();
        o.first_slot = MAX_SLOT;
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let mut o = slots.clone();
        o.num = MAX_SLOTS_PER_ENTRY;
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let mut o = slots.clone();
        o.slots = BitVec::new_fill(false, 7); // Length not a multiple of 8
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let mut o = slots.clone();
        o.slots = BitVec::with_capacity(8); // capacity() not equal to len()
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let compressed = Flate2::deflate(slots).unwrap();
        assert!(compressed.sanitize().is_ok());

        let mut o = compressed.clone();
        o.first_slot = MAX_SLOT;
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let mut o = compressed;
        o.num = MAX_SLOTS_PER_ENTRY;
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let mut slots = EpochSlots::default();
        let range: Vec<Slot> = (0..5000).collect();
        assert_eq!(slots.fill(&range, 1), 5000);
        assert_eq!(slots.wallclock, 1);
        assert!(slots.sanitize().is_ok());

        let mut o = slots;
        o.wallclock = MAX_WALLCLOCK;
        assert_eq!(o.sanitize(), Err(SanitizeError::ValueOutOfBounds));
    }

    #[test]
    fn test_epoch_slots_fill_range() {
        let range: Vec<Slot> = (0..5000).collect();
        let mut slots = EpochSlots::default();
        assert_eq!(slots.fill(&range, 1), 5000);
        assert_eq!(slots.wallclock, 1);
        assert_eq!(slots.to_slots(0), range);
        assert_eq!(slots.to_slots(4999), vec![4999]);
        assert!(slots.to_slots(5000).is_empty());
    }
    #[test]
    fn test_epoch_slots_fill_sparce_range() {
        let range: Vec<Slot> = (0..5000).map(|x| x * 3).collect();
        let mut slots = EpochSlots::default();
        assert_eq!(slots.fill(&range, 2), 5000);
        assert_eq!(slots.wallclock, 2);
        assert_eq!(slots.slots.len(), 3);
        assert_eq!(slots.slots[0].first_slot(), 0);
        assert_ne!(slots.slots[0].num_slots(), 0);
        let next = slots.slots[0].num_slots() as u64 + slots.slots[0].first_slot();
        assert!(slots.slots[1].first_slot() >= next);
        assert_ne!(slots.slots[1].num_slots(), 0);
        assert_ne!(slots.slots[2].num_slots(), 0);
        assert_eq!(slots.to_slots(0), range);
        assert_eq!(slots.to_slots(4999 * 3), vec![4999 * 3]);
    }

    #[test]
    fn test_epoch_slots_fill_large_sparce_range() {
        let range: Vec<Slot> = (0..5000).map(|x| x * 7).collect();
        let mut slots = EpochSlots::default();
        assert_eq!(slots.fill(&range, 2), 5000);
        assert_eq!(slots.to_slots(0), range);
    }

    fn make_rand_slots<R: Rng>(rng: &mut R) -> impl Iterator<Item = Slot> + '_ {
        repeat_with(|| rng.gen_range(1..5)).scan(0, |slot, step| {
            *slot += step;
            Some(*slot)
        })
    }

    #[test]
    fn test_epoch_slots_fill_uncompressed_random_range() {
        let mut rng = rand::thread_rng();
        for _ in 0..10 {
            let range: Vec<Slot> = make_rand_slots(&mut rng).take(5000).collect();
            let sz = EpochSlots::default().max_compressed_slot_size();
            let mut slots = Uncompressed::new(sz as usize);
            let sz = slots.add(&range);
            let slots = slots.to_slots(0);
            assert_eq!(slots.len(), sz);
            assert_eq!(slots[..], range[..sz]);
        }
    }

    #[test]
    fn test_epoch_slots_fill_compressed_random_range() {
        let mut rng = rand::thread_rng();
        for _ in 0..10 {
            let range: Vec<Slot> = make_rand_slots(&mut rng).take(5000).collect();
            let sz = EpochSlots::default().max_compressed_slot_size();
            let mut slots = Uncompressed::new(sz as usize);
            let sz = slots.add(&range);
            let mut slots = CompressedSlots::Uncompressed(slots);
            slots.deflate().unwrap();
            let slots = slots.to_slots(0).unwrap();
            assert_eq!(slots.len(), sz);
            assert_eq!(slots[..], range[..sz]);
        }
    }

    #[test]
    fn test_epoch_slots_fill_random_range() {
        let mut rng = rand::thread_rng();
        for _ in 0..10 {
            let range: Vec<Slot> = make_rand_slots(&mut rng).take(5000).collect();
            let mut slots = EpochSlots::default();
            let sz = slots.fill(&range, 1);
            let last = range[sz - 1];
            assert_eq!(
                last,
                slots.slots.last().unwrap().first_slot()
                    + slots.slots.last().unwrap().num_slots() as u64
                    - 1
            );
            for s in &slots.slots {
                assert!(s.to_slots(0).is_ok());
            }
            let slots = slots.to_slots(0);
            assert_eq!(slots[..], range[..slots.len()]);
            assert_eq!(sz, slots.len())
        }
    }
}
