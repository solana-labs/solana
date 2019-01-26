//! The `tpu` module implements the Transaction Processing Unit, a
//! multi-stage transaction processing pipeline in software.

use crate::bank::Bank;
use crate::banking_stage::{BankingStage, BankingStageReturnType};
use crate::cluster_info::ClusterInfo;
use crate::cluster_info_vote_listener::ClusterInfoVoteListener;
use crate::entry::Entry;
use crate::fetch_stage::FetchStage;
use crate::poh_service::Config;
use crate::service::Service;
use crate::sigverify_stage::SigVerifyStage;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};
use std::thread;

pub enum TpuReturnType {
    LeaderRotation,
}

pub struct Tpu {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    cluster_info_vote_listener: ClusterInfoVoteListener,
    exit: Arc<AtomicBool>,
}

impl Tpu {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        bank: &Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        tick_duration: Config,
        transactions_sockets: Vec<UdpSocket>,
        sigverify_disabled: bool,
        max_tick_height: Option<u64>,
        last_entry_id: &Hash,
        leader_id: Pubkey,
    ) -> (Self, Receiver<Vec<Entry>>, Arc<AtomicBool>) {
        let exit = Arc::new(AtomicBool::new(false));

        let (packet_sender, packet_receiver) = channel();
        let fetch_stage =
            FetchStage::new_with_sender(transactions_sockets, exit.clone(), packet_sender.clone());
        let cluster_info_vote_listener =
            ClusterInfoVoteListener::new(exit.clone(), cluster_info, packet_sender);

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(packet_receiver, sigverify_disabled);

        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            tick_duration,
            last_entry_id,
            max_tick_height,
            leader_id,
        );

        let tpu = Self {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            exit: exit.clone(),
        };

        (tpu, entry_receiver, exit)
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn is_exited(&self) -> bool {
        self.exit.load(Ordering::Relaxed)
    }

    pub fn close(self) -> thread::Result<Option<TpuReturnType>> {
        self.fetch_stage.close();
        self.join()
    }
}

impl Service for Tpu {
    type JoinReturnType = Option<TpuReturnType>;

    fn join(self) -> thread::Result<(Option<TpuReturnType>)> {
        self.fetch_stage.join()?;
        self.sigverify_stage.join()?;
        self.cluster_info_vote_listener.join()?;
        match self.banking_stage.join()? {
            Some(BankingStageReturnType::LeaderRotation) => Ok(Some(TpuReturnType::LeaderRotation)),
            _ => Ok(None),
        }
    }
}
