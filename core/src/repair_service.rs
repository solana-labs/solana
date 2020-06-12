//! The `repair_service` module implements the tools necessary to generate a thread which
//! regularly finds missing shreds in the ledger and sends repair requests for those shreds
use crate::{
    cluster_info::ClusterInfo,
    cluster_slots::ClusterSlots,
    commitment::VOTE_THRESHOLD_SIZE,
    result::Result,
    serve_repair::{RepairType, ServeRepair, DEFAULT_NONCE},
};
use crossbeam_channel::{Receiver as CrossbeamReceiver, Sender as CrossbeamSender};
use solana_ledger::{
    bank_forks::BankForks,
    blockstore::{Blockstore, CompletedSlotsReceiver, SlotMeta},
    shred::Nonce,
};
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, epoch_schedule::EpochSchedule, pubkey::Pubkey, timing::timestamp};
use std::{
    collections::HashMap,
    iter::Iterator,
    net::SocketAddr,
    net::UdpSocket,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::sleep,
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

pub type DuplicateSlotsResetSender = CrossbeamSender<Slot>;
pub type DuplicateSlotsResetReceiver = CrossbeamReceiver<Slot>;

#[derive(Default)]
pub struct RepairStatsGroup {
    pub count: u64,
    pub min: u64,
    pub max: u64,
}

impl RepairStatsGroup {
    pub fn update(&mut self, slot: u64) {
        self.count += 1;
        self.min = std::cmp::min(self.min, slot);
        self.max = std::cmp::max(self.max, slot);
    }
}

#[derive(Default)]
pub struct RepairStats {
    pub shred: RepairStatsGroup,
    pub highest_shred: RepairStatsGroup,
    pub orphan: RepairStatsGroup,
}

pub const MAX_REPAIR_LENGTH: usize = 512;
pub const MAX_REPAIR_PER_DUPLICATE: usize = 20;
pub const MAX_DUPLICATE_WAIT_MS: usize = 10_000;
pub const REPAIR_MS: u64 = 100;
pub const MAX_ORPHANS: usize = 5;

pub struct RepairInfo {
    pub bank_forks: Arc<RwLock<BankForks>>,
    pub completed_slots_receiver: CompletedSlotsReceiver,
    pub epoch_schedule: EpochSchedule,
    pub duplicate_slots_reset_sender: DuplicateSlotsResetSender,
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

#[derive(Default, Clone)]
pub struct DuplicateSlotRepairStatus {
    start: u64,
    repair_addr: Option<SocketAddr>,
}

pub struct RepairService {
    t_repair: JoinHandle<()>,
}

impl RepairService {
    pub fn new(
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
        repair_socket: Arc<UdpSocket>,
        cluster_info: Arc<ClusterInfo>,
        repair_info: RepairInfo,
        cluster_slots: Arc<ClusterSlots>,
    ) -> Self {
        let t_repair = Builder::new()
            .name("solana-repair-service".to_string())
            .spawn(move || {
                Self::run(
                    &blockstore,
                    &exit,
                    &repair_socket,
                    cluster_info,
                    repair_info,
                    &cluster_slots,
                )
            })
            .unwrap();

        RepairService { t_repair }
    }

    fn run(
        blockstore: &Blockstore,
        exit: &AtomicBool,
        repair_socket: &UdpSocket,
        cluster_info: Arc<ClusterInfo>,
        repair_info: RepairInfo,
        cluster_slots: &ClusterSlots,
    ) {
        let serve_repair = ServeRepair::new(cluster_info.clone());
        let id = cluster_info.id();
        Self::initialize_lowest_slot(id, blockstore, &cluster_info);
        let mut repair_stats = RepairStats::default();
        let mut last_stats = Instant::now();
        let mut duplicate_slot_repair_statuses = HashMap::new();
        Self::initialize_epoch_slots(
            blockstore,
            &cluster_info,
            &repair_info.completed_slots_receiver,
        );
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let repairs = {
                let root_bank = repair_info.bank_forks.read().unwrap().root_bank().clone();
                let new_root = root_bank.slot();
                let lowest_slot = blockstore.lowest_slot();
                Self::update_lowest_slot(&id, lowest_slot, &cluster_info);
                Self::update_completed_slots(&repair_info.completed_slots_receiver, &cluster_info);
                cluster_slots.update(new_root, &cluster_info, &repair_info.bank_forks);
                let new_duplicate_slots = Self::find_new_duplicate_slots(
                    &duplicate_slot_repair_statuses,
                    blockstore,
                    cluster_slots,
                    &root_bank,
                );
                Self::process_new_duplicate_slots(
                    &new_duplicate_slots,
                    &mut duplicate_slot_repair_statuses,
                    cluster_slots,
                    &root_bank,
                    blockstore,
                    &serve_repair,
                    &repair_info.duplicate_slots_reset_sender,
                );
                Self::generate_and_send_duplicate_repairs(
                    &mut duplicate_slot_repair_statuses,
                    cluster_slots,
                    blockstore,
                    &serve_repair,
                    &mut repair_stats,
                    &repair_socket,
                );
                Self::generate_repairs(
                    blockstore,
                    root_bank.slot(),
                    MAX_REPAIR_LENGTH,
                    &duplicate_slot_repair_statuses,
                )
            };

            if let Ok(repairs) = repairs {
                let mut cache = HashMap::new();
                repairs.into_iter().for_each(|repair_request| {
                    if let Ok((to, req)) = serve_repair.repair_request(
                        &cluster_slots,
                        repair_request,
                        &mut cache,
                        &mut repair_stats,
                    ) {
                        repair_socket.send_to(&req, to).unwrap_or_else(|e| {
                            info!("{} repair req send_to({}) error {:?}", id, to, e);
                            0
                        });
                    }
                });
            }

            if last_stats.elapsed().as_secs() > 1 {
                let repair_total = repair_stats.shred.count
                    + repair_stats.highest_shred.count
                    + repair_stats.orphan.count;
                if repair_total > 0 {
                    datapoint_info!(
                        "serve_repair-repair",
                        ("repair-total", repair_total, i64),
                        ("shred-count", repair_stats.shred.count, i64),
                        ("highest-shred-count", repair_stats.highest_shred.count, i64),
                        ("orphan-count", repair_stats.orphan.count, i64),
                        ("repair-highest-slot", repair_stats.highest_shred.max, i64),
                        ("repair-orphan", repair_stats.orphan.max, i64),
                    );
                }
                repair_stats = RepairStats::default();
                last_stats = Instant::now();
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
        duplicate_slot_repair_statuses: &HashMap<Slot, DuplicateSlotRepairStatus>,
    ) -> Result<Vec<RepairType>> {
        // Slot height and shred indexes for shreds we want to repair
        let mut repairs: Vec<RepairType> = vec![];
        Self::generate_repairs_for_fork(
            blockstore,
            &mut repairs,
            max_repairs,
            root,
            duplicate_slot_repair_statuses,
        );

        // TODO: Incorporate gossip to determine priorities for repair?

        // Try to resolve orphans in blockstore
        let orphans = blockstore.orphans_iterator(root + 1).unwrap();
        Self::generate_repairs_for_orphans(orphans, &mut repairs);

        Ok(repairs)
    }

    fn generate_duplicate_repairs_for_slot(
        blockstore: &Blockstore,
        slot: Slot,
    ) -> Option<Vec<RepairType>> {
        if let Some(slot_meta) = blockstore.meta(slot).unwrap() {
            if slot_meta.is_full() {
                // If the slot is full, no further need to repair this slot
                None
            } else {
                Some(Self::generate_repairs_for_slot(
                    blockstore,
                    slot,
                    &slot_meta,
                    MAX_REPAIR_PER_DUPLICATE,
                ))
            }
        } else {
            error!("Slot meta for duplicate slot does not exist, cannot generate repairs");
            // Filter out this slot from the set of duplicates to be repaired as
            // the SlotMeta has to exist for duplicates to be generated
            None
        }
    }

    fn generate_and_send_duplicate_repairs(
        duplicate_slot_repair_statuses: &mut HashMap<Slot, DuplicateSlotRepairStatus>,
        cluster_slots: &ClusterSlots,
        blockstore: &Blockstore,
        serve_repair: &ServeRepair,
        repair_stats: &mut RepairStats,
        repair_socket: &UdpSocket,
    ) {
        duplicate_slot_repair_statuses.retain(|slot, status| {
            Self::update_duplicate_slot_repair_addr(*slot, status, cluster_slots, serve_repair);
            if let Some(repair_addr) = status.repair_addr {
                let repairs = Self::generate_duplicate_repairs_for_slot(&blockstore, *slot);

                if let Some(repairs) = repairs {
                    for repair_type in repairs {
                        if let Err(e) = Self::serialize_and_send_request(
                            &repair_type,
                            repair_socket,
                            &repair_addr,
                            serve_repair,
                            repair_stats,
                            DEFAULT_NONCE,
                        ) {
                            info!("repair req send_to({}) error {:?}", repair_addr, e);
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                true
            }
        })
    }

    fn serialize_and_send_request(
        repair_type: &RepairType,
        repair_socket: &UdpSocket,
        to: &SocketAddr,
        serve_repair: &ServeRepair,
        repair_stats: &mut RepairStats,
        nonce: Nonce,
    ) -> Result<()> {
        let req = serve_repair.map_repair_request(&repair_type, repair_stats, nonce)?;
        repair_socket.send_to(&req, to)?;
        Ok(())
    }

    fn update_duplicate_slot_repair_addr(
        slot: Slot,
        status: &mut DuplicateSlotRepairStatus,
        cluster_slots: &ClusterSlots,
        serve_repair: &ServeRepair,
    ) {
        let now = timestamp();
        if status.repair_addr.is_none()
            || now.saturating_sub(status.start) >= MAX_DUPLICATE_WAIT_MS as u64
        {
            let repair_addr =
                serve_repair.repair_request_duplicate_compute_best_peer(slot, cluster_slots);
            status.repair_addr = repair_addr.ok();
            status.start = timestamp();
        }
    }

    fn process_new_duplicate_slots(
        new_duplicate_slots: &[Slot],
        duplicate_slot_repair_statuses: &mut HashMap<Slot, DuplicateSlotRepairStatus>,
        cluster_slots: &ClusterSlots,
        root_bank: &Bank,
        blockstore: &Blockstore,
        serve_repair: &ServeRepair,
        duplicate_slots_reset_sender: &DuplicateSlotsResetSender,
    ) {
        for slot in new_duplicate_slots {
            warn!(
                "Cluster completed slot: {}, dumping our current version and repairing",
                slot
            );
            // Clear the slot signatures from status cache for this slot
            root_bank.clear_slot_signatures(*slot);

            // Clear the accounts for this slot
            root_bank.remove_unrooted_slot(*slot);

            // Clear the slot-related data in blockstore. This will:
            // 1) Clear old shreds allowing new ones to be inserted
            // 2) Clear the "dead" flag allowing ReplayStage to start replaying
            // this slot
            blockstore.clear_unconfirmed_slot(*slot);

            // Signal ReplayStage to clear its progress map so that a different
            // version of this slot can be replayed
            let _ = duplicate_slots_reset_sender.send(*slot);

            // Mark this slot as special repair, try to download from single
            // validator to avoid corruption
            let repair_addr = serve_repair
                .repair_request_duplicate_compute_best_peer(*slot, cluster_slots)
                .ok();
            let new_duplicate_slot_repair_status = DuplicateSlotRepairStatus {
                start: timestamp(),
                repair_addr,
            };
            duplicate_slot_repair_statuses.insert(*slot, new_duplicate_slot_repair_status);
        }
    }

    fn find_new_duplicate_slots(
        duplicate_slot_repair_statuses: &HashMap<Slot, DuplicateSlotRepairStatus>,
        blockstore: &Blockstore,
        cluster_slots: &ClusterSlots,
        root_bank: &Bank,
    ) -> Vec<Slot> {
        let dead_slots_iter = blockstore
            .dead_slots_iterator(root_bank.slot() + 1)
            .expect("Couldn't get dead slots iterator from blockstore");
        dead_slots_iter
            .filter_map(|dead_slot| {
                if let Some(status) = duplicate_slot_repair_statuses.get(&dead_slot) {
                    // Newly repaired version of this slot has been marked dead again,
                    // time to purge again
                    warn!(
                        "Repaired version of slot {} most recently (but maybe not entirely)
                        from {:?} has failed again",
                        dead_slot, status.repair_addr
                    );
                }
                cluster_slots
                    .lookup(dead_slot)
                    .and_then(|completed_dead_slot_pubkeys| {
                        let epoch = root_bank.get_epoch_and_slot_index(dead_slot).0;
                        if let Some(epoch_stakes) = root_bank.epoch_stakes(epoch) {
                            let total_stake = epoch_stakes.total_stake();
                            let node_id_to_vote_accounts = epoch_stakes.node_id_to_vote_accounts();
                            let total_completed_slot_stake: u64 = completed_dead_slot_pubkeys
                                .read()
                                .unwrap()
                                .iter()
                                .map(|(node_key, _)| {
                                    node_id_to_vote_accounts
                                        .get(node_key)
                                        .map(|v| v.total_stake)
                                        .unwrap_or(0)
                                })
                                .sum();
                            if total_completed_slot_stake as f64 / total_stake as f64
                                > VOTE_THRESHOLD_SIZE
                            {
                                Some(dead_slot)
                            } else {
                                None
                            }
                        } else {
                            error!(
                                "Dead slot {} is too far ahead of root bank {}",
                                dead_slot,
                                root_bank.slot()
                            );
                            None
                        }
                    })
            })
            .collect()
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
        slot: Slot,
        duplicate_slot_repair_statuses: &HashMap<Slot, DuplicateSlotRepairStatus>,
    ) {
        let mut pending_slots = vec![slot];
        while repairs.len() < max_repairs && !pending_slots.is_empty() {
            let slot = pending_slots.pop().unwrap();
            if duplicate_slot_repair_statuses.contains_key(&slot) {
                // These are repaired through a different path
                continue;
            }
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

    fn initialize_lowest_slot(id: Pubkey, blockstore: &Blockstore, cluster_info: &ClusterInfo) {
        // Safe to set into gossip because by this time, the leader schedule cache should
        // also be updated with the latest root (done in blockstore_processor) and thus
        // will provide a schedule to window_service for any incoming shreds up to the
        // last_confirmed_epoch.
        cluster_info.push_lowest_slot(id, blockstore.lowest_slot());
    }

    fn update_completed_slots(
        completed_slots_receiver: &CompletedSlotsReceiver,
        cluster_info: &ClusterInfo,
    ) {
        let mut slots: Vec<Slot> = vec![];
        while let Ok(mut more) = completed_slots_receiver.try_recv() {
            slots.append(&mut more);
        }
        slots.sort();
        if !slots.is_empty() {
            cluster_info.push_epoch_slots(&slots);
        }
    }

    fn update_lowest_slot(id: &Pubkey, lowest_slot: Slot, cluster_info: &ClusterInfo) {
        cluster_info.push_lowest_slot(*id, lowest_slot);
    }

    fn initialize_epoch_slots(
        blockstore: &Blockstore,
        cluster_info: &ClusterInfo,
        completed_slots_receiver: &CompletedSlotsReceiver,
    ) {
        let root = blockstore.last_root();
        let mut slots: Vec<_> = blockstore
            .live_slots_iterator(root)
            .filter_map(|(slot, slot_meta)| {
                if slot_meta.is_full() {
                    Some(slot)
                } else {
                    None
                }
            })
            .collect();

        while let Ok(mut more) = completed_slots_receiver.try_recv() {
            slots.append(&mut more);
        }
        slots.sort();
        slots.dedup();
        if !slots.is_empty() {
            cluster_info.push_epoch_slots(&slots);
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
    use crossbeam_channel::unbounded;
    use solana_ledger::blockstore::{
        make_chaining_slot_entries, make_many_slot_entries, make_slot_entries,
    };
    use solana_ledger::shred::max_ticks_per_n_shreds;
    use solana_ledger::{blockstore::Blockstore, get_tmp_ledger_path};
    use solana_runtime::genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs};
    use solana_sdk::signature::Signer;
    use solana_vote_program::vote_transaction;

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
                RepairService::generate_repairs(&blockstore, 0, 2, &HashMap::new()).unwrap(),
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
                RepairService::generate_repairs(&blockstore, 0, 2, &HashMap::new()).unwrap(),
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
                RepairService::generate_repairs(&blockstore, 0, std::usize::MAX, &HashMap::new())
                    .unwrap(),
                expected
            );

            assert_eq!(
                RepairService::generate_repairs(
                    &blockstore,
                    0,
                    expected.len() - 2,
                    &HashMap::new()
                )
                .unwrap()[..],
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
                RepairService::generate_repairs(&blockstore, 0, std::usize::MAX, &HashMap::new())
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
            let num_entries_per_slot = max_ticks_per_n_shreds(1, None) + 1;

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
                            &repair_slot_range,
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
                    &repair_slot_range,
                )
                .unwrap(),
                expected
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_update_lowest_slot() {
        let node_info = Node::new_localhost_with_pubkey(&Pubkey::default());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(node_info.info);
        RepairService::update_lowest_slot(&Pubkey::default(), 5, &cluster_info);
        let lowest = cluster_info
            .get_lowest_slot_for_node(&Pubkey::default(), None, |lowest_slot, _| {
                lowest_slot.clone()
            })
            .unwrap();
        assert_eq!(lowest.lowest, 5);
    }

    #[test]
    pub fn test_generate_duplicate_repairs_for_slot() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let dead_slot = 9;

        // SlotMeta doesn't exist, should make no repairs
        assert!(
            RepairService::generate_duplicate_repairs_for_slot(&blockstore, dead_slot,).is_none()
        );

        // Insert some shreds to create a SlotMeta, should make repairs
        let num_entries_per_slot = max_ticks_per_n_shreds(1, None) + 1;
        let (mut shreds, _) = make_slot_entries(dead_slot, dead_slot - 1, num_entries_per_slot);
        blockstore
            .insert_shreds(shreds[..shreds.len() - 1].to_vec(), None, false)
            .unwrap();
        assert!(
            RepairService::generate_duplicate_repairs_for_slot(&blockstore, dead_slot,).is_some()
        );

        // SlotMeta is full, should make no repairs
        blockstore
            .insert_shreds(vec![shreds.pop().unwrap()], None, false)
            .unwrap();
        assert!(
            RepairService::generate_duplicate_repairs_for_slot(&blockstore, dead_slot,).is_none()
        );
    }

    #[test]
    pub fn test_generate_and_send_duplicate_repairs() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let cluster_slots = ClusterSlots::default();
        let serve_repair = ServeRepair::new_with_invalid_keypair(Node::new_localhost().info);
        let mut duplicate_slot_repair_statuses = HashMap::new();
        let dead_slot = 9;
        let receive_socket = &UdpSocket::bind("0.0.0.0:0").unwrap();
        let duplicate_status = DuplicateSlotRepairStatus {
            start: std::u64::MAX,
            repair_addr: None,
        };

        // Insert some shreds to create a SlotMeta,
        let num_entries_per_slot = max_ticks_per_n_shreds(1, None) + 1;
        let (mut shreds, _) = make_slot_entries(dead_slot, dead_slot - 1, num_entries_per_slot);
        blockstore
            .insert_shreds(shreds[..shreds.len() - 1].to_vec(), None, false)
            .unwrap();

        duplicate_slot_repair_statuses.insert(dead_slot, duplicate_status);

        // There is no repair_addr, so should not get filtered because the timeout
        // `std::u64::MAX` has not expired
        RepairService::generate_and_send_duplicate_repairs(
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &blockstore,
            &serve_repair,
            &mut RepairStats::default(),
            &UdpSocket::bind("0.0.0.0:0").unwrap(),
        );
        assert!(duplicate_slot_repair_statuses
            .get(&dead_slot)
            .unwrap()
            .repair_addr
            .is_none());
        assert!(duplicate_slot_repair_statuses.get(&dead_slot).is_some());

        // Give the slot a repair address
        duplicate_slot_repair_statuses
            .get_mut(&dead_slot)
            .unwrap()
            .repair_addr = Some(receive_socket.local_addr().unwrap());

        // Slot is not yet full, should not get filtered from `duplicate_slot_repair_statuses`
        RepairService::generate_and_send_duplicate_repairs(
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &blockstore,
            &serve_repair,
            &mut RepairStats::default(),
            &UdpSocket::bind("0.0.0.0:0").unwrap(),
        );
        assert_eq!(duplicate_slot_repair_statuses.len(), 1);
        assert!(duplicate_slot_repair_statuses.get(&dead_slot).is_some());

        // Insert rest of shreds. Slot is full, should get filtered from
        // `duplicate_slot_repair_statuses`
        blockstore
            .insert_shreds(vec![shreds.pop().unwrap()], None, false)
            .unwrap();
        RepairService::generate_and_send_duplicate_repairs(
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &blockstore,
            &serve_repair,
            &mut RepairStats::default(),
            &UdpSocket::bind("0.0.0.0:0").unwrap(),
        );
        assert!(duplicate_slot_repair_statuses.is_empty());
    }

    #[test]
    pub fn test_update_duplicate_slot_repair_addr() {
        let dummy_addr = Some(UdpSocket::bind("0.0.0.0:0").unwrap().local_addr().unwrap());
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(
            Node::new_localhost().info,
        ));
        let serve_repair = ServeRepair::new(cluster_info.clone());
        let valid_repair_peer = Node::new_localhost().info;

        // Signal that this peer has completed the dead slot, and is thus
        // a valid target for repair
        let dead_slot = 9;
        let cluster_slots = ClusterSlots::default();
        cluster_slots.insert_node_id(dead_slot, Arc::new(valid_repair_peer.id));
        cluster_info.insert_info(valid_repair_peer);

        // Not enough time has passed, should not update the
        // address
        let mut duplicate_status = DuplicateSlotRepairStatus {
            start: std::u64::MAX,
            repair_addr: dummy_addr,
        };
        RepairService::update_duplicate_slot_repair_addr(
            dead_slot,
            &mut duplicate_status,
            &cluster_slots,
            &serve_repair,
        );
        assert_eq!(duplicate_status.repair_addr, dummy_addr);

        // If the repair address is None, should try to update
        let mut duplicate_status = DuplicateSlotRepairStatus {
            start: std::u64::MAX,
            repair_addr: None,
        };
        RepairService::update_duplicate_slot_repair_addr(
            dead_slot,
            &mut duplicate_status,
            &cluster_slots,
            &serve_repair,
        );
        assert!(duplicate_status.repair_addr.is_some());

        // If sufficient time has passssed, should try to update
        let mut duplicate_status = DuplicateSlotRepairStatus {
            start: timestamp() - MAX_DUPLICATE_WAIT_MS as u64,
            repair_addr: dummy_addr,
        };
        RepairService::update_duplicate_slot_repair_addr(
            dead_slot,
            &mut duplicate_status,
            &cluster_slots,
            &serve_repair,
        );
        assert_ne!(duplicate_status.repair_addr, dummy_addr);
    }

    #[test]
    pub fn test_process_new_duplicate_slots() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let cluster_slots = ClusterSlots::default();
        let serve_repair = ServeRepair::new_with_invalid_keypair(Node::new_localhost().info);
        let mut duplicate_slot_repair_statuses = HashMap::new();
        let duplicate_slot = 9;

        // Fill blockstore for dead slot
        blockstore.set_dead_slot(duplicate_slot).unwrap();
        assert!(blockstore.is_dead(duplicate_slot));
        let (shreds, _) = make_slot_entries(duplicate_slot, 0, 1);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        let keypairs = ValidatorVoteKeypairs::new_rand();
        let (reset_sender, reset_receiver) = unbounded();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = genesis_utils::create_genesis_config_with_vote_accounts(
            1_000_000_000,
            &[&keypairs],
            10000,
        );
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let bank9 = Bank::new_from_parent(&bank0, &Pubkey::default(), duplicate_slot);
        let old_balance = bank9.get_balance(&keypairs.node_keypair.pubkey());
        bank9
            .transfer(10_000, &mint_keypair, &keypairs.node_keypair.pubkey())
            .unwrap();
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            bank0.hash(),
            bank0.last_blockhash(),
            &keypairs.node_keypair,
            &keypairs.vote_keypair,
            &keypairs.vote_keypair,
        );
        bank9.process_transaction(&vote_tx).unwrap();
        assert!(bank9.get_signature_status(&vote_tx.signatures[0]).is_some());

        RepairService::process_new_duplicate_slots(
            &[duplicate_slot],
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &bank9,
            &blockstore,
            &serve_repair,
            &reset_sender,
        );

        // Blockstore should have been cleared
        assert!(!blockstore.is_dead(duplicate_slot));

        // Should not be able to find signature for slot 9 for the tx
        assert!(bank9.get_signature_status(&vote_tx.signatures[0]).is_none());

        // Getting balance should return the old balance (acounts were cleared)
        assert_eq!(
            bank9.get_balance(&keypairs.node_keypair.pubkey()),
            old_balance
        );

        // Should add the duplicate slot to the tracker
        assert!(duplicate_slot_repair_statuses
            .get(&duplicate_slot)
            .is_some());

        // A signal should be sent to clear ReplayStage
        assert!(reset_receiver.try_recv().is_ok());
    }

    #[test]
    pub fn test_find_new_duplicate_slots() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let cluster_slots = ClusterSlots::default();
        let duplicate_slot_repair_statuses = HashMap::new();
        let keypairs = ValidatorVoteKeypairs::new_rand();
        let only_node_id = Arc::new(keypairs.node_keypair.pubkey());
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                1_000_000_000,
                &[keypairs],
                100,
            );
        let bank0 = Bank::new(&genesis_config);

        // Empty blockstore should have no duplicates
        assert!(RepairService::find_new_duplicate_slots(
            &duplicate_slot_repair_statuses,
            &blockstore,
            &cluster_slots,
            &bank0,
        )
        .is_empty());

        // Insert a dead slot, but is not confirmed by network so should not
        // be marked as duplicate
        let dead_slot = 9;
        blockstore.set_dead_slot(dead_slot).unwrap();
        assert!(RepairService::find_new_duplicate_slots(
            &duplicate_slot_repair_statuses,
            &blockstore,
            &cluster_slots,
            &bank0,
        )
        .is_empty());

        // If supermajority confirms the slot, then dead slot should be
        // marked as a duplicate that needs to be repaired
        cluster_slots.insert_node_id(dead_slot, only_node_id);
        assert_eq!(
            RepairService::find_new_duplicate_slots(
                &duplicate_slot_repair_statuses,
                &blockstore,
                &cluster_slots,
                &bank0,
            ),
            vec![dead_slot]
        );
    }
}
