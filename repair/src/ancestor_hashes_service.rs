use {
    crate::{
        duplicate_repair_status::{AncestorRequestStatus, DuplicateAncestorDecision},
        outstanding_requests::OutstandingRequests,
        packet_threshold::DynamicPacketToProcessThreshold,
        repair_service::{DuplicateSlotsResetSender, RepairInfo, RepairStatsGroup},
        serve_repair::{
            AncestorHashesRepairType, AncestorHashesResponse, RepairProtocol, ServeRepair,
        },
    },
    bincode::serialize,
    crossbeam_channel::{unbounded, Receiver, Sender},
    dashmap::{mapref::entry::Entry::Occupied, DashMap},
    solana_cluster_slots::cluster_slots::ClusterSlots,
    solana_consensus::consensus::DUPLICATE_THRESHOLD,
    solana_gossip::{cluster_info::ClusterInfo, ping_pong::Pong},
    solana_ledger::blockstore::Blockstore,
    solana_perf::{
        packet::{deserialize_from_with_limit, Packet, PacketBatch},
        recycler::Recycler,
    },
    solana_result::result::{Error, Result},
    solana_runtime::bank::Bank,
    solana_sdk::{
        clock::{Slot, DEFAULT_MS_PER_SLOT},
        pubkey::Pubkey,
        signature::Signable,
        signer::keypair::Keypair,
        timing::timestamp,
    },
    solana_streamer::streamer::{self, PacketBatchReceiver, StreamerReceiveStats},
    std::{
        collections::HashSet,
        io::{Cursor, Read},
        net::UdpSocket,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

#[derive(Debug, PartialEq, Eq)]
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

pub type RetryableSlotsSender = Sender<Slot>;
pub type RetryableSlotsReceiver = Receiver<Slot>;
pub type OutstandingAncestorHashesRepairs = OutstandingRequests<AncestorHashesRepairType>;

#[derive(Default)]
pub struct AncestorHashesResponsesStats {
    total_packets: usize,
    processed: usize,
    dropped_packets: usize,
    invalid_packets: usize,
    ping_count: usize,
    ping_err_verify_count: usize,
}

impl AncestorHashesResponsesStats {
    fn report(&mut self) {
        datapoint_info!(
            "ancestor_hashes_responses",
            ("total_packets", self.total_packets, i64),
            ("processed", self.processed, i64),
            ("dropped_packets", self.dropped_packets, i64),
            ("invalid_packets", self.invalid_packets, i64),
            ("ping_count", self.ping_count, i64),
            ("ping_err_verify_count", self.ping_err_verify_count, i64),
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
        let (response_sender, response_receiver) = unbounded();
        let t_receiver = streamer::receiver(
            ancestor_hashes_request_socket.clone(),
            exit.clone(),
            response_sender,
            Recycler::default(),
            Arc::new(StreamerReceiveStats::new(
                "ancestor_hashes_response_receiver",
            )),
            Duration::from_millis(1), // coalesce
            false,
            None,
        );

        let ancestor_hashes_request_statuses: Arc<DashMap<Slot, AncestorRequestStatus>> =
            Arc::new(DashMap::new());
        let (retryable_slots_sender, retryable_slots_receiver) = unbounded();

        // Listen for responses to our ancestor requests
        let t_ancestor_hashes_responses = Self::run_responses_listener(
            ancestor_hashes_request_statuses.clone(),
            response_receiver,
            blockstore,
            outstanding_requests.clone(),
            exit.clone(),
            repair_info.duplicate_slots_reset_sender.clone(),
            retryable_slots_sender,
            repair_info.cluster_info.clone(),
            ancestor_hashes_request_socket.clone(),
        );

        // Generate ancestor requests for dead slots that are repairable
        let t_ancestor_requests = Self::run_manage_ancestor_requests(
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            repair_info,
            outstanding_requests,
            exit,
            ancestor_hashes_replay_update_receiver,
            retryable_slots_receiver,
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
        ancestor_hashes_request_statuses: Arc<DashMap<Slot, AncestorRequestStatus>>,
        response_receiver: PacketBatchReceiver,
        blockstore: Arc<Blockstore>,
        outstanding_requests: Arc<RwLock<OutstandingAncestorHashesRepairs>>,
        exit: Arc<AtomicBool>,
        duplicate_slots_reset_sender: DuplicateSlotsResetSender,
        retryable_slots_sender: RetryableSlotsSender,
        cluster_info: Arc<ClusterInfo>,
        ancestor_socket: Arc<UdpSocket>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solAncHashesSvc".to_string())
            .spawn(move || {
                let mut last_stats_report = Instant::now();
                let mut stats = AncestorHashesResponsesStats::default();
                let mut packet_threshold = DynamicPacketToProcessThreshold::default();
                loop {
                    let keypair = cluster_info.keypair().clone();
                    let result = Self::process_new_packets_from_channel(
                        &ancestor_hashes_request_statuses,
                        &response_receiver,
                        &blockstore,
                        &outstanding_requests,
                        &mut stats,
                        &mut packet_threshold,
                        &duplicate_slots_reset_sender,
                        &retryable_slots_sender,
                        &keypair,
                        &ancestor_socket,
                    );
                    match result {
                        Err(Error::RecvTimeout(_)) | Ok(_) => {}
                        Err(err) => info!("ancestors hashes responses listener error: {:?}", err),
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
    #[allow(clippy::too_many_arguments)]
    fn process_new_packets_from_channel(
        ancestor_hashes_request_statuses: &DashMap<Slot, AncestorRequestStatus>,
        response_receiver: &PacketBatchReceiver,
        blockstore: &Blockstore,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        stats: &mut AncestorHashesResponsesStats,
        packet_threshold: &mut DynamicPacketToProcessThreshold,
        duplicate_slots_reset_sender: &DuplicateSlotsResetSender,
        retryable_slots_sender: &RetryableSlotsSender,
        keypair: &Keypair,
        ancestor_socket: &UdpSocket,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let mut packet_batches = vec![response_receiver.recv_timeout(timeout)?];
        let mut total_packets = packet_batches[0].len();

        let mut dropped_packets = 0;
        while let Ok(batch) = response_receiver.try_recv() {
            total_packets += batch.len();
            if packet_threshold.should_drop(total_packets) {
                dropped_packets += batch.len();
            } else {
                packet_batches.push(batch);
            }
        }

        stats.dropped_packets += dropped_packets;
        stats.total_packets += total_packets;

        let timer = Instant::now();
        for packet_batch in packet_batches {
            Self::process_packet_batch(
                ancestor_hashes_request_statuses,
                packet_batch,
                stats,
                outstanding_requests,
                blockstore,
                duplicate_slots_reset_sender,
                retryable_slots_sender,
                keypair,
                ancestor_socket,
            );
        }
        packet_threshold.update(total_packets, timer.elapsed());
        Ok(())
    }

    fn process_packet_batch(
        ancestor_hashes_request_statuses: &DashMap<Slot, AncestorRequestStatus>,
        packet_batch: PacketBatch,
        stats: &mut AncestorHashesResponsesStats,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        blockstore: &Blockstore,
        duplicate_slots_reset_sender: &DuplicateSlotsResetSender,
        retryable_slots_sender: &RetryableSlotsSender,
        keypair: &Keypair,
        ancestor_socket: &UdpSocket,
    ) {
        packet_batch.iter().for_each(|packet| {
            let decision = Self::verify_and_process_ancestor_response(
                packet,
                ancestor_hashes_request_statuses,
                stats,
                outstanding_requests,
                blockstore,
                keypair,
                ancestor_socket,
            );
            if let Some((slot, decision)) = decision {
                Self::handle_ancestor_request_decision(
                    slot,
                    decision,
                    duplicate_slots_reset_sender,
                    retryable_slots_sender,
                );
            }
        });
    }

    /// Returns `Some((request_slot, decision))`, where `decision` is an actionable
    /// result after processing sufficient responses for the subject of the query,
    /// `request_slot`
    // pub for tests
    pub fn verify_and_process_ancestor_response(
        packet: &Packet,
        ancestor_hashes_request_statuses: &DashMap<Slot, AncestorRequestStatus>,
        stats: &mut AncestorHashesResponsesStats,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        blockstore: &Blockstore,
        keypair: &Keypair,
        ancestor_socket: &UdpSocket,
    ) -> Option<(Slot, DuplicateAncestorDecision)> {
        let from_addr = packet.meta().socket_addr();
        let packet_data = match packet.data(..) {
            Some(data) => data,
            None => {
                stats.invalid_packets += 1;
                return None;
            }
        };
        let mut cursor = Cursor::new(packet_data);
        let response = match deserialize_from_with_limit(&mut cursor) {
            Ok(response) => response,
            Err(_) => {
                stats.invalid_packets += 1;
                return None;
            }
        };

        match response {
            AncestorHashesResponse::Hashes(ref hashes) => {
                // deserialize trailing nonce
                let nonce = match deserialize_from_with_limit(&mut cursor) {
                    Ok(nonce) => nonce,
                    Err(_) => {
                        stats.invalid_packets += 1;
                        return None;
                    }
                };

                // verify that packet does not contain extraneous data
                if cursor.bytes().next().is_some() {
                    stats.invalid_packets += 1;
                    return None;
                }

                let request_slot = outstanding_requests.write().unwrap().register_response(
                    nonce,
                    &response,
                    timestamp(),
                    // If the response is valid, return the slot the request
                    // was for
                    |ancestor_hashes_request| ancestor_hashes_request.0,
                );

                if request_slot.is_none() {
                    stats.invalid_packets += 1;
                    return None;
                }

                // If was a valid response, there must be a valid `request_slot`
                let request_slot = request_slot.unwrap();
                stats.processed += 1;

                if let Occupied(mut ancestor_hashes_status_ref) =
                    ancestor_hashes_request_statuses.entry(request_slot)
                {
                    let decision = ancestor_hashes_status_ref.get_mut().add_response(
                        &from_addr,
                        hashes.clone(),
                        blockstore,
                    );
                    if decision.is_some() {
                        // Once a request is completed, remove it from the map so that new
                        // requests for the same slot can be made again if necessary. It's
                        // important to hold the `write` lock here via
                        // `ancestor_hashes_status_ref` so that we don't race with deletion +
                        // insertion from the `t_ancestor_requests` thread, which may
                        // 1) Remove expired statuses from `ancestor_hashes_request_statuses`
                        // 2) Insert another new one via `manage_ancestor_requests()`.
                        // In which case we wouldn't want to delete the newly inserted entry here.
                        ancestor_hashes_status_ref.remove();
                    }
                    decision.map(|decision| (request_slot, decision))
                } else {
                    None
                }
            }
            AncestorHashesResponse::Ping(ping) => {
                // verify that packet does not contain extraneous data
                if cursor.bytes().next().is_some() {
                    stats.invalid_packets += 1;
                    return None;
                }
                if !ping.verify() {
                    stats.ping_err_verify_count += 1;
                    return None;
                }
                stats.ping_count += 1;
                if let Ok(pong) = Pong::new(&ping, keypair) {
                    let pong = RepairProtocol::Pong(pong);
                    if let Ok(pong_bytes) = serialize(&pong) {
                        let _ignore = ancestor_socket.send_to(&pong_bytes[..], from_addr);
                    }
                }
                None
            }
        }
    }

    // pub for tests
    pub fn handle_ancestor_request_decision(
        slot: Slot,
        decision: DuplicateAncestorDecision,
        duplicate_slots_reset_sender: &DuplicateSlotsResetSender,
        retryable_slots_sender: &RetryableSlotsSender,
    ) {
        if decision.is_retryable() {
            let _ = retryable_slots_sender.send(slot);
        }
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

        // Now signal ReplayStage about the new updated slots. It's important to do this
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

    // pub for tests
    pub fn process_replay_updates(
        ancestor_hashes_replay_update_receiver: &AncestorHashesReplayUpdateReceiver,
        ancestor_hashes_request_statuses: &DashMap<Slot, AncestorRequestStatus>,
        dead_slot_pool: &mut HashSet<Slot>,
        repairable_dead_slot_pool: &mut HashSet<Slot>,
        root_slot: Slot,
    ) {
        for update in ancestor_hashes_replay_update_receiver.try_iter() {
            let slot = update.slot();
            if slot <= root_slot || ancestor_hashes_request_statuses.contains_key(&slot) {
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
        ancestor_hashes_request_statuses: Arc<DashMap<Slot, AncestorRequestStatus>>,
        ancestor_hashes_request_socket: Arc<UdpSocket>,
        repair_info: RepairInfo,
        outstanding_requests: Arc<RwLock<OutstandingAncestorHashesRepairs>>,
        exit: Arc<AtomicBool>,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
        retryable_slots_receiver: RetryableSlotsReceiver,
    ) -> JoinHandle<()> {
        let serve_repair = ServeRepair::new(
            repair_info.cluster_info.clone(),
            repair_info.bank_forks.clone(),
            repair_info.repair_whitelist.clone(),
        );
        let mut repair_stats = AncestorRepairRequestsStats::default();

        let mut dead_slot_pool = HashSet::new();
        let mut repairable_dead_slot_pool = HashSet::new();

        // Sliding window that limits the number of slots repaired via AncestorRepair
        // to MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND/second
        let mut request_throttle = vec![];
        Builder::new()
            .name("solManAncReqs".to_string())
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
                    &retryable_slots_receiver,
                    &serve_repair,
                    &mut repair_stats,
                    &mut dead_slot_pool,
                    &mut repairable_dead_slot_pool,
                    &mut request_throttle,
                );

                sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT));
            })
            .unwrap()
    }

    // pub for tests
    #[allow(clippy::too_many_arguments)]
    pub fn manage_ancestor_requests(
        ancestor_hashes_request_statuses: &DashMap<Slot, AncestorRequestStatus>,
        ancestor_hashes_request_socket: &UdpSocket,
        repair_info: &RepairInfo,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        ancestor_hashes_replay_update_receiver: &AncestorHashesReplayUpdateReceiver,
        retryable_slots_receiver: &RetryableSlotsReceiver,
        serve_repair: &ServeRepair,
        repair_stats: &mut AncestorRepairRequestsStats,
        dead_slot_pool: &mut HashSet<Slot>,
        repairable_dead_slot_pool: &mut HashSet<Slot>,
        request_throttle: &mut Vec<u64>,
    ) {
        let root_bank = repair_info.bank_forks.read().unwrap().root_bank();
        for slot in retryable_slots_receiver.try_iter() {
            datapoint_info!("ancestor-repair-retry", ("slot", slot, i64));
            repairable_dead_slot_pool.insert(slot);
        }

        Self::process_replay_updates(
            ancestor_hashes_replay_update_receiver,
            ancestor_hashes_request_statuses,
            dead_slot_pool,
            repairable_dead_slot_pool,
            root_bank.slot(),
        );

        Self::find_epoch_slots_frozen_dead_slots(
            &repair_info.cluster_slots,
            dead_slot_pool,
            repairable_dead_slot_pool,
            &root_bank,
        );

        dead_slot_pool.retain(|slot| *slot > root_bank.slot());
        repairable_dead_slot_pool.retain(|slot| *slot > root_bank.slot());

        ancestor_hashes_request_statuses.retain(|slot, status| {
            if *slot <= root_bank.slot() {
                false
            } else if status.is_expired() {
                // Add the slot back to the repairable pool to retry
                repairable_dead_slot_pool.insert(*slot);
                false
            } else {
                true
            }
        });

        // Keep around the last second of requests in the throttler.
        request_throttle.retain(|request_time| *request_time > (timestamp() - 1000));

        let identity_keypair: &Keypair = &repair_info.cluster_info.keypair().clone();

        let number_of_allowed_requests =
            MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND.saturating_sub(request_throttle.len());

        // Find dead slots for which it's worthwhile to ask the network for their
        // ancestors
        for _ in 0..number_of_allowed_requests {
            let slot = repairable_dead_slot_pool.iter().next().cloned();
            if let Some(slot) = slot {
                warn!(
                    "Cluster froze slot: {}, but we marked it as dead.
                    Initiating protocol to sample cluster for dead slot ancestors.",
                    slot
                );

                if Self::initiate_ancestor_hashes_requests_for_duplicate_slot(
                    ancestor_hashes_request_statuses,
                    ancestor_hashes_request_socket,
                    &repair_info.cluster_slots,
                    serve_repair,
                    &repair_info.repair_validators,
                    slot,
                    repair_stats,
                    outstanding_requests,
                    identity_keypair,
                ) {
                    request_throttle.push(timestamp());
                    repairable_dead_slot_pool.take(&slot).unwrap();
                }
            } else {
                break;
            }
        }

        repair_stats.report();
    }

    /// Find if any dead slots in `dead_slot_pool` have been frozen by sufficient
    /// number of nodes in the cluster to justify adding to the `repairable_dead_slot_pool`.
    // pub for tests
    pub fn find_epoch_slots_frozen_dead_slots(
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
                    // If sufficient number of validators froze this slot, then there's a chance
                    // this dead slot was duplicate confirmed and will make it into in the main fork.
                    // This means it's worth asking the cluster to get the correct version.
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

    /// Returns true if a request was successfully made and the status
    /// added to `ancestor_hashes_request_statuses`
    // pub for tests
    #[allow(clippy::too_many_arguments)]
    pub fn initiate_ancestor_hashes_requests_for_duplicate_slot(
        ancestor_hashes_request_statuses: &DashMap<Slot, AncestorRequestStatus>,
        ancestor_hashes_request_socket: &UdpSocket,
        cluster_slots: &ClusterSlots,
        serve_repair: &ServeRepair,
        repair_validators: &Option<HashSet<Pubkey>>,
        duplicate_slot: Slot,
        repair_stats: &mut AncestorRepairRequestsStats,
        outstanding_requests: &RwLock<OutstandingAncestorHashesRepairs>,
        identity_keypair: &Keypair,
    ) -> bool {
        let sampled_validators = serve_repair.repair_request_ancestor_hashes_sample_peers(
            duplicate_slot,
            cluster_slots,
            repair_validators,
        );

        if let Ok(sampled_validators) = sampled_validators {
            for (pubkey, socket_addr) in sampled_validators.iter() {
                repair_stats
                    .ancestor_requests
                    .update(pubkey, duplicate_slot, 0);
                let nonce = outstanding_requests
                    .write()
                    .unwrap()
                    .add_request(AncestorHashesRepairType(duplicate_slot), timestamp());
                let request_bytes = serve_repair.ancestor_repair_request_bytes(
                    identity_keypair,
                    pubkey,
                    duplicate_slot,
                    nonce,
                );
                if let Ok(request_bytes) = request_bytes {
                    let _ = ancestor_hashes_request_socket.send_to(&request_bytes, socket_addr);
                }
            }

            let ancestor_request_status = AncestorRequestStatus::new(
                sampled_validators
                    .into_iter()
                    .map(|(_pk, socket_addr)| socket_addr),
                duplicate_slot,
            );
            assert!(!ancestor_hashes_request_statuses.contains_key(&duplicate_slot));
            ancestor_hashes_request_statuses.insert(duplicate_slot, ancestor_request_status);
            true
        } else {
            false
        }
    }
}
