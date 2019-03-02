//! The `tpu` module implements the Transaction Processing Unit, a
//! multi-stage transaction processing pipeline in software.

use crate::banking_stage::BankingStage;
use crate::blocktree::Blocktree;
use crate::broadcast_stage::BroadcastStage;
use crate::cluster_info::ClusterInfo;
use crate::cluster_info_vote_listener::ClusterInfoVoteListener;
use crate::fetch_stage::FetchStage;
use crate::poh_recorder::PohRecorder;
use crate::service::Service;
use crate::sigverify_stage::SigVerifyStage;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

pub struct LeaderServices {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    cluster_info_vote_listener: ClusterInfoVoteListener,
    broadcast_stage: BroadcastStage,
}

impl LeaderServices {
    fn new(
        fetch_stage: FetchStage,
        sigverify_stage: SigVerifyStage,
        banking_stage: BankingStage,
        cluster_info_vote_listener: ClusterInfoVoteListener,
        broadcast_stage: BroadcastStage,
    ) -> Self {
        LeaderServices {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_stage,
        }
    }

    fn exit(&self) {
        self.fetch_stage.close();
    }

    fn join(self) -> thread::Result<()> {
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

    fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }
}

pub struct Tpu {
    leader_services: LeaderServices,
    exit: Arc<AtomicBool>,
    id: Pubkey,
}

impl Tpu {
    pub fn new(
        id: Pubkey,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        bank_receiver: Receiver<Arc<Bank>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        transactions_sockets: Vec<UdpSocket>,
        broadcast_socket: UdpSocket,
        sigverify_disabled: bool,
        blocktree: &Arc<Blocktree>,
    ) -> Self {
        cluster_info.write().unwrap().set_leader(id);

        let exit = Arc::new(AtomicBool::new(false));
        let (packet_sender, packet_receiver) = channel();
        let fetch_stage =
            FetchStage::new_with_sender(transactions_sockets, exit.clone(), &packet_sender.clone());
        let cluster_info_vote_listener =
            ClusterInfoVoteListener::new(exit.clone(), cluster_info.clone(), packet_sender);

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(packet_receiver, sigverify_disabled);

        let (banking_stage, entry_receiver) =
            BankingStage::new(bank_receiver, poh_recorder, verified_receiver, id);

        let broadcast_stage = BroadcastStage::new(
            broadcast_socket,
            cluster_info.clone(),
            entry_receiver,
            exit.clone(),
            blocktree,
        );

        let leader_services = LeaderServices::new(
            fetch_stage,
            sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_stage,
        );
        Self {
            leader_services,
            exit,
            id,
        }
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn is_exited(&self) -> bool {
        self.exit.load(Ordering::Relaxed)
    }

    pub fn close(mut self) -> thread::Result<()> {
        self.exit();
        self.join()
    }
}

impl Service for Tpu {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.leader_services.join()
    }
}
