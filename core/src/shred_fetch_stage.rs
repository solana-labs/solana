//! The `shred_fetch_stage` pulls shreds from UDP sockets and sends it to a channel.

use {
    crate::packet_hasher::PacketHasher,
    lru::LruCache,
    solana_ledger::shred::{get_shred_slot_index_type, ShredFetchStats},
    solana_perf::{
        cuda_runtime::PinnedVec,
        packet::{Packet, PacketBatchRecycler},
        recycler::Recycler,
    },
    solana_runtime::bank_forks::BankForks,
    solana_sdk::clock::{Slot, DEFAULT_MS_PER_SLOT},
    solana_streamer::streamer::{self, PacketBatchReceiver, PacketBatchSender},
    std::{
        net::UdpSocket,
        sync::{atomic::AtomicBool, mpsc::channel, Arc, RwLock},
        thread::{self, Builder, JoinHandle},
        time::Instant,
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
        p.meta.discard = true;
        if let Some((slot, _index, _shred_type)) = get_shred_slot_index_type(p, stats) {
            // Seems reasonable to limit shreds to 2 epochs away
            if slot > last_root && slot < (last_slot + 2 * slots_per_epoch) {
                // Shred filter

                let hash = packet_hasher.hash_packet(p);

                if shreds_received.get(&hash).is_none() {
                    shreds_received.put(hash, ());
                    p.meta.discard = false;
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
        sendr: PacketBatchSender,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        name: &'static str,
        modify: F,
    ) where
        F: Fn(&mut Packet),
    {
        let mut shreds_received = LruCache::new(DEFAULT_LRU_SIZE);
        let mut last_updated = Instant::now();

        // In the case of bank_forks=None, setup to accept any slot range
        let mut last_root = 0;
        let mut last_slot = std::u64::MAX;
        let mut slots_per_epoch = 0;

        let mut last_stats = Instant::now();
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
            }
            stats.shred_count += packet_batch.packets.len();
            packet_batch.packets.iter_mut().for_each(|packet| {
                Self::process_packet(
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
            if last_stats.elapsed().as_millis() > 1000 {
                datapoint_info!(
                    name,
                    ("index_overrun", stats.index_overrun, i64),
                    ("shred_count", stats.shred_count, i64),
                    ("slot_bad_deserialize", stats.slot_bad_deserialize, i64),
                    ("index_bad_deserialize", stats.index_bad_deserialize, i64),
                    ("index_out_of_bounds", stats.index_out_of_bounds, i64),
                    ("slot_out_of_range", stats.slot_out_of_range, i64),
                    ("duplicate_shred", stats.duplicate_shred, i64),
                );
                stats = ShredFetchStats::default();
                last_stats = Instant::now();
            }
            if sendr.send(packet_batch).is_err() {
                break;
            }
        }
    }

    fn packet_modifier<F>(
        sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: PacketBatchSender,
        recycler: Recycler<PinnedVec<Packet>>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        name: &'static str,
        modify: F,
    ) -> (Vec<JoinHandle<()>>, JoinHandle<()>)
    where
        F: Fn(&mut Packet) + Send + 'static,
    {
        let (packet_sender, packet_receiver) = channel();
        let streamers = sockets
            .into_iter()
            .map(|s| {
                streamer::receiver(
                    s,
                    exit,
                    packet_sender.clone(),
                    recycler.clone(),
                    "packet_modifier",
                    1,
                    true,
                )
            })
            .collect();

        let modifier_hdl = Builder::new()
            .name("solana-tvu-fetch-stage-packet-modifier".to_string())
            .spawn(move || Self::modify_packets(packet_receiver, sender, bank_forks, name, modify))
            .unwrap();
        (streamers, modifier_hdl)
    }

    pub fn new(
        sockets: Vec<Arc<UdpSocket>>,
        forward_sockets: Vec<Arc<UdpSocket>>,
        repair_socket: Arc<UdpSocket>,
        sender: &PacketBatchSender,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let recycler: PacketBatchRecycler = Recycler::warmed(100, 1024);

        let (mut tvu_threads, tvu_filter) = Self::packet_modifier(
            sockets,
            exit,
            sender.clone(),
            recycler.clone(),
            bank_forks.clone(),
            "shred_fetch",
            |_| {},
        );

        let (tvu_forwards_threads, fwd_thread_hdl) = Self::packet_modifier(
            forward_sockets,
            exit,
            sender.clone(),
            recycler.clone(),
            bank_forks.clone(),
            "shred_fetch_tvu_forwards",
            |p| p.meta.forward = true,
        );

        let (repair_receiver, repair_handler) = Self::packet_modifier(
            vec![repair_socket],
            exit,
            sender.clone(),
            recycler,
            bank_forks,
            "shred_fetch_repair",
            |p| p.meta.repair = true,
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
        assert!(!packet.meta.discard);
        let coding = solana_ledger::shred::Shredder::generate_coding_shreds(
            &[shred],
            false, // is_last_in_slot
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
        assert!(!packet.meta.discard);
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
        assert!(packet.meta.discard);
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
        assert!(packet.meta.discard);

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
        assert!(!packet.meta.discard);

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
        assert!(packet.meta.discard);

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
        assert!(packet.meta.discard);

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
        assert!(packet.meta.discard);
    }
}
