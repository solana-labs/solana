//! The `tvu` module implements the Transaction Validation Unit, a multi-stage transaction
//! validation pipeline in software.

use {
    crate::{
        broadcast_stage::RetransmitSlotsSender,
        cache_block_meta_service::CacheBlockMetaSender,
        cluster_info_vote_listener::{
            GossipDuplicateConfirmedSlotsReceiver, GossipVerifiedVoteHashReceiver,
            VerifiedVoteReceiver, VoteTracker,
        },
        cluster_slots::ClusterSlots,
        cluster_slots_service::ClusterSlotsService,
        completed_data_sets_service::CompletedDataSetsSender,
        cost_update_service::CostUpdateService,
        drop_bank_service::DropBankService,
        ledger_cleanup_service::LedgerCleanupService,
        repair_service::RepairInfo,
        replay_stage::{ReplayStage, ReplayStageConfig},
        retransmit_stage::RetransmitStage,
        rewards_recorder_service::RewardsRecorderSender,
        shred_fetch_stage::ShredFetchStage,
        sigverify_shreds,
        tower_storage::TowerStorage,
        validator::ProcessBlockStore,
        voting_service::VotingService,
        warm_quic_cache_service::WarmQuicCacheService,
        window_service::WindowService,
    },
    crossbeam_channel::{unbounded, Receiver},
    solana_client::connection_cache::ConnectionCache,
    solana_geyser_plugin_manager::block_metadata_notifier_interface::BlockMetadataNotifierLock,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{
        blockstore::Blockstore, blockstore_processor::TransactionStatusSender,
        leader_schedule_cache::LeaderScheduleCache,
    },
    solana_poh::poh_recorder::PohRecorder,
    solana_rpc::{
        max_slots::MaxSlots, optimistically_confirmed_bank_tracker::BankNotificationSender,
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_runtime::{
        accounts_background_service::AbsRequestSender, bank_forks::BankForks,
        commitment::BlockCommitmentCache, prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Keypair},
    std::{
        collections::HashSet,
        net::UdpSocket,
        sync::{atomic::AtomicBool, Arc, RwLock},
        thread::{self, JoinHandle},
    },
};

pub struct Tvu {
    fetch_stage: ShredFetchStage,
    shred_sigverify: JoinHandle<()>,
    retransmit_stage: RetransmitStage,
    window_service: WindowService,
    cluster_slots_service: ClusterSlotsService,
    replay_stage: ReplayStage,
    ledger_cleanup_service: Option<LedgerCleanupService>,
    cost_update_service: CostUpdateService,
    voting_service: VotingService,
    warm_quic_cache_service: Option<WarmQuicCacheService>,
    drop_bank_service: DropBankService,
}

pub struct TvuSockets {
    pub fetch: Vec<UdpSocket>,
    pub repair: UdpSocket,
    pub retransmit: Vec<UdpSocket>,
    pub forwards: Vec<UdpSocket>,
    pub ancestor_hashes_requests: UdpSocket,
}

#[derive(Default)]
pub struct TvuConfig {
    pub max_ledger_shreds: Option<u64>,
    pub shred_version: u16,
    // Validators from which repairs are requested
    pub repair_validators: Option<HashSet<Pubkey>>,
    // Validators which should be given priority when serving repairs
    pub repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>,
    pub wait_for_vote_to_start_leader: bool,
    pub replay_slots_concurrently: bool,
}

impl Tvu {
    /// This service receives messages from a leader in the network and processes the transactions
    /// on the bank state.
    /// # Arguments
    /// * `cluster_info` - The cluster_info state.
    /// * `sockets` - fetch, repair, and retransmit sockets
    /// * `blockstore` - the ledger itself
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new(
        vote_account: &Pubkey,
        authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<ClusterInfo>,
        sockets: TvuSockets,
        blockstore: Arc<Blockstore>,
        ledger_signal_receiver: Receiver<bool>,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        maybe_process_block_store: Option<ProcessBlockStore>,
        tower_storage: Arc<dyn TowerStorage>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        exit: &Arc<AtomicBool>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        turbine_disabled: Arc<AtomicBool>,
        transaction_status_sender: Option<TransactionStatusSender>,
        rewards_recorder_sender: Option<RewardsRecorderSender>,
        cache_block_meta_sender: Option<CacheBlockMetaSender>,
        vote_tracker: Arc<VoteTracker>,
        retransmit_slots_sender: RetransmitSlotsSender,
        gossip_verified_vote_hash_receiver: GossipVerifiedVoteHashReceiver,
        verified_vote_receiver: VerifiedVoteReceiver,
        replay_vote_sender: ReplayVoteSender,
        completed_data_sets_sender: CompletedDataSetsSender,
        bank_notification_sender: Option<BankNotificationSender>,
        gossip_confirmed_slots_receiver: GossipDuplicateConfirmedSlotsReceiver,
        tvu_config: TvuConfig,
        max_slots: &Arc<MaxSlots>,
        block_metadata_notifier: Option<BlockMetadataNotifierLock>,
        wait_to_vote_slot: Option<Slot>,
        accounts_background_request_sender: AbsRequestSender,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: &Arc<ConnectionCache>,
        prioritization_fee_cache: &Arc<PrioritizationFeeCache>,
    ) -> Result<Self, String> {
        let TvuSockets {
            repair: repair_socket,
            fetch: fetch_sockets,
            retransmit: retransmit_sockets,
            forwards: tvu_forward_sockets,
            ancestor_hashes_requests: ancestor_hashes_socket,
        } = sockets;

        let (fetch_sender, fetch_receiver) = unbounded();

        let repair_socket = Arc::new(repair_socket);
        let ancestor_hashes_socket = Arc::new(ancestor_hashes_socket);
        let fetch_sockets: Vec<Arc<UdpSocket>> = fetch_sockets.into_iter().map(Arc::new).collect();
        let forward_sockets: Vec<Arc<UdpSocket>> =
            tvu_forward_sockets.into_iter().map(Arc::new).collect();
        let fetch_stage = ShredFetchStage::new(
            fetch_sockets,
            forward_sockets,
            repair_socket.clone(),
            fetch_sender,
            tvu_config.shred_version,
            bank_forks.clone(),
            cluster_info.clone(),
            exit,
        );

        let (verified_sender, verified_receiver) = unbounded();
        let (retransmit_sender, retransmit_receiver) = unbounded();
        let shred_sigverify = sigverify_shreds::spawn_shred_sigverify(
            cluster_info.id(),
            bank_forks.clone(),
            leader_schedule_cache.clone(),
            fetch_receiver,
            retransmit_sender.clone(),
            verified_sender,
            turbine_disabled,
        );

        let retransmit_stage = RetransmitStage::new(
            bank_forks.clone(),
            leader_schedule_cache.clone(),
            cluster_info.clone(),
            Arc::new(retransmit_sockets),
            retransmit_receiver,
            max_slots.clone(),
            Some(rpc_subscriptions.clone()),
        );

        let cluster_slots = Arc::new(ClusterSlots::default());
        let (duplicate_slots_reset_sender, duplicate_slots_reset_receiver) = unbounded();
        let (duplicate_slots_sender, duplicate_slots_receiver) = unbounded();
        let (ancestor_hashes_replay_update_sender, ancestor_hashes_replay_update_receiver) =
            unbounded();
        let (dumped_slots_sender, dumped_slots_receiver) = unbounded();
        let window_service = {
            let epoch_schedule = *bank_forks.read().unwrap().working_bank().epoch_schedule();
            let repair_info = RepairInfo {
                bank_forks: bank_forks.clone(),
                epoch_schedule,
                duplicate_slots_reset_sender,
                repair_validators: tvu_config.repair_validators,
                repair_whitelist: tvu_config.repair_whitelist,
                cluster_info: cluster_info.clone(),
                cluster_slots: cluster_slots.clone(),
            };
            WindowService::new(
                blockstore.clone(),
                verified_receiver,
                retransmit_sender,
                repair_socket,
                ancestor_hashes_socket,
                exit.clone(),
                repair_info,
                leader_schedule_cache.clone(),
                verified_vote_receiver,
                completed_data_sets_sender,
                duplicate_slots_sender,
                ancestor_hashes_replay_update_receiver,
                dumped_slots_receiver,
            )
        };

        let (cluster_slots_update_sender, cluster_slots_update_receiver) = unbounded();
        let cluster_slots_service = ClusterSlotsService::new(
            blockstore.clone(),
            cluster_slots.clone(),
            bank_forks.clone(),
            cluster_info.clone(),
            cluster_slots_update_receiver,
            exit.clone(),
        );

        let (ledger_cleanup_slot_sender, ledger_cleanup_slot_receiver) = unbounded();
        let replay_stage_config = ReplayStageConfig {
            vote_account: *vote_account,
            authorized_voter_keypairs,
            exit: exit.clone(),
            rpc_subscriptions: rpc_subscriptions.clone(),
            leader_schedule_cache: leader_schedule_cache.clone(),
            latest_root_senders: vec![ledger_cleanup_slot_sender],
            accounts_background_request_sender,
            block_commitment_cache,
            transaction_status_sender,
            rewards_recorder_sender,
            cache_block_meta_sender,
            bank_notification_sender,
            wait_for_vote_to_start_leader: tvu_config.wait_for_vote_to_start_leader,
            ancestor_hashes_replay_update_sender,
            tower_storage: tower_storage.clone(),
            wait_to_vote_slot,
            replay_slots_concurrently: tvu_config.replay_slots_concurrently,
        };

        let (voting_sender, voting_receiver) = unbounded();
        let voting_service = VotingService::new(
            voting_receiver,
            cluster_info.clone(),
            poh_recorder.clone(),
            tower_storage,
            bank_forks.clone(),
        );

        let warm_quic_cache_service = if connection_cache.use_quic() {
            Some(WarmQuicCacheService::new(
                connection_cache.clone(),
                cluster_info.clone(),
                poh_recorder.clone(),
                exit.clone(),
            ))
        } else {
            None
        };
        let (cost_update_sender, cost_update_receiver) = unbounded();
        let cost_update_service = CostUpdateService::new(blockstore.clone(), cost_update_receiver);

        let (drop_bank_sender, drop_bank_receiver) = unbounded();

        let drop_bank_service = DropBankService::new(drop_bank_receiver);

        let replay_stage = ReplayStage::new(
            replay_stage_config,
            blockstore.clone(),
            bank_forks.clone(),
            cluster_info.clone(),
            ledger_signal_receiver,
            duplicate_slots_receiver,
            poh_recorder.clone(),
            maybe_process_block_store,
            vote_tracker,
            cluster_slots,
            retransmit_slots_sender,
            duplicate_slots_reset_receiver,
            replay_vote_sender,
            gossip_confirmed_slots_receiver,
            gossip_verified_vote_hash_receiver,
            cluster_slots_update_sender,
            cost_update_sender,
            voting_sender,
            drop_bank_sender,
            block_metadata_notifier,
            log_messages_bytes_limit,
            prioritization_fee_cache.clone(),
            dumped_slots_sender,
        )?;

        let ledger_cleanup_service = tvu_config.max_ledger_shreds.map(|max_ledger_shreds| {
            LedgerCleanupService::new(
                ledger_cleanup_slot_receiver,
                blockstore.clone(),
                max_ledger_shreds,
                exit,
            )
        });

        Ok(Tvu {
            fetch_stage,
            shred_sigverify,
            retransmit_stage,
            window_service,
            cluster_slots_service,
            replay_stage,
            ledger_cleanup_service,
            cost_update_service,
            voting_service,
            warm_quic_cache_service,
            drop_bank_service,
        })
    }

    pub fn join(self) -> thread::Result<()> {
        self.retransmit_stage.join()?;
        self.window_service.join()?;
        self.cluster_slots_service.join()?;
        self.fetch_stage.join()?;
        self.shred_sigverify.join()?;
        if self.ledger_cleanup_service.is_some() {
            self.ledger_cleanup_service.unwrap().join()?;
        }
        self.replay_stage.join()?;
        self.cost_update_service.join()?;
        self.voting_service.join()?;
        if let Some(warmup_service) = self.warm_quic_cache_service {
            warmup_service.join()?;
        }
        self.drop_bank_service.join()?;
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        serial_test::serial,
        solana_gossip::cluster_info::{ClusterInfo, Node},
        solana_ledger::{
            blockstore::BlockstoreSignals,
            blockstore_options::BlockstoreOptions,
            create_new_tmp_ledger,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_poh::poh_recorder::create_test_recorder,
        solana_rpc::optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
        solana_runtime::bank::Bank,
        solana_sdk::signature::{Keypair, Signer},
        solana_streamer::socket::SocketAddrSpace,
        std::sync::atomic::{AtomicU64, Ordering},
    };

    #[ignore]
    #[test]
    #[serial]
    fn test_tvu_exit() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(&target1_keypair.pubkey());

        let starting_balance = 10_000;
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(starting_balance);

        let bank_forks = BankForks::new(Bank::new_for_tests(&genesis_config));

        //start cluster_info1
        let cluster_info1 = ClusterInfo::new(
            target1.info.clone(),
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        );
        cluster_info1.insert_info(leader.info);
        let cref1 = Arc::new(cluster_info1);

        let (blockstore_path, _) = create_new_tmp_ledger!(&genesis_config);
        let BlockstoreSignals {
            blockstore,
            ledger_signal_receiver,
            ..
        } = Blockstore::open_with_signal(&blockstore_path, BlockstoreOptions::default())
            .expect("Expected to successfully open ledger");
        let blockstore = Arc::new(blockstore);
        let bank = bank_forks.working_bank();
        let (exit, poh_recorder, poh_service, _entry_receiver) =
            create_test_recorder(&bank, &blockstore, None, None);
        let vote_keypair = Keypair::new();
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let (retransmit_slots_sender, _retransmit_slots_receiver) = unbounded();
        let (_gossip_verified_vote_hash_sender, gossip_verified_vote_hash_receiver) = unbounded();
        let (_verified_vote_sender, verified_vote_receiver) = unbounded();
        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        let (completed_data_sets_sender, _completed_data_sets_receiver) = unbounded();
        let (_, gossip_confirmed_slots_receiver) = unbounded();
        let bank_forks = Arc::new(RwLock::new(bank_forks));
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let tvu = Tvu::new(
            &vote_keypair.pubkey(),
            Arc::new(RwLock::new(vec![Arc::new(vote_keypair)])),
            &bank_forks,
            &cref1,
            {
                TvuSockets {
                    repair: target1.sockets.repair,
                    retransmit: target1.sockets.retransmit_sockets,
                    fetch: target1.sockets.tvu,
                    forwards: target1.sockets.tvu_forwards,
                    ancestor_hashes_requests: target1.sockets.ancestor_hashes_requests,
                }
            },
            blockstore,
            ledger_signal_receiver,
            &Arc::new(RpcSubscriptions::new_for_tests(
                &exit,
                max_complete_transaction_status_slot,
                bank_forks.clone(),
                block_commitment_cache.clone(),
                OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
            )),
            &poh_recorder,
            None,
            Arc::new(crate::tower_storage::FileTowerStorage::default()),
            &leader_schedule_cache,
            &exit,
            block_commitment_cache,
            Arc::<AtomicBool>::default(),
            None,
            None,
            None,
            Arc::<VoteTracker>::default(),
            retransmit_slots_sender,
            gossip_verified_vote_hash_receiver,
            verified_vote_receiver,
            replay_vote_sender,
            completed_data_sets_sender,
            None,
            gossip_confirmed_slots_receiver,
            TvuConfig::default(),
            &Arc::new(MaxSlots::default()),
            None,
            None,
            AbsRequestSender::default(),
            None,
            &Arc::new(ConnectionCache::default()),
            &_ignored_prioritization_fee_cache,
        )
        .expect("assume success");
        exit.store(true, Ordering::Relaxed);
        tvu.join().unwrap();
        poh_service.join().unwrap();
    }
}
