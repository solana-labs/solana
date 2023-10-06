//! The `tpu` module implements the Transaction Processing Unit, a
//! multi-stage transaction processing pipeline in software.

pub use solana_sdk::net::DEFAULT_TPU_COALESCE;
use {
    crate::{
        banking_stage::BankingStage,
        banking_trace::{BankingTracer, TracerThread},
        cluster_info_vote_listener::{
            ClusterInfoVoteListener, GossipDuplicateConfirmedSlotsSender,
            GossipVerifiedVoteHashSender, VerifiedVoteSender, VoteTracker,
        },
        fetch_stage::FetchStage,
        sigverify::TransactionSigVerifier,
        sigverify_stage::SigVerifyStage,
        staked_nodes_updater_service::StakedNodesUpdaterService,
        tpu_entry_notifier::TpuEntryNotifier,
        validator::{BlockProductionMethod, GeneratorConfig},
    },
    bytes::Bytes,
    crossbeam_channel::{unbounded, Receiver},
    solana_client::connection_cache::{ConnectionCache, Protocol},
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{
        blockstore::Blockstore, blockstore_processor::TransactionStatusSender,
        entry_notifier_service::EntryNotifierSender,
    },
    solana_poh::poh_recorder::{PohRecorder, WorkingBankEntry},
    solana_rpc::{
        optimistically_confirmed_bank_tracker::BankNotificationSender,
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_runtime::{bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache},
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Keypair},
    solana_streamer::{
        nonblocking::quic::DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
        quic::{spawn_server, MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
        streamer::StakedNodes,
    },
    solana_turbine::broadcast_stage::{BroadcastStage, BroadcastStageType},
    solana_vote::vote_sender_types::{ReplayVoteReceiver, ReplayVoteSender},
    std::{
        collections::HashMap,
        net::{SocketAddr, UdpSocket},
        sync::{atomic::AtomicBool, Arc, RwLock},
        thread,
        time::Duration,
    },
    tokio::sync::mpsc::Sender as AsyncSender,
};

// allow multiple connections for NAT and any open/close overlap
pub const MAX_QUIC_CONNECTIONS_PER_PEER: usize = 8;

pub struct TpuSockets {
    pub transactions: Vec<UdpSocket>,
    pub transaction_forwards: Vec<UdpSocket>,
    pub vote: Vec<UdpSocket>,
    pub broadcast: Vec<UdpSocket>,
    pub transactions_quic: UdpSocket,
    pub transactions_forwards_quic: UdpSocket,
}

pub struct Tpu {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    vote_sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    cluster_info_vote_listener: ClusterInfoVoteListener,
    broadcast_stage: BroadcastStage,
    tpu_quic_t: thread::JoinHandle<()>,
    tpu_forwards_quic_t: thread::JoinHandle<()>,
    tpu_entry_notifier: Option<TpuEntryNotifier>,
    staked_nodes_updater_service: StakedNodesUpdaterService,
    tracer_thread_hdl: TracerThread,
}

impl Tpu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        entry_receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: Receiver<Slot>,
        sockets: TpuSockets,
        subscriptions: &Arc<RpcSubscriptions>,
        transaction_status_sender: Option<TransactionStatusSender>,
        entry_notification_sender: Option<EntryNotifierSender>,
        blockstore: Arc<Blockstore>,
        broadcast_type: &BroadcastStageType,
        exit: Arc<AtomicBool>,
        shred_version: u16,
        vote_tracker: Arc<VoteTracker>,
        bank_forks: Arc<RwLock<BankForks>>,
        verified_vote_sender: VerifiedVoteSender,
        gossip_verified_vote_hash_sender: GossipVerifiedVoteHashSender,
        replay_vote_receiver: ReplayVoteReceiver,
        replay_vote_sender: ReplayVoteSender,
        bank_notification_sender: Option<BankNotificationSender>,
        tpu_coalesce: Duration,
        cluster_confirmed_slot_sender: GossipDuplicateConfirmedSlotsSender,
        connection_cache: &Arc<ConnectionCache>,
        turbine_quic_endpoint_sender: AsyncSender<(SocketAddr, Bytes)>,
        keypair: &Keypair,
        log_messages_bytes_limit: Option<usize>,
        staked_nodes: &Arc<RwLock<StakedNodes>>,
        shared_staked_nodes_overrides: Arc<RwLock<HashMap<Pubkey, u64>>>,
        banking_tracer: Arc<BankingTracer>,
        tracer_thread_hdl: TracerThread,
        tpu_enable_udp: bool,
        prioritization_fee_cache: &Arc<PrioritizationFeeCache>,
        block_production_method: BlockProductionMethod,
        _generator_config: Option<GeneratorConfig>, /* vestigial code for replay invalidator */
    ) -> Self {
        let TpuSockets {
            transactions: transactions_sockets,
            transaction_forwards: tpu_forwards_sockets,
            vote: tpu_vote_sockets,
            broadcast: broadcast_sockets,
            transactions_quic: transactions_quic_sockets,
            transactions_forwards_quic: transactions_forwards_quic_sockets,
        } = sockets;

        let (packet_sender, packet_receiver) = unbounded();
        let (vote_packet_sender, vote_packet_receiver) = unbounded();
        let (forwarded_packet_sender, forwarded_packet_receiver) = unbounded();
        let fetch_stage = FetchStage::new_with_sender(
            transactions_sockets,
            tpu_forwards_sockets,
            tpu_vote_sockets,
            exit.clone(),
            &packet_sender,
            &vote_packet_sender,
            &forwarded_packet_sender,
            forwarded_packet_receiver,
            poh_recorder,
            tpu_coalesce,
            Some(bank_forks.read().unwrap().get_vote_only_mode_signal()),
            tpu_enable_udp,
        );

        let staked_nodes_updater_service = StakedNodesUpdaterService::new(
            exit.clone(),
            bank_forks.clone(),
            staked_nodes.clone(),
            shared_staked_nodes_overrides,
        );

        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();

        let (_, tpu_quic_t) = spawn_server(
            "quic_streamer_tpu",
            transactions_quic_sockets,
            keypair,
            cluster_info
                .my_contact_info()
                .tpu(Protocol::QUIC)
                .expect("Operator must spin up node with valid (QUIC) TPU address")
                .ip(),
            packet_sender,
            exit.clone(),
            MAX_QUIC_CONNECTIONS_PER_PEER,
            staked_nodes.clone(),
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            tpu_coalesce,
        )
        .unwrap();

        let (_, tpu_forwards_quic_t) = spawn_server(
            "quic_streamer_tpu_forwards",
            transactions_forwards_quic_sockets,
            keypair,
            cluster_info
                .my_contact_info()
                .tpu_forwards(Protocol::QUIC)
                .expect("Operator must spin up node with valid (QUIC) TPU-forwards address")
                .ip(),
            forwarded_packet_sender,
            exit.clone(),
            MAX_QUIC_CONNECTIONS_PER_PEER,
            staked_nodes.clone(),
            MAX_STAKED_CONNECTIONS.saturating_add(MAX_UNSTAKED_CONNECTIONS),
            0, // Prevent unstaked nodes from forwarding transactions
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            tpu_coalesce,
        )
        .unwrap();

        let sigverify_stage = {
            let verifier = TransactionSigVerifier::new(non_vote_sender);
            SigVerifyStage::new(packet_receiver, verifier, "tpu-verifier")
        };

        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();

        let vote_sigverify_stage = {
            let verifier = TransactionSigVerifier::new_reject_non_vote(tpu_vote_sender);
            SigVerifyStage::new(vote_packet_receiver, verifier, "tpu-vote-verifier")
        };

        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let cluster_info_vote_listener = ClusterInfoVoteListener::new(
            exit.clone(),
            cluster_info.clone(),
            gossip_vote_sender,
            poh_recorder.clone(),
            vote_tracker,
            bank_forks.clone(),
            subscriptions.clone(),
            verified_vote_sender,
            gossip_verified_vote_hash_sender,
            replay_vote_receiver,
            blockstore.clone(),
            bank_notification_sender,
            cluster_confirmed_slot_sender,
        );

        let banking_stage = BankingStage::new(
            block_production_method,
            cluster_info,
            poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            transaction_status_sender,
            replay_vote_sender,
            log_messages_bytes_limit,
            connection_cache.clone(),
            bank_forks.clone(),
            prioritization_fee_cache,
        );

        let (entry_receiver, tpu_entry_notifier) =
            if let Some(entry_notification_sender) = entry_notification_sender {
                let (broadcast_entry_sender, broadcast_entry_receiver) = unbounded();
                let tpu_entry_notifier = TpuEntryNotifier::new(
                    entry_receiver,
                    entry_notification_sender,
                    broadcast_entry_sender,
                    exit.clone(),
                );
                (broadcast_entry_receiver, Some(tpu_entry_notifier))
            } else {
                (entry_receiver, None)
            };

        let broadcast_stage = broadcast_type.new_broadcast_stage(
            broadcast_sockets,
            cluster_info.clone(),
            entry_receiver,
            retransmit_slots_receiver,
            exit,
            blockstore,
            bank_forks,
            shred_version,
            turbine_quic_endpoint_sender,
        );

        Self {
            fetch_stage,
            sigverify_stage,
            vote_sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_stage,
            tpu_quic_t,
            tpu_forwards_quic_t,
            tpu_entry_notifier,
            staked_nodes_updater_service,
            tracer_thread_hdl,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        let results = vec![
            self.fetch_stage.join(),
            self.sigverify_stage.join(),
            self.vote_sigverify_stage.join(),
            self.cluster_info_vote_listener.join(),
            self.banking_stage.join(),
            self.staked_nodes_updater_service.join(),
            self.tpu_quic_t.join(),
            self.tpu_forwards_quic_t.join(),
        ];
        let broadcast_result = self.broadcast_stage.join();
        for result in results {
            result?;
        }
        if let Some(tpu_entry_notifier) = self.tpu_entry_notifier {
            tpu_entry_notifier.join()?;
        }
        let _ = broadcast_result?;
        if let Some(tracer_thread_hdl) = self.tracer_thread_hdl {
            if let Err(tracer_result) = tracer_thread_hdl.join()? {
                error!(
                    "banking tracer thread returned error after successful thread join: {:?}",
                    tracer_result
                );
            }
        }
        Ok(())
    }
}
