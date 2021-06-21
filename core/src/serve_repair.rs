use crate::{
    cluster_slots::ClusterSlots,
    repair_response,
    repair_service::{OutstandingRepairs, RepairStats},
    request_response::RequestResponse,
    result::{Error, Result},
};
use bincode::serialize;
use rand::distributions::{Distribution, WeightedIndex};
use solana_gossip::{
    cluster_info::{ClusterInfo, ClusterInfoError},
    contact_info::ContactInfo,
    weighted_shuffle::weighted_best,
};
use solana_ledger::{
    blockstore::Blockstore,
    shred::{Nonce, Shred},
};
use solana_measure::measure::Measure;
use solana_metrics::inc_new_counter_debug;
use solana_perf::packet::{limited_deserialize, Packets, PacketsRecycler};
use solana_sdk::{clock::Slot, pubkey::Pubkey, timing::duration_as_ms};
use solana_streamer::streamer::{PacketReceiver, PacketSender};
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::{Builder, JoinHandle},
    time::{Duration, Instant},
};

/// the number of slots to respond with when responding to `Orphan` requests
pub const MAX_ORPHAN_REPAIR_RESPONSES: usize = 10;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum RepairType {
    Orphan(Slot),
    HighestShred(Slot, u64),
    Shred(Slot, u64),
}

impl RepairType {
    pub fn slot(&self) -> Slot {
        match self {
            RepairType::Orphan(slot) => *slot,
            RepairType::HighestShred(slot, _) => *slot,
            RepairType::Shred(slot, _) => *slot,
        }
    }
}

impl RequestResponse for RepairType {
    type Response = Shred;
    fn num_expected_responses(&self) -> u32 {
        match self {
            RepairType::Orphan(_) => (MAX_ORPHAN_REPAIR_RESPONSES + 1) as u32, // run_orphan uses <= MAX_ORPHAN_REPAIR_RESPONSES
            RepairType::HighestShred(_, _) => 1,
            RepairType::Shred(_, _) => 1,
        }
    }
    fn verify_response(&self, response_shred: &Shred) -> bool {
        match self {
            RepairType::Orphan(slot) => response_shred.slot() <= *slot,
            RepairType::HighestShred(slot, index) => {
                response_shred.slot() as u64 == *slot && response_shred.index() as u64 >= *index
            }
            RepairType::Shred(slot, index) => {
                response_shred.slot() as u64 == *slot && response_shred.index() as u64 == *index
            }
        }
    }
}

#[derive(Default)]
pub struct ServeRepairStats {
    pub total_packets: usize,
    pub dropped_packets: usize,
    pub processed: usize,
    pub self_repair: usize,
    pub window_index: usize,
    pub highest_window_index: usize,
    pub orphan: usize,
}

/// Window protocol messages
#[derive(Serialize, Deserialize, Debug)]
pub enum RepairProtocol {
    WindowIndex(ContactInfo, u64, u64),
    HighestWindowIndex(ContactInfo, u64, u64),
    Orphan(ContactInfo, u64),
    WindowIndexWithNonce(ContactInfo, u64, u64, Nonce),
    HighestWindowIndexWithNonce(ContactInfo, u64, u64, Nonce),
    OrphanWithNonce(ContactInfo, u64, Nonce),
}

#[derive(Clone)]
pub struct ServeRepair {
    cluster_info: Arc<ClusterInfo>,
}

type RepairCache = HashMap<Slot, (Vec<ContactInfo>, WeightedIndex<u64>)>;

impl ServeRepair {
    /// Without a valid keypair gossip will not function. Only useful for tests.
    pub fn new_with_invalid_keypair(contact_info: ContactInfo) -> Self {
        Self::new(Arc::new(ClusterInfo::new_with_invalid_keypair(
            contact_info,
        )))
    }

    pub fn new(cluster_info: Arc<ClusterInfo>) -> Self {
        Self { cluster_info }
    }

    fn my_info(&self) -> ContactInfo {
        self.cluster_info.my_contact_info()
    }

    pub(crate) fn my_id(&self) -> Pubkey {
        self.cluster_info.id()
    }

    fn get_repair_sender(request: &RepairProtocol) -> &ContactInfo {
        match request {
            RepairProtocol::WindowIndex(ref from, _, _) => from,
            RepairProtocol::HighestWindowIndex(ref from, _, _) => from,
            RepairProtocol::Orphan(ref from, _) => from,
            RepairProtocol::WindowIndexWithNonce(ref from, _, _, _) => from,
            RepairProtocol::HighestWindowIndexWithNonce(ref from, _, _, _) => from,
            RepairProtocol::OrphanWithNonce(ref from, _, _) => from,
        }
    }

    fn handle_repair(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        request: RepairProtocol,
        stats: &mut ServeRepairStats,
    ) -> Option<Packets> {
        let now = Instant::now();

        let my_id = me.read().unwrap().my_id();
        //TODO: verify `from` is signed
        let from = Self::get_repair_sender(&request);
        if from.id == my_id {
            stats.self_repair += 1;
            return None;
        }

        let (res, label) = {
            match &request {
                RepairProtocol::WindowIndexWithNonce(_, slot, shred_index, nonce) => {
                    stats.window_index += 1;
                    (
                        Self::run_window_request(
                            recycler,
                            from,
                            from_addr,
                            blockstore,
                            &my_id,
                            *slot,
                            *shred_index,
                            *nonce,
                        ),
                        "WindowIndexWithNonce",
                    )
                }
                RepairProtocol::HighestWindowIndexWithNonce(_, slot, highest_index, nonce) => {
                    stats.highest_window_index += 1;
                    (
                        Self::run_highest_window_request(
                            recycler,
                            from_addr,
                            blockstore,
                            *slot,
                            *highest_index,
                            *nonce,
                        ),
                        "HighestWindowIndexWithNonce",
                    )
                }
                RepairProtocol::OrphanWithNonce(_, slot, nonce) => {
                    stats.orphan += 1;
                    (
                        Self::run_orphan(
                            recycler,
                            from_addr,
                            blockstore,
                            *slot,
                            MAX_ORPHAN_REPAIR_RESPONSES,
                            *nonce,
                        ),
                        "OrphanWithNonce",
                    )
                }
                _ => (None, "Unsupported repair type"),
            }
        };

        trace!("{}: received repair request: {:?}", my_id, request);
        Self::report_time_spent(label, &now.elapsed(), "");
        res
    }

    fn report_time_spent(label: &str, time: &Duration, extra: &str) {
        let count = duration_as_ms(time);
        if count > 5 {
            info!("{} took: {} ms {}", label, count, extra);
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        blockstore: Option<&Arc<Blockstore>>,
        requests_receiver: &PacketReceiver,
        response_sender: &PacketSender,
        stats: &mut ServeRepairStats,
        max_packets: &mut usize,
    ) -> Result<()> {
        //TODO cache connections
        let timeout = Duration::new(1, 0);
        let mut reqs_v = vec![requests_receiver.recv_timeout(timeout)?];
        let mut total_packets = reqs_v[0].packets.len();

        let mut dropped_packets = 0;
        while let Ok(more) = requests_receiver.try_recv() {
            total_packets += more.packets.len();
            if total_packets < *max_packets {
                // Drop the rest in the channel in case of dos
                reqs_v.push(more);
            } else {
                dropped_packets += more.packets.len();
            }
        }

        stats.dropped_packets += dropped_packets;
        stats.total_packets += total_packets;

        let mut time = Measure::start("repair::handle_packets");
        for reqs in reqs_v {
            Self::handle_packets(obj, recycler, blockstore, reqs, response_sender, stats);
        }
        time.stop();
        if total_packets >= *max_packets {
            if time.as_ms() > 1000 {
                *max_packets = (*max_packets * 9) / 10;
            } else {
                *max_packets = (*max_packets * 10) / 9;
            }
        }
        Ok(())
    }

    fn report_reset_stats(me: &Arc<RwLock<Self>>, stats: &mut ServeRepairStats) {
        if stats.self_repair > 0 {
            let my_id = me.read().unwrap().cluster_info.id();
            warn!(
                "{}: Ignored received repair requests from ME: {}",
                my_id, stats.self_repair,
            );
            inc_new_counter_debug!("serve_repair-handle-repair--eq", stats.self_repair);
        }

        inc_new_counter_info!("serve_repair-total_packets", stats.total_packets);
        inc_new_counter_info!("serve_repair-dropped_packets", stats.dropped_packets);

        debug!(
            "repair_listener: total_packets: {} passed: {}",
            stats.total_packets, stats.processed
        );

        inc_new_counter_debug!("serve_repair-request-window-index", stats.window_index);
        inc_new_counter_debug!(
            "serve_repair-request-highest-window-index",
            stats.highest_window_index
        );
        inc_new_counter_debug!("serve_repair-request-orphan", stats.orphan);

        *stats = ServeRepairStats::default();
    }

    pub fn listen(
        me: Arc<RwLock<Self>>,
        blockstore: Option<Arc<Blockstore>>,
        requests_receiver: PacketReceiver,
        response_sender: PacketSender,
        exit: &Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let exit = exit.clone();
        let recycler = PacketsRecycler::default();
        Builder::new()
            .name("solana-repair-listen".to_string())
            .spawn(move || {
                let mut last_print = Instant::now();
                let mut stats = ServeRepairStats::default();
                let mut max_packets = 1024;
                loop {
                    let result = Self::run_listen(
                        &me,
                        &recycler,
                        blockstore.as_ref(),
                        &requests_receiver,
                        &response_sender,
                        &mut stats,
                        &mut max_packets,
                    );
                    match result {
                        Err(Error::RecvTimeout(_)) | Ok(_) => {}
                        Err(err) => info!("repair listener error: {:?}", err),
                    };
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    if last_print.elapsed().as_secs() > 2 {
                        Self::report_reset_stats(&me, &mut stats);
                        last_print = Instant::now();
                    }
                }
            })
            .unwrap()
    }

    fn handle_packets(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        blockstore: Option<&Arc<Blockstore>>,
        packets: Packets,
        response_sender: &PacketSender,
        stats: &mut ServeRepairStats,
    ) {
        // iter over the packets
        packets.packets.iter().for_each(|packet| {
            let from_addr = packet.meta.addr();
            limited_deserialize(&packet.data[..packet.meta.size])
                .into_iter()
                .for_each(|request| {
                    stats.processed += 1;
                    let rsp =
                        Self::handle_repair(me, recycler, &from_addr, blockstore, request, stats);
                    if let Some(rsp) = rsp {
                        let _ignore_disconnect = response_sender.send(rsp);
                    }
                });
        });
    }

    fn window_index_request_bytes(
        &self,
        slot: Slot,
        shred_index: u64,
        nonce: Nonce,
    ) -> Result<Vec<u8>> {
        let req = RepairProtocol::WindowIndexWithNonce(self.my_info(), slot, shred_index, nonce);
        let out = serialize(&req)?;
        Ok(out)
    }

    fn window_highest_index_request_bytes(
        &self,
        slot: Slot,
        shred_index: u64,
        nonce: Nonce,
    ) -> Result<Vec<u8>> {
        let req =
            RepairProtocol::HighestWindowIndexWithNonce(self.my_info(), slot, shred_index, nonce);
        let out = serialize(&req)?;
        Ok(out)
    }

    fn orphan_bytes(&self, slot: Slot, nonce: Nonce) -> Result<Vec<u8>> {
        let req = RepairProtocol::OrphanWithNonce(self.my_info(), slot, nonce);
        let out = serialize(&req)?;
        Ok(out)
    }

    pub fn repair_request(
        &self,
        cluster_slots: &ClusterSlots,
        repair_request: RepairType,
        cache: &mut RepairCache,
        repair_stats: &mut RepairStats,
        repair_validators: &Option<HashSet<Pubkey>>,
        outstanding_requests: &mut OutstandingRepairs,
    ) -> Result<(SocketAddr, Vec<u8>)> {
        // find a peer that appears to be accepting replication and has the desired slot, as indicated
        // by a valid tvu port location
        let slot = repair_request.slot();
        let (repair_peers, weighted_index) = match cache.entry(slot) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let repair_peers = self.repair_peers(repair_validators, slot);
                if repair_peers.is_empty() {
                    return Err(Error::from(ClusterInfoError::NoPeers));
                }
                let weights = cluster_slots.compute_weights(slot, &repair_peers);
                debug_assert_eq!(weights.len(), repair_peers.len());
                let weighted_index = WeightedIndex::new(weights)?;
                entry.insert((repair_peers, weighted_index))
            }
        };
        let n = weighted_index.sample(&mut rand::thread_rng());
        let addr = repair_peers[n].serve_repair; // send the request to the peer's serve_repair port
        let nonce =
            outstanding_requests.add_request(repair_request, solana_sdk::timing::timestamp());
        let repair_peer_id = repair_peers[n].id;
        let out = self.map_repair_request(&repair_request, &repair_peer_id, repair_stats, nonce)?;
        Ok((addr, out))
    }

    pub fn repair_request_duplicate_compute_best_peer(
        &self,
        slot: Slot,
        cluster_slots: &ClusterSlots,
        repair_validators: &Option<HashSet<Pubkey>>,
    ) -> Result<(Pubkey, SocketAddr)> {
        let repair_peers: Vec<_> = self.repair_peers(repair_validators, slot);
        if repair_peers.is_empty() {
            return Err(ClusterInfoError::NoPeers.into());
        }
        let weights = cluster_slots.compute_weights_exclude_noncomplete(slot, &repair_peers);
        let n = weighted_best(&weights, solana_sdk::pubkey::new_rand().to_bytes());
        Ok((repair_peers[n].id, repair_peers[n].serve_repair))
    }

    pub fn map_repair_request(
        &self,
        repair_request: &RepairType,
        repair_peer_id: &Pubkey,
        repair_stats: &mut RepairStats,
        nonce: Nonce,
    ) -> Result<Vec<u8>> {
        match repair_request {
            RepairType::Shred(slot, shred_index) => {
                repair_stats
                    .shred
                    .update(repair_peer_id, *slot, *shred_index);
                Ok(self.window_index_request_bytes(*slot, *shred_index, nonce)?)
            }
            RepairType::HighestShred(slot, shred_index) => {
                repair_stats
                    .highest_shred
                    .update(repair_peer_id, *slot, *shred_index);
                Ok(self.window_highest_index_request_bytes(*slot, *shred_index, nonce)?)
            }
            RepairType::Orphan(slot) => {
                repair_stats.orphan.update(repair_peer_id, *slot, 0);
                Ok(self.orphan_bytes(*slot, nonce)?)
            }
        }
    }

    fn repair_peers(
        &self,
        repair_validators: &Option<HashSet<Pubkey>>,
        slot: Slot,
    ) -> Vec<ContactInfo> {
        if let Some(repair_validators) = repair_validators {
            repair_validators
                .iter()
                .filter_map(|key| {
                    if *key != self.my_id() {
                        self.cluster_info.lookup_contact_info(key, |ci| ci.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            self.cluster_info.repair_peers(slot)
        }
    }

    fn run_window_request(
        recycler: &PacketsRecycler,
        from: &ContactInfo,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        my_id: &Pubkey,
        slot: Slot,
        shred_index: u64,
        nonce: Nonce,
    ) -> Option<Packets> {
        if let Some(blockstore) = blockstore {
            // Try to find the requested index in one of the slots
            let packet = repair_response::repair_response_packet(
                blockstore,
                slot,
                shred_index,
                from_addr,
                nonce,
            );

            if let Some(packet) = packet {
                inc_new_counter_debug!("serve_repair-window-request-ledger", 1);
                return Some(Packets::new_unpinned_with_recycler_data(
                    recycler,
                    "run_window_request",
                    vec![packet],
                ));
            }
        }

        inc_new_counter_debug!("serve_repair-window-request-fail", 1);
        trace!(
            "{}: failed WindowIndex {} {} {}",
            my_id,
            from.id,
            slot,
            shred_index,
        );

        None
    }

    fn run_highest_window_request(
        recycler: &PacketsRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        slot: Slot,
        highest_index: u64,
        nonce: Nonce,
    ) -> Option<Packets> {
        let blockstore = blockstore?;
        // Try to find the requested index in one of the slots
        let meta = blockstore.meta(slot).ok()??;
        if meta.received > highest_index {
            // meta.received must be at least 1 by this point
            let packet = repair_response::repair_response_packet(
                blockstore,
                slot,
                meta.received - 1,
                from_addr,
                nonce,
            )?;
            return Some(Packets::new_unpinned_with_recycler_data(
                recycler,
                "run_highest_window_request",
                vec![packet],
            ));
        }
        None
    }

    fn run_orphan(
        recycler: &PacketsRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        mut slot: Slot,
        max_responses: usize,
        nonce: Nonce,
    ) -> Option<Packets> {
        let mut res = Packets::new_unpinned_with_recycler(recycler.clone(), 64, "run_orphan");
        if let Some(blockstore) = blockstore {
            // Try to find the next "n" parent slots of the input slot
            while let Ok(Some(meta)) = blockstore.meta(slot) {
                if meta.received == 0 {
                    break;
                }
                let packet = repair_response::repair_response_packet(
                    blockstore,
                    slot,
                    meta.received - 1,
                    from_addr,
                    nonce,
                );
                if let Some(packet) = packet {
                    res.packets.push(packet);
                } else {
                    break;
                }
                if meta.is_parent_set() && res.packets.len() <= max_responses {
                    slot = meta.parent_slot;
                } else {
                    break;
                }
            }
        }
        if res.is_empty() {
            return None;
        }
        Some(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{repair_response, result::Error};
    use solana_gossip::{socketaddr, socketaddr_any};
    use solana_ledger::get_tmp_ledger_path;
    use solana_ledger::{
        blockstore::make_many_slot_entries,
        blockstore_processor::fill_blockstore_slot_with_ticks,
        shred::{max_ticks_per_n_shreds, Shred},
    };
    use solana_perf::packet::Packet;
    use solana_sdk::{hash::Hash, pubkey::Pubkey, timing::timestamp};

    #[test]
    fn test_run_highest_window_request() {
        run_highest_window_request(5, 3, 9);
    }

    /// test run_window_request responds with the right shred, and do not overrun
    fn run_highest_window_request(slot: Slot, num_slots: u64, nonce: Nonce) {
        let recycler = PacketsRecycler::default();
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                0,
                0,
                nonce,
            );
            assert!(rv.is_none());

            let _ = fill_blockstore_slot_with_ticks(
                &blockstore,
                max_ticks_per_n_shreds(1, None) + 1,
                slot,
                slot - num_slots + 1,
                Hash::default(),
            );

            let index = 1;
            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                slot,
                index,
                nonce,
            )
            .expect("packets");
            let request = RepairType::HighestShred(slot, index);
            verify_responses(&request, rv.packets.iter());

            let rv: Vec<Shred> = rv
                .packets
                .into_iter()
                .filter_map(|b| {
                    assert_eq!(repair_response::nonce(&b.data[..]).unwrap(), nonce);
                    Shred::new_from_serialized_shred(b.data.to_vec()).ok()
                })
                .collect();
            assert!(!rv.is_empty());
            let index = blockstore.meta(slot).unwrap().unwrap().received - 1;
            assert_eq!(rv[0].index(), index as u32);
            assert_eq!(rv[0].slot(), slot);

            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                slot,
                index + 1,
                nonce,
            );
            assert!(rv.is_none());
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_run_window_request() {
        run_window_request(2, 9);
    }

    /// test window requests respond with the right shred, and do not overrun
    fn run_window_request(slot: Slot, nonce: Nonce) {
        let recycler = PacketsRecycler::default();
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let me = ContactInfo {
                id: solana_sdk::pubkey::new_rand(),
                gossip: socketaddr!("127.0.0.1:1234"),
                tvu: socketaddr!("127.0.0.1:1235"),
                tvu_forwards: socketaddr!("127.0.0.1:1236"),
                repair: socketaddr!("127.0.0.1:1237"),
                tpu: socketaddr!("127.0.0.1:1238"),
                tpu_forwards: socketaddr!("127.0.0.1:1239"),
                unused: socketaddr!("127.0.0.1:1240"),
                rpc: socketaddr!("127.0.0.1:1241"),
                rpc_pubsub: socketaddr!("127.0.0.1:1242"),
                serve_repair: socketaddr!("127.0.0.1:1243"),
                wallclock: 0,
                shred_version: 0,
            };
            let rv = ServeRepair::run_window_request(
                &recycler,
                &me,
                &socketaddr_any!(),
                Some(&blockstore),
                &me.id,
                slot,
                0,
                nonce,
            );
            assert!(rv.is_none());
            let shred = Shred::new_from_data(slot, 1, 1, None, false, false, 0, 2, 0);

            blockstore
                .insert_shreds(vec![shred], None, false)
                .expect("Expect successful ledger write");

            let index = 1;
            let rv = ServeRepair::run_window_request(
                &recycler,
                &me,
                &socketaddr_any!(),
                Some(&blockstore),
                &me.id,
                slot,
                index,
                nonce,
            )
            .expect("packets");
            let request = RepairType::Shred(slot, index);
            verify_responses(&request, rv.packets.iter());
            let rv: Vec<Shred> = rv
                .packets
                .into_iter()
                .filter_map(|b| {
                    assert_eq!(repair_response::nonce(&b.data[..]).unwrap(), nonce);
                    Shred::new_from_serialized_shred(b.data.to_vec()).ok()
                })
                .collect();
            assert_eq!(rv[0].index(), 1);
            assert_eq!(rv[0].slot(), slot);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn window_index_request() {
        let cluster_slots = ClusterSlots::default();
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(me));
        let serve_repair = ServeRepair::new(cluster_info.clone());
        let mut outstanding_requests = OutstandingRepairs::default();
        let rv = serve_repair.repair_request(
            &cluster_slots,
            RepairType::Shred(0, 0),
            &mut HashMap::new(),
            &mut RepairStats::default(),
            &None,
            &mut outstanding_requests,
        );
        assert_matches!(rv, Err(Error::ClusterInfo(ClusterInfoError::NoPeers)));

        let serve_repair_addr = socketaddr!([127, 0, 0, 1], 1243);
        let nxt = ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            gossip: socketaddr!([127, 0, 0, 1], 1234),
            tvu: socketaddr!([127, 0, 0, 1], 1235),
            tvu_forwards: socketaddr!([127, 0, 0, 1], 1236),
            repair: socketaddr!([127, 0, 0, 1], 1237),
            tpu: socketaddr!([127, 0, 0, 1], 1238),
            tpu_forwards: socketaddr!([127, 0, 0, 1], 1239),
            unused: socketaddr!([127, 0, 0, 1], 1240),
            rpc: socketaddr!([127, 0, 0, 1], 1241),
            rpc_pubsub: socketaddr!([127, 0, 0, 1], 1242),
            serve_repair: serve_repair_addr,
            wallclock: 0,
            shred_version: 0,
        };
        cluster_info.insert_info(nxt.clone());
        let rv = serve_repair
            .repair_request(
                &cluster_slots,
                RepairType::Shred(0, 0),
                &mut HashMap::new(),
                &mut RepairStats::default(),
                &None,
                &mut outstanding_requests,
            )
            .unwrap();
        assert_eq!(nxt.serve_repair, serve_repair_addr);
        assert_eq!(rv.0, nxt.serve_repair);

        let serve_repair_addr2 = socketaddr!([127, 0, 0, 2], 1243);
        let nxt = ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            gossip: socketaddr!([127, 0, 0, 1], 1234),
            tvu: socketaddr!([127, 0, 0, 1], 1235),
            tvu_forwards: socketaddr!([127, 0, 0, 1], 1236),
            repair: socketaddr!([127, 0, 0, 1], 1237),
            tpu: socketaddr!([127, 0, 0, 1], 1238),
            tpu_forwards: socketaddr!([127, 0, 0, 1], 1239),
            unused: socketaddr!([127, 0, 0, 1], 1240),
            rpc: socketaddr!([127, 0, 0, 1], 1241),
            rpc_pubsub: socketaddr!([127, 0, 0, 1], 1242),
            serve_repair: serve_repair_addr2,
            wallclock: 0,
            shred_version: 0,
        };
        cluster_info.insert_info(nxt);
        let mut one = false;
        let mut two = false;
        while !one || !two {
            //this randomly picks an option, so eventually it should pick both
            let rv = serve_repair
                .repair_request(
                    &cluster_slots,
                    RepairType::Shred(0, 0),
                    &mut HashMap::new(),
                    &mut RepairStats::default(),
                    &None,
                    &mut outstanding_requests,
                )
                .unwrap();
            if rv.0 == serve_repair_addr {
                one = true;
            }
            if rv.0 == serve_repair_addr2 {
                two = true;
            }
        }
        assert!(one && two);
    }

    #[test]
    fn test_run_orphan() {
        run_orphan(2, 3, 9);
    }

    fn run_orphan(slot: Slot, num_slots: u64, nonce: Nonce) {
        solana_logger::setup();
        let recycler = PacketsRecycler::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let rv = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                slot,
                0,
                nonce,
            );
            assert!(rv.is_none());

            // Create slots [slot, slot + num_slots) with 5 shreds apiece
            let (shreds, _) = make_many_slot_entries(slot, num_slots, 5);

            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");

            // We don't have slot `slot + num_slots`, so we don't know how to service this request
            let rv = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                slot + num_slots,
                5,
                nonce,
            );
            assert!(rv.is_none());

            // For a orphan request for `slot + num_slots - 1`, we should return the highest shreds
            // from slots in the range [slot, slot + num_slots - 1]
            let rv: Vec<_> = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                slot + num_slots - 1,
                5,
                nonce,
            )
            .expect("run_orphan packets")
            .packets
            .iter()
            .cloned()
            .collect();

            // Verify responses
            let request = RepairType::Orphan(slot);
            verify_responses(&request, rv.iter());

            let expected: Vec<_> = (slot..slot + num_slots)
                .rev()
                .filter_map(|slot| {
                    let index = blockstore.meta(slot).unwrap().unwrap().received - 1;
                    repair_response::repair_response_packet(
                        &blockstore,
                        slot,
                        index,
                        &socketaddr_any!(),
                        nonce,
                    )
                })
                .collect();
            assert_eq!(rv, expected);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn run_orphan_corrupted_shred_size() {
        solana_logger::setup();
        let recycler = PacketsRecycler::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            // Create slots [1, 2] with 1 shred apiece
            let (mut shreds, _) = make_many_slot_entries(1, 2, 1);

            // Make shred for slot 1 too large
            assert_eq!(shreds[0].slot(), 1);
            assert_eq!(shreds[0].index(), 0);
            shreds[0].payload.push(10);
            shreds[0].data_header.size = shreds[0].payload.len() as u16;
            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");
            let nonce = 42;
            // Make sure repair response is corrupted
            assert!(repair_response::repair_response_packet(
                &blockstore,
                1,
                0,
                &socketaddr_any!(),
                nonce,
            )
            .is_none());

            // Orphan request for slot 2 should only return slot 1 since
            // calling `repair_response_packet` on slot 1's shred will
            // be corrupted
            let rv: Vec<_> = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                2,
                5,
                nonce,
            )
            .expect("run_orphan packets")
            .packets
            .iter()
            .cloned()
            .collect();

            // Verify responses
            let expected = vec![repair_response::repair_response_packet(
                &blockstore,
                2,
                0,
                &socketaddr_any!(),
                nonce,
            )
            .unwrap()];
            assert_eq!(rv, expected);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_repair_with_repair_validators() {
        let cluster_slots = ClusterSlots::default();
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(me.clone()));

        // Insert two peers on the network
        let contact_info2 =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let contact_info3 =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        cluster_info.insert_info(contact_info2.clone());
        cluster_info.insert_info(contact_info3.clone());
        let serve_repair = ServeRepair::new(cluster_info);

        // If:
        // 1) repair validator set doesn't exist in gossip
        // 2) repair validator set only includes our own id
        // then no repairs should be generated
        for pubkey in &[solana_sdk::pubkey::new_rand(), me.id] {
            let trusted_validators = Some(vec![*pubkey].into_iter().collect());
            assert!(serve_repair.repair_peers(&trusted_validators, 1).is_empty());
            assert!(serve_repair
                .repair_request(
                    &cluster_slots,
                    RepairType::Shred(0, 0),
                    &mut HashMap::new(),
                    &mut RepairStats::default(),
                    &trusted_validators,
                    &mut OutstandingRepairs::default(),
                )
                .is_err());
        }

        // If trusted validator exists in gossip, should return repair successfully
        let trusted_validators = Some(vec![contact_info2.id].into_iter().collect());
        let repair_peers = serve_repair.repair_peers(&trusted_validators, 1);
        assert_eq!(repair_peers.len(), 1);
        assert_eq!(repair_peers[0].id, contact_info2.id);
        assert!(serve_repair
            .repair_request(
                &cluster_slots,
                RepairType::Shred(0, 0),
                &mut HashMap::new(),
                &mut RepairStats::default(),
                &trusted_validators,
                &mut OutstandingRepairs::default(),
            )
            .is_ok());

        // Using no trusted validators should default to all
        // validator's available in gossip, excluding myself
        let repair_peers: HashSet<Pubkey> = serve_repair
            .repair_peers(&None, 1)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(repair_peers.len(), 2);
        assert!(repair_peers.contains(&contact_info2.id));
        assert!(repair_peers.contains(&contact_info3.id));
        assert!(serve_repair
            .repair_request(
                &cluster_slots,
                RepairType::Shred(0, 0),
                &mut HashMap::new(),
                &mut RepairStats::default(),
                &None,
                &mut OutstandingRepairs::default(),
            )
            .is_ok());
    }

    #[test]
    fn test_verify_response() {
        let repair = RepairType::Orphan(9);
        // Ensure new options are addded to this test
        match repair {
            RepairType::Orphan(_) => (),
            RepairType::HighestShred(_, _) => (),
            RepairType::Shred(_, _) => (),
        };

        let slot = 9;
        let index = 5;

        // Orphan
        let mut shred = Shred::new_empty_data_shred();
        shred.set_slot(slot);
        let request = RepairType::Orphan(slot);
        assert!(request.verify_response(&shred));
        shred.set_slot(slot - 1);
        assert!(request.verify_response(&shred));
        shred.set_slot(slot + 1);
        assert!(!request.verify_response(&shred));

        // HighestShred
        shred = Shred::new_empty_data_shred();
        shred.set_slot(slot);
        shred.set_index(index);
        let request = RepairType::HighestShred(slot, index as u64);
        assert!(request.verify_response(&shred));
        shred.set_index(index + 1);
        assert!(request.verify_response(&shred));
        shred.set_index(index - 1);
        assert!(!request.verify_response(&shred));
        shred.set_slot(slot - 1);
        shred.set_index(index);
        assert!(!request.verify_response(&shred));
        shred.set_slot(slot + 1);
        assert!(!request.verify_response(&shred));

        // Shred
        shred = Shred::new_empty_data_shred();
        shred.set_slot(slot);
        shred.set_index(index);
        let request = RepairType::Shred(slot, index as u64);
        assert!(request.verify_response(&shred));
        shred.set_index(index + 1);
        assert!(!request.verify_response(&shred));
        shred.set_slot(slot + 1);
        shred.set_index(index);
        assert!(!request.verify_response(&shred));
    }

    fn verify_responses<'a>(request: &RepairType, packets: impl Iterator<Item = &'a Packet>) {
        for packet in packets {
            let shred_payload = packet.data.to_vec();
            let shred = Shred::new_from_serialized_shred(shred_payload).unwrap();
            request.verify_response(&shred);
        }
    }
}
