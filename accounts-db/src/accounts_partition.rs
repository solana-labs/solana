//! Partitioning of the accounts into chunks for rent collection
use {
    itertools::Itertools,
    log::trace,
    solana_sdk::{
        clock::{Slot, SlotCount, SlotIndex},
        pubkey::Pubkey,
        stake_history::Epoch,
        sysvar::epoch_schedule::EpochSchedule,
    },
    std::{collections::HashSet, mem, ops::RangeInclusive},
};

// Eager rent collection repeats in cyclic manner.
// Each cycle is composed of <partition_count> number of tiny pubkey subranges
// to scan, which is always multiple of the number of slots in epoch.
pub type PartitionIndex = u64;
type PartitionsPerCycle = u64;
pub type Partition = (PartitionIndex, PartitionIndex, PartitionsPerCycle);
type RentCollectionCycleParams = (
    Epoch,
    SlotCount,
    bool,
    Epoch,
    EpochCount,
    PartitionsPerCycle,
);
type EpochCount = u64;

fn partition_index_from_slot_index(
    slot_index_in_epoch: SlotIndex,
    (
        epoch,
        slot_count_per_epoch,
        _,
        base_epoch,
        epoch_count_per_cycle,
        _,
    ): RentCollectionCycleParams,
) -> PartitionIndex {
    let epoch_offset = epoch - base_epoch;
    let epoch_index_in_cycle = epoch_offset % epoch_count_per_cycle;
    slot_index_in_epoch + epoch_index_in_cycle * slot_count_per_epoch
}

pub fn get_partition_from_slot_indexes(
    cycle_params: RentCollectionCycleParams,
    start_slot_index: SlotIndex,
    end_slot_index: SlotIndex,
    generated_for_gapped_epochs: bool,
) -> Partition {
    let (_, _, in_multi_epoch_cycle, _, _, partition_count) = cycle_params;

    // use common codepath for both very likely and very unlikely for the sake of minimized
    // risk of any miscalculation instead of negligibly faster computation per slot for the
    // likely case.
    let mut start_partition_index = partition_index_from_slot_index(start_slot_index, cycle_params);
    let mut end_partition_index = partition_index_from_slot_index(end_slot_index, cycle_params);

    // Adjust partition index for some edge cases
    let is_special_new_epoch = start_slot_index == 0 && end_slot_index != 1;
    let in_middle_of_cycle = start_partition_index > 0;
    if in_multi_epoch_cycle && is_special_new_epoch && in_middle_of_cycle {
        // Adjust slot indexes so that the final partition ranges are continuous!
        // This is need because the caller gives us off-by-one indexes when
        // an epoch boundary is crossed.
        // Usually there is no need for this adjustment because cycles are aligned
        // with epochs. But for multi-epoch cycles, adjust the indexes if it
        // happens in the middle of a cycle for both gapped and not-gapped cases:
        //
        // epoch (slot range)|slot idx.*1|raw part. idx.|adj. part. idx.|epoch boundary
        // ------------------+-----------+--------------+---------------+--------------
        // 3 (20..30)        | [7..8]    |   7.. 8      |   7.. 8
        //                   | [8..9]    |   8.. 9      |   8.. 9
        // 4 (30..40)        | [0..0]    |<10>..10      | <9>..10      <--- not gapped
        //                   | [0..1]    |  10..11      |  10..12
        //                   | [1..2]    |  11..12      |  11..12
        //                   | [2..9   *2|  12..19      |  12..19      <-+
        // 5 (40..50)        |  0..0   *2|<20>..<20>    |<19>..<19> *3 <-+- gapped
        //                   |  0..4]    |<20>..24      |<19>..24      <-+
        //                   | [4..5]    |  24..25      |  24..25
        //                   | [5..6]    |  25..26      |  25..26
        //
        // NOTE: <..> means the adjusted slots
        //
        // *1: The range of parent_bank.slot() and current_bank.slot() is firstly
        //     split by the epoch boundaries and then the split ones are given to us.
        //     The original ranges are denoted as [...]
        // *2: These are marked with generated_for_gapped_epochs = true.
        // *3: This becomes no-op partition
        start_partition_index -= 1;
        if generated_for_gapped_epochs {
            assert_eq!(start_slot_index, end_slot_index);
            end_partition_index -= 1;
        }
    }

    (start_partition_index, end_partition_index, partition_count)
}

/// used only by filler accounts in debug path
/// previous means slot - 1, not parent
// These functions/fields are only usable from a dev context (i.e. tests and benches)
#[cfg(feature = "dev-context-only-utils")]
pub fn variable_cycle_partition_from_previous_slot(
    epoch_schedule: &EpochSchedule,
    slot: Slot,
) -> Partition {
    // similar code to Bank::variable_cycle_partitions
    let (current_epoch, current_slot_index) = epoch_schedule.get_epoch_and_slot_index(slot);
    let (parent_epoch, mut parent_slot_index) =
        epoch_schedule.get_epoch_and_slot_index(slot.saturating_sub(1));
    let cycle_params = rent_single_epoch_collection_cycle_params(
        current_epoch,
        epoch_schedule.get_slots_in_epoch(current_epoch),
    );

    if parent_epoch < current_epoch {
        parent_slot_index = 0;
    }

    let generated_for_gapped_epochs = false;
    get_partition_from_slot_indexes(
        cycle_params,
        parent_slot_index,
        current_slot_index,
        generated_for_gapped_epochs,
    )
}

/// return all end partition indexes for the given partition
/// partition could be (0, 1, N). In this case we only return [1]
///  the single 'end_index' that covers this partition.
/// partition could be (0, 2, N). In this case, we return [1, 2], which are all
/// the 'end_index' values contained in that range.
/// (0, 0, N) returns [0] as a special case.
/// There is a relationship between
/// 1. 'pubkey_range_from_partition'
/// 2. 'partition_from_pubkey'
/// 3. this function
pub fn get_partition_end_indexes(partition: &Partition) -> Vec<PartitionIndex> {
    if partition.0 == partition.1 && partition.0 == 0 {
        // special case for start=end=0. ie. (0, 0, N). This returns [0]
        vec![0]
    } else {
        // normal case of (start, end, N)
        // so, we want [start+1, start+2, ..=end]
        // if start == end, then return []
        (partition.0..partition.1).map(|index| index + 1).collect()
    }
}

pub fn rent_single_epoch_collection_cycle_params(
    epoch: Epoch,
    slot_count_per_epoch: SlotCount,
) -> RentCollectionCycleParams {
    (
        epoch,
        slot_count_per_epoch,
        false,
        0,
        1,
        slot_count_per_epoch,
    )
}

pub fn rent_multi_epoch_collection_cycle_params(
    epoch: Epoch,
    slot_count_per_epoch: SlotCount,
    first_normal_epoch: Epoch,
    epoch_count_in_cycle: Epoch,
) -> RentCollectionCycleParams {
    let partition_count = slot_count_per_epoch * epoch_count_in_cycle;
    (
        epoch,
        slot_count_per_epoch,
        true,
        first_normal_epoch,
        epoch_count_in_cycle,
        partition_count,
    )
}

pub fn get_partitions(
    slot: Slot,
    parent_slot: Slot,
    slot_count_in_two_day: SlotCount,
) -> Vec<Partition> {
    let parent_cycle = parent_slot / slot_count_in_two_day;
    let current_cycle = slot / slot_count_in_two_day;
    let mut parent_cycle_index = parent_slot % slot_count_in_two_day;
    let current_cycle_index = slot % slot_count_in_two_day;
    let mut partitions = vec![];
    if parent_cycle < current_cycle {
        if current_cycle_index > 0 {
            // generate and push gapped partitions because some slots are skipped
            let parent_last_cycle_index = slot_count_in_two_day - 1;

            // ... for parent cycle
            partitions.push((
                parent_cycle_index,
                parent_last_cycle_index,
                slot_count_in_two_day,
            ));

            // ... for current cycle
            partitions.push((0, 0, slot_count_in_two_day));
        }
        parent_cycle_index = 0;
    }

    partitions.push((
        parent_cycle_index,
        current_cycle_index,
        slot_count_in_two_day,
    ));

    partitions
}

// Mostly, the pair (start_index & end_index) is equivalent to this range:
// start_index..=end_index. But it has some exceptional cases, including
// this important and valid one:
//   0..=0: the first partition in the new epoch when crossing epochs
pub fn pubkey_range_from_partition(
    (start_index, end_index, partition_count): Partition,
) -> RangeInclusive<Pubkey> {
    assert!(start_index <= end_index);
    assert!(start_index < partition_count);
    assert!(end_index < partition_count);
    assert!(0 < partition_count);

    type Prefix = u64;
    const PREFIX_SIZE: usize = mem::size_of::<Prefix>();
    const PREFIX_MAX: Prefix = Prefix::max_value();

    let mut start_pubkey = [0x00u8; 32];
    let mut end_pubkey = [0xffu8; 32];

    if partition_count == 1 {
        assert_eq!(start_index, 0);
        assert_eq!(end_index, 0);
        return Pubkey::new_from_array(start_pubkey)..=Pubkey::new_from_array(end_pubkey);
    }

    // not-overflowing way of `(Prefix::max_value() + 1) / partition_count`
    let partition_width = (PREFIX_MAX - partition_count + 1) / partition_count + 1;
    let mut start_key_prefix = if start_index == 0 && end_index == 0 {
        0
    } else if start_index + 1 == partition_count {
        PREFIX_MAX
    } else {
        (start_index + 1) * partition_width
    };

    let mut end_key_prefix = if end_index + 1 == partition_count {
        PREFIX_MAX
    } else {
        (end_index + 1) * partition_width - 1
    };

    if start_index != 0 && start_index == end_index {
        // n..=n (n != 0): a noop pair across epochs without a gap under
        // multi_epoch_cycle, just nullify it.
        if end_key_prefix == PREFIX_MAX {
            start_key_prefix = end_key_prefix;
            start_pubkey = end_pubkey;
        } else {
            end_key_prefix = start_key_prefix;
            end_pubkey = start_pubkey;
        }
    }

    start_pubkey[0..PREFIX_SIZE].copy_from_slice(&start_key_prefix.to_be_bytes());
    end_pubkey[0..PREFIX_SIZE].copy_from_slice(&end_key_prefix.to_be_bytes());
    let start_pubkey_final = Pubkey::new_from_array(start_pubkey);
    let end_pubkey_final = Pubkey::new_from_array(end_pubkey);
    trace!(
        "pubkey_range_from_partition: ({}-{})/{} [{}]: {}-{}",
        start_index,
        end_index,
        partition_count,
        (end_key_prefix - start_key_prefix),
        start_pubkey.iter().map(|x| format!("{x:02x}")).join(""),
        end_pubkey.iter().map(|x| format!("{x:02x}")).join(""),
    );
    #[cfg(test)]
    if start_index != end_index {
        assert_eq!(
            if start_index == 0 && end_index == 0 {
                0
            } else {
                start_index + 1
            },
            partition_from_pubkey(&start_pubkey_final, partition_count),
            "{start_index}, {end_index}, start_key_prefix: {start_key_prefix}, {start_pubkey_final}, {partition_count}"
        );
        assert_eq!(
            end_index,
            partition_from_pubkey(&end_pubkey_final, partition_count),
            "{start_index}, {end_index}, {end_pubkey_final}, {partition_count}"
        );
        if start_index != 0 {
            start_pubkey[0..PREFIX_SIZE]
                .copy_from_slice(&start_key_prefix.saturating_sub(1).to_be_bytes());
            let pubkey_test = Pubkey::new_from_array(start_pubkey);
            assert_eq!(
                start_index,
                partition_from_pubkey(&pubkey_test, partition_count),
                "{}, {}, start_key_prefix-1: {}, {}, {}",
                start_index,
                end_index,
                start_key_prefix.saturating_sub(1),
                pubkey_test,
                partition_count
            );
        }
        if end_index != partition_count - 1 && end_index != 0 {
            end_pubkey[0..PREFIX_SIZE]
                .copy_from_slice(&end_key_prefix.saturating_add(1).to_be_bytes());
            let pubkey_test = Pubkey::new_from_array(end_pubkey);
            assert_eq!(
                end_index.saturating_add(1),
                partition_from_pubkey(&pubkey_test, partition_count),
                "start: {}, end: {}, pubkey: {}, partition_count: {}, prefix_before_addition: {}, prefix after: {}",
                start_index,
                end_index,
                pubkey_test,
                partition_count,
                end_key_prefix,
                end_key_prefix.saturating_add(1),
            );
        }
    }
    // should be an inclusive range (a closed interval) like this:
    // [0xgg00-0xhhff], [0xii00-0xjjff], ... (where 0xii00 == 0xhhff + 1)
    start_pubkey_final..=end_pubkey_final
}

pub fn prefix_from_pubkey(pubkey: &Pubkey) -> u64 {
    const PREFIX_SIZE: usize = mem::size_of::<u64>();
    u64::from_be_bytes(pubkey.as_ref()[0..PREFIX_SIZE].try_into().unwrap())
}

/// This is the inverse of pubkey_range_from_partition.
/// return the lowest end_index which would contain this pubkey
pub fn partition_from_pubkey(
    pubkey: &Pubkey,
    partition_count: PartitionsPerCycle,
) -> PartitionIndex {
    type Prefix = u64;
    const PREFIX_MAX: Prefix = Prefix::max_value();

    if partition_count == 1 {
        return 0;
    }

    // not-overflowing way of `(Prefix::max_value() + 1) / partition_count`
    let partition_width = (PREFIX_MAX - partition_count + 1) / partition_count + 1;

    let prefix = prefix_from_pubkey(pubkey);
    if prefix == 0 {
        return 0;
    }

    if prefix == PREFIX_MAX {
        return partition_count - 1;
    }

    let mut result = (prefix + 1) / partition_width;
    if (prefix + 1) % partition_width == 0 {
        // adjust for integer divide
        result = result.saturating_sub(1);
    }
    result
}

lazy_static! {
    static ref EMPTY_HASHSET: HashSet<Pubkey> = HashSet::default();
}

/// populated at startup with the accounts that were found that are rent paying.
/// These are the 'possible' rent paying accounts.
/// This set can never grow during runtime since it is not possible to create rent paying accounts now.
/// It can shrink during execution if a rent paying account is dropped to lamports=0 or is topped off.
/// The next time the validator restarts, it will remove the account from this list.
#[derive(Debug, Default)]
pub struct RentPayingAccountsByPartition {
    /// 1st index is partition end index, 0..=432_000
    /// 2nd dimension is list of pubkeys which were identified at startup to be rent paying
    /// At the moment, we use this data structure to verify all rent paying accounts are expected.
    /// When we stop iterating the accounts index to FIND rent paying accounts, we will no longer need this to be a hashset.
    /// It can just be a vec.
    pub accounts: Vec<HashSet<Pubkey>>,
    partition_count: PartitionsPerCycle,
}

impl RentPayingAccountsByPartition {
    /// create new struct. Need slots per epoch from 'epoch_schedule'
    pub fn new(epoch_schedule: &EpochSchedule) -> Self {
        let partition_count = epoch_schedule.slots_per_epoch;
        Self {
            partition_count,
            accounts: (0..=partition_count)
                .map(|_| HashSet::<Pubkey>::default())
                .collect(),
        }
    }
    /// Remember that 'pubkey' can possibly be rent paying.
    pub fn add_account(&mut self, pubkey: &Pubkey) {
        let partition_end_index = partition_from_pubkey(pubkey, self.partition_count);
        let list = &mut self.accounts[partition_end_index as usize];

        list.insert(*pubkey);
    }
    /// return all pubkeys that can possibly be rent paying with this partition end_index
    pub fn get_pubkeys_in_partition_index(
        &self,
        partition_end_index: PartitionIndex,
    ) -> &HashSet<Pubkey> {
        self.accounts
            .get(partition_end_index as usize)
            .unwrap_or(&EMPTY_HASHSET)
    }
    pub fn is_initialized(&self) -> bool {
        self.partition_count != 0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use {super::*, std::str::FromStr};

    #[test]
    fn test_get_partition_end_indexes() {
        for n in 5..7 {
            assert_eq!(vec![0], get_partition_end_indexes(&(0, 0, n)));
            assert!(get_partition_end_indexes(&(1, 1, n)).is_empty());
            assert_eq!(vec![1], get_partition_end_indexes(&(0, 1, n)));
            assert_eq!(vec![1, 2], get_partition_end_indexes(&(0, 2, n)));
            assert_eq!(vec![3, 4], get_partition_end_indexes(&(2, 4, n)));
        }
    }

    #[test]
    fn test_rent_pubkey_range_max() {
        // start==end && start != 0 is curious behavior. Verifying it here.
        solana_logger::setup();
        let range = pubkey_range_from_partition((1, 1, 3));
        let p = partition_from_pubkey(range.start(), 3);
        assert_eq!(p, 2);
        let range = pubkey_range_from_partition((1, 2, 3));
        let p = partition_from_pubkey(range.start(), 3);
        assert_eq!(p, 2);
        let range = pubkey_range_from_partition((2, 2, 3));
        let p = partition_from_pubkey(range.start(), 3);
        assert_eq!(p, 2);
        let range = pubkey_range_from_partition((1, 1, 16));
        let p = partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 2);
        let range = pubkey_range_from_partition((1, 2, 16));
        let p = partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 2);
        let range = pubkey_range_from_partition((2, 2, 16));
        let p = partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 3);
        let range = pubkey_range_from_partition((15, 15, 16));
        let p = partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 15);
    }

    #[test]
    fn test_rent_eager_pubkey_range_minimal() {
        let range = pubkey_range_from_partition((0, 0, 1));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])..=Pubkey::new_from_array([0xff; 32])
        );
    }

    #[test]
    fn test_rent_eager_pubkey_range_maximum() {
        let max = !0;

        let range = pubkey_range_from_partition((0, 0, max));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let range = pubkey_range_from_partition((0, 1, max));
        const ONE: u8 = 0x01;
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, ONE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ])
                ..=Pubkey::new_from_array([
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let range = pubkey_range_from_partition((max - 3, max - 2, max));
        const FD: u8 = 0xfd;
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfd, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ])
                ..=Pubkey::new_from_array([
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, FD, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let range = pubkey_range_from_partition((max - 2, max - 1, max));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ])..=pubkey_max_value()
        );

        fn should_cause_overflow(partition_count: u64) -> bool {
            // Check `partition_width = (u64::max_value() + 1) / partition_count` is exact and
            // does not have a remainder.
            // This way, `partition_width * partition_count == (u64::max_value() + 1)`,
            // so the test actually tests for overflow
            (u64::max_value() - partition_count + 1) % partition_count == 0
        }

        let max_exact = 64;
        // Make sure `max_exact` divides evenly when calculating `calculate_partition_width`
        assert!(should_cause_overflow(max_exact));
        // Make sure `max_inexact` doesn't divide evenly when calculating `calculate_partition_width`
        let max_inexact = 10;
        assert!(!should_cause_overflow(max_inexact));

        for max in &[max_exact, max_inexact] {
            let range = pubkey_range_from_partition((max - 1, max - 1, *max));
            assert_eq!(range, pubkey_max_value()..=pubkey_max_value());
        }
    }

    #[test]
    fn test_rent_eager_pubkey_range_noop_range() {
        let test_map = map_to_test_bad_range();

        let range = pubkey_range_from_partition((0, 0, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x54, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = pubkey_range_from_partition((1, 1, 3));
        let same = Pubkey::new_from_array([
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]);
        assert_eq!(range, same..=same);
        let _ = test_map.range(range);

        let range = pubkey_range_from_partition((2, 2, 3));
        assert_eq!(range, pubkey_max_value()..=pubkey_max_value());
        let _ = test_map.range(range);
    }

    fn map_to_test_bad_range() -> std::collections::BTreeMap<Pubkey, i8> {
        let mut map = std::collections::BTreeMap::new();
        // when empty, std::collections::BTreeMap doesn't sanitize given range...
        map.insert(solana_sdk::pubkey::new_rand(), 1);
        map
    }

    #[test]
    #[should_panic(expected = "range start is greater than range end in BTreeMap")]
    fn test_rent_eager_bad_range() {
        let test_map = map_to_test_bad_range();
        let _ = test_map.range(
            Pubkey::new_from_array([
                0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x01,
            ])
                ..=Pubkey::new_from_array([
                    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                ]),
        );
    }

    fn pubkey_max_value() -> Pubkey {
        let highest = Pubkey::from_str("JEKNVnkbo3jma5nREBBJCDoXFVeKkD56V3xKrvRmWxFG").unwrap();
        let arr = Pubkey::new_from_array([0xff; 32]);
        assert_eq!(highest, arr);
        arr
    }

    #[test]
    fn test_rent_eager_pubkey_range_dividable() {
        let test_map = map_to_test_bad_range();
        let range = pubkey_range_from_partition((0, 0, 2));

        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = pubkey_range_from_partition((0, 1, 2));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);
    }

    #[test]
    fn test_rent_eager_pubkey_range_not_dividable() {
        solana_logger::setup();

        let test_map = map_to_test_bad_range();
        let range = pubkey_range_from_partition((0, 0, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x54, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = pubkey_range_from_partition((0, 1, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa9, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = pubkey_range_from_partition((1, 2, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);
    }

    #[test]
    fn test_rent_eager_pubkey_range_gap() {
        solana_logger::setup();

        let test_map = map_to_test_bad_range();
        let range = pubkey_range_from_partition((120, 1023, 12345));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x02, 0x82, 0x5a, 0x89, 0xd1, 0xac, 0x58, 0x9c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0x15, 0x3c, 0x1d, 0xf1, 0xc6, 0x39, 0xef, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);
    }

    #[test]
    fn test_add() {
        let mut test = RentPayingAccountsByPartition::new(&EpochSchedule::custom(32, 0, false));
        let pk = Pubkey::from([1; 32]);
        test.add_account(&pk);
        // make sure duplicate adds only result in a single item
        test.add_account(&pk);
        assert_eq!(test.get_pubkeys_in_partition_index(0).len(), 1);
        assert!(test.get_pubkeys_in_partition_index(1).is_empty());
        assert!(test.is_initialized());
        let test = RentPayingAccountsByPartition::default();
        assert!(!test.is_initialized());
    }
}
