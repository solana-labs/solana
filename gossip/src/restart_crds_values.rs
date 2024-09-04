use {
    crate::crds_value::{new_rand_timestamp, sanitize_wallclock},
    bv::BitVec,
    itertools::Itertools,
    rand::Rng,
    solana_sanitize::{Sanitize, SanitizeError},
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    solana_serde_varint as serde_varint,
    thiserror::Error,
};

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct RestartLastVotedForkSlots {
    pub from: Pubkey,
    pub wallclock: u64,
    offsets: SlotsOffsets,
    pub last_voted_slot: Slot,
    pub last_voted_hash: Hash,
    pub shred_version: u16,
}

#[derive(Debug, Error)]
pub enum RestartLastVotedForkSlotsError {
    #[error("Last voted fork cannot be empty")]
    LastVotedForkEmpty,
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct RestartHeaviestFork {
    pub from: Pubkey,
    pub wallclock: u64,
    pub last_slot: Slot,
    pub last_slot_hash: Hash,
    pub observed_stake: u64,
    pub shred_version: u16,
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample, AbiEnumVisitor))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
enum SlotsOffsets {
    RunLengthEncoding(RunLengthEncoding),
    RawOffsets(RawOffsets),
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct U16(#[serde(with = "serde_varint")] u16);

// The vector always starts with 1. Encode number of 1's and 0's consecutively.
// For example, 110000111 is [2, 4, 3].
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RunLengthEncoding(Vec<U16>);

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RawOffsets(BitVec<u8>);

impl Sanitize for RestartLastVotedForkSlots {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.last_voted_hash.sanitize()
    }
}

impl RestartLastVotedForkSlots {
    // This number is MAX_CRDS_OBJECT_SIZE - empty serialized RestartLastVotedForkSlots.
    const MAX_BYTES: usize = 824;

    // Per design doc, we should start wen_restart within 7 hours.
    pub const MAX_SLOTS: usize = u16::MAX as usize;

    pub fn new(
        from: Pubkey,
        now: u64,
        last_voted_fork: &[Slot],
        last_voted_hash: Hash,
        shred_version: u16,
    ) -> Result<Self, RestartLastVotedForkSlotsError> {
        let Some((&first_voted_slot, &last_voted_slot)) =
            last_voted_fork.iter().minmax().into_option()
        else {
            return Err(RestartLastVotedForkSlotsError::LastVotedForkEmpty);
        };
        let max_size = last_voted_slot.saturating_sub(first_voted_slot) + 1;
        let mut uncompressed_bitvec = BitVec::new_fill(false, max_size);
        for slot in last_voted_fork {
            uncompressed_bitvec.set(last_voted_slot - *slot, true);
        }
        let run_length_encoding = RunLengthEncoding::new(&uncompressed_bitvec);
        let offsets =
            if run_length_encoding.num_encoded_slots() > RestartLastVotedForkSlots::MAX_BYTES * 8 {
                SlotsOffsets::RunLengthEncoding(run_length_encoding)
            } else {
                SlotsOffsets::RawOffsets(RawOffsets::new(uncompressed_bitvec))
            };
        Ok(Self {
            from,
            wallclock: now,
            offsets,
            last_voted_slot,
            last_voted_hash,
            shred_version,
        })
    }

    /// New random Version for tests and benchmarks.
    pub(crate) fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        let pubkey = pubkey.unwrap_or_else(solana_sdk::pubkey::new_rand);
        let num_slots = rng.gen_range(2..20);
        let slots = std::iter::repeat_with(|| 47825632 + rng.gen_range(0..512))
            .take(num_slots)
            .collect::<Vec<Slot>>();
        RestartLastVotedForkSlots::new(
            pubkey,
            new_rand_timestamp(rng),
            &slots,
            Hash::new_unique(),
            1,
        )
        .unwrap()
    }

    pub fn to_slots(&self, min_slot: Slot) -> Vec<Slot> {
        match &self.offsets {
            SlotsOffsets::RunLengthEncoding(run_length_encoding) => {
                run_length_encoding.to_slots(self.last_voted_slot, min_slot)
            }
            SlotsOffsets::RawOffsets(raw_offsets) => {
                raw_offsets.to_slots(self.last_voted_slot, min_slot)
            }
        }
    }
}

impl Sanitize for RestartHeaviestFork {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.last_slot_hash.sanitize()
    }
}

impl RestartHeaviestFork {
    pub(crate) fn new_rand<R: Rng>(rng: &mut R, from: Option<Pubkey>) -> Self {
        let from = from.unwrap_or_else(solana_sdk::pubkey::new_rand);
        Self {
            from,
            wallclock: new_rand_timestamp(rng),
            last_slot: rng.gen_range(0..1000),
            last_slot_hash: Hash::new_unique(),
            observed_stake: rng.gen_range(1..u64::MAX),
            shred_version: 1,
        }
    }
}

impl RunLengthEncoding {
    fn new(bits: &BitVec<u8>) -> Self {
        let encoded = (0..bits.len())
            .map(|i| bits.get(i))
            .dedup_with_count()
            .map_while(|(count, _)| u16::try_from(count).ok())
            .scan(0, |current_bytes, count| {
                *current_bytes += ((u16::BITS - count.leading_zeros() + 6) / 7).max(1) as usize;
                (*current_bytes <= RestartLastVotedForkSlots::MAX_BYTES).then_some(U16(count))
            })
            .collect();
        Self(encoded)
    }

    fn num_encoded_slots(&self) -> usize {
        self.0.iter().map(|x| usize::from(x.0)).sum()
    }

    fn to_slots(&self, last_slot: Slot, min_slot: Slot) -> Vec<Slot> {
        let mut slots: Vec<Slot> = self
            .0
            .iter()
            .map(|bit_count| usize::from(bit_count.0))
            .zip([1, 0].iter().cycle())
            .flat_map(|(bit_count, bit)| std::iter::repeat(bit).take(bit_count))
            .enumerate()
            .filter(|(_, bit)| **bit == 1)
            .map_while(|(offset, _)| {
                let offset = Slot::try_from(offset).ok()?;
                last_slot.checked_sub(offset)
            })
            .take(RestartLastVotedForkSlots::MAX_SLOTS)
            .take_while(|slot| *slot >= min_slot)
            .collect();
        slots.reverse();
        slots
    }
}

impl RawOffsets {
    fn new(mut bits: BitVec<u8>) -> Self {
        bits.truncate(RestartLastVotedForkSlots::MAX_BYTES as u64 * 8);
        bits.shrink_to_fit();
        Self(bits)
    }

    fn to_slots(&self, last_slot: Slot, min_slot: Slot) -> Vec<Slot> {
        let mut slots: Vec<Slot> = (0..self.0.len())
            .filter(|index| self.0.get(*index))
            .map_while(|offset| last_slot.checked_sub(offset))
            .take_while(|slot| *slot >= min_slot)
            .collect();
        slots.reverse();
        slots
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{
            cluster_info::MAX_CRDS_OBJECT_SIZE,
            crds_value::{CrdsData, CrdsValue, CrdsValueLabel},
        },
        bincode::serialized_size,
        solana_sdk::{signature::Signer, signer::keypair::Keypair, timing::timestamp},
        std::iter::repeat_with,
    };

    fn make_rand_slots<R: Rng>(rng: &mut R) -> impl Iterator<Item = Slot> + '_ {
        repeat_with(|| rng.gen_range(1..5)).scan(0, |slot, step| {
            *slot += step;
            Some(*slot)
        })
    }

    #[test]
    fn test_restart_last_voted_fork_slots_max_bytes() {
        let keypair = Keypair::new();
        let header = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &[1, 2],
            Hash::default(),
            0,
        )
        .unwrap();
        // If the following assert fails, please update RestartLastVotedForkSlots::MAX_BYTES
        assert_eq!(
            RestartLastVotedForkSlots::MAX_BYTES,
            MAX_CRDS_OBJECT_SIZE - serialized_size(&header).unwrap() as usize
        );

        // Create large enough slots to make sure we are discarding some to make slots fit.
        let mut rng = rand::thread_rng();
        let large_length = 8000;
        let range: Vec<Slot> = make_rand_slots(&mut rng).take(large_length).collect();
        let large_slots = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &range,
            Hash::default(),
            0,
        )
        .unwrap();
        assert!(serialized_size(&large_slots).unwrap() <= MAX_CRDS_OBJECT_SIZE as u64);
        let retrieved_slots = large_slots.to_slots(0);
        assert!(retrieved_slots.len() <= range.len());
        assert!(retrieved_slots.last().unwrap() - retrieved_slots.first().unwrap() > 5000);
    }

    #[test]
    fn test_restart_last_voted_fork_slots() {
        let keypair = Keypair::new();
        let slot = 53;
        let slot_parent = slot - 5;
        let shred_version = 21;
        let original_slots_vec = [slot_parent, slot];
        let slots = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &original_slots_vec,
            Hash::default(),
            shred_version,
        )
        .unwrap();
        let value =
            CrdsValue::new_signed(CrdsData::RestartLastVotedForkSlots(slots.clone()), &keypair);
        assert_eq!(value.sanitize(), Ok(()));
        let label = value.label();
        assert_eq!(
            label,
            CrdsValueLabel::RestartLastVotedForkSlots(keypair.pubkey())
        );
        assert_eq!(label.pubkey(), keypair.pubkey());
        assert_eq!(value.wallclock(), slots.wallclock);
        let retrieved_slots = slots.to_slots(0);
        assert_eq!(retrieved_slots.len(), 2);
        assert_eq!(retrieved_slots[0], slot_parent);
        assert_eq!(retrieved_slots[1], slot);

        let bad_value = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &[],
            Hash::default(),
            shred_version,
        );
        assert!(bad_value.is_err());

        let last_slot: Slot = 8000;
        let large_slots_vec: Vec<Slot> = (0..last_slot + 1).collect();
        let large_slots = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &large_slots_vec,
            Hash::default(),
            shred_version,
        )
        .unwrap();
        assert!(serialized_size(&large_slots).unwrap() < MAX_CRDS_OBJECT_SIZE as u64);
        let retrieved_slots = large_slots.to_slots(0);
        assert_eq!(retrieved_slots, large_slots_vec);
    }

    fn check_run_length_encoding(slots: Vec<Slot>) {
        let last_voted_slot = slots[slots.len() - 1];
        let mut bitvec = BitVec::new_fill(false, last_voted_slot - slots[0] + 1);
        for slot in &slots {
            bitvec.set(last_voted_slot - slot, true);
        }
        let rle = RunLengthEncoding::new(&bitvec);
        let retrieved_slots = rle.to_slots(last_voted_slot, 0);
        assert_eq!(retrieved_slots, slots);
    }

    #[test]
    fn test_run_length_encoding() {
        check_run_length_encoding((1000..16384 + 1000).map(|x| x as Slot).collect_vec());
        check_run_length_encoding([1000 as Slot].into());
        check_run_length_encoding(
            [
                1000 as Slot,
                RestartLastVotedForkSlots::MAX_SLOTS as Slot + 999,
            ]
            .into(),
        );
        check_run_length_encoding((1000..1800).step_by(2).map(|x| x as Slot).collect_vec());

        let mut rng = rand::thread_rng();
        let large_length = 500;
        let range: Vec<Slot> = make_rand_slots(&mut rng).take(large_length).collect();
        check_run_length_encoding(range);
    }

    #[test]
    fn test_restart_heaviest_fork() {
        let keypair = Keypair::new();
        let slot = 53;
        let mut fork = RestartHeaviestFork {
            from: keypair.pubkey(),
            wallclock: timestamp(),
            last_slot: slot,
            last_slot_hash: Hash::default(),
            observed_stake: 800_000,
            shred_version: 1,
        };
        assert_eq!(fork.sanitize(), Ok(()));
        assert_eq!(fork.observed_stake, 800_000);
        fork.wallclock = crate::crds_value::MAX_WALLCLOCK;
        assert_eq!(fork.sanitize(), Err(SanitizeError::ValueOutOfBounds));
    }
}
