//! The `repair_service` module implements the tools necessary to generate a thread which
//! regularly finds missing shreds in the ledger and sends repair requests for those shreds
use crate::{
    cluster_info::ClusterInfo,
    cluster_slots::ClusterSlots,
    result::Result,
    serve_repair::{RepairType, ServeRepair},
};
use rand::seq::SliceRandom;
use rand::thread_rng;
use solana_ledger::{
    bank_forks::BankForks,
    blockstore::{Blockstore, CompletedSlotsReceiver, SlotMeta},
};
use solana_sdk::{clock::Slot, epoch_schedule::EpochSchedule, pubkey::Pubkey};

use std::{
    collections::HashSet,
    iter::Iterator,
    net::{SocketAddr, UdpSocket},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::sleep,
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub const MAX_REPAIR_LENGTH: usize = 512;
pub const REPAIR_MS: u64 = 100;
pub const MAX_ORPHANS: usize = 5;

pub enum RepairStrategy {
    RepairRange(RepairSlotRange),
    RepairAll {
        bank_forks: Arc<RwLock<BankForks>>,
        completed_slots_receiver: CompletedSlotsReceiver,
        epoch_schedule: EpochSchedule,
    },
}
impl RepairStrategy {
    pub fn bank_forks(&self) -> Option<&Arc<RwLock<BankForks>>> {
        match self {
            RepairStrategy::RepairRange(_) => None,
            RepairStrategy::RepairAll { ref bank_forks, .. } => Some(bank_forks),
        }
    }
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

    fn generate_repair_all(
        self_id: &Pubkey,
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        cluster_info: &RwLock<ClusterInfo>,
        cluster_slots: &mut ClusterSlots,
        completed_slots_receiver: &CompletedSlotsReceiver,
    ) -> Result<Vec<RepairType>> {
        let new_root = blockstore.last_root();
        let mut full: Vec<_> = Self::find_completed_slots(blockstore, new_root)
            .into_iter()
            .collect();
        let partial: Vec<_> = Self::find_incomplete_slots(blockstore, new_root)
            .into_iter()
            .collect();
        let mut received_completed = Self::recv_completed(completed_slots_receiver);
        full.append(&mut received_completed);
        Self::update_cluster_info_epoch_slots(
            self_id,
            cluster_info,
            cluster_slots,
            &full,
            &partial,
        );
        cluster_slots.update(new_root, &cluster_info, &bank_forks);
        let mut orphans =
            cluster_slots.generate_repairs_for_missing_slots(&self_id, new_root);
        orphans.shuffle(&mut thread_rng());
        let mut repairs = Self::generate_repairs(
            blockstore,
            new_root,
            &partial,
            MAX_REPAIR_LENGTH,
        )?;
        repairs.append(&mut orphans);
        repairs.truncate(MAX_REPAIR_LENGTH);
        Ok(repairs)
    }

    fn run(
        blockstore: &Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
        repair_socket: &Arc<UdpSocket>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        repair_strategy: RepairStrategy,
    ) {
        let serve_repair = ServeRepair::new(cluster_info.clone());
        let mut cluster_slots = ClusterSlots::default();
        let self_id = cluster_info.read().unwrap().id();
        if let RepairStrategy::RepairAll {
            ref epoch_schedule, ..
        } = repair_strategy
        {
            Self::initialize_epoch_slots(blockstore, epoch_schedule, cluster_info);
        }
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let (mine, theirs, weight) = cluster_slots.stats();
            if mine != theirs {
                info!(
                    "{}:CLUSTER_SLOTS have {}/{} slots their weight: {}",
                    self_id, mine, theirs, weight
                );
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
                        ref bank_forks,
                        ..
                    } => {
                        Self::generate_repair_all(&self_id, &blockstore, bank_forks, &cluster_info, &mut cluster_slots, completed_slots_receiver)
                    }
                }
            };

            if let Ok(repairs) = repairs {
                debug!("{}:CLUSTER_SLOTS REPAIRS: {:?}", self_id, repairs);
                let reqs: Vec<((SocketAddr, Vec<u8>), RepairType)> = repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        let peers = cluster_slots.peers(repair_request.slot());
                        assert!(peers.iter().find(|x| *x.0 == self_id).is_none());
                        debug!("{}:REPAIR_REQUEST PEERS: {:?}", self_id, peers.len());
                        let e = serve_repair
                            .repair_request(&peers, &repair_request)
                            .map(|result| (result, repair_request));
                        if e.is_err() {
                            warn!("{}:REPAIR_REQUEST FAILED: {:?}", self_id, e);
                        }
                        e.ok()
                    })
                    .collect();
                debug!("{}:CLUSTER_SLOTS REQUESTS: {}", self_id, reqs.len());
                for ((to, req), _) in reqs {
                    repair_socket.send_to(&req, to).unwrap_or_else(|e| {
                        debug!("{} repair req send_to({}) error {:?}", self_id, to, e);
                        0
                    });
                }
            }
            sleep(Duration::from_millis(REPAIR_MS));
        }
    }

    fn update_cluster_info_epoch_slots(
        self_id: &Pubkey,
        cluster_info: &RwLock<ClusterInfo>,
        cluster_slots: &ClusterSlots,
        incomplete: &[Slot],
        complete: &[Slot],
    ) {
        let mine = cluster_slots.collect(self_id);
        let mut new = vec![];
        new.extend(incomplete.iter().filter(|x| !mine.contains(x)).cloned());
        new.extend(complete.iter().filter(|x| !mine.contains(x)).cloned());
        new.sort();
        new.dedup();
        if !new.is_empty() {
            cluster_info.write().unwrap().push_epoch_slots(&new);
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

    #[cfg(test)]
    fn sort_repairs(repairs: &mut Vec<RepairType>) {
        pub fn rt_index(rep: &RepairType) -> u64 {
            match rep {
                RepairType::Orphan(_) => 0,
                RepairType::HighestShred(_, ix) => *ix,
                RepairType::Shred(_, ix) => *ix,
            }
        }
        repairs.sort_by_key(|x| (x.slot(), rt_index(&x)));
    }
    #[cfg(test)]
    fn test_generate_repairs(
        mut repairs: Vec<RepairType>,
        blockstore: &Blockstore,
        root: Slot,
        max_repairs: usize,
    ) -> Result<Vec<RepairType>> {
        let incomplete: Vec<_> = Self::find_incomplete_slots(blockstore, root)
            .into_iter()
            .collect();
        let mut vec = Self::generate_repairs(blockstore, root, &incomplete, max_repairs)?;
        vec.append(&mut repairs);
        Self::sort_repairs(&mut vec);
        Ok(vec)
    }

    fn generate_repairs(
        blockstore: &Blockstore,
        root: Slot,
        incomplete: &[Slot],
        max_repairs: usize,
    ) -> Result<Vec<RepairType>> {
        let mut repairs = vec![];
        // Slot height and shred indexes for shreds we want to repair
        Self::generate_repairs_for_fork(blockstore, &mut repairs, max_repairs, incomplete);

        // TODO: Incorporate gossip to determine priorities for repair?

        // Try to resolve orphans in blockstore
        let orphans = blockstore.orphans_iterator(root + 1).unwrap();
        Self::generate_repairs_for_orphans(orphans, &mut repairs);
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

    fn generate_repairs_for_orphans(
        orphans: impl Iterator<Item = u64>,
        repairs: &mut Vec<RepairType>,
    ) {
        repairs.extend(orphans.take(MAX_ORPHANS).map(RepairType::Orphan));
    }

    /// Repairs any fork starting at the input slot
    fn generate_repairs_for_fork(
        blockstore: &Blockstore,
        repairs: &mut Vec<RepairType>,
        max_repairs: usize,
        incomplete: &[Slot],
    ) {
        for slot in incomplete {
            if let Some(slot_meta) = blockstore.meta(*slot).unwrap() {
                let new_repairs = Self::generate_repairs_for_slot(
                    blockstore,
                    *slot,
                    &slot_meta,
                    max_repairs - repairs.len(),
                );
                repairs.extend(new_repairs);
            }
        }
    }

    fn get_completed_slots_past_root(
        blockstore: &Blockstore,
        root: Slot,
        epoch_schedule: &EpochSchedule,
    ) -> Vec<Slot> {
        let last_confirmed_epoch = epoch_schedule.get_leader_schedule_epoch(root);
        let last_epoch_slot = epoch_schedule.get_last_slot_in_epoch(last_confirmed_epoch);
        let mut new_slots = Self::find_completed_slots(blockstore, root);
        new_slots.retain(|x| *x <= last_epoch_slot);
        new_slots.remove(&root);
        let mut rv: Vec<_> = new_slots.into_iter().collect();
        rv.sort();
        rv
    }

    fn initialize_epoch_slots(
        blockstore: &Blockstore,
        epoch_schedule: &EpochSchedule,
        cluster_info: &RwLock<ClusterInfo>,
    ) {
        let root = blockstore.last_root();
        let new_slots = Self::get_completed_slots_past_root(blockstore, root, epoch_schedule);

        // Safe to set into gossip because by this time, the leader schedule cache should
        // also be updated with the latest root (done in blockstore_processor) and thus
        // will provide a schedule to window_service for any incoming shreds up to the
        // last_confirmed_epoch.
        debug!(
            "{}: initialize_epoch_slots: {:?}",
            cluster_info.read().unwrap().id(),
            new_slots
        );
        cluster_info.write().unwrap().push_epoch_slots(&new_slots);
    }

    fn find_completed_slots(blockstore: &Blockstore, root: Slot) -> HashSet<Slot> {
        blockstore
            .live_slots_iterator(root)
            .filter_map(|(slot, slot_meta)| {
                if slot_meta.is_full() {
                    Some(slot)
                } else {
                    None
                }
            })
            .collect()
    }

    // Update the gossiped structure used for the "Repairmen" repair protocol. See book
    // for details.
    fn recv_completed(completed_slots_receiver: &CompletedSlotsReceiver) -> Vec<Slot> {
        let mut completed = vec![];
        while let Ok(mut new) = completed_slots_receiver.try_recv() {
            completed.append(&mut new);
        }
        completed
    }

    fn find_incomplete_slots(blockstore: &Blockstore, root: Slot) -> HashSet<Slot> {
        blockstore
            .live_slots_iterator(root)
            .filter_map(|(slot, slot_meta)| {
                if !slot_meta.is_full() {
                    Some(slot)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_repair.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_ledger::blockstore::{
        make_chaining_slot_entries, make_many_slot_entries, make_slot_entries,
    };
    use solana_ledger::shred::max_ticks_per_n_shreds;
    use solana_ledger::{blockstore::Blockstore, get_tmp_ledger_path};

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
            let repairs = RepairService::test_generate_repairs(vec![], &blockstore, 0, 2).unwrap();
            assert_eq!(
                repairs,
                vec![
                    RepairType::HighestShred(0, 0),
                    RepairType::HighestShred(2, 0),
                    RepairType::Orphan(2)
                ]
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
                RepairService::test_generate_repairs(vec![], &blockstore, 0, 2).unwrap(),
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
            let mut expected: Vec<RepairType> = (0..num_slots)
                .flat_map(|slot| {
                    missing_indexes_per_slot
                        .iter()
                        .map(move |shred_index| RepairType::Shred(slot as u64, *shred_index))
                })
                .collect();
            RepairService::sort_repairs(&mut expected);
            assert_eq!(
                RepairService::test_generate_repairs(vec![], &blockstore, 0, std::usize::MAX)
                    .unwrap(),
                expected
            );

            assert_eq!(
                RepairService::test_generate_repairs(vec![], &blockstore, 0, expected.len() - 2)
                    .unwrap()
                    .len(),
                expected.len() - 2
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
                RepairService::test_generate_repairs(vec![], &blockstore, 0, std::usize::MAX)
                    .unwrap(),
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
            let fork2 = vec![8, root, 12];
            let fork2_shreds = make_chaining_slot_entries(&fork2, num_entries_per_slot);

            // Remove the last shred from each slot to make an incomplete slot
            let fork2_incomplete_shreds: Vec<_> = fork2_shreds
                .into_iter()
                .flat_map(|(mut shreds, _)| {
                    shreds.pop();
                    shreds
                })
                .collect();

            blockstore.insert_shreds(fork1_shreds, None, false).unwrap();
            blockstore
                .insert_shreds(fork2_incomplete_shreds, None, false)
                .unwrap();

            // Test that only slots > root from fork1 were included, and that none
            // of the incomplete slots from fork 2 were included
            let epoch_schedule = EpochSchedule::custom(32, 32, false);

            let mut full_slots =
                RepairService::get_completed_slots_past_root(&blockstore, root, &epoch_schedule);
            full_slots.sort();

            let mut expected: Vec<_> = fork1.into_iter().filter(|x| *x > root).collect();
            full_slots.sort();
            assert_eq!(full_slots, expected);

            // Test that slots past the last confirmed epoch boundary don't get included
            let last_epoch = epoch_schedule.get_leader_schedule_epoch(root);
            let last_slot = epoch_schedule.get_last_slot_in_epoch(last_epoch);
            let fork3 = vec![root, last_slot, last_slot + 1];
            let fork3_shreds: Vec<_> = make_chaining_slot_entries(&fork3, num_entries_per_slot)
                .into_iter()
                .flat_map(|(shreds, _)| shreds)
                .collect();

            blockstore.insert_shreds(fork3_shreds, None, false).unwrap();
            let mut full_slots =
                RepairService::get_completed_slots_past_root(&blockstore, root, &epoch_schedule);
            expected.push(last_slot);
            full_slots.sort();
            assert_eq!(full_slots, expected);
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_find_incomplete_slots() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();
            let num_entries_per_slot = 100;
            let (mut shreds, _) = make_slot_entries(0, 0, num_entries_per_slot);
            assert!(shreds.len() > 1);
            let (shreds4, _) = make_slot_entries(4, 0, num_entries_per_slot);
            shreds.extend(shreds4);
            blockstore.insert_shreds(shreds, None, false).unwrap();

            // Nothing is incomplete
            assert!(RepairService::find_incomplete_slots(&blockstore, 0).is_empty());

            // Insert a slot 5 that chains to an incomplete orphan slot 3
            let (shreds5, _) = make_slot_entries(5, 3, num_entries_per_slot);
            blockstore.insert_shreds(shreds5, None, false).unwrap();
            assert_eq!(
                RepairService::find_incomplete_slots(&blockstore, 0),
                vec![3].into_iter().collect()
            );

            // Insert another incomplete orphan slot 2 that is the parent of slot 3.
            // Both should be incomplete
            let (shreds3, _) = make_slot_entries(3, 2, num_entries_per_slot);
            blockstore
                .insert_shreds(shreds3[1..].to_vec(), None, false)
                .unwrap();
            assert_eq!(
                RepairService::find_incomplete_slots(&blockstore, 0),
                vec![2, 3].into_iter().collect()
            );

            // Insert a incomplete slot 6 that chains to the root 0,
            // should also be incomplete
            let (shreds6, _) = make_slot_entries(6, 0, num_entries_per_slot);
            blockstore
                .insert_shreds(shreds6[1..].to_vec(), None, false)
                .unwrap();
            assert_eq!(
                RepairService::find_incomplete_slots(&blockstore, 0),
                vec![2, 3, 6].into_iter().collect()
            );

            // Complete slot 3, should no longer be marked incomplete
            blockstore
                .insert_shreds(shreds3[..].to_vec(), None, false)
                .unwrap();

            assert_eq!(
                RepairService::find_incomplete_slots(&blockstore, 0),
                vec![2, 6].into_iter().collect()
            );
        }

        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }
}
