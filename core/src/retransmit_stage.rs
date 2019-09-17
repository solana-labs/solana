//! The `retransmit_stage` retransmits blobs between validators

use crate::bank_forks::BankForks;
use crate::blocktree::{Blocktree, CompletedSlotsReceiver};
use crate::cluster_info::{compute_retransmit_peers, ClusterInfo, DATA_PLANE_FANOUT};
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::repair_service::RepairStrategy;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::staking_utils;
use crate::streamer::PacketReceiver;
use crate::window_service::{should_retransmit_and_persist, WindowService};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_metrics::{datapoint_info, inc_new_counter_error};
use solana_runtime::epoch_schedule::EpochSchedule;
use std::cmp;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

fn retransmit(
    bank_forks: &Arc<RwLock<BankForks>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    r: &PacketReceiver,
    sock: &UdpSocket,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut packets = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        packets.packets.append(&mut nq.packets);
    }

    datapoint_info!("retransmit-stage", ("count", packets.packets.len(), i64));

    let r_bank = bank_forks.read().unwrap().working_bank();
    let bank_epoch = r_bank.get_stakers_epoch(r_bank.slot());
    let mut peers_len = 0;
    for packet in &packets.packets {
        let (my_index, mut peers) = cluster_info.read().unwrap().shuffle_peers_and_index(
            staking_utils::staked_nodes_at_epoch(&r_bank, bank_epoch).as_ref(),
            ChaChaRng::from_seed(packet.meta.seed),
        );
        peers_len = cmp::max(peers_len, peers.len());
        peers.remove(my_index);

        let (neighbors, children) = compute_retransmit_peers(DATA_PLANE_FANOUT, my_index, peers);

        let leader = leader_schedule_cache.slot_leader_at(packet.meta.slot, Some(r_bank.as_ref()));
        if !packet.meta.forward {
            ClusterInfo::retransmit_to(&cluster_info, &neighbors, packet, leader, sock, true)?;
            ClusterInfo::retransmit_to(&cluster_info, &children, packet, leader, sock, false)?;
        } else {
            ClusterInfo::retransmit_to(&cluster_info, &children, packet, leader, sock, true)?;
        }
    }
    datapoint_info!("cluster_info-num_nodes", ("count", peers_len, i64));
    Ok(())
}

/// Service to retransmit messages from the leader or layer 1 to relevant peer nodes.
/// See `cluster_info` for network layer definitions.
/// # Arguments
/// * `sock` - Socket to read from.  Read timeout is set to 1.
/// * `exit` - Boolean to signal system exit.
/// * `cluster_info` - This structure needs to be updated and populated by the bank and via gossip.
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
fn retransmitter(
    sock: Arc<UdpSocket>,
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    r: PacketReceiver,
) -> JoinHandle<()> {
    let bank_forks = bank_forks.clone();
    let leader_schedule_cache = leader_schedule_cache.clone();
    Builder::new()
        .name("solana-retransmitter".to_string())
        .spawn(move || {
            trace!("retransmitter started");
            loop {
                if let Err(e) = retransmit(
                    &bank_forks,
                    &leader_schedule_cache,
                    &cluster_info,
                    &r,
                    &sock,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter_error!("streamer-retransmit-error", 1, 1);
                        }
                    }
                }
            }
            trace!("exiting retransmitter");
        })
        .unwrap()
}

pub struct RetransmitStage {
    thread_hdls: Vec<JoinHandle<()>>,
    window_service: WindowService,
}

impl RetransmitStage {
    #[allow(clippy::new_ret_no_self)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        blocktree: Arc<Blocktree>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        retransmit_socket: Arc<UdpSocket>,
        repair_socket: Arc<UdpSocket>,
        fetch_stage_receiver: PacketReceiver,
        exit: &Arc<AtomicBool>,
        completed_slots_receiver: CompletedSlotsReceiver,
        epoch_schedule: EpochSchedule,
    ) -> Self {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = retransmitter(
            retransmit_socket,
            bank_forks.clone(),
            leader_schedule_cache,
            cluster_info.clone(),
            retransmit_receiver,
        );

        let repair_strategy = RepairStrategy::RepairAll {
            bank_forks,
            completed_slots_receiver,
            epoch_schedule,
        };
        let leader_schedule_cache = leader_schedule_cache.clone();
        let window_service = WindowService::new(
            blocktree,
            cluster_info.clone(),
            fetch_stage_receiver,
            retransmit_sender,
            repair_socket,
            exit,
            repair_strategy,
            &leader_schedule_cache.clone(),
            move |id, shred, working_bank, last_root| {
                should_retransmit_and_persist(
                    shred,
                    working_bank,
                    &leader_schedule_cache,
                    id,
                    last_root,
                )
            },
        );

        let thread_hdls = vec![t_retransmit];
        Self {
            thread_hdls,
            window_service,
        }
    }
}

impl Service for RetransmitStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        self.window_service.join()?;
        Ok(())
    }
}
