//! The `shred_fetch_stage` pulls shreds from UDP sockets and sends it to a channel.

use {
    crate::{packet_hasher::PacketHasher, serve_repair::ServeRepair},
    crossbeam_channel::{unbounded, Sender},
    lru::LruCache,
<<<<<<< HEAD
    solana_ledger::shred::{get_shred_slot_index_type, ShredFetchStats},
=======
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::shred::{should_discard_shred, ShredFetchStats},
>>>>>>> 857be1e23 (sign repair requests (#26833))
    solana_perf::packet::{Packet, PacketBatch, PacketBatchRecycler, PacketFlags},
    solana_runtime::bank_forks::BankForks,
    solana_sdk::clock::{Slot, DEFAULT_MS_PER_SLOT},
    solana_streamer::streamer::{self, PacketBatchReceiver, StreamerReceiveStats},
    std::{
        net::UdpSocket,
        sync::{atomic::AtomicBool, Arc, RwLock},
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const DEFAULT_LRU_SIZE: usize = 10_000;
pub type ShredsReceived = LruCache<u64, ()>;

pub struct ShredFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ShredFetchStage {
    fn process_packet<F>(
        p: &mut Packet,
        shreds_received: &mut ShredsReceived,
        stats: &mut ShredFetchStats,
        last_root: Slot,
        last_slot: Slot,
        slots_per_epoch: u64,
        modify: &F,
        packet_hasher: &PacketHasher,
    ) where
        F: Fn(&mut Packet),
    {
        p.meta.set_discard(true);
        if let Some((slot, _index, _shred_type)) = get_shred_slot_index_type(p, stats) {
            // Seems reasonable to limit shreds to 2 epochs away
            if slot > last_root && slot < (last_slot + 2 * slots_per_epoch) {
                // Shred filter

                let hash = packet_hasher.hash_packet(p);

                if shreds_received.get(&hash).is_none() {
                    shreds_received.put(hash, ());
                    p.meta.set_discard(false);
                    modify(p);
                } else {
                    stats.duplicate_shred += 1;
                }
            } else {
                stats.slot_out_of_range += 1;
            }
        }
    }

    // updates packets received on a channel and sends them on another channel
    fn modify_packets<F>(
        recvr: PacketBatchReceiver,
        sendr: Sender<Vec<PacketBatch>>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        name: &'static str,
<<<<<<< HEAD
        modify: F,
    ) where
        F: Fn(&mut Packet),
    {
=======
        flags: PacketFlags,
        repair_context: Option<(&UdpSocket, &ClusterInfo)>,
    ) {
>>>>>>> 857be1e23 (sign repair requests (#26833))
        const STATS_SUBMIT_CADENCE: Duration = Duration::from_secs(1);
        let mut shreds_received = LruCache::new(DEFAULT_LRU_SIZE);
        let mut last_updated = Instant::now();
        let mut keypair = repair_context
            .as_ref()
            .map(|(_, cluster_info)| cluster_info.keypair().clone());

        // In the case of bank_forks=None, setup to accept any slot range
        let mut last_root = 0;
        let mut last_slot = std::u64::MAX;
        let mut slots_per_epoch = 0;

        let mut stats = ShredFetchStats::default();
        let mut packet_hasher = PacketHasher::default();

        while let Some(mut packet_batch) = recvr.iter().next() {
            if last_updated.elapsed().as_millis() as u64 > DEFAULT_MS_PER_SLOT {
                last_updated = Instant::now();
                packet_hasher.reset();
                shreds_received.clear();
                if let Some(bank_forks) = bank_forks.as_ref() {
                    let bank_forks_r = bank_forks.read().unwrap();
                    last_root = bank_forks_r.root();
                    let working_bank = bank_forks_r.working_bank();
                    last_slot = working_bank.slot();
                    let root_bank = bank_forks_r.root_bank();
                    slots_per_epoch = root_bank.get_slots_in_epoch(root_bank.epoch());
                }
                keypair = repair_context
                    .as_ref()
                    .map(|(_, cluster_info)| cluster_info.keypair().clone());
            }
            stats.shred_count += packet_batch.len();
<<<<<<< HEAD
            packet_batch.iter_mut().for_each(|packet| {
                Self::process_packet(
=======

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
            for packet in packet_batch.iter_mut() {
                if should_discard_packet(
>>>>>>> 857be1e23 (sign repair requests (#26833))
                    packet,
                    &mut shreds_received,
                    &mut stats,
                    last_root,
                    last_slot,
                    slots_per_epoch,
                    &modify,
                    &packet_hasher,
                );
            });
            stats.maybe_submit(name, STATS_SUBMIT_CADENCE);
            if sendr.send(vec![packet_batch]).is_err() {
                break;
            }
        }
    }

    fn packet_modifier<F>(
        sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: Sender<Vec<PacketBatch>>,
        recycler: PacketBatchRecycler,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        name: &'static str,
<<<<<<< HEAD
        modify: F,
    ) -> (Vec<JoinHandle<()>>, JoinHandle<()>)
    where
        F: Fn(&mut Packet) + Send + 'static,
    {
=======
        flags: PacketFlags,
        repair_context: Option<(Arc<UdpSocket>, Arc<ClusterInfo>)>,
    ) -> (Vec<JoinHandle<()>>, JoinHandle<()>) {
>>>>>>> 857be1e23 (sign repair requests (#26833))
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
                    1,
                    true,
                    None,
                )
            })
            .collect();
        let modifier_hdl = Builder::new()
            .name("solana-tvu-fetch-stage-packet-modifier".to_string())
<<<<<<< HEAD
            .spawn(move || Self::modify_packets(packet_receiver, sender, bank_forks, name, modify))
=======
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
                )
            })
>>>>>>> 857be1e23 (sign repair requests (#26833))
            .unwrap();
        (streamers, modifier_hdl)
    }

    pub fn new(
        sockets: Vec<Arc<UdpSocket>>,
        forward_sockets: Vec<Arc<UdpSocket>>,
        repair_socket: Arc<UdpSocket>,
<<<<<<< HEAD
        sender: &Sender<Vec<PacketBatch>>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
=======
        sender: Sender<PacketBatch>,
        shred_version: u16,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
>>>>>>> 857be1e23 (sign repair requests (#26833))
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let recycler = PacketBatchRecycler::warmed(100, 1024);

        let (mut tvu_threads, tvu_filter) = Self::packet_modifier(
            sockets,
            exit,
            sender.clone(),
            recycler.clone(),
            bank_forks.clone(),
            "shred_fetch",
<<<<<<< HEAD
            |_| {},
=======
            PacketFlags::empty(),
            None, // repair_context
>>>>>>> 857be1e23 (sign repair requests (#26833))
        );

        let (tvu_forwards_threads, fwd_thread_hdl) = Self::packet_modifier(
            forward_sockets,
            exit,
            sender.clone(),
            recycler.clone(),
            bank_forks.clone(),
            "shred_fetch_tvu_forwards",
<<<<<<< HEAD
            |p| p.meta.flags.insert(PacketFlags::FORWARDED),
=======
            PacketFlags::FORWARDED,
            None, // repair_context
>>>>>>> 857be1e23 (sign repair requests (#26833))
        );

        let (repair_receiver, repair_handler) = Self::packet_modifier(
            vec![repair_socket.clone()],
            exit,
            sender.clone(),
            recycler,
            bank_forks,
            "shred_fetch_repair",
<<<<<<< HEAD
            |p| p.meta.flags.insert(PacketFlags::REPAIR),
=======
            PacketFlags::REPAIR,
            Some((repair_socket, cluster_info)),
>>>>>>> 857be1e23 (sign repair requests (#26833))
        );

        tvu_threads.extend(tvu_forwards_threads.into_iter());
        tvu_threads.extend(repair_receiver.into_iter());
        tvu_threads.push(tvu_filter);
        tvu_threads.push(fwd_thread_hdl);
        tvu_threads.push(repair_handler);

        Self {
            thread_hdls: tvu_threads,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_ledger::{blockstore::MAX_DATA_SHREDS_PER_SLOT, shred::Shred},
    };

    #[test]
    fn test_data_code_same_index() {
        solana_logger::setup();
        let mut shreds_received = LruCache::new(DEFAULT_LRU_SIZE);
        let mut packet = Packet::default();
        let mut stats = ShredFetchStats::default();

        let slot = 1;
        let shred = Shred::new_from_data(
            slot, 3,    // shred index
            0,    // parent offset
            None, // data
            true, // is_last_in_fec_set
            true, // is_last_in_slot
            0,    // reference_tick
            0,    // version
            3,    // fec_set_index
        );
        shred.copy_to_packet(&mut packet);

        let hasher = PacketHasher::default();

        let last_root = 0;
        let last_slot = 100;
        let slots_per_epoch = 10;
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(!packet.meta.discard());
        let coding = solana_ledger::shred::Shredder::generate_coding_shreds(
            &[shred],
            false, // is_last_in_slot
            3,     // next_code_index
        );
        coding[0].copy_to_packet(&mut packet);
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(!packet.meta.discard());
    }

    #[test]
    fn test_shred_filter() {
        solana_logger::setup();
        let mut shreds_received = LruCache::new(DEFAULT_LRU_SIZE);
        let mut packet = Packet::default();
        let mut stats = ShredFetchStats::default();
        let last_root = 0;
        let last_slot = 100;
        let slots_per_epoch = 10;

        let hasher = PacketHasher::default();

        // packet size is 0, so cannot get index
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert_eq!(stats.index_overrun, 1);
        assert!(packet.meta.discard());
        let shred = Shred::new_from_data(1, 3, 0, None, true, true, 0, 0, 0);
        shred.copy_to_packet(&mut packet);

        // rejected slot is 1, root is 3
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            3,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(packet.meta.discard());

        // Accepted for 1,3
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(!packet.meta.discard());

        // shreds_received should filter duplicate
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(packet.meta.discard());

        let shred = Shred::new_from_data(1_000_000, 3, 0, None, true, true, 0, 0, 0);
        shred.copy_to_packet(&mut packet);

        // Slot 1 million is too high
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(packet.meta.discard());

        let index = MAX_DATA_SHREDS_PER_SLOT as u32;
        let shred = Shred::new_from_data(5, index, 0, None, true, true, 0, 0, 0);
        shred.copy_to_packet(&mut packet);
        ShredFetchStage::process_packet(
            &mut packet,
            &mut shreds_received,
            &mut stats,
            last_root,
            last_slot,
            slots_per_epoch,
            &|_p| {},
            &hasher,
        );
        assert!(packet.meta.discard());
    }
}
