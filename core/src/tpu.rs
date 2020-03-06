//! The `tpu` module implements the Transaction Processing Unit, a
//! multi-stage transaction processing pipeline in software.

use crate::{
    banking_stage::BankingStage,
    broadcast_stage::{BroadcastStage, BroadcastStageType},
    cluster_info::ClusterInfo,
    cluster_info_vote_listener::{ClusterInfoVoteListener, VoteTracker},
    fetch_stage::FetchStage,
    poh_recorder::{PohRecorder, WorkingBankEntry},
    sigverify::TransactionSigVerifier,
    sigverify_stage::{DisabledSigVerifier, SigVerifyStage},
};
use crossbeam_channel::unbounded;
use solana_ledger::{
    bank_forks::BankForks, blockstore::Blockstore, blockstore_processor::TransactionStatusSender,
};
use std::{
    net::UdpSocket,
    sync::{
        atomic::AtomicBool,
        mpsc::{channel, Receiver},
        Arc, Mutex, RwLock,
    },
    thread,
};

pub struct Tpu {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    cluster_info_vote_listener: ClusterInfoVoteListener,
    broadcast_stage: BroadcastStage,
}

impl Tpu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        entry_receiver: Receiver<WorkingBankEntry>,
        transactions_sockets: Vec<UdpSocket>,
        tpu_forwards_sockets: Vec<UdpSocket>,
        broadcast_sockets: Vec<UdpSocket>,
        sigverify_disabled: bool,
        transaction_status_sender: Option<TransactionStatusSender>,
        blockstore: &Arc<Blockstore>,
        broadcast_type: &BroadcastStageType,
        exit: &Arc<AtomicBool>,
        shred_version: u16,
        vote_tracker: Arc<VoteTracker>,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        let (packet_sender, packet_receiver) = channel();
        let fetch_stage = FetchStage::new_with_sender(
            transactions_sockets,
            tpu_forwards_sockets,
            &exit,
            &packet_sender,
            &poh_recorder,
        );
        let (verified_sender, verified_receiver) = unbounded();

        let sigverify_stage = if !sigverify_disabled {
            let verifier = TransactionSigVerifier::default();
            SigVerifyStage::new(packet_receiver, verified_sender, verifier)
        } else {
            let verifier = DisabledSigVerifier::default();
            SigVerifyStage::new(packet_receiver, verified_sender, verifier)
        };

        let (verified_vote_sender, verified_vote_receiver) = unbounded();
        let cluster_info_vote_listener = ClusterInfoVoteListener::new(
            &exit,
            cluster_info.clone(),
            sigverify_disabled,
            verified_vote_sender,
            &poh_recorder,
            vote_tracker,
            bank_forks,
        );

        let banking_stage = BankingStage::new(
            &cluster_info,
            poh_recorder,
            verified_receiver,
            verified_vote_receiver,
            transaction_status_sender,
        );

        let broadcast_stage = broadcast_type.new_broadcast_stage(
            broadcast_sockets,
            cluster_info.clone(),
            entry_receiver,
            &exit,
            blockstore,
            shred_version,
        );

        Self {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_stage,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        let mut results = vec![];
        results.push(self.fetch_stage.join());
        results.push(self.sigverify_stage.join());
        results.push(self.cluster_info_vote_listener.join());
        results.push(self.banking_stage.join());
        let broadcast_result = self.broadcast_stage.join();
        for result in results {
            result?;
        }
        let _ = broadcast_result?;
        Ok(())
    }
}
