use crate::{
    duplicate_repair_status::DeadSlotAncestorRequestStatus,
    outstanding_requests::OutstandingRequests,
    repair_response::{self},
    repair_service::{DuplicateSlotsResetSender, RepairInfo, RepairStatsGroup},
    result::{Error, Result},
    serve_repair::AncestorHashesRepairType,
};
use crossbeam_channel::{Receiver, Sender};
use dashmap::{mapref::entry::Entry::Occupied, DashMap};
use solana_ledger::{blockstore::Blockstore, shred::SIZE_OF_NONCE};
use solana_measure::measure::Measure;
use solana_perf::{packet::limited_deserialize, recycler::Recycler};
use solana_sdk::{
    clock::{Slot, SLOT_MS},
    timing::timestamp,
};
use solana_streamer::{
    packet::Packets,
    streamer::{self, PacketReceiver},
};
use std::{
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        {Arc, RwLock},
    },
    thread::{self, sleep, Builder, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq)]
pub enum AncestorHashesReplayUpdate {
    Dead(Slot),
    DeadDuplicateConfirmed(Slot),
}

impl AncestorHashesReplayUpdate {
    fn slot(&self) -> Slot {
        match self {
            AncestorHashesReplayUpdate::Dead(slot) => *slot,
            AncestorHashesReplayUpdate::DeadDuplicateConfirmed(slot) => *slot,
        }
    }
}

pub const MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND: usize = 2;

pub type AncestorHashesReplayUpdateSender = Sender<AncestorHashesReplayUpdate>;
pub type AncestorHashesReplayUpdateReceiver = Receiver<AncestorHashesReplayUpdate>;
type OutstandingAncestorHashesRepairs = OutstandingRequests<AncestorHashesRepairType>;

#[derive(Default)]
pub struct AncestorHashesResponsesStats {
    pub total_packets: usize,
    pub dropped_packets: usize,
    pub invalid_packets: usize,
    pub processed: usize,
}

impl AncestorHashesResponsesStats {
    fn report(&mut self) {
        inc_new_counter_info!(
            "ancestor_hashes_responses-total_packets",
            self.total_packets
        );
        inc_new_counter_info!("ancestor_hashes_responses-processed", self.processed);
        inc_new_counter_info!(
            "ancestor_hashes_responses-dropped_packets",
            self.dropped_packets
        );
        inc_new_counter_info!(
            "ancestor_hashes_responses-invalid_packets",
            self.invalid_packets
        );
        *self = AncestorHashesResponsesStats::default();
    }
}

pub struct AncestorRepairRequestsStats {
    pub ancestor_requests: RepairStatsGroup,
    last_report: Instant,
}

impl Default for AncestorRepairRequestsStats {
    fn default() -> Self {
        AncestorRepairRequestsStats {
            ancestor_requests: RepairStatsGroup::default(),
            last_report: Instant::now(),
        }
    }
}

impl AncestorRepairRequestsStats {
    fn report(&mut self) {
        let slot_to_count: Vec<_> = self
            .ancestor_requests
            .slot_pubkeys
            .iter()
            .map(|(slot, slot_repairs)| {
                (
                    slot,
                    slot_repairs
                        .pubkey_repairs()
                        .iter()
                        .map(|(_key, count)| count)
                        .sum::<u64>(),
                )
            })
            .collect();

        let repair_total = self.ancestor_requests.count;
        if self.last_report.elapsed().as_secs() > 2 && repair_total > 0 {
            info!("ancestor_repair_requests_stats: {:?}", slot_to_count);
            datapoint_info!(
                "ancestor-repair",
                ("ancestor-repair-count", self.ancestor_requests.count, i64)
            );

            *self = AncestorRepairRequestsStats::default();
        }
    }
}

pub struct AncestorHashesService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl AncestorHashesService {
    pub fn new(
        exit: Arc<AtomicBool>,
        blockstore: Arc<Blockstore>,
        ancestor_hashes_request_socket: Arc<UdpSocket>,
        repair_info: RepairInfo,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
    ) -> Self {
        let outstanding_requests: Arc<RwLock<OutstandingAncestorHashesRepairs>> =
            Arc::new(RwLock::new(OutstandingAncestorHashesRepairs::default()));
        let (response_sender, response_receiver) = channel();
        let t_receiver = streamer::receiver(
            ancestor_hashes_request_socket.clone(),
            &exit,
            response_sender,
            Recycler::default(),
            "ancestor_hashes_response_receiver",
            1,
            false,
        );

        // Listen for responses to our ancestor requests
        let ancestor_hashes_request_statuses: Arc<DashMap<Slot, DeadSlotAncestorRequestStatus>> =
            Arc::new(DashMap::new());
        let t_ancestor_hashes_responses = Self::run_responses_listener(
            ancestor_hashes_request_statuses.clone(),
            response_receiver,
            blockstore,
            outstanding_requests.clone(),
            exit.clone(),
            repair_info.duplicate_slots_reset_sender.clone(),
        );

        // Generate ancestor requests for dead slots that are repairable
        let t_ancestor_requests = Self::run_manage_ancestor_requests(
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            repair_info,
            outstanding_requests,
            exit,
            ancestor_hashes_replay_update_receiver,
        );
        let thread_hdls = vec![t_receiver, t_ancestor_hashes_responses, t_ancestor_requests];
        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }

    /// Listen for responses to our ancestors hashes repair requests
    fn run_responses_listener(
        ancestor_hashes_request_statuses: Arc<DashMap<Slot, DeadSlotAncestorRequestStatus>>,
        response_receiver: PacketReceiver,
        blockstore: Arc<Blockstore>,
        outstanding_requests: Arc<RwLock<OutstandingAncestorHashesRepairs>>,
        exit: Arc<AtomicBool>,
        duplicate_slots_reset_sender: DuplicateSlotsResetSender,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solana-ancestor-hashes-responses-service".to_string())
            .spawn(move || {
                let mut last_stats_report = Instant::now();
                let mut stats = AncestorHashesResponsesStats::default();
                let mut max_packets = 1024;
                loop {
                    let result = Self::process_new_responses(
                        &ancestor_hashes_request_statuses,
                        &response_receiver,
                        &blockstore,
                        &outstanding_requests,
                        &mut stats,
                        &mut max_packets,
                        &duplicate_slots_reset_sender,
                    );
                    match result {
                        Err(Error::RecvTimeout(_)) | Ok(_) => {}
                        Err(err) => info!("ancestors hashes reponses listener error: {:?}", err),
                    };
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    if last_stats_report.elapsed().as_secs() > 2 {
                        stats.report();
                        last_stats_report = Instant::now();
                    }
                }
            })
            .unwrap()
    }

    /// Process messages from the network
    fn process_new_responses(
        ancestor_hashes_request_statuses: &DashMap<Slot, DeadSlotAncestorRequestStatus>,
        response_receiver: &PacketReceiver,
        blockstore: &Blockstore,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        stats: &mut AncestorHashesResponsesStats,
        max_packets: &mut usize,
        duplicate_slots_reset_sender: &DuplicateSlotsResetSender,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let mut responses = vec![response_receiver.recv_timeout(timeout)?];
        let mut total_packets = responses[0].packets.len();

        let mut dropped_packets = 0;
        while let Ok(more) = response_receiver.try_recv() {
            total_packets += more.packets.len();
            if total_packets < *max_packets {
                // Drop the rest in the channel in case of DOS
                responses.push(more);
            } else {
                dropped_packets += more.packets.len();
            }
        }

        stats.dropped_packets += dropped_packets;
        stats.total_packets += total_packets;

        let mut time = Measure::start("ancestor_hashes::handle_packets");
        for response in responses {
            Self::handle_packets(
                ancestor_hashes_request_statuses,
                response,
                stats,
                outstanding_requests,
                blockstore,
                duplicate_slots_reset_sender,
            );
        }
        time.stop();
        if total_packets >= *max_packets {
            if time.as_ms() > 1000 {
                *max_packets = (*max_packets * 9) / 10;
            } else {
                *max_packets = (*max_packets * 10) / 9;
            }
        }
        Ok(())
    }

    fn handle_packets(
        ancestor_hashes_request_statuses: &DashMap<Slot, DeadSlotAncestorRequestStatus>,
        packets: Packets,
        stats: &mut AncestorHashesResponsesStats,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        blockstore: &Blockstore,
        duplicate_slots_reset_sender: &DuplicateSlotsResetSender,
    ) {
        // iter over the packets
        packets.packets.iter().for_each(|packet| {
            let from_addr = packet.meta.addr();
            if let Ok(ancestor_hashes_response) =
                limited_deserialize(&packet.data[..packet.meta.size - SIZE_OF_NONCE])
            {
                // Verify the response
                let request_slot = repair_response::nonce(&packet.data[..packet.meta.size])
                    .and_then(|nonce| {
                        outstanding_requests.write().unwrap().register_response(
                            nonce,
                            &ancestor_hashes_response,
                            timestamp(),
                            // If the response is valid, return the slot the request
                            // was for
                            |ancestor_hashes_request| ancestor_hashes_request.0,
                        )
                    });

                if request_slot.is_none() {
                    stats.invalid_packets += 1;
                    return;
                }

                // If was a valid response, there must be a valid `request_slot`
                let request_slot = request_slot.unwrap();
                stats.processed += 1;

                // Check if we can make any decisions.
                if let Occupied(mut ancestor_hashes_status_ref) =
                    ancestor_hashes_request_statuses.entry(request_slot)
                {
                    if let Some(decision) = ancestor_hashes_status_ref.get_mut().add_response(
                        &from_addr,
                        ancestor_hashes_response.into_slot_hashes(),
                        blockstore,
                    ) {
                        let potential_slots_to_dump = {
                            // TODO: In the case of DuplicateAncestorDecision::ContinueSearch
                            // This means all the ancestors were mismatched, which
                            // means the earliest mismatched ancestor has yet to be found.
                            //
                            // In the best case scenario, this means after ReplayStage dumps
                            // the earliest known ancestor `A` here, and then repairs `A`,
                            // because we may still have the incorrect version of some ancestor
                            // of `A`, we will mark `A` as dead and then continue the search
                            // protocol through another round of ancestor repairs.
                            //
                            // However this process is a bit slow, so in an ideal world, the
                            // protocol could be extended to keep searching by making
                            // another ancestor repair request from the earliest returned
                            // ancestor from this search.
                            decision
                                .repair_status()
                                .map(|status| status.correct_ancestors_to_repair.clone())
                        };

                        // Once a request is completed, remove it from the map so that new
                        // requests for the same slot can be made again if necessary.
                        ancestor_hashes_status_ref.remove();

                        // Now signal replay the new updated slots. It's important to do this
                        // AFTER we've removed the ancestor_hashes_status_ref in case replay
                        // then sends us another dead slot signal based on the updates we are
                        // about to send.
                        if let Some(potential_slots_to_dump) = potential_slots_to_dump {
                            // Signal ReplayStage to dump the fork that is descended from
                            // `earliest_mismatched_slot_to_dump`.
                            if !potential_slots_to_dump.is_empty() {
                                let _ = duplicate_slots_reset_sender.send(potential_slots_to_dump);
                            }
                        }
                    }
                }
            }
        });
    }

    fn process_replay_updates(
        ancestor_hashes_replay_update_receiver: &AncestorHashesReplayUpdateReceiver,
        ancestor_hashes_request_statuses: &DashMap<Slot, DeadSlotAncestorRequestStatus>,
        dead_slot_pool: &mut HashSet<Slot>,
        repairable_dead_slot_pool: &mut HashSet<Slot>,
    ) {
        for update in ancestor_hashes_replay_update_receiver.try_iter() {
            let slot = update.slot();
            if ancestor_hashes_request_statuses.contains_key(&slot) {
                return;
            }
            match update {
                AncestorHashesReplayUpdate::Dead(dead_slot) => {
                    if repairable_dead_slot_pool.contains(&dead_slot) {
                        return;
                    } else {
                        dead_slot_pool.insert(dead_slot);
                    }
                }
                AncestorHashesReplayUpdate::DeadDuplicateConfirmed(dead_slot) => {
                    dead_slot_pool.remove(&dead_slot);
                    repairable_dead_slot_pool.insert(dead_slot);
                }
            }
        }
    }

    fn run_manage_ancestor_requests(
        ancestor_hashes_request_statuses: Arc<DashMap<Slot, DeadSlotAncestorRequestStatus>>,
        ancestor_hashes_request_socket: Arc<UdpSocket>,
        repair_info: RepairInfo,
        outstanding_requests: Arc<RwLock<OutstandingAncestorHashesRepairs>>,
        exit: Arc<AtomicBool>,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
    ) -> JoinHandle<()> {
        let mut repair_stats = AncestorRepairRequestsStats::default();

        let mut dead_slot_pool = HashSet::new();
        let mut repairable_dead_slot_pool = HashSet::new();

        // Sliding window that limits the number of slots repaired via AncestorRepair
        // to MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND/second
        let mut request_throttle = vec![];
        Builder::new()
            .name("solana-manage-ancestor-requests".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    return;
                }

                Self::manage_ancestor_requests(
                    &ancestor_hashes_request_statuses,
                    &ancestor_hashes_request_socket,
                    &repair_info,
                    &outstanding_requests,
                    &ancestor_hashes_replay_update_receiver,
                    &serve_repair,
                    &mut repair_stats,
                    &mut dead_slot_pool,
                    &mut repairable_dead_slot_pool,
                    &mut request_throttle,
                );

                sleep(Duration::from_millis(SLOT_MS));
            })
            .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn manage_ancestor_requests(
        ancestor_hashes_request_statuses: &DashMap<Slot, DeadSlotAncestorRequestStatus>,
        ancestor_hashes_request_socket: &UdpSocket,
        repair_info: &RepairInfo,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        ancestor_hashes_replay_update_receiver: &AncestorHashesReplayUpdateReceiver,
        serve_repair: &ServeRepair,
        repair_stats: &mut AncestorRepairRequestsStats,
        dead_slot_pool: &mut HashSet<Slot>,
        repairable_dead_slot_pool: &mut HashSet<Slot>,
        request_throttle: &mut Vec<u64>,
    ) {
        Self::process_replay_updates(
            ancestor_hashes_replay_update_receiver,
            ancestor_hashes_request_statuses,
            dead_slot_pool,
            repairable_dead_slot_pool,
        );
        let root_bank = repair_info.bank_forks.read().unwrap().root_bank();

        Self::find_epoch_slots_frozen_dead_slots(
            &repair_info.cluster_slots,
            dead_slot_pool,
            repairable_dead_slot_pool,
            &root_bank,
        );

        ancestor_hashes_request_statuses.retain(|slot, status| {
            if status.is_expired() {
                // Add the slot back to the repairable pool to retry
                repairable_dead_slot_pool.insert(*slot);
                false
            } else {
                true
            }
        });

        // Keep around the last second of requests in the throttler.
        request_throttle.retain(|request_time| *request_time > (timestamp() - 1000));

        let number_of_allowed_requests =
            MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND.saturating_sub(request_throttle.len());

        // Find dead slots for which it's worthwhile to ask the network for their
        // ancestors
        for _ in 0..number_of_allowed_requests {
            let slot = repairable_dead_slot_pool.iter().next().cloned().unwrap();
            repairable_dead_slot_pool.take(&slot).unwrap();
            warn!(
                "Cluster froze slot: {}, but we marked it as dead.
                Initiating protocol to sample cluster for dead slot ancestors.",
                slot
            );

            Self::initiate_ancestor_hashes_requests_for_duplicate_slot(
                ancestor_hashes_request_statuses,
                ancestor_hashes_request_socket,
                &repair_info.cluster_slots,
                serve_repair,
                &repair_info.repair_validators,
                slot,
                repair_stats,
                &outstanding_requests,
            );

            request_throttle.push(timestamp());
        }

        repair_stats.report();
    }

    /// Find if any dead slots in `dead_slot_pool` have been frozen by sufficient
    /// number of nodes in the cluster to justify adding to the `repairable_dead_slot_pool`.
    fn find_epoch_slots_frozen_dead_slots(
        cluster_slots: &ClusterSlots,
        dead_slot_pool: &mut HashSet<Slot>,
        repairable_dead_slot_pool: &mut HashSet<Slot>,
        root_bank: &Bank,
    ) {
        dead_slot_pool.retain(|dead_slot| {
            let epoch = root_bank.get_epoch_and_slot_index(*dead_slot).0;
            if let Some(epoch_stakes) = root_bank.epoch_stakes(epoch) {
                let status = cluster_slots.lookup(*dead_slot);
                if let Some(completed_dead_slot_pubkeys) = status {
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
                    if total_completed_slot_stake as f64 / total_stake as f64 > DUPLICATE_THRESHOLD
                    {
                        repairable_dead_slot_pool.insert(*dead_slot);
                        false
                    } else {
                        true
                    }
                } else {
                    true
                }
            } else {
                warn!(
                    "Dead slot {} is too far ahead of root bank {}",
                    dead_slot,
                    root_bank.slot()
                );
                false
            }
        })
    }

    fn initiate_ancestor_hashes_requests_for_duplicate_slot(
        ancestor_hashes_request_statuses: &DashMap<Slot, DeadSlotAncestorRequestStatus>,
        ancestor_hashes_request_socket: &UdpSocket,
        cluster_slots: &ClusterSlots,
        serve_repair: &ServeRepair,
        repair_validators: &Option<HashSet<Pubkey>>,
        duplicate_slot: Slot,
        repair_stats: &mut AncestorRepairRequestsStats,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
    ) {
        let sampled_validators = serve_repair.repair_request_ancestor_hashes_sample_peers(
            duplicate_slot,
            cluster_slots,
            repair_validators,
        );

        if let Ok(sampled_validators) = sampled_validators {
            for (pubkey, socket_addr) in sampled_validators.iter() {
                repair_stats
                    .ancestor_requests
                    .update(&pubkey, duplicate_slot, 0);
                let nonce = outstanding_requests
                    .write()
                    .unwrap()
                    .add_request(AncestorHashesRepairType(duplicate_slot), timestamp());
                let request_bytes =
                    serve_repair.ancestor_repair_request_bytes(duplicate_slot, nonce);
                if let Ok(request_bytes) = request_bytes {
                    let _ = ancestor_hashes_request_socket.send_to(&request_bytes, socket_addr);
                }
            }

            let ancestor_request_status = DeadSlotAncestorRequestStatus::new(
                sampled_validators
                    .into_iter()
                    .map(|(_pk, socket_addr)| socket_addr),
                duplicate_slot,
            );
            assert!(!ancestor_hashes_request_statuses.contains_key(&duplicate_slot));
            ancestor_hashes_request_statuses.insert(duplicate_slot, ancestor_request_status);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::vote_simulator::VoteSimulator;
    use crossbeam_channel::unbounded;

    #[test]
    pub fn test_ancestor_hashes_service_process_replay_updates() {
        let (ancestor_hashes_replay_update_sender, ancestor_hashes_replay_update_receiver) =
            unbounded();
        let ancestor_hashes_request_statuses = DashMap::new();
        let mut dead_slot_pool = HashSet::new();
        let mut repairable_dead_slot_pool = HashSet::new();
        let slot = 10;

        // Getting a dead signal should only add the slot to the dead pool
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
        );
        assert!(dead_slot_pool.contains(&slot));
        assert!(!repairable_dead_slot_pool.contains(&slot));

        // Getting a duplicate confirmed dead slot should move the slot
        // from the dead pool to the repairable pool
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
        );
        assert!(!dead_slot_pool.contains(&slot));
        assert!(repairable_dead_slot_pool.contains(&slot));

        // Getting another dead signal should not add it back to the dead pool
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
        );
        assert!(!dead_slot_pool.contains(&slot));
        assert!(repairable_dead_slot_pool.contains(&slot));

        // If an outstanding request for a slot already exists, should
        // ignore any signals from replay stage
        ancestor_hashes_request_statuses.insert(slot, DeadSlotAncestorRequestStatus::default());
        dead_slot_pool.clear();
        repairable_dead_slot_pool.clear();
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(slot))
            .unwrap();
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
        );
        assert!(!dead_slot_pool.contains(&slot));
        assert!(!repairable_dead_slot_pool.contains(&slot));
    }

    #[test]
    fn test_ancestor_hashes_service_find_epoch_slots_frozen_dead_slots() {
        let vote_simulator = VoteSimulator::new(3);
        let cluster_slots = ClusterSlots::default();
        let mut dead_slot_pool = HashSet::new();
        let mut repairable_dead_slot_pool = HashSet::new();
        let root_bank = vote_simulator.bank_forks.read().unwrap().root_bank();
        let dead_slot = 10;
        dead_slot_pool.insert(dead_slot);

        // ClusterSlots doesn't have an entry for this slot yet, shouldn't move the slot
        // from the dead slot pool.
        AncestorHashesService::find_epoch_slots_frozen_dead_slots(
            &cluster_slots,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &root_bank,
        );
        assert_eq!(dead_slot_pool.len(), 1);
        assert!(dead_slot_pool.contains(&dead_slot));
        assert!(repairable_dead_slot_pool.is_empty());

        let max_epoch = root_bank.epoch_stakes_map().keys().max().unwrap();
        let slot_outside_known_epochs = root_bank
            .epoch_schedule()
            .get_last_slot_in_epoch(*max_epoch)
            + 1;
        dead_slot_pool.insert(slot_outside_known_epochs);

        // Should remove `slot_outside_known_epochs`
        AncestorHashesService::find_epoch_slots_frozen_dead_slots(
            &cluster_slots,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &root_bank,
        );
        assert_eq!(dead_slot_pool.len(), 1);
        assert!(dead_slot_pool.contains(&dead_slot));
        assert!(repairable_dead_slot_pool.is_empty());

        // Slot hasn't reached the threshold
        for (i, key) in (0..2).zip(vote_simulator.node_pubkeys.iter()) {
            cluster_slots.insert_node_id(dead_slot, *key);
            AncestorHashesService::find_epoch_slots_frozen_dead_slots(
                &cluster_slots,
                &mut dead_slot_pool,
                &mut repairable_dead_slot_pool,
                &root_bank,
            );
            if i == 0 {
                assert_eq!(dead_slot_pool.len(), 1);
                assert!(dead_slot_pool.contains(&dead_slot));
                assert!(repairable_dead_slot_pool.is_empty());
            } else {
                assert!(dead_slot_pool.is_empty());
                assert_eq!(repairable_dead_slot_pool.len(), 1);
                assert!(repairable_dead_slot_pool.contains(&dead_slot));
            }
        }
    }
}
