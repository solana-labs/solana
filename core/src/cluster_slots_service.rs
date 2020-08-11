use crate::{cluster_info::ClusterInfo, cluster_slots::ClusterSlots};
use solana_ledger::blockstore::{Blockstore, CompletedSlotsReceiver};
use solana_measure::measure::Measure;
use solana_runtime::bank_forks::BankForks;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        {Arc, RwLock},
    },
    thread::sleep,
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Default, Debug)]
struct ClusterSlotsServiceTiming {
    pub lowest_slot_elapsed: u64,
    pub update_completed_slots_elapsed: u64,
}

impl ClusterSlotsServiceTiming {
    fn update(&mut self, lowest_slot_elapsed: u64, update_completed_slots_elapsed: u64) {
        self.lowest_slot_elapsed += lowest_slot_elapsed;
        self.update_completed_slots_elapsed += update_completed_slots_elapsed;
    }
}

pub struct ClusterSlotsService {
    t_cluster_slots_service: JoinHandle<()>,
}

impl ClusterSlotsService {
    pub fn new(
        blockstore: Arc<Blockstore>,
        cluster_slots: Arc<ClusterSlots>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
        completed_slots_receiver: CompletedSlotsReceiver,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let id = cluster_info.id();
        Self::initialize_lowest_slot(id, &blockstore, &cluster_info);
        Self::initialize_epoch_slots(&blockstore, &cluster_info, &completed_slots_receiver);
        let t_cluster_slots_service = Builder::new()
            .name("solana-cluster-slots-service".to_string())
            .spawn(move || {
                Self::run(
                    blockstore,
                    cluster_slots,
                    bank_forks,
                    cluster_info,
                    completed_slots_receiver,
                    exit,
                )
            })
            .unwrap();

        ClusterSlotsService {
            t_cluster_slots_service,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_cluster_slots_service.join()
    }

    fn run(
        blockstore: Arc<Blockstore>,
        cluster_slots: Arc<ClusterSlots>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
        completed_slots_receiver: CompletedSlotsReceiver,
        exit: Arc<AtomicBool>,
    ) {
        let mut cluster_slots_service_timing = ClusterSlotsServiceTiming::default();
        let mut last_stats = Instant::now();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let new_root = bank_forks.read().unwrap().root();
            let id = cluster_info.id();
            let mut lowest_slot_elapsed = Measure::start("lowest_slot_elapsed");
            let lowest_slot = blockstore.lowest_slot();
            Self::update_lowest_slot(&id, lowest_slot, &cluster_info);
            lowest_slot_elapsed.stop();
            let mut update_completed_slots_elapsed =
                Measure::start("update_completed_slots_elapsed");
            Self::update_completed_slots(&completed_slots_receiver, &cluster_info);
            cluster_slots.update(new_root, &cluster_info, &bank_forks);
            update_completed_slots_elapsed.stop();

            cluster_slots_service_timing.update(
                lowest_slot_elapsed.as_us(),
                update_completed_slots_elapsed.as_us(),
            );

            if last_stats.elapsed().as_secs() > 2 {
                datapoint_info!(
                    "cluster_slots_service-timing",
                    (
                        "lowest_slot_elapsed",
                        cluster_slots_service_timing.lowest_slot_elapsed,
                        i64
                    ),
                    (
                        "update_completed_slots_elapsed",
                        cluster_slots_service_timing.update_completed_slots_elapsed,
                        i64
                    ),
                );
                cluster_slots_service_timing = ClusterSlotsServiceTiming::default();
                last_stats = Instant::now();
            }
            sleep(Duration::from_millis(200));
        }
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

    fn initialize_lowest_slot(id: Pubkey, blockstore: &Blockstore, cluster_info: &ClusterInfo) {
        // Safe to set into gossip because by this time, the leader schedule cache should
        // also be updated with the latest root (done in blockstore_processor) and thus
        // will provide a schedule to window_service for any incoming shreds up to the
        // last_confirmed_epoch.
        cluster_info.push_lowest_slot(id, blockstore.lowest_slot());
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
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cluster_info::Node;

    #[test]
    pub fn test_update_lowest_slot() {
        let node_info = Node::new_localhost_with_pubkey(&Pubkey::default());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(node_info.info);
        ClusterSlotsService::update_lowest_slot(&Pubkey::default(), 5, &cluster_info);
        let lowest = cluster_info
            .get_lowest_slot_for_node(&Pubkey::default(), None, |lowest_slot, _| {
                lowest_slot.clone()
            })
            .unwrap();
        assert_eq!(lowest.lowest, 5);
    }
}
