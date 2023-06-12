//! The `shred_fetch_stage` pulls shreds from UDP sockets and sends it to a channel.

use {
    crate::{cluster_nodes::check_feature_activation, serve_repair::ServeRepair},
    crossbeam_channel::{unbounded, Sender},
    solana_gossip::{cluster_info::ClusterInfo, contact_info::Protocol},
    solana_ledger::shred::{should_discard_shred, ShredFetchStats},
    solana_perf::packet::{PacketBatch, PacketBatchRecycler, PacketFlags},
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        clock::{Slot, DEFAULT_MS_PER_SLOT},
        feature_set,
    },
    solana_streamer::streamer::{self, PacketBatchReceiver, StakedNodes, StreamerReceiveStats},
    std::{
        net::UdpSocket,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const MAX_QUIC_CONNECTIONS_PER_PEER: usize = 8;
const MAX_STAKED_QUIC_CONNECTIONS: usize = 4000;
const MAX_UNSTAKED_QUIC_CONNECTIONS: usize = 2000;
const QUIC_WAIT_FOR_CHUNK_TIMEOUT: Duration = Duration::from_secs(5);
const QUIC_COALESCE_WAIT: Duration = Duration::from_millis(10);

pub(crate) struct ShredFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ShredFetchStage {
    // updates packets received on a channel and sends them on another channel
    fn modify_packets(
        recvr: PacketBatchReceiver,
        sendr: Sender<PacketBatch>,
        bank_forks: &RwLock<BankForks>,
        shred_version: u16,
        name: &'static str,
        flags: PacketFlags,
        repair_context: Option<(&UdpSocket, &ClusterInfo)>,
        turbine_disabled: Arc<AtomicBool>,
    ) {
        const STATS_SUBMIT_CADENCE: Duration = Duration::from_secs(1);
        let mut last_updated = Instant::now();
        let mut keypair = repair_context
            .as_ref()
            .map(|(_, cluster_info)| cluster_info.keypair().clone());

        // In the case of bank_forks=None, setup to accept any slot range
        let mut root_bank = bank_forks.read().unwrap().root_bank();
        let mut last_root = 0;
        let mut last_slot = std::u64::MAX;
        let mut slots_per_epoch = 0;

        let mut stats = ShredFetchStats::default();

        for mut packet_batch in recvr {
            if last_updated.elapsed().as_millis() as u64 > DEFAULT_MS_PER_SLOT {
                last_updated = Instant::now();
                {
                    let bank_forks_r = bank_forks.read().unwrap();
                    last_root = bank_forks_r.root();
                    let working_bank = bank_forks_r.working_bank();
                    last_slot = working_bank.slot();
                    root_bank = bank_forks_r.root_bank();
                    slots_per_epoch = root_bank.get_slots_in_epoch(root_bank.epoch());
                }
                keypair = repair_context
                    .as_ref()
                    .map(|(_, cluster_info)| cluster_info.keypair().clone());
            }
            stats.shred_count += packet_batch.len();

            if let Some((udp_socket, _)) = repair_context {
                debug_assert_eq!(flags, PacketFlags::REPAIR);
                debug_assert!(keypair.is_some());
                if let Some(ref keypair) = keypair {
                    ServeRepair::handle_repair_response_pings(
                        udp_socket,
                        keypair,
                        &mut packet_batch,
                        &mut stats,
                    );
                }
            }

            // Limit shreds to 2 epochs away.
            let max_slot = last_slot + 2 * slots_per_epoch;
            let should_drop_merkle_shreds =
                |shred_slot| should_drop_merkle_shreds(shred_slot, &root_bank);
            let turbine_disabled = turbine_disabled.load(Ordering::Relaxed);
            for packet in packet_batch.iter_mut().filter(|p| !p.meta().discard()) {
                if turbine_disabled
                    || should_discard_shred(
                        packet,
                        last_root,
                        max_slot,
                        shred_version,
                        should_drop_merkle_shreds,
                        &mut stats,
                    )
                {
                    packet.meta_mut().set_discard(true);
                } else {
                    packet.meta_mut().flags.insert(flags);
                }
            }
            stats.maybe_submit(name, STATS_SUBMIT_CADENCE);
            if sendr.send(packet_batch).is_err() {
                break;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn packet_modifier(
        sockets: Vec<Arc<UdpSocket>>,
        exit: Arc<AtomicBool>,
        sender: Sender<PacketBatch>,
        recycler: PacketBatchRecycler,
        bank_forks: Arc<RwLock<BankForks>>,
        shred_version: u16,
        name: &'static str,
        flags: PacketFlags,
        repair_context: Option<(Arc<UdpSocket>, Arc<ClusterInfo>)>,
        turbine_disabled: Arc<AtomicBool>,
    ) -> (Vec<JoinHandle<()>>, JoinHandle<()>) {
        let (packet_sender, packet_receiver) = unbounded();
        let streamers = sockets
            .into_iter()
            .map(|s| {
                streamer::receiver(
                    s,
                    exit.clone(),
                    packet_sender.clone(),
                    recycler.clone(),
                    Arc::new(StreamerReceiveStats::new("packet_modifier")),
                    Duration::from_millis(1), // coalesce
                    true,
                    None,
                )
            })
            .collect();
        let modifier_hdl = Builder::new()
            .name("solTvuFetchPMod".to_string())
            .spawn(move || {
                let repair_context = repair_context
                    .as_ref()
                    .map(|(socket, cluster_info)| (socket.as_ref(), cluster_info.as_ref()));
                Self::modify_packets(
                    packet_receiver,
                    sender,
                    &bank_forks,
                    shred_version,
                    name,
                    flags,
                    repair_context,
                    turbine_disabled,
                )
            })
            .unwrap();
        (streamers, modifier_hdl)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        sockets: Vec<Arc<UdpSocket>>,
        quic_socket: UdpSocket,
        forward_sockets: Vec<Arc<UdpSocket>>,
        repair_socket: Arc<UdpSocket>,
        sender: Sender<PacketBatch>,
        shred_version: u16,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
        staked_nodes: Arc<RwLock<StakedNodes>>,
        turbine_disabled: Arc<AtomicBool>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let recycler = PacketBatchRecycler::warmed(100, 1024);

        let (mut tvu_threads, tvu_filter) = Self::packet_modifier(
            sockets,
            exit.clone(),
            sender.clone(),
            recycler.clone(),
            bank_forks.clone(),
            shred_version,
            "shred_fetch",
            PacketFlags::empty(),
            None, // repair_context
            turbine_disabled.clone(),
        );

        let (tvu_forwards_threads, fwd_thread_hdl) = Self::packet_modifier(
            forward_sockets,
            exit.clone(),
            sender.clone(),
            recycler.clone(),
            bank_forks.clone(),
            shred_version,
            "shred_fetch_tvu_forwards",
            PacketFlags::FORWARDED,
            None, // repair_context
            turbine_disabled.clone(),
        );

        let (repair_receiver, repair_handler) = Self::packet_modifier(
            vec![repair_socket.clone()],
            exit.clone(),
            sender.clone(),
            recycler,
            bank_forks.clone(),
            shred_version,
            "shred_fetch_repair",
            PacketFlags::REPAIR,
            Some((repair_socket, cluster_info.clone())),
            turbine_disabled.clone(),
        );

        tvu_threads.extend(tvu_forwards_threads.into_iter());
        tvu_threads.extend(repair_receiver.into_iter());
        tvu_threads.push(tvu_filter);
        tvu_threads.push(fwd_thread_hdl);
        tvu_threads.push(repair_handler);

        let keypair = cluster_info.keypair().clone();
        let ip_addr = cluster_info
            .my_contact_info()
            .tvu(Protocol::QUIC)
            .expect("Operator must spin up node with valid (QUIC) TVU address")
            .ip();
        let (packet_sender, packet_receiver) = unbounded();
        let (_endpoint, join_handle) = solana_streamer::quic::spawn_server(
            "quic_streamer_tvu",
            quic_socket,
            &keypair,
            ip_addr,
            packet_sender,
            exit,
            MAX_QUIC_CONNECTIONS_PER_PEER,
            staked_nodes,
            MAX_STAKED_QUIC_CONNECTIONS,
            MAX_UNSTAKED_QUIC_CONNECTIONS,
            QUIC_WAIT_FOR_CHUNK_TIMEOUT,
            QUIC_COALESCE_WAIT,
        )
        .unwrap();
        tvu_threads.push(join_handle);

        tvu_threads.push(
            Builder::new()
                .name("solTvuFetchPMod".to_string())
                .spawn(move || {
                    Self::modify_packets(
                        packet_receiver,
                        sender,
                        &bank_forks,
                        shred_version,
                        "shred_fetch_quic",
                        PacketFlags::empty(),
                        None, // repair_context
                        turbine_disabled,
                    )
                })
                .unwrap(),
        );

        Self {
            thread_hdls: tvu_threads,
        }
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[must_use]
fn should_drop_merkle_shreds(shred_slot: Slot, root_bank: &Bank) -> bool {
    check_feature_activation(
        &feature_set::keep_merkle_shreds::id(),
        shred_slot,
        root_bank,
    ) && !check_feature_activation(
        &feature_set::drop_merkle_shreds::id(),
        shred_slot,
        root_bank,
    )
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_ledger::{
            blockstore::MAX_DATA_SHREDS_PER_SLOT,
            shred::{ReedSolomonCache, Shred, ShredFlags},
        },
        solana_sdk::packet::Packet,
    };

    #[test]
    fn test_data_code_same_index() {
        solana_logger::setup();
        let mut packet = Packet::default();
        let mut stats = ShredFetchStats::default();

        let slot = 2;
        let shred_version = 45189;
        let shred = Shred::new_from_data(
            slot,
            3,   // shred index
            1,   // parent offset
            &[], // data
            ShredFlags::LAST_SHRED_IN_SLOT,
            0, // reference_tick
            shred_version,
            3, // fec_set_index
        );
        shred.copy_to_packet(&mut packet);

        let last_root = 0;
        let last_slot = 100;
        let slots_per_epoch = 10;
        let max_slot = last_slot + 2 * slots_per_epoch;
        assert!(!should_discard_shred(
            &packet,
            last_root,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));
        let coding = solana_ledger::shred::Shredder::generate_coding_shreds(
            &[shred],
            3, // next_code_index
            &ReedSolomonCache::default(),
        );
        coding[0].copy_to_packet(&mut packet);
        assert!(!should_discard_shred(
            &packet,
            last_root,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));
    }

    #[test]
    fn test_shred_filter() {
        solana_logger::setup();
        let mut packet = Packet::default();
        let mut stats = ShredFetchStats::default();
        let last_root = 0;
        let last_slot = 100;
        let slots_per_epoch = 10;
        let shred_version = 59445;
        let max_slot = last_slot + 2 * slots_per_epoch;

        // packet size is 0, so cannot get index
        assert!(should_discard_shred(
            &packet,
            last_root,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));
        assert_eq!(stats.index_overrun, 1);
        let shred = Shred::new_from_data(
            2,   // slot
            3,   // index
            1,   // parent_offset
            &[], // data
            ShredFlags::LAST_SHRED_IN_SLOT,
            0, // reference_tick
            shred_version,
            0, // fec_set_index
        );
        shred.copy_to_packet(&mut packet);

        // rejected slot is 2, root is 3
        assert!(should_discard_shred(
            &packet,
            3,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));
        assert_eq!(stats.slot_out_of_range, 1);

        assert!(should_discard_shred(
            &packet,
            last_root,
            max_slot,
            345,       // shred_version
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));
        assert_eq!(stats.shred_version_mismatch, 1);

        // Accepted for 1,3
        assert!(!should_discard_shred(
            &packet,
            last_root,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));

        let shred = Shred::new_from_data(
            1_000_000,
            3,
            0,
            &[],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0,
        );
        shred.copy_to_packet(&mut packet);

        // Slot 1 million is too high
        assert!(should_discard_shred(
            &packet,
            last_root,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));

        let index = MAX_DATA_SHREDS_PER_SLOT as u32;
        let shred = Shred::new_from_data(5, index, 0, &[], ShredFlags::LAST_SHRED_IN_SLOT, 0, 0, 0);
        shred.copy_to_packet(&mut packet);
        assert!(should_discard_shred(
            &packet,
            last_root,
            max_slot,
            shred_version,
            |_| false, // should_drop_merkle_shreds
            &mut stats,
        ));
    }
}
