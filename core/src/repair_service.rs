//! The `repair_service` module implements the tools necessary to generate a thread which
//! regularly finds missing shreds in the ledger and sends repair requests for those shreds
use crate::{
    cluster_info::ClusterInfo,
    result::Result,
    serve_repair::{RepairType, ServeRepair},
};
use solana_ledger::{
    bank_forks::BankForks,
    blockstore::{Blockstore, CompletedSlotsReceiver, SlotMeta},
};
use solana_sdk::clock::DEFAULT_SLOTS_PER_EPOCH;
use solana_sdk::{clock::Slot, epoch_schedule::EpochSchedule, pubkey::Pubkey};
use std::{
    collections::BTreeSet,
    net::UdpSocket,
    ops::Bound::{Included, Unbounded},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::sleep,
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub const MAX_REPAIR_LENGTH: usize = 512;
pub const REPAIR_MS: u64 = 100;
pub const MAX_ORPHANS: usize = 5;

const MAX_COMPLETED_SLOT_CACHE_LEN: usize = 256;
const COMPLETED_SLOT_CACHE_FLUSH_TRIGGER: usize = 512;

pub enum RepairStrategy {
    RepairRange(RepairSlotRange),
    RepairAll {
        bank_forks: Arc<RwLock<BankForks>>,
        completed_slots_receiver: CompletedSlotsReceiver,
        epoch_schedule: EpochSchedule,
    },
}

pub struct RepairSlotRange {
    pub start: Slot,
    pub end: Slot,
}

impl Default for RepairSlotRange {
    fn default() -> Self {
        RepairSlotRange {
            start: 0,
            end: std::u64::MAX,
        }
    }
}

pub struct RepairService {
    t_repair: JoinHandle<()>,
}

impl RepairService {
    pub fn new(
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
        repair_socket: Arc<UdpSocket>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        repair_strategy: RepairStrategy,
    ) -> Self {
        let t_repair = Builder::new()
            .name("solana-repair-service".to_string())
            .spawn(move || {
                Self::run(
                    &blockstore,
                    &exit,
                    &repair_socket,
                    &cluster_info,
                    repair_strategy,
                )
            })
            .unwrap();

        RepairService { t_repair }
    }

    fn run(
        blockstore: &Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
        repair_socket: &Arc<UdpSocket>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        repair_strategy: RepairStrategy,
    ) {
        let serve_repair = ServeRepair::new(cluster_info.clone());
        let mut epoch_slots: BTreeSet<Slot> = BTreeSet::new();
        let mut old_incomplete_slots: BTreeSet<Slot> = BTreeSet::new();
        let id = cluster_info.read().unwrap().id();
        if let RepairStrategy::RepairAll {
            ref epoch_schedule, ..
        } = repair_strategy
        {
            let current_root = blockstore.last_root();
            Self::initialize_epoch_slots(
                id,
                blockstore,
                &mut epoch_slots,
                &old_incomplete_slots,
                current_root,
                epoch_schedule,
                cluster_info,
            );
        }
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let repairs = {
                match repair_strategy {
                    RepairStrategy::RepairRange(ref repair_slot_range) => {
                        // Strategy used by archivers
                        Self::generate_repairs_in_range(
                            blockstore,
                            MAX_REPAIR_LENGTH,
                            repair_slot_range,
                        )
                    }

                    RepairStrategy::RepairAll {
                        ref completed_slots_receiver,
                        ..
                    } => {
                        let new_root = blockstore.last_root();
                        let lowest_slot = blockstore.lowest_slot();
                        Self::update_epoch_slots(
                            id,
                            new_root,
                            lowest_slot,
                            &mut epoch_slots,
                            &mut old_incomplete_slots,
                            &cluster_info,
                            completed_slots_receiver,
                        );
                        Self::generate_repairs(blockstore, new_root, MAX_REPAIR_LENGTH)
                    }
                }
            };

            if let Ok(repairs) = repairs {
                let reqs: Vec<_> = repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        serve_repair
                            .repair_request(&repair_request)
                            .map(|result| (result, repair_request))
                            .ok()
                    })
                    .collect();

                for ((to, req), _) in reqs {
                    repair_socket.send_to(&req, to).unwrap_or_else(|e| {
                        info!("{} repair req send_to({}) error {:?}", id, to, e);
                        0
                    });
                }
            }
            sleep(Duration::from_millis(REPAIR_MS));
        }
    }

    // Generate repairs for all slots `x` in the repair_range.start <= x <= repair_range.end
    pub fn generate_repairs_in_range(
        blockstore: &Blockstore,
        max_repairs: usize,
        repair_range: &RepairSlotRange,
    ) -> Result<Vec<RepairType>> {
        // Slot height and shred indexes for shreds we want to repair
        let mut repairs: Vec<RepairType> = vec![];
        for slot in repair_range.start..=repair_range.end {
            if repairs.len() >= max_repairs {
                break;
            }

            let meta = blockstore
                .meta(slot)
                .expect("Unable to lookup slot meta")
                .unwrap_or(SlotMeta {
                    slot,
                    ..SlotMeta::default()
                });

            let new_repairs = Self::generate_repairs_for_slot(
                blockstore,
                slot,
                &meta,
                max_repairs - repairs.len(),
            );
            repairs.extend(new_repairs);
        }

        Ok(repairs)
    }

    fn generate_repairs(
        blockstore: &Blockstore,
        root: Slot,
        max_repairs: usize,
    ) -> Result<Vec<RepairType>> {
        // Slot height and shred indexes for shreds we want to repair
        let mut repairs: Vec<RepairType> = vec![];
        Self::generate_repairs_for_fork(blockstore, &mut repairs, max_repairs, root);

        // TODO: Incorporate gossip to determine priorities for repair?

        // Try to resolve orphans in blockstore
        let mut orphans = blockstore.get_orphans(Some(MAX_ORPHANS));
        orphans.retain(|x| *x > root);

        Self::generate_repairs_for_orphans(&orphans[..], &mut repairs);
        Ok(repairs)
    }

    fn generate_repairs_for_slot(
        blockstore: &Blockstore,
        slot: Slot,
        slot_meta: &SlotMeta,
        max_repairs: usize,
    ) -> Vec<RepairType> {
        if slot_meta.is_full() {
            vec![]
        } else if slot_meta.consumed == slot_meta.received {
            vec![RepairType::HighestShred(slot, slot_meta.received)]
        } else {
            let reqs = blockstore.find_missing_data_indexes(
                slot,
                slot_meta.first_shred_timestamp,
                slot_meta.consumed,
                slot_meta.received,
                max_repairs,
            );
            reqs.into_iter()
                .map(|i| RepairType::Shred(slot, i))
                .collect()
        }
    }

    fn generate_repairs_for_orphans(orphans: &[u64], repairs: &mut Vec<RepairType>) {
        repairs.extend(orphans.iter().map(|h| RepairType::Orphan(*h)));
    }

    /// Repairs any fork starting at the input slot
    fn generate_repairs_for_fork(
        blockstore: &Blockstore,
        repairs: &mut Vec<RepairType>,
        max_repairs: usize,
        slot: Slot,
    ) {
        let mut pending_slots = vec![slot];
        while repairs.len() < max_repairs && !pending_slots.is_empty() {
            let slot = pending_slots.pop().unwrap();
            if let Some(slot_meta) = blockstore.meta(slot).unwrap() {
                let new_repairs = Self::generate_repairs_for_slot(
                    blockstore,
                    slot,
                    &slot_meta,
                    max_repairs - repairs.len(),
                );
                repairs.extend(new_repairs);
                let next_slots = slot_meta.next_slots;
                pending_slots.extend(next_slots);
            } else {
                break;
            }
        }
    }

    fn get_completed_slots_past_root(
        blockstore: &Blockstore,
        slots_in_gossip: &mut BTreeSet<Slot>,
        root: Slot,
        epoch_schedule: &EpochSchedule,
    ) {
        let last_confirmed_epoch = epoch_schedule.get_leader_schedule_epoch(root);
        let last_epoch_slot = epoch_schedule.get_last_slot_in_epoch(last_confirmed_epoch);

        let meta_iter = blockstore
            .slot_meta_iterator(root + 1)
            .expect("Couldn't get db iterator");

        for (current_slot, meta) in meta_iter {
            if current_slot > last_epoch_slot {
                break;
            }
            if meta.is_full() {
                slots_in_gossip.insert(current_slot);
            }
        }
    }

    fn initialize_epoch_slots(
        id: Pubkey,
        blockstore: &Blockstore,
        slots_in_gossip: &mut BTreeSet<Slot>,
        old_incomplete_slots: &BTreeSet<Slot>,
        root: Slot,
        epoch_schedule: &EpochSchedule,
        cluster_info: &RwLock<ClusterInfo>,
    ) {
        Self::get_completed_slots_past_root(blockstore, slots_in_gossip, root, epoch_schedule);

        // Safe to set into gossip because by this time, the leader schedule cache should
        // also be updated with the latest root (done in blockstore_processor) and thus
        // will provide a schedule to window_service for any incoming shreds up to the
        // last_confirmed_epoch.
        cluster_info.write().unwrap().push_epoch_slots(
            id,
            root,
            blockstore.lowest_slot(),
            slots_in_gossip.clone(),
            old_incomplete_slots,
        );
    }

    // Update the gossiped structure used for the "Repairmen" repair protocol. See book
    // for details.
    fn update_epoch_slots(
        id: Pubkey,
        latest_known_root: Slot,
        lowest_slot: Slot,
        completed_slot_cache: &mut BTreeSet<Slot>,
        incomplete_slot_stash: &mut BTreeSet<Slot>,
        cluster_info: &RwLock<ClusterInfo>,
        completed_slots_receiver: &CompletedSlotsReceiver,
    ) {
        let mut should_update = false;
        while let Ok(completed_slots) = completed_slots_receiver.try_recv() {
            for slot in completed_slots {
                let last_slot_in_stash = *incomplete_slot_stash.iter().next_back().unwrap_or(&0);
                let removed_from_stash = incomplete_slot_stash.remove(&slot);
                // If the newly completed slot was not being tracked in stash, and is > last
                // slot being tracked in stash, add it to cache. Also, update gossip
                if !removed_from_stash && slot >= last_slot_in_stash {
                    should_update |= completed_slot_cache.insert(slot);
                }
                // If the slot was removed from stash, update gossip
                should_update |= removed_from_stash;
            }
        }

        if should_update {
            if completed_slot_cache.len() >= COMPLETED_SLOT_CACHE_FLUSH_TRIGGER {
                Self::stash_old_incomplete_slots(completed_slot_cache, incomplete_slot_stash);
                let lowest_completed_slot_in_cache =
                    *completed_slot_cache.iter().next().unwrap_or(&0);
                Self::prune_incomplete_slot_stash(
                    incomplete_slot_stash,
                    lowest_completed_slot_in_cache,
                );
            }

            cluster_info.write().unwrap().push_epoch_slots(
                id,
                latest_known_root,
                lowest_slot,
                completed_slot_cache.clone(),
                incomplete_slot_stash,
            );
        }
    }

    fn stash_old_incomplete_slots(cache: &mut BTreeSet<Slot>, stash: &mut BTreeSet<Slot>) {
        if cache.len() > MAX_COMPLETED_SLOT_CACHE_LEN {
            let mut prev = *cache.iter().next().expect("Expected to find some slot");
            cache.remove(&prev);
            while cache.len() >= MAX_COMPLETED_SLOT_CACHE_LEN {
                let next = *cache.iter().next().expect("Expected to find some slot");
                cache.remove(&next);
                // Prev slot and next slot are not included in incomplete slot list.
                (prev + 1..next).for_each(|slot| {
                    stash.insert(slot);
                });
                prev = next;
            }
        }
    }

    fn prune_incomplete_slot_stash(
        stash: &mut BTreeSet<Slot>,
        lowest_completed_slot_in_cache: Slot,
    ) {
        if let Some(oldest_incomplete_slot) = stash.iter().next() {
            // Prune old slots
            // Prune in batches to reduce overhead. Pruning starts when oldest slot is 1.5 epochs
            // earlier than the new root. But, we prune all the slots that are older than 1 epoch.
            // So slots in a batch of half epoch are getting pruned
            if oldest_incomplete_slot + DEFAULT_SLOTS_PER_EPOCH + DEFAULT_SLOTS_PER_EPOCH / 2
                < lowest_completed_slot_in_cache
            {
                let oldest_slot_to_retain =
                    lowest_completed_slot_in_cache.saturating_sub(DEFAULT_SLOTS_PER_EPOCH);
                *stash = stash
                    .range((Included(&oldest_slot_to_retain), Unbounded))
                    .cloned()
                    .collect();
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_repair.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cluster_info::Node;
    use itertools::Itertools;
    use rand::seq::SliceRandom;
    use rand::{thread_rng, Rng};
    use solana_ledger::blockstore::{
        make_chaining_slot_entries, make_many_slot_entries, make_slot_entries,
    };
    use solana_ledger::shred::max_ticks_per_n_shreds;
    use solana_ledger::{blockstore::Blockstore, get_tmp_ledger_path};
    use std::thread::Builder;

    #[test]
    pub fn test_repair_orphan() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            // Create some orphan slots
            let (mut shreds, _) = make_slot_entries(1, 0, 1);
            let (shreds2, _) = make_slot_entries(5, 2, 1);
            shreds.extend(shreds2);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert_eq!(
                RepairService::generate_repairs(&blockstore, 0, 2).unwrap(),
                vec![RepairType::HighestShred(0, 0), RepairType::Orphan(2)]
            );
        }

        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_empty_slot() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(2, 0, 1);

            // Write this shred to slot 2, should chain to slot 0, which we haven't received
            // any shreds for
            blockstore.insert_shreds(shreds, None, false).unwrap();

            // Check that repair tries to patch the empty slot
            assert_eq!(
                RepairService::generate_repairs(&blockstore, 0, 2).unwrap(),
                vec![RepairType::HighestShred(0, 0)]
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_repairs() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let nth = 3;
            let num_slots = 2;

            // Create some shreds
            let (mut shreds, _) = make_many_slot_entries(0, num_slots as u64, 150 as u64);
            let num_shreds = shreds.len() as u64;
            let num_shreds_per_slot = num_shreds / num_slots;

            // write every nth shred
            let mut shreds_to_write = vec![];
            let mut missing_indexes_per_slot = vec![];
            for i in (0..num_shreds).rev() {
                let index = i % num_shreds_per_slot;
                if index % nth == 0 {
                    shreds_to_write.insert(0, shreds.remove(i as usize));
                } else if i < num_shreds_per_slot {
                    missing_indexes_per_slot.insert(0, index);
                }
            }
            blockstore
                .insert_shreds(shreds_to_write, None, false)
                .unwrap();
            // sleep so that the holes are ready for repair
            sleep(Duration::from_secs(1));
            let expected: Vec<RepairType> = (0..num_slots)
                .flat_map(|slot| {
                    missing_indexes_per_slot
                        .iter()
                        .map(move |shred_index| RepairType::Shred(slot as u64, *shred_index))
                })
                .collect();

            assert_eq!(
                RepairService::generate_repairs(&blockstore, 0, std::usize::MAX).unwrap(),
                expected
            );

            assert_eq!(
                RepairService::generate_repairs(&blockstore, 0, expected.len() - 2).unwrap()[..],
                expected[0..expected.len() - 2]
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_highest_repair() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let num_entries_per_slot = 100;

            // Create some shreds
            let (mut shreds, _) = make_slot_entries(0, 0, num_entries_per_slot as u64);
            let num_shreds_per_slot = shreds.len() as u64;

            // Remove last shred (which is also last in slot) so that slot is not complete
            shreds.pop();

            blockstore.insert_shreds(shreds, None, false).unwrap();

            // We didn't get the last shred for this slot, so ask for the highest shred for that slot
            let expected: Vec<RepairType> =
                vec![RepairType::HighestShred(0, num_shreds_per_slot - 1)];

            assert_eq!(
                RepairService::generate_repairs(&blockstore, 0, std::usize::MAX).unwrap(),
                expected
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_range() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let slots: Vec<u64> = vec![1, 3, 5, 7, 8];
            let num_entries_per_slot = max_ticks_per_n_shreds(1) + 1;

            let shreds = make_chaining_slot_entries(&slots, num_entries_per_slot);
            for (mut slot_shreds, _) in shreds.into_iter() {
                slot_shreds.remove(0);
                blockstore.insert_shreds(slot_shreds, None, false).unwrap();
            }
            // sleep to make slot eligible for repair
            sleep(Duration::from_secs(1));
            // Iterate through all possible combinations of start..end (inclusive on both
            // sides of the range)
            for start in 0..slots.len() {
                for end in start..slots.len() {
                    let mut repair_slot_range = RepairSlotRange::default();
                    repair_slot_range.start = slots[start];
                    repair_slot_range.end = slots[end];
                    let expected: Vec<RepairType> = (repair_slot_range.start
                        ..=repair_slot_range.end)
                        .map(|slot_index| {
                            if slots.contains(&(slot_index as u64)) {
                                RepairType::Shred(slot_index as u64, 0)
                            } else {
                                RepairType::HighestShred(slot_index as u64, 0)
                            }
                        })
                        .collect();

                    assert_eq!(
                        RepairService::generate_repairs_in_range(
                            &blockstore,
                            std::usize::MAX,
                            &repair_slot_range
                        )
                        .unwrap(),
                        expected
                    );
                }
            }
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_range_highest() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let num_entries_per_slot = 10;

            let num_slots = 1;
            let start = 5;

            // Create some shreds in slots 0..num_slots
            for i in start..start + num_slots {
                let parent = if i > 0 { i - 1 } else { 0 };
                let (shreds, _) = make_slot_entries(i, parent, num_entries_per_slot as u64);

                blockstore.insert_shreds(shreds, None, false).unwrap();
            }

            let end = 4;
            let expected: Vec<RepairType> = vec![
                RepairType::HighestShred(end - 2, 0),
                RepairType::HighestShred(end - 1, 0),
                RepairType::HighestShred(end, 0),
            ];

            let mut repair_slot_range = RepairSlotRange::default();
            repair_slot_range.start = 2;
            repair_slot_range.end = end;

            assert_eq!(
                RepairService::generate_repairs_in_range(
                    &blockstore,
                    std::usize::MAX,
                    &repair_slot_range
                )
                .unwrap(),
                expected
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_completed_slots_past_root() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();
            let num_entries_per_slot = 10;
            let root = 10;

            let fork1 = vec![5, 7, root, 15, 20, 21];
            let fork1_shreds: Vec<_> = make_chaining_slot_entries(&fork1, num_entries_per_slot)
                .into_iter()
                .flat_map(|(shreds, _)| shreds)
                .collect();
            let fork2 = vec![8, 12];
            let fork2_shreds = make_chaining_slot_entries(&fork2, num_entries_per_slot);

            // Remove the last shred from each slot to make an incomplete slot
            let fork2_incomplete_shreds: Vec<_> = fork2_shreds
                .into_iter()
                .flat_map(|(mut shreds, _)| {
                    shreds.pop();
                    shreds
                })
                .collect();
            let mut full_slots = BTreeSet::new();

            blockstore.insert_shreds(fork1_shreds, None, false).unwrap();
            blockstore
                .insert_shreds(fork2_incomplete_shreds, None, false)
                .unwrap();

            // Test that only slots > root from fork1 were included
            let epoch_schedule = EpochSchedule::custom(32, 32, false);

            RepairService::get_completed_slots_past_root(
                &blockstore,
                &mut full_slots,
                root,
                &epoch_schedule,
            );

            let mut expected: BTreeSet<_> = fork1.into_iter().filter(|x| *x > root).collect();
            assert_eq!(full_slots, expected);

            // Test that slots past the last confirmed epoch boundary don't get included
            let last_epoch = epoch_schedule.get_leader_schedule_epoch(root);
            let last_slot = epoch_schedule.get_last_slot_in_epoch(last_epoch);
            let fork3 = vec![last_slot, last_slot + 1];
            let fork3_shreds: Vec<_> = make_chaining_slot_entries(&fork3, num_entries_per_slot)
                .into_iter()
                .flat_map(|(shreds, _)| shreds)
                .collect();
            blockstore.insert_shreds(fork3_shreds, None, false).unwrap();
            RepairService::get_completed_slots_past_root(
                &blockstore,
                &mut full_slots,
                root,
                &epoch_schedule,
            );
            expected.insert(last_slot);
            assert_eq!(full_slots, expected);
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_update_epoch_slots() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            // Create blockstore
            let (blockstore, _, completed_slots_receiver) =
                Blockstore::open_with_signal(&blockstore_path).unwrap();

            let blockstore = Arc::new(blockstore);

            let mut root = 0;
            let num_slots = 100;
            let entries_per_slot = 5;
            let blockstore_ = blockstore.clone();

            // Spin up thread to write to blockstore
            let writer = Builder::new()
                .name("writer".to_string())
                .spawn(move || {
                    let slots: Vec<_> = (1..num_slots + 1).collect();
                    let mut shreds: Vec<_> = make_chaining_slot_entries(&slots, entries_per_slot)
                        .into_iter()
                        .flat_map(|(shreds, _)| shreds)
                        .collect();
                    shreds.shuffle(&mut thread_rng());
                    let mut i = 0;
                    let max_step = entries_per_slot * 4;
                    let repair_interval_ms = 10;
                    let mut rng = rand::thread_rng();
                    let num_shreds = shreds.len();
                    while i < num_shreds {
                        let step = rng.gen_range(1, max_step + 1) as usize;
                        let step = std::cmp::min(step, num_shreds - i);
                        let shreds_to_insert = shreds.drain(..step).collect_vec();
                        blockstore_
                            .insert_shreds(shreds_to_insert, None, false)
                            .unwrap();
                        sleep(Duration::from_millis(repair_interval_ms));
                        i += step;
                    }
                })
                .unwrap();

            let mut completed_slots = BTreeSet::new();
            let node_info = Node::new_localhost_with_pubkey(&Pubkey::default());
            let cluster_info = RwLock::new(ClusterInfo::new_with_invalid_keypair(
                node_info.info.clone(),
            ));

            let mut old_incomplete_slots: BTreeSet<Slot> = BTreeSet::new();
            while completed_slots.len() < num_slots as usize {
                RepairService::update_epoch_slots(
                    Pubkey::default(),
                    root,
                    blockstore.lowest_slot(),
                    &mut completed_slots,
                    &mut old_incomplete_slots,
                    &cluster_info,
                    &completed_slots_receiver,
                );
            }

            let mut expected: BTreeSet<_> = (1..num_slots + 1).collect();
            assert_eq!(completed_slots, expected);

            // Update with new root, should filter out the slots <= root
            root = num_slots / 2;
            let (shreds, _) = make_slot_entries(num_slots + 2, num_slots + 1, entries_per_slot);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            RepairService::update_epoch_slots(
                Pubkey::default(),
                root,
                0,
                &mut completed_slots,
                &mut old_incomplete_slots,
                &cluster_info,
                &completed_slots_receiver,
            );
            expected.insert(num_slots + 2);
            assert_eq!(completed_slots, expected);
            writer.join().unwrap();
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_stash_old_incomplete_slots() {
        let mut cache: BTreeSet<Slot> = BTreeSet::new();
        let mut stash: BTreeSet<Slot> = BTreeSet::new();

        // When cache is empty.
        RepairService::stash_old_incomplete_slots(&mut cache, &mut stash);
        assert_eq!(stash.len(), 0);

        // Insert some slots in cache ( < MAX_COMPLETED_SLOT_CACHE_LEN + 1)
        cache.insert(101);
        cache.insert(102);
        cache.insert(104);
        cache.insert(105);

        // Not enough slots in cache. So stash should remain empty.
        RepairService::stash_old_incomplete_slots(&mut cache, &mut stash);
        assert_eq!(stash.len(), 0);
        assert_eq!(cache.len(), 4);

        // Insert slots in cache ( = MAX_COMPLETED_SLOT_CACHE_LEN)
        let mut cache: BTreeSet<Slot> = BTreeSet::new();
        (0..MAX_COMPLETED_SLOT_CACHE_LEN as u64)
            .into_iter()
            .for_each(|slot| {
                cache.insert(slot);
            });

        // Not enough slots in cache. So stash should remain empty.
        RepairService::stash_old_incomplete_slots(&mut cache, &mut stash);
        assert_eq!(stash.len(), 0);
        assert_eq!(cache.len(), MAX_COMPLETED_SLOT_CACHE_LEN);

        // Insert 1 more to cross the threshold
        cache.insert(MAX_COMPLETED_SLOT_CACHE_LEN as u64);
        RepairService::stash_old_incomplete_slots(&mut cache, &mut stash);
        // Stash is still empty, as no missing slots
        assert_eq!(stash.len(), 0);
        // It removed some entries from cache
        assert_eq!(cache.len(), MAX_COMPLETED_SLOT_CACHE_LEN - 1);

        // Insert more slots to create a missing slot
        let mut cache: BTreeSet<Slot> = BTreeSet::new();
        cache.insert(0);
        (2..=MAX_COMPLETED_SLOT_CACHE_LEN as u64 + 2)
            .into_iter()
            .for_each(|slot| {
                cache.insert(slot);
            });
        RepairService::stash_old_incomplete_slots(&mut cache, &mut stash);

        // Stash is not empty
        assert!(stash.contains(&1));
        // It removed some entries from cache
        assert_eq!(cache.len(), MAX_COMPLETED_SLOT_CACHE_LEN - 1);

        // Test multiple missing slots at dispersed locations
        let mut cache: BTreeSet<Slot> = BTreeSet::new();
        (0..MAX_COMPLETED_SLOT_CACHE_LEN as u64 * 2)
            .into_iter()
            .for_each(|slot| {
                cache.insert(slot);
            });

        cache.remove(&10);
        cache.remove(&11);

        cache.remove(&28);
        cache.remove(&29);

        cache.remove(&148);
        cache.remove(&149);
        cache.remove(&150);
        cache.remove(&151);

        RepairService::stash_old_incomplete_slots(&mut cache, &mut stash);

        // Stash is not empty
        assert!(stash.contains(&10));
        assert!(stash.contains(&11));
        assert!(stash.contains(&28));
        assert!(stash.contains(&29));
        assert!(stash.contains(&148));
        assert!(stash.contains(&149));
        assert!(stash.contains(&150));
        assert!(stash.contains(&151));

        assert!(!stash.contains(&147));
        assert!(!stash.contains(&152));
        // It removed some entries from cache
        assert_eq!(cache.len(), MAX_COMPLETED_SLOT_CACHE_LEN - 1);
        (MAX_COMPLETED_SLOT_CACHE_LEN + 1..MAX_COMPLETED_SLOT_CACHE_LEN * 2)
            .into_iter()
            .for_each(|slot| {
                let slot: u64 = slot as u64;
                assert!(cache.contains(&slot));
            });
    }

    #[test]
    fn test_prune_incomplete_slot_stash() {
        // Prune empty stash
        let mut stash: BTreeSet<Slot> = BTreeSet::new();
        RepairService::prune_incomplete_slot_stash(&mut stash, 0);
        assert!(stash.is_empty());

        // Prune stash with slots < DEFAULT_SLOTS_PER_EPOCH
        stash.insert(0);
        stash.insert(10);
        stash.insert(11);
        stash.insert(50);
        assert_eq!(stash.len(), 4);
        RepairService::prune_incomplete_slot_stash(&mut stash, 100);
        assert_eq!(stash.len(), 4);

        // Prune stash with slots > DEFAULT_SLOTS_PER_EPOCH, but < 1.5 * DEFAULT_SLOTS_PER_EPOCH
        stash.insert(DEFAULT_SLOTS_PER_EPOCH + 50);
        assert_eq!(stash.len(), 5);
        RepairService::prune_incomplete_slot_stash(&mut stash, DEFAULT_SLOTS_PER_EPOCH + 100);
        assert_eq!(stash.len(), 5);

        // Prune stash with slots > 1.5 * DEFAULT_SLOTS_PER_EPOCH
        stash.insert(DEFAULT_SLOTS_PER_EPOCH + DEFAULT_SLOTS_PER_EPOCH / 2);
        assert_eq!(stash.len(), 6);
        RepairService::prune_incomplete_slot_stash(
            &mut stash,
            DEFAULT_SLOTS_PER_EPOCH + DEFAULT_SLOTS_PER_EPOCH / 2 + 1,
        );
        assert_eq!(stash.len(), 2);
    }
}
