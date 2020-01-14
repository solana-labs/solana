//! The `retransmit_stage` retransmits shreds between validators

use crate::{
    cluster_info::{compute_retransmit_peers, ClusterInfo, DATA_PLANE_FANOUT},
    packet::Packets,
    partition_cfg::PartitionCfg,
    repair_service::RepairStrategy,
    result::{Error, Result},
    streamer::PacketReceiver,
    window_service::{should_retransmit_and_persist, WindowService},
};
use crossbeam_channel::Receiver as CrossbeamReceiver;
use solana_ledger::{
    bank_forks::BankForks,
    blockstore::{Blockstore, CompletedSlotsReceiver},
    leader_schedule_cache::LeaderScheduleCache,
    staking_utils,
};
use solana_measure::measure::Measure;
use solana_metrics::inc_new_counter_error;
use solana_sdk::epoch_schedule::EpochSchedule;
use std::{
    cmp,
    net::UdpSocket,
    sync::atomic::AtomicBool,
    sync::mpsc::channel,
    sync::mpsc::RecvTimeoutError,
    sync::Mutex,
    sync::{Arc, RwLock},
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

// Limit a given thread to consume about this many packets so that
// it doesn't pull up too much work.
const MAX_PACKET_BATCH_SIZE: usize = 100;

fn retransmit(
    bank_forks: &Arc<RwLock<BankForks>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    r: &Arc<Mutex<PacketReceiver>>,
    sock: &UdpSocket,
    id: u32,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let r_lock = r.lock().unwrap();
    let packets = r_lock.recv_timeout(timer)?;
    let mut timer_start = Measure::start("retransmit");
    let mut total_packets = packets.packets.len();
    let mut packet_v = vec![packets];
    while let Ok(nq) = r_lock.try_recv() {
        total_packets += nq.packets.len();
        packet_v.push(nq);
        if total_packets >= MAX_PACKET_BATCH_SIZE {
            break;
        }
    }
    drop(r_lock);

    let r_bank = bank_forks.read().unwrap().working_bank();
    let bank_epoch = r_bank.get_leader_schedule_epoch(r_bank.slot());
    let mut peers_len = 0;
    let stakes = staking_utils::staked_nodes_at_epoch(&r_bank, bank_epoch);
    let stakes = stakes.map(Arc::new);
    let (peers, stakes_and_index) = cluster_info
        .read()
        .unwrap()
        .sorted_retransmit_peers_and_stakes(stakes);
    let me = cluster_info.read().unwrap().my_data();
    let mut discard_total = 0;
    let mut repair_total = 0;
    let mut retransmit_total = 0;
    let mut compute_turbine_peers_total = 0;
    for mut packets in packet_v {
        for packet in packets.packets.iter_mut() {
            // skip discarded packets and repair packets
            if packet.meta.discard {
                total_packets -= 1;
                discard_total += 1;
                continue;
            }
            if packet.meta.repair {
                repair_total += 1;
            }

            let mut compute_turbine_peers = Measure::start("turbine_start");
            let (my_index, mut shuffled_stakes_and_index) = ClusterInfo::shuffle_peers_and_index(
                &me.id,
                &peers,
                &stakes_and_index,
                packet.meta.seed,
            );
            peers_len = cmp::max(peers_len, shuffled_stakes_and_index.len());
            shuffled_stakes_and_index.remove(my_index);
            // split off the indexes, we don't need the stakes anymore
            let indexes = shuffled_stakes_and_index
                .into_iter()
                .map(|(_, index)| index)
                .collect();

            let (neighbors, children) =
                compute_retransmit_peers(DATA_PLANE_FANOUT, my_index, indexes);
            let neighbors: Vec<_> = neighbors.into_iter().map(|index| &peers[index]).collect();
            let children: Vec<_> = children.into_iter().map(|index| &peers[index]).collect();
            compute_turbine_peers.stop();
            compute_turbine_peers_total += compute_turbine_peers.as_ms();

            let leader =
                leader_schedule_cache.slot_leader_at(packet.meta.slot, Some(r_bank.as_ref()));
            let mut retransmit_time = Measure::start("retransmit_to");
            // If I am on the critical path for this packet, send it to everyone
            if my_index % DATA_PLANE_FANOUT == 0 {
                ClusterInfo::retransmit_to(&neighbors, packet, leader, sock, true)?;
                ClusterInfo::retransmit_to(&children, packet, leader, sock, false)?;
            } else {
                ClusterInfo::retransmit_to(&children, packet, leader, sock, true)?;
            }
            retransmit_time.stop();
            retransmit_total += retransmit_time.as_ms();
        }
    }
    timer_start.stop();
    debug!(
        "retransmitted {} packets in {}ms retransmit_time: {}ms id: {}",
        total_packets,
        timer_start.as_ms(),
        retransmit_total,
        id,
    );
    datapoint_debug!("cluster_info-num_nodes", ("count", peers_len, i64));
    datapoint_debug!(
        "retransmit-stage",
        ("total_time", timer_start.as_ms() as i64, i64),
        ("total_packets", total_packets as i64, i64),
        ("retransmit_total", retransmit_total as i64, i64),
        ("compute_turbine", compute_turbine_peers_total as i64, i64),
        ("repair_total", i64::from(repair_total), i64),
        ("discard_total", i64::from(discard_total), i64),
    );
    Ok(())
}

/// Service to retransmit messages from the leader or layer 1 to relevant peer nodes.
/// See `cluster_info` for network layer definitions.
/// # Arguments
/// * `sockets` - Sockets to read from.
/// * `bank_forks` - The BankForks structure
/// * `leader_schedule_cache` - The leader schedule to verify shreds
/// * `cluster_info` - This structure needs to be updated and populated by the bank and via gossip.
/// * `r` - Receive channel for shreds to be retransmitted to all the layer 1 nodes.
pub fn retransmitter(
    sockets: Arc<Vec<UdpSocket>>,
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    r: Arc<Mutex<PacketReceiver>>,
) -> Vec<JoinHandle<()>> {
    (0..sockets.len())
        .map(|s| {
            let sockets = sockets.clone();
            let bank_forks = bank_forks.clone();
            let leader_schedule_cache = leader_schedule_cache.clone();
            let r = r.clone();
            let cluster_info = cluster_info.clone();

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
                            &sockets[s],
                            s as u32,
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
        })
        .collect()
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
        blockstore: Arc<Blockstore>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        retransmit_sockets: Arc<Vec<UdpSocket>>,
        repair_socket: Arc<UdpSocket>,
        verified_receiver: CrossbeamReceiver<Vec<Packets>>,
        exit: &Arc<AtomicBool>,
        completed_slots_receiver: CompletedSlotsReceiver,
        epoch_schedule: EpochSchedule,
        cfg: Option<PartitionCfg>,
        shred_version: u16,
    ) -> Self {
        let (retransmit_sender, retransmit_receiver) = channel();

        let retransmit_receiver = Arc::new(Mutex::new(retransmit_receiver));
        let t_retransmit = retransmitter(
            retransmit_sockets,
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
            blockstore,
            cluster_info.clone(),
            verified_receiver,
            retransmit_sender,
            repair_socket,
            exit,
            repair_strategy,
            &leader_schedule_cache.clone(),
            move |id, shred, working_bank, last_root| {
                let is_connected = cfg
                    .as_ref()
                    .map(|x| x.is_connected(&working_bank, &leader_schedule_cache, shred))
                    .unwrap_or(true);
                let rv = should_retransmit_and_persist(
                    shred,
                    working_bank,
                    &leader_schedule_cache,
                    id,
                    last_root,
                    shred_version,
                );
                rv && is_connected
            },
        );

        let thread_hdls = t_retransmit;
        Self {
            thread_hdls,
            window_service,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        self.window_service.join()?;
        Ok(())
    }
}
