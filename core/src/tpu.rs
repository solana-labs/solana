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
        packet_deduper::PacketDeduper,
        sigverify::TransactionSigVerifier,
        sigverify_stage::SigVerifyStage,
    },
    crossbeam_channel::unbounded,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{blockstore::Blockstore, blockstore_processor::TransactionStatusSender},
    solana_perf::packet::{ExtendedPacketBatch, StandardPackets},
    solana_poh::poh_recorder::{PohRecorder, WorkingBankEntry},
    solana_rpc::{
        optimistically_confirmed_bank_tracker::BankNotificationSender,
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_runtime::{
        bank_forks::BankForks,
        cost_model::CostModel,
        vote_sender_types::{ReplayVoteReceiver, ReplayVoteSender},
    },
    solana_sdk::packet::{ExtendedPacket, Packet},
    std::{
        net::UdpSocket,
        sync::{
            atomic::AtomicBool,
            mpsc::{channel, Receiver},
            Arc, Mutex, RwLock,
        },
        thread,
    },
};

pub const DEFAULT_TPU_COALESCE_MS: u64 = 5;

pub struct Tpu {
    fetch_stage: FetchStage,
    tx_sigverify_stage: SigVerifyStage,
    tx_extended_sigverify_stage: SigVerifyStage,
    vote_sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    cluster_info_vote_listener: ClusterInfoVoteListener,
    broadcast_stage: BroadcastStage,
}

impl Tpu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        entry_receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: RetransmitSlotsReceiver,
        tpu_tx_sockets: Vec<UdpSocket>,
        tpu_tx_forwards_sockets: Vec<UdpSocket>,
        tpu_tx_extended_sockets: Vec<UdpSocket>,
        tpu_tx_extended_forwards_sockets: Vec<UdpSocket>,
        tpu_vote_sockets: Vec<UdpSocket>,
        broadcast_sockets: Vec<UdpSocket>,
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
        cost_model: &Arc<RwLock<CostModel>>,
    ) -> Self {
        let (packet_sender, packet_receiver) = channel();
        let (extended_sender, extended_receiver) = channel();
        let (vote_packet_sender, vote_packet_receiver) = channel();

        let fetch_stage: FetchStage = FetchStage::new_with_sender(
            tpu_tx_sockets,
            tpu_tx_forwards_sockets,
            tpu_tx_extended_sockets,
            tpu_tx_extended_forwards_sockets,
            tpu_vote_sockets,
            exit,
            &packet_sender,
            &extended_sender,
            &vote_packet_sender,
            poh_recorder,
            tpu_coalesce_ms,
        );

        let (verified_sender, verified_receiver) = unbounded::<Vec<StandardPackets>>();
        let tx_sigverify_stage = {
            let verifier = TransactionSigVerifier::<Packet>::default();
            SigVerifyStage::new(packet_receiver, verified_sender, verifier)
        };

        let (verified_extended_sender, verified_extended_receiver) =
            unbounded::<Vec<ExtendedPacketBatch>>();
        let tx_extended_sigverify_stage = {
            let verifier = TransactionSigVerifier::<ExtendedPacket>::default();
            SigVerifyStage::new(extended_receiver, verified_extended_sender, verifier)
        };

        let (verified_tpu_vote_packets_sender, verified_tpu_vote_packets_receiver) =
            unbounded::<Vec<StandardPackets>>();
        let vote_sigverify_stage = {
            let verifier = TransactionSigVerifier::<Packet>::new_reject_non_vote();
            SigVerifyStage::new(
                vote_packet_receiver,
                verified_tpu_vote_packets_sender,
                verifier,
            )
        };

        let (verified_gossip_vote_packets_sender, verified_gossip_vote_packets_receiver) =
            unbounded::<Vec<StandardPackets>>();
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

        let banking_stage: BankingStage = BankingStage::new(
            cluster_info,
            poh_recorder,
            verified_receiver,
            verified_extended_receiver,
            verified_tpu_vote_packets_receiver,
            verified_gossip_vote_packets_receiver,
            transaction_status_sender,
            replay_vote_sender,
            cost_model.clone(),
            PacketDeduper::default(),
        );

        let broadcast_stage = broadcast_type.new_broadcast_stage(
            broadcast_sockets,
            cluster_info.clone(),
            entry_receiver,
            retransmit_slots_receiver,
            exit,
            blockstore,
            &bank_forks,
            shred_version,
        );

        Self {
            fetch_stage,
            tx_sigverify_stage,
            tx_extended_sigverify_stage,
            vote_sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_stage,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        let results = vec![
            self.fetch_stage.join(),
            self.tx_sigverify_stage.join(),
            self.tx_extended_sigverify_stage.join(),
            self.vote_sigverify_stage.join(),
            self.cluster_info_vote_listener.join(),
            self.banking_stage.join(),
        ];
        let broadcast_result = self.broadcast_stage.join();
        for result in results {
            result?;
        }
        let _ = broadcast_result?;
        Ok(())
    }
}
