//! The `tpu` module implements the Transaction Processing Unit, a
//! multi-stage transaction processing pipeline in software.

use crate::bank::Bank;
use crate::banking_stage::{BankingStage, BankingStageReturnType};
use crate::broadcast_service::BroadcastService;
use crate::cluster_info::ClusterInfo;
use crate::cluster_info_vote_listener::ClusterInfoVoteListener;
use crate::fetch_stage::FetchStage;
use crate::fullnode::TpuRotationSender;
use crate::poh_service::Config;
use crate::service::Service;
use crate::sigverify_stage::SigVerifyStage;
use crate::streamer::BlobSender;
use crate::tpu_forwarder::TpuForwarder;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread;

pub enum TpuReturnType {
    LeaderRotation(u64),
    Abort,
}

pub enum TpuMode {
    Leader(LeaderServices),
    Forwarder(ForwarderServices),
}

pub struct LeaderServices {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    cluster_info_vote_listener: ClusterInfoVoteListener,
    broadcast_service: BroadcastService,
}

impl LeaderServices {
    fn new(
        fetch_stage: FetchStage,
        sigverify_stage: SigVerifyStage,
        banking_stage: BankingStage,
        cluster_info_vote_listener: ClusterInfoVoteListener,
        broadcast_service: BroadcastService,
    ) -> Self {
        LeaderServices {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_service,
        }
    }
}

pub struct ForwarderServices {
    tpu_forwarder: TpuForwarder,
}

impl ForwarderServices {
    fn new(tpu_forwarder: TpuForwarder) -> Self {
        ForwarderServices { tpu_forwarder }
    }
}

pub struct Tpu {
    tpu_mode: TpuMode,
    exit: Arc<AtomicBool>,
}

impl Tpu {
    #[allow(clippy::new_ret_no_self)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bank: &Arc<Bank>,
        tick_duration: Config,
        transactions_sockets: Vec<UdpSocket>,
        broadcast_socket: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        entry_height: u64,
        sigverify_disabled: bool,
        max_tick_height: u64,
        last_entry_id: &Hash,
        leader_id: Pubkey,
        is_leader: bool,
        to_validator_sender: &TpuRotationSender,
        blob_sender: &BlobSender,
    ) -> Self {
        let exit = Arc::new(AtomicBool::new(false));
        let tpu_mode = if is_leader {
            let (packet_sender, packet_receiver) = channel();
            let fetch_stage = FetchStage::new_with_sender(
                transactions_sockets,
                exit.clone(),
                &packet_sender.clone(),
            );
            let cluster_info_vote_listener =
                ClusterInfoVoteListener::new(exit.clone(), cluster_info.clone(), packet_sender);

            let (sigverify_stage, verified_receiver) =
                SigVerifyStage::new(packet_receiver, sigverify_disabled);

            let (banking_stage, entry_receiver) = BankingStage::new(
                &bank,
                verified_receiver,
                tick_duration,
                last_entry_id,
                max_tick_height,
                leader_id,
                &to_validator_sender,
            );

            let broadcast_service = BroadcastService::new(
                bank.clone(),
                broadcast_socket,
                cluster_info,
                entry_height,
                bank.leader_scheduler.clone(),
                entry_receiver,
                max_tick_height,
                exit.clone(),
                blob_sender,
            );

            let svcs = LeaderServices::new(
                fetch_stage,
                sigverify_stage,
                banking_stage,
                cluster_info_vote_listener,
                broadcast_service,
            );
            TpuMode::Leader(svcs)
        } else {
            let tpu_forwarder = TpuForwarder::new(transactions_sockets, cluster_info);
            let svcs = ForwarderServices::new(tpu_forwarder);
            TpuMode::Forwarder(svcs)
        };

        Self {
            tpu_mode,
            exit: exit.clone(),
        }
    }

    pub fn switch_to_forwarder(
        &mut self,
        transactions_sockets: Vec<UdpSocket>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
    ) {
        match &self.tpu_mode {
            TpuMode::Leader(svcs) => {
                svcs.fetch_stage.close();
            }
            TpuMode::Forwarder(svcs) => {
                svcs.tpu_forwarder.close();
            }
        }
        let tpu_forwarder = TpuForwarder::new(transactions_sockets, cluster_info);
        self.tpu_mode = TpuMode::Forwarder(ForwarderServices::new(tpu_forwarder));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn switch_to_leader(
        &mut self,
        bank: &Arc<Bank>,
        tick_duration: Config,
        transactions_sockets: Vec<UdpSocket>,
        broadcast_socket: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        sigverify_disabled: bool,
        max_tick_height: u64,
        entry_height: u64,
        last_entry_id: &Hash,
        leader_id: Pubkey,
        to_validator_sender: &TpuRotationSender,
        blob_sender: &BlobSender,
    ) {
        match &self.tpu_mode {
            TpuMode::Leader(svcs) => {
                svcs.fetch_stage.close();
            }
            TpuMode::Forwarder(svcs) => {
                svcs.tpu_forwarder.close();
            }
        }
        self.exit = Arc::new(AtomicBool::new(false));
        let (packet_sender, packet_receiver) = channel();
        let fetch_stage = FetchStage::new_with_sender(
            transactions_sockets,
            self.exit.clone(),
            &packet_sender.clone(),
        );
        let cluster_info_vote_listener =
            ClusterInfoVoteListener::new(self.exit.clone(), cluster_info.clone(), packet_sender);

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(packet_receiver, sigverify_disabled);

        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            tick_duration,
            last_entry_id,
            max_tick_height,
            leader_id,
            &to_validator_sender,
        );

        let broadcast_service = BroadcastService::new(
            bank.clone(),
            broadcast_socket,
            cluster_info,
            entry_height,
            bank.leader_scheduler.clone(),
            entry_receiver,
            max_tick_height,
            self.exit.clone(),
            blob_sender,
        );

        let svcs = LeaderServices::new(
            fetch_stage,
            sigverify_stage,
            banking_stage,
            cluster_info_vote_listener,
            broadcast_service,
        );
        self.tpu_mode = TpuMode::Leader(svcs);
    }

    pub fn is_leader(&self) -> bool {
        match self.tpu_mode {
            TpuMode::Forwarder(_) => false,
            TpuMode::Leader(_) => true,
        }
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn is_exited(&self) -> bool {
        self.exit.load(Ordering::Relaxed)
    }

    pub fn close(self) -> thread::Result<Option<TpuReturnType>> {
        match &self.tpu_mode {
            TpuMode::Leader(svcs) => {
                svcs.fetch_stage.close();
            }
            TpuMode::Forwarder(svcs) => {
                svcs.tpu_forwarder.close();
            }
        }
        self.join()
    }
}

impl Service for Tpu {
    type JoinReturnType = Option<TpuReturnType>;

    fn join(self) -> thread::Result<(Option<TpuReturnType>)> {
        match self.tpu_mode {
            TpuMode::Leader(svcs) => {
                svcs.broadcast_service.join()?;
                svcs.fetch_stage.join()?;
                svcs.sigverify_stage.join()?;
                svcs.cluster_info_vote_listener.join()?;
                match svcs.banking_stage.join()? {
                    Some(BankingStageReturnType::LeaderRotation(tick_height)) => {
                        Ok(Some(TpuReturnType::LeaderRotation(tick_height)))
                    }
                    _ => Ok(None),
                }
            }
            TpuMode::Forwarder(svcs) => {
                svcs.tpu_forwarder.join()?;
                Ok(None)
            }
        }
    }
}
