#[cfg(test)]
mod test {
    use {
        crate::{
            replay_stage::{
                tests::{replay_blockstore_components, ReplayBlockstoreComponents},
                ReplayStage,
            },
            vote_simulator::VoteSimulator,
        },
        crossbeam_channel::unbounded,
        dashmap::DashMap,
        solana_cluster_slots::cluster_slots::ClusterSlots,
        solana_gossip::{
            cluster_info::{ClusterInfo, Node},
            contact_info::ContactInfo,
        },
        solana_ledger::{
            blockstore::{make_many_slot_entries, Blockstore},
            get_tmp_ledger_path,
            shred::Nonce,
        },
        solana_perf::{packet::Packet, recycler::Recycler},
        solana_repair::{
            ancestor_hashes_service::{
                AncestorHashesReplayUpdate, AncestorHashesReplayUpdateReceiver,
                AncestorHashesReplayUpdateSender, AncestorHashesResponsesStats,
                AncestorHashesService, AncestorRepairRequestsStats,
                OutstandingAncestorHashesRepairs, RetryableSlotsReceiver, RetryableSlotsSender,
                MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND,
            },
            cluster_slot_state_verifier::{DuplicateSlotsToRepair, PurgeRepairSlotCounter},
            duplicate_repair_status::{AncestorRequestStatus, DuplicateAncestorDecision},
            repair_service::RepairInfo,
            serve_repair::{ServeRepair, MAX_ANCESTOR_RESPONSES},
        },
        solana_runtime::{
            accounts_background_service::AbsRequestSender, bank::Bank, bank_forks::BankForks,
        },
        solana_sdk::{
            clock::Slot,
            hash::Hash,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::{
            socket::SocketAddrSpace,
            streamer::{self, PacketBatchReceiver, StreamerReceiveStats},
        },
        std::{
            collections::{HashMap, HashSet},
            net::UdpSocket,
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, RwLock,
            },
            thread::JoinHandle,
            time::Duration,
        },
        trees::tr,
    };

    #[test]
    pub fn test_ancestor_hashes_service_process_replay_updates() {
        let (ancestor_hashes_replay_update_sender, ancestor_hashes_replay_update_receiver) =
            unbounded();
        let ancestor_hashes_request_statuses = DashMap::new();
        let mut dead_slot_pool = HashSet::new();
        let mut repairable_dead_slot_pool = HashSet::new();
        let slot = 10;
        let mut root_slot = 0;

        // 1) Getting a dead signal should only add the slot to the dead pool
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            root_slot,
        );
        assert!(dead_slot_pool.contains(&slot));
        assert!(repairable_dead_slot_pool.is_empty());

        // 2) Getting a duplicate confirmed dead slot should move the slot
        // from the dead pool to the repairable pool
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            root_slot,
        );
        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.contains(&slot));

        // 3) Getting another dead signal should not add it back to the dead pool
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(slot))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            root_slot,
        );
        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.contains(&slot));

        // 4) If an outstanding request for a slot already exists, should
        // ignore any signals from replay stage
        ancestor_hashes_request_statuses.insert(slot, AncestorRequestStatus::default());
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
            root_slot,
        );
        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.is_empty());

        // 5) If we get any signals for slots <= root_slot, they should be ignored
        root_slot = 15;
        ancestor_hashes_request_statuses.clear();
        dead_slot_pool.clear();
        repairable_dead_slot_pool.clear();
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(root_slot - 1))
            .unwrap();
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(
                root_slot - 2,
            ))
            .unwrap();
        AncestorHashesService::process_replay_updates(
            &ancestor_hashes_replay_update_receiver,
            &ancestor_hashes_request_statuses,
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            root_slot,
        );
        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.is_empty());
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

    struct ResponderThreads {
        t_request_receiver: JoinHandle<()>,
        t_listen: JoinHandle<()>,
        exit: Arc<AtomicBool>,
        responder_info: ContactInfo,
        response_receiver: PacketBatchReceiver,
        correct_bank_hashes: HashMap<Slot, Hash>,
    }

    impl ResponderThreads {
        fn shutdown(self) {
            self.exit.store(true, Ordering::Relaxed);
            self.t_request_receiver.join().unwrap();
            self.t_listen.join().unwrap();
        }

        fn new(slot_to_query: Slot) -> Self {
            assert!(slot_to_query >= MAX_ANCESTOR_RESPONSES as Slot);
            let vote_simulator = VoteSimulator::new(3);
            let keypair = Keypair::new();
            let responder_node = Node::new_localhost_with_pubkey(&keypair.pubkey());
            let cluster_info = ClusterInfo::new(
                responder_node.info.clone(),
                Arc::new(keypair),
                SocketAddrSpace::Unspecified,
            );
            let responder_serve_repair = ServeRepair::new(
                Arc::new(cluster_info),
                vote_simulator.bank_forks,
                Arc::<RwLock<HashSet<_>>>::default(), // repair whitelist
            );

            // Set up thread to give us responses
            let ledger_path = get_tmp_ledger_path!();
            let exit = Arc::new(AtomicBool::new(false));
            let (requests_sender, requests_receiver) = unbounded();
            let (response_sender, response_receiver) = unbounded();

            // Set up blockstore for responses
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            // Create slots [slot, slot + MAX_ANCESTOR_RESPONSES) with 5 shreds apiece
            let (shreds, _) =
                make_many_slot_entries(slot_to_query, MAX_ANCESTOR_RESPONSES as u64, 5);
            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");
            let mut correct_bank_hashes = HashMap::new();
            for duplicate_confirmed_slot in
                slot_to_query - MAX_ANCESTOR_RESPONSES as Slot + 1..=slot_to_query
            {
                let hash = Hash::new_unique();
                correct_bank_hashes.insert(duplicate_confirmed_slot, hash);
                blockstore.insert_bank_hash(duplicate_confirmed_slot, hash, true);
            }

            // Set up response threads
            let t_request_receiver = streamer::receiver(
                Arc::new(responder_node.sockets.serve_repair),
                exit.clone(),
                requests_sender,
                Recycler::default(),
                Arc::new(StreamerReceiveStats::new(
                    "ancestor_hashes_response_receiver",
                )),
                Duration::from_millis(1), // coalesce
                false,
                None,
            );
            let t_listen = responder_serve_repair.listen(
                blockstore,
                requests_receiver,
                response_sender,
                exit.clone(),
            );

            Self {
                t_request_receiver,
                t_listen,
                exit,
                responder_info: responder_node.info,
                response_receiver,
                correct_bank_hashes,
            }
        }
    }

    struct ManageAncestorHashesState {
        ancestor_hashes_request_statuses: Arc<DashMap<Slot, AncestorRequestStatus>>,
        ancestor_hashes_request_socket: Arc<UdpSocket>,
        requester_serve_repair: ServeRepair,
        repair_info: RepairInfo,
        outstanding_requests: Arc<RwLock<OutstandingAncestorHashesRepairs>>,
        dead_slot_pool: HashSet<Slot>,
        repairable_dead_slot_pool: HashSet<Slot>,
        request_throttle: Vec<u64>,
        repair_stats: AncestorRepairRequestsStats,
        retryable_slots_sender: RetryableSlotsSender,
        retryable_slots_receiver: RetryableSlotsReceiver,
        ancestor_hashes_replay_update_sender: AncestorHashesReplayUpdateSender,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
    }

    impl ManageAncestorHashesState {
        fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
            let ancestor_hashes_request_statuses = Arc::new(DashMap::new());
            let ancestor_hashes_request_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());
            let epoch_schedule = *bank_forks.read().unwrap().root_bank().epoch_schedule();
            let keypair = Keypair::new();
            let requester_cluster_info = Arc::new(ClusterInfo::new(
                Node::new_localhost_with_pubkey(&keypair.pubkey()).info,
                Arc::new(keypair),
                SocketAddrSpace::Unspecified,
            ));
            let repair_whitelist = Arc::new(RwLock::new(HashSet::default()));
            let requester_serve_repair = ServeRepair::new(
                requester_cluster_info.clone(),
                bank_forks.clone(),
                repair_whitelist.clone(),
            );
            let (duplicate_slots_reset_sender, _duplicate_slots_reset_receiver) = unbounded();
            let repair_info = RepairInfo {
                bank_forks,
                cluster_info: requester_cluster_info,
                cluster_slots: Arc::new(ClusterSlots::default()),
                epoch_schedule,
                duplicate_slots_reset_sender,
                repair_validators: None,
                repair_whitelist,
            };

            let (ancestor_hashes_replay_update_sender, ancestor_hashes_replay_update_receiver) =
                unbounded();
            let (retryable_slots_sender, retryable_slots_receiver) = unbounded();
            Self {
                ancestor_hashes_request_statuses,
                ancestor_hashes_request_socket,
                requester_serve_repair,
                repair_info,
                outstanding_requests: Arc::new(RwLock::new(
                    OutstandingAncestorHashesRepairs::default(),
                )),
                dead_slot_pool: HashSet::new(),
                repairable_dead_slot_pool: HashSet::new(),
                request_throttle: vec![],
                repair_stats: AncestorRepairRequestsStats::default(),
                ancestor_hashes_replay_update_sender,
                ancestor_hashes_replay_update_receiver,
                retryable_slots_sender,
                retryable_slots_receiver,
            }
        }
    }

    fn setup_dead_slot(
        dead_slot: Slot,
        correct_bank_hashes: &HashMap<Slot, Hash>,
    ) -> ReplayBlockstoreComponents {
        assert!(dead_slot >= MAX_ANCESTOR_RESPONSES as Slot);
        let mut forks = tr(0);

        // Create a bank_forks that includes everything but the dead slot
        for slot in 1..dead_slot {
            forks.push_front(tr(slot));
        }
        let mut replay_blockstore_components = replay_blockstore_components(Some(forks), 1, None);
        let ReplayBlockstoreComponents {
            ref blockstore,
            ref mut vote_simulator,
            ..
        } = replay_blockstore_components;

        // Create dead slot in bank_forks
        let is_frozen = false;
        vote_simulator.fill_bank_forks(
            tr(dead_slot - 1) / tr(dead_slot),
            &HashMap::new(),
            is_frozen,
        );

        // Create slots [slot, slot + num_ancestors) with 5 shreds apiece
        let (shreds, _) = make_many_slot_entries(dead_slot, dead_slot, 5);
        blockstore
            .insert_shreds(shreds, None, false)
            .expect("Expect successful ledger write");
        for duplicate_confirmed_slot in 0..dead_slot {
            let bank_hash = correct_bank_hashes
                .get(&duplicate_confirmed_slot)
                .cloned()
                .unwrap_or_else(Hash::new_unique);
            blockstore.insert_bank_hash(duplicate_confirmed_slot, bank_hash, true);
        }
        blockstore.set_dead_slot(dead_slot).unwrap();
        replay_blockstore_components
    }

    fn send_ancestor_repair_request(
        requester_serve_repair: &ServeRepair,
        requester_cluster_info: &ClusterInfo,
        responder_info: &ContactInfo,
        ancestor_hashes_request_socket: &UdpSocket,
        dead_slot: Slot,
        nonce: Nonce,
    ) {
        let request_bytes = requester_serve_repair.ancestor_repair_request_bytes(
            &requester_cluster_info.keypair(),
            responder_info.pubkey(),
            dead_slot,
            nonce,
        );
        if let Ok(request_bytes) = request_bytes {
            let socket = responder_info.serve_repair().unwrap();
            let _ = ancestor_hashes_request_socket.send_to(&request_bytes, socket);
        }
    }

    #[test]
    fn test_ancestor_hashes_service_initiate_ancestor_hashes_requests_for_duplicate_slot() {
        let dead_slot = MAX_ANCESTOR_RESPONSES as Slot;
        let responder_threads = ResponderThreads::new(dead_slot);

        let ResponderThreads {
            ref responder_info,
            ref response_receiver,
            ref correct_bank_hashes,
            ..
        } = responder_threads;

        let ReplayBlockstoreComponents {
            blockstore: requester_blockstore,
            vote_simulator,
            ..
        } = setup_dead_slot(dead_slot, correct_bank_hashes);

        let ManageAncestorHashesState {
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            repair_info,
            outstanding_requests,
            requester_serve_repair,
            mut repair_stats,
            ..
        } = ManageAncestorHashesState::new(vote_simulator.bank_forks);

        let RepairInfo {
            cluster_info: requester_cluster_info,
            cluster_slots,
            repair_validators,
            ..
        } = repair_info;

        AncestorHashesService::initiate_ancestor_hashes_requests_for_duplicate_slot(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &cluster_slots,
            &requester_serve_repair,
            &repair_validators,
            dead_slot,
            &mut repair_stats,
            &outstanding_requests,
            &requester_cluster_info.keypair(),
        );
        assert!(ancestor_hashes_request_statuses.is_empty());

        // Send a request to generate a ping
        send_ancestor_repair_request(
            &requester_serve_repair,
            &requester_cluster_info,
            responder_info,
            &ancestor_hashes_request_socket,
            dead_slot,
            /*nonce*/ 123,
        );
        // Should have received valid response
        let mut response_packet = response_receiver
            .recv_timeout(Duration::from_millis(10_000))
            .unwrap();
        let packet = &mut response_packet[0];
        packet
            .meta_mut()
            .set_socket_addr(&responder_info.serve_repair().unwrap());
        let decision = AncestorHashesService::verify_and_process_ancestor_response(
            packet,
            &ancestor_hashes_request_statuses,
            &mut AncestorHashesResponsesStats::default(),
            &outstanding_requests,
            &requester_blockstore,
            &requester_cluster_info.keypair(),
            &ancestor_hashes_request_socket,
        );
        // should have processed a ping packet
        assert_eq!(decision, None);

        // Add the responder to the eligible list for requests
        let responder_id = *responder_info.pubkey();
        cluster_slots.insert_node_id(dead_slot, responder_id);
        requester_cluster_info.insert_info(responder_info.clone());
        // Now the request should actually be made
        AncestorHashesService::initiate_ancestor_hashes_requests_for_duplicate_slot(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &cluster_slots,
            &requester_serve_repair,
            &repair_validators,
            dead_slot,
            &mut repair_stats,
            &outstanding_requests,
            &requester_cluster_info.keypair(),
        );

        assert_eq!(ancestor_hashes_request_statuses.len(), 1);
        assert!(ancestor_hashes_request_statuses.contains_key(&dead_slot));

        // Should have received valid response
        let mut response_packet = response_receiver
            .recv_timeout(Duration::from_millis(10_000))
            .unwrap();
        let packet = &mut response_packet[0];
        packet
            .meta_mut()
            .set_socket_addr(&responder_info.serve_repair().unwrap());
        let decision = AncestorHashesService::verify_and_process_ancestor_response(
            packet,
            &ancestor_hashes_request_statuses,
            &mut AncestorHashesResponsesStats::default(),
            &outstanding_requests,
            &requester_blockstore,
            &requester_cluster_info.keypair(),
            &ancestor_hashes_request_socket,
        )
        .unwrap();

        assert_matches!(
            decision,
            (
                _dead_slot,
                DuplicateAncestorDecision::EarliestAncestorNotFrozen(_)
            )
        );
        assert_eq!(
            decision
                .1
                .repair_status()
                .unwrap()
                .correct_ancestors_to_repair,
            vec![(dead_slot, *correct_bank_hashes.get(&dead_slot).unwrap())]
        );

        // Should have removed the ancestor status on successful
        // completion
        assert!(ancestor_hashes_request_statuses.is_empty());
        responder_threads.shutdown();
    }

    #[test]
    fn test_ancestor_hashes_service_manage_ancestor_requests() {
        let vote_simulator = VoteSimulator::new(3);
        let ManageAncestorHashesState {
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            requester_serve_repair,
            repair_info,
            outstanding_requests,
            mut dead_slot_pool,
            mut repairable_dead_slot_pool,
            mut request_throttle,
            ancestor_hashes_replay_update_sender,
            ancestor_hashes_replay_update_receiver,
            retryable_slots_receiver,
            ..
        } = ManageAncestorHashesState::new(vote_simulator.bank_forks);
        let responder_node = Node::new_localhost();
        let RepairInfo {
            ref bank_forks,
            ref cluster_info,
            ..
        } = repair_info;
        cluster_info.insert_info(responder_node.info);
        bank_forks.read().unwrap().root_bank().epoch_schedule();
        // 1) No signals from ReplayStage, no requests should be made
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );

        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.is_empty());
        assert!(ancestor_hashes_request_statuses.is_empty());

        // 2) Simulate signals from ReplayStage, should make a request
        // for `dead_duplicate_confirmed_slot`
        let dead_slot = 10;
        let dead_duplicate_confirmed_slot = 14;
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::Dead(dead_slot))
            .unwrap();
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(
                dead_duplicate_confirmed_slot,
            ))
            .unwrap();
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(
                dead_duplicate_confirmed_slot,
            ))
            .unwrap();
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );

        assert_eq!(dead_slot_pool.len(), 1);
        assert!(dead_slot_pool.contains(&dead_slot));
        assert!(repairable_dead_slot_pool.is_empty());
        assert_eq!(ancestor_hashes_request_statuses.len(), 1);
        assert!(ancestor_hashes_request_statuses.contains_key(&dead_duplicate_confirmed_slot));

        // 3) Simulate an outstanding request timing out
        ancestor_hashes_request_statuses
            .get_mut(&dead_duplicate_confirmed_slot)
            .unwrap()
            .value_mut()
            .make_expired();

        // If the request timed out, we should remove the slot from `ancestor_hashes_request_statuses`,
        // and add it to `repairable_dead_slot_pool`. Because the request_throttle is at its limit,
        // we should not immediately retry the timed request.
        request_throttle.resize(MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND, std::u64::MAX);
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );

        assert_eq!(dead_slot_pool.len(), 1);
        assert!(dead_slot_pool.contains(&dead_slot));
        assert_eq!(repairable_dead_slot_pool.len(), 1);
        assert!(repairable_dead_slot_pool.contains(&dead_duplicate_confirmed_slot));
        assert!(ancestor_hashes_request_statuses.is_empty());

        // 4) If the throttle only has expired timestamps from more than a second ago,
        // then on the next iteration, we should clear the entries in the throttle
        // and retry a request for the timed out request
        request_throttle.clear();
        request_throttle.resize(
            MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND,
            timestamp() - 1001,
        );
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );
        assert_eq!(dead_slot_pool.len(), 1);
        assert!(dead_slot_pool.contains(&dead_slot));
        assert!(repairable_dead_slot_pool.is_empty());
        assert_eq!(ancestor_hashes_request_statuses.len(), 1);
        assert!(ancestor_hashes_request_statuses.contains_key(&dead_duplicate_confirmed_slot));
        // Request throttle includes one item for the request we just made
        assert_eq!(
            request_throttle.len(),
            ancestor_hashes_request_statuses.len()
        );

        // 5) If we've reached the throttle limit, no requests should be made,
        // but should still read off the channel for replay updates
        request_throttle.clear();
        request_throttle.resize(MAX_ANCESTOR_HASHES_SLOT_REQUESTS_PER_SECOND, std::u64::MAX);
        let dead_duplicate_confirmed_slot_2 = 15;
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(
                dead_duplicate_confirmed_slot_2,
            ))
            .unwrap();
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );

        assert_eq!(dead_slot_pool.len(), 1);
        assert!(dead_slot_pool.contains(&dead_slot));
        assert_eq!(repairable_dead_slot_pool.len(), 1);
        assert!(repairable_dead_slot_pool.contains(&dead_duplicate_confirmed_slot_2));
        assert_eq!(ancestor_hashes_request_statuses.len(), 1);
        assert!(ancestor_hashes_request_statuses.contains_key(&dead_duplicate_confirmed_slot));

        // 6) If root moves past slot, should remove it from all state
        let bank_forks = &repair_info.bank_forks;
        let root_bank = bank_forks.read().unwrap().root_bank();
        let new_root_slot = dead_duplicate_confirmed_slot_2 + 1;
        let new_root_bank = Bank::new_from_parent(&root_bank, &Pubkey::default(), new_root_slot);
        new_root_bank.freeze();
        {
            let mut w_bank_forks = bank_forks.write().unwrap();
            w_bank_forks.insert(new_root_bank);
            w_bank_forks.set_root(new_root_slot, &AbsRequestSender::default(), None);
        }
        assert!(!dead_slot_pool.is_empty());
        assert!(!repairable_dead_slot_pool.is_empty());
        assert!(!ancestor_hashes_request_statuses.is_empty());
        request_throttle.clear();
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );
        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.is_empty());
        assert!(ancestor_hashes_request_statuses.is_empty());
    }

    #[test]
    fn test_verify_and_process_ancestor_responses_invalid_packet() {
        let bank0 = Bank::default_for_tests();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank0)));

        let ManageAncestorHashesState {
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            outstanding_requests,
            repair_info,
            ..
        } = ManageAncestorHashesState::new(bank_forks);

        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();

        // Create invalid packet with fewer bytes than the size of the nonce
        let mut packet = Packet::default();
        packet.meta_mut().size = 0;

        assert!(AncestorHashesService::verify_and_process_ancestor_response(
            &packet,
            &ancestor_hashes_request_statuses,
            &mut AncestorHashesResponsesStats::default(),
            &outstanding_requests,
            &blockstore,
            &repair_info.cluster_info.keypair(),
            &ancestor_hashes_request_socket,
        )
        .is_none());
    }

    #[test]
    fn test_ancestor_hashes_service_manage_ancestor_hashes_after_replay_dump() {
        let dead_slot = MAX_ANCESTOR_RESPONSES as Slot;
        let responder_threads = ResponderThreads::new(dead_slot);

        let ResponderThreads {
            ref responder_info,
            ref response_receiver,
            ref correct_bank_hashes,
            ..
        } = responder_threads;

        let ReplayBlockstoreComponents {
            blockstore: requester_blockstore,
            vote_simulator,
            my_pubkey,
            leader_schedule_cache,
            ..
        } = setup_dead_slot(dead_slot, correct_bank_hashes);

        let VoteSimulator {
            bank_forks,
            mut progress,
            ..
        } = vote_simulator;

        let ManageAncestorHashesState {
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            requester_serve_repair,
            repair_info,
            outstanding_requests,
            mut dead_slot_pool,
            mut repairable_dead_slot_pool,
            mut request_throttle,
            ancestor_hashes_replay_update_sender,
            ancestor_hashes_replay_update_receiver,
            retryable_slots_receiver,
            ..
        } = ManageAncestorHashesState::new(bank_forks.clone());

        let RepairInfo {
            cluster_info: ref requester_cluster_info,
            ref cluster_slots,
            ..
        } = repair_info;
        let (dumped_slots_sender, _dumped_slots_receiver) = unbounded();

        // Add the responder to the eligible list for requests
        let responder_id = *responder_info.pubkey();
        cluster_slots.insert_node_id(dead_slot, responder_id);
        requester_cluster_info.insert_info(responder_info.clone());

        // Send a request to generate a ping
        send_ancestor_repair_request(
            &requester_serve_repair,
            requester_cluster_info,
            responder_info,
            &ancestor_hashes_request_socket,
            dead_slot,
            /*nonce*/ 123,
        );
        // Should have received valid response
        let mut response_packet = response_receiver
            .recv_timeout(Duration::from_millis(10_000))
            .unwrap();
        let packet = &mut response_packet[0];
        packet
            .meta_mut()
            .set_socket_addr(&responder_info.serve_repair().unwrap());
        let decision = AncestorHashesService::verify_and_process_ancestor_response(
            packet,
            &ancestor_hashes_request_statuses,
            &mut AncestorHashesResponsesStats::default(),
            &outstanding_requests,
            &requester_blockstore,
            &requester_cluster_info.keypair(),
            &ancestor_hashes_request_socket,
        );
        // Should have processed a ping packet
        assert_eq!(decision, None);

        // Simulate getting duplicate confirmed dead slot
        ancestor_hashes_replay_update_sender
            .send(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(
                dead_slot,
            ))
            .unwrap();

        // Simulate Replay dumping this slot
        let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
        duplicate_slots_to_repair.insert(dead_slot, Hash::new_unique());
        ReplayStage::dump_then_repair_correct_slots(
            &mut duplicate_slots_to_repair,
            &mut bank_forks.read().unwrap().ancestors(),
            &mut bank_forks.read().unwrap().descendants(),
            &mut progress,
            &bank_forks,
            &requester_blockstore,
            None,
            &mut PurgeRepairSlotCounter::default(),
            &dumped_slots_sender,
            &my_pubkey,
            &leader_schedule_cache,
        );

        // Simulate making a request
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );

        assert_eq!(ancestor_hashes_request_statuses.len(), 1);
        assert!(ancestor_hashes_request_statuses.contains_key(&dead_slot));

        // Should have received valid response
        let mut response_packet = response_receiver
            .recv_timeout(Duration::from_millis(10_000))
            .unwrap();
        let packet = &mut response_packet[0];
        packet
            .meta_mut()
            .set_socket_addr(&responder_info.serve_repair().unwrap());
        let decision = AncestorHashesService::verify_and_process_ancestor_response(
            packet,
            &ancestor_hashes_request_statuses,
            &mut AncestorHashesResponsesStats::default(),
            &outstanding_requests,
            &requester_blockstore,
            &requester_cluster_info.keypair(),
            &ancestor_hashes_request_socket,
        )
        .unwrap();

        assert_matches!(
            decision,
            (
                _dead_slot,
                DuplicateAncestorDecision::EarliestAncestorNotFrozen(_)
            )
        );
        assert_eq!(
            decision
                .1
                .repair_status()
                .unwrap()
                .correct_ancestors_to_repair,
            vec![(dead_slot, *correct_bank_hashes.get(&dead_slot).unwrap())]
        );

        // Should have removed the ancestor status on successful
        // completion
        assert!(ancestor_hashes_request_statuses.is_empty());
        responder_threads.shutdown();
    }

    #[test]
    fn test_ancestor_hashes_service_retryable_duplicate_ancestor_decision() {
        let vote_simulator = VoteSimulator::new(1);
        let ManageAncestorHashesState {
            ancestor_hashes_request_statuses,
            ancestor_hashes_request_socket,
            requester_serve_repair,
            repair_info,
            outstanding_requests,
            mut dead_slot_pool,
            mut repairable_dead_slot_pool,
            mut request_throttle,
            ancestor_hashes_replay_update_receiver,
            retryable_slots_receiver,
            retryable_slots_sender,
            ..
        } = ManageAncestorHashesState::new(vote_simulator.bank_forks);

        let decision = DuplicateAncestorDecision::SampleNotDuplicateConfirmed;
        assert!(decision.is_retryable());

        // Simulate network response processing thread reaching a retryable
        // decision
        let request_slot = 10;
        AncestorHashesService::handle_ancestor_request_decision(
            request_slot,
            decision,
            &repair_info.duplicate_slots_reset_sender,
            &retryable_slots_sender,
        );

        // Simulate ancestor request thread getting the retry signal
        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.is_empty());
        AncestorHashesService::manage_ancestor_requests(
            &ancestor_hashes_request_statuses,
            &ancestor_hashes_request_socket,
            &repair_info,
            &outstanding_requests,
            &ancestor_hashes_replay_update_receiver,
            &retryable_slots_receiver,
            &requester_serve_repair,
            &mut AncestorRepairRequestsStats::default(),
            &mut dead_slot_pool,
            &mut repairable_dead_slot_pool,
            &mut request_throttle,
        );

        assert!(dead_slot_pool.is_empty());
        assert!(repairable_dead_slot_pool.contains(&request_slot));
    }
}
