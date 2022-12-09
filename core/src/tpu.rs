//! The `tpu` module implements the Transaction Processing Unit, a
//! multi-stage transaction processing pipeline in software.

use {
    crate::{
        banking_stage::BankingStage,
        broadcast_stage::{BroadcastStage, BroadcastStageType, RetransmitSlotsReceiver},
        cluster_info_vote_listener::{
            ClusterInfoVoteListener, GossipDuplicateConfirmedSlotsSender,
            GossipVerifiedVoteHashSender, VerifiedVoteSender, VoteTracker,
        },
        fetch_stage::FetchStage,
        find_packet_sender_stake_stage::FindPacketSenderStakeStage,
        sigverify::TransactionSigVerifier,
        sigverify_stage::SigVerifyStage,
        staked_nodes_updater_service::StakedNodesUpdaterService,
    },
    crossbeam_channel::{unbounded, Receiver},
    solana_client::connection_cache::ConnectionCache,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{blockstore::Blockstore, blockstore_processor::TransactionStatusSender},
    solana_poh::poh_recorder::{PohRecorder, WorkingBankEntry},
    solana_rpc::{
        optimistically_confirmed_bank_tracker::BankNotificationSender,
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_runtime::{
        bank_forks::BankForks,
        vote_sender_types::{ReplayVoteReceiver, ReplayVoteSender},
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    solana_streamer::{
        nonblocking::quic::DEFAULT_WAIT_FOR_CHUNK_TIMEOUT_MS,
        quic::{spawn_server, StreamStats, MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
        streamer::StakedNodes,
    },
    std::{
        collections::HashMap,
        net::UdpSocket,
        sync::{atomic::AtomicBool, Arc, RwLock},
        thread,
    },
};

pub const DEFAULT_TPU_COALESCE_MS: u64 = 5;

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
    find_packet_sender_stake_stage: FindPacketSenderStakeStage,
    vote_find_packet_sender_stake_stage: FindPacketSenderStakeStage,
    staked_nodes_updater_service: StakedNodesUpdaterService,
}

impl Tpu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        entry_receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: RetransmitSlotsReceiver,
        sockets: TpuSockets,
        subscriptions: &Arc<RpcSubscriptions>,
        transaction_status_sender: Option<TransactionStatusSender>,
        blockstore: &Arc<Blockstore>,
        broadcast_type: &BroadcastStageType,
        exit: &Arc<AtomicBool>,
        shred_version: u16,
        vote_tracker: Arc<VoteTracker>,
        bank_forks: Arc<RwLock<BankForks>>,
        verified_vote_sender: VerifiedVoteSender,
        gossip_verified_vote_hash_sender: GossipVerifiedVoteHashSender,
        replay_vote_receiver: ReplayVoteReceiver,
        replay_vote_sender: ReplayVoteSender,
        bank_notification_sender: Option<BankNotificationSender>,
        tpu_coalesce_ms: u64,
        cluster_confirmed_slot_sender: GossipDuplicateConfirmedSlotsSender,
        connection_cache: &Arc<ConnectionCache>,
        keypair: &Keypair,
        log_messages_bytes_limit: Option<usize>,
        staked_nodes: &Arc<RwLock<StakedNodes>>,
        shared_staked_nodes_overrides: Arc<RwLock<HashMap<Pubkey, u64>>>,
        tpu_enable_udp: bool,
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
            exit,
            &packet_sender,
            &vote_packet_sender,
            &forwarded_packet_sender,
            forwarded_packet_receiver,
            poh_recorder,
            tpu_coalesce_ms,
            Some(bank_forks.read().unwrap().get_vote_only_mode_signal()),
            tpu_enable_udp,
        );

        let staked_nodes_updater_service = StakedNodesUpdaterService::new(
            exit.clone(),
            cluster_info.clone(),
            bank_forks.clone(),
            staked_nodes.clone(),
            shared_staked_nodes_overrides,
        );

        let (find_packet_sender_stake_sender, find_packet_sender_stake_receiver) = unbounded();

        let find_packet_sender_stake_stage = FindPacketSenderStakeStage::new(
            packet_receiver,
            find_packet_sender_stake_sender,
            staked_nodes.clone(),
            "Tpu",
        );

        let (vote_find_packet_sender_stake_sender, vote_find_packet_sender_stake_receiver) =
            unbounded();

        let vote_find_packet_sender_stake_stage = FindPacketSenderStakeStage::new(
            vote_packet_receiver,
            vote_find_packet_sender_stake_sender,
            staked_nodes.clone(),
            "Vote",
        );

        let (verified_sender, verified_receiver) = unbounded();

        let stats = Arc::new(StreamStats::default());
        let (_, tpu_quic_t) = spawn_server(
            transactions_quic_sockets,
            keypair,
            cluster_info.my_contact_info().tpu.ip(),
            packet_sender,
            exit.clone(),
            MAX_QUIC_CONNECTIONS_PER_PEER,
            staked_nodes.clone(),
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats.clone(),
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT_MS,
        )
        .unwrap();

        let (_, tpu_forwards_quic_t) = spawn_server(
            transactions_forwards_quic_sockets,
            keypair,
            cluster_info.my_contact_info().tpu_forwards.ip(),
            forwarded_packet_sender,
            exit.clone(),
            MAX_QUIC_CONNECTIONS_PER_PEER,
            staked_nodes.clone(),
            MAX_STAKED_CONNECTIONS.saturating_add(MAX_UNSTAKED_CONNECTIONS),
            0, // Prevent unstaked nodes from forwarding transactions
            stats,
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT_MS,
        )
        .unwrap();

        let sigverify_stage = {
            let verifier = TransactionSigVerifier::new(verified_sender);
            SigVerifyStage::new(find_packet_sender_stake_receiver, verifier, "tpu-verifier")
        };

        let (verified_tpu_vote_packets_sender, verified_tpu_vote_packets_receiver) = unbounded();

        let vote_sigverify_stage = {
            let verifier =
                TransactionSigVerifier::new_reject_non_vote(verified_tpu_vote_packets_sender);
            SigVerifyStage::new(
                vote_find_packet_sender_stake_receiver,
                verifier,
                "tpu-vote-verifier",
            )
        };

        let (verified_gossip_vote_packets_sender, verified_gossip_vote_packets_receiver) =
            unbounded();
        let cluster_info_vote_listener = ClusterInfoVoteListener::new(
            exit.clone(),
            cluster_info.clone(),
            verified_gossip_vote_packets_sender,
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
            cluster_info,
            poh_recorder,
            verified_receiver,
            verified_tpu_vote_packets_receiver,
            verified_gossip_vote_packets_receiver,
            transaction_status_sender,
            replay_vote_sender,
            log_messages_bytes_limit,
            connection_cache.clone(),
            bank_forks.clone(),
        );

        let broadcast_stage = broadcast_type.new_broadcast_stage(
            broadcast_sockets,
            cluster_info.clone(),
            entry_receiver,
            retransmit_slots_receiver,
            exit.clone(),
            blockstore.clone(),
            bank_forks,
            shred_version,
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
            find_packet_sender_stake_stage,
            vote_find_packet_sender_stake_stage,
            staked_nodes_updater_service,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        let results = vec![
            self.fetch_stage.join(),
            self.sigverify_stage.join(),
            self.vote_sigverify_stage.join(),
            self.cluster_info_vote_listener.join(),
            self.banking_stage.join(),
            self.find_packet_sender_stake_stage.join(),
            self.vote_find_packet_sender_stake_stage.join(),
            self.staked_nodes_updater_service.join(),
            self.tpu_quic_t.join(),
            self.tpu_forwards_quic_t.join(),
        ];
        let broadcast_result = self.broadcast_stage.join();
        for result in results {
            result?;
        }
        let _ = broadcast_result?;
        Ok(())
    }
}
