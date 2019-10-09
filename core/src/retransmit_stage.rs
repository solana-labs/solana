//! The `retransmit_stage` retransmits blobs between validators

use crate::{
    bank_forks::BankForks,
    blocktree::{Blocktree, CompletedSlotsReceiver},
    cluster_info::{compute_retransmit_peers, ClusterInfo, DATA_PLANE_FANOUT},
    leader_schedule_cache::LeaderScheduleCache,
    repair_service::RepairStrategy,
    result::{Error, Result},
    service::Service,
    staking_utils,
    streamer::PacketReceiver,
    window_service::{should_retransmit_and_persist, WindowService},
};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_measure::measure::Measure;
use solana_metrics::inc_new_counter_error;
use solana_sdk::epoch_schedule::EpochSchedule;
use std::{
    cmp,
    net::UdpSocket,
    sync::atomic::AtomicBool,
    sync::mpsc::channel,
    sync::mpsc::RecvTimeoutError,
    sync::{Arc, RwLock},
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub fn retransmit(
    bank_forks: &Arc<RwLock<BankForks>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    r: &PacketReceiver,
    sock: &UdpSocket,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let packets = r.recv_timeout(timer)?;
    let mut timer_start = Measure::start("retransmit");
    let mut total_packets = packets.packets.len();
    let mut packet_v = vec![packets];
    while let Ok(nq) = r.try_recv() {
        total_packets += nq.packets.len();
        packet_v.push(nq);
    }

    let r_bank = bank_forks.read().unwrap().working_bank();
    let bank_epoch = r_bank.get_leader_schedule_epoch(r_bank.slot());
    let mut peers_len = 0;
    let stakes = staking_utils::staked_nodes_at_epoch(&r_bank, bank_epoch);
    let (peers, stakes_and_index) = cluster_info
        .read()
        .unwrap()
        .sorted_retransmit_peers_and_stakes(stakes.as_ref());
    let me = cluster_info.read().unwrap().my_data().clone();
    let mut retransmit_total = 0;
    let mut compute_turbine_peers_total = 0;
    for packets in packet_v {
        for packet in &packets.packets {
            // skip repair packets
            if packet.meta.repair {
                total_packets -= 1;
                continue;
            }
            let mut compute_turbine_peers = Measure::start("turbine_start");
            let (my_index, mut shuffled_stakes_and_index) = ClusterInfo::shuffle_peers_and_index(
                &me.id,
                &peers,
                &stakes_and_index,
                ChaChaRng::from_seed(packet.meta.seed),
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
            if !packet.meta.forward {
                ClusterInfo::retransmit_to(&me.id, &neighbors, packet, leader, sock, true)?;
                ClusterInfo::retransmit_to(&me.id, &children, packet, leader, sock, false)?;
            } else {
                ClusterInfo::retransmit_to(&me.id, &children, packet, leader, sock, true)?;
            }
            retransmit_time.stop();
            retransmit_total += retransmit_time.as_ms();
        }
    }
    timer_start.stop();
    debug!(
        "retransmitted {} packets in {}ms retransmit_time: {}ms",
        total_packets,
        timer_start.as_ms(),
        retransmit_total
    );
    datapoint_debug!("cluster_info-num_nodes", ("count", peers_len, i64));
    datapoint_debug!(
        "retransmit-stage",
        ("total_time", timer_start.as_ms() as i64, i64),
        ("total_packets", total_packets as i64, i64),
        ("retransmit_total", retransmit_total as i64, i64),
        ("compute_turbine", compute_turbine_peers_total as i64, i64),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::create_new_tmp_ledger;
    use crate::blocktree_processor::{process_blocktree, ProcessOptions};
    use crate::contact_info::ContactInfo;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::packet::{Meta, Packet, Packets};
    use solana_netutil::find_available_port_in_range;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_skip_repair() {
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            full_leader_cache: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, _, cached_leader_schedule) =
            process_blocktree(&genesis_block, &blocktree, None, opts).unwrap();
        let leader_schedule_cache = Arc::new(cached_leader_schedule);
        let bank_forks = Arc::new(RwLock::new(bank_forks));

        let mut me = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
        let port = find_available_port_in_range((8000, 10000)).unwrap();
        let me_retransmit = UdpSocket::bind(format!("127.0.0.1:{}", port)).unwrap();
        // need to make sure tvu and tpu are valid addresses
        me.tvu_forwards = me_retransmit.local_addr().unwrap();
        let port = find_available_port_in_range((8000, 10000)).unwrap();
        me.tvu = UdpSocket::bind(format!("127.0.0.1:{}", port))
            .unwrap()
            .local_addr()
            .unwrap();

        let other = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(other);
        cluster_info.insert_info(me);

        let retransmit_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let (retransmit_sender, retransmit_receiver) = channel();
        let t_retransmit = retransmitter(
            retransmit_socket,
            bank_forks,
            &leader_schedule_cache,
            cluster_info,
            retransmit_receiver,
        );
        let _thread_hdls = vec![t_retransmit];

        let packets = Packets::new(vec![Packet::default()]);
        // it should send this over the sockets.
        retransmit_sender.send(packets).unwrap();
        let mut packets = Packets::new(vec![]);
        packets.recv_from(&me_retransmit).unwrap();
        assert_eq!(packets.packets.len(), 1);
        assert_eq!(packets.packets[0].meta.repair, false);

        let repair = Packet {
            meta: Meta {
                repair: true,
                ..Meta::default()
            },
            ..Packet::default()
        };

        // send 1 repair and 1 "regular" packet so that we don't block forever on the recv_from
        let packets = Packets::new(vec![repair, Packet::default()]);
        retransmit_sender.send(packets).unwrap();
        let mut packets = Packets::new(vec![]);
        packets.recv_from(&me_retransmit).unwrap();
        assert_eq!(packets.packets.len(), 1);
        assert_eq!(packets.packets[0].meta.repair, false);
    }
}
