use crate::packet::limited_deserialize;
use crate::streamer::{PacketReceiver, PacketSender};
use crate::{
    cluster_info::{ClusterInfo, ClusterInfoError},
    contact_info::ContactInfo,
    packet::Packet,
    result::{Error, Result},
};
use bincode::serialize;
use rand::{thread_rng, Rng};
use solana_ledger::blockstore::Blockstore;
use solana_measure::thread_mem_usage;
use solana_metrics::{datapoint_debug, inc_new_counter_debug};
use solana_perf::packet::{Packets, PacketsRecycler};
use solana_sdk::{
    clock::Slot,
    signature::{Keypair, KeypairUtil},
    timing::duration_as_ms,
};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::{Builder, JoinHandle},
    time::{Duration, Instant},
};

/// the number of slots to respond with when responding to `Orphan` requests
pub const MAX_ORPHAN_REPAIR_RESPONSES: usize = 10;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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

/// Window protocol messages
#[derive(Serialize, Deserialize, Debug)]
enum RepairProtocol {
    WindowIndex(ContactInfo, u64, u64),
    HighestWindowIndex(ContactInfo, u64, u64),
    Orphan(ContactInfo, u64),
}

#[derive(Clone)]
pub struct ServeRepair {
    /// set the keypair that will be used to sign repair responses
    keypair: Arc<Keypair>,
    my_info: ContactInfo,
    cluster_info: Arc<RwLock<ClusterInfo>>,
}

impl ServeRepair {
    /// Without a valid keypair gossip will not function. Only useful for tests.
    pub fn new_with_invalid_keypair(contact_info: ContactInfo) -> Self {
        Self::new(Arc::new(RwLock::new(
            ClusterInfo::new_with_invalid_keypair(contact_info),
        )))
    }

    pub fn new(cluster_info: Arc<RwLock<ClusterInfo>>) -> Self {
        let (keypair, my_info) = {
            let r_cluster_info = cluster_info.read().unwrap();
            (r_cluster_info.keypair.clone(), r_cluster_info.my_data())
        };
        Self {
            keypair,
            my_info,
            cluster_info,
        }
    }

    pub fn my_info(&self) -> &ContactInfo {
        &self.my_info
    }

    pub fn keypair(&self) -> &Arc<Keypair> {
        &self.keypair
    }

    fn get_repair_sender(request: &RepairProtocol) -> &ContactInfo {
        match request {
            RepairProtocol::WindowIndex(ref from, _, _) => from,
            RepairProtocol::HighestWindowIndex(ref from, _, _) => from,
            RepairProtocol::Orphan(ref from, _) => from,
        }
    }

    fn handle_repair(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        request: RepairProtocol,
    ) -> Option<Packets> {
        let now = Instant::now();

        //TODO verify from is signed
        let my_id = me.read().unwrap().keypair.pubkey();
        let from = Self::get_repair_sender(&request);
        if from.id == my_id {
            warn!(
                "{}: Ignored received repair request from ME {}",
                my_id, from.id,
            );
            inc_new_counter_debug!("serve_repair-handle-repair--eq", 1);
            return None;
        }

        let (res, label) = {
            match &request {
                RepairProtocol::WindowIndex(from, slot, shred_index) => {
                    inc_new_counter_debug!("serve_repair-request-window-index", 1);
                    (
                        Self::run_window_request(
                            recycler,
                            from,
                            &from_addr,
                            blockstore,
                            &me.read().unwrap().my_info,
                            *slot,
                            *shred_index,
                        ),
                        "WindowIndex",
                    )
                }

                RepairProtocol::HighestWindowIndex(_, slot, highest_index) => {
                    inc_new_counter_debug!("serve_repair-request-highest-window-index", 1);
                    (
                        Self::run_highest_window_request(
                            recycler,
                            &from_addr,
                            blockstore,
                            *slot,
                            *highest_index,
                        ),
                        "HighestWindowIndex",
                    )
                }
                RepairProtocol::Orphan(_, slot) => {
                    inc_new_counter_debug!("serve_repair-request-orphan", 1);
                    (
                        Self::run_orphan(
                            recycler,
                            &from_addr,
                            blockstore,
                            *slot,
                            MAX_ORPHAN_REPAIR_RESPONSES,
                        ),
                        "Orphan",
                    )
                }
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
    ) -> Result<()> {
        //TODO cache connections
        let timeout = Duration::new(1, 0);
        let reqs = requests_receiver.recv_timeout(timeout)?;

        Self::handle_packets(obj, &recycler, blockstore, reqs, response_sender);
        Ok(())
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
            .spawn(move || loop {
                let result = Self::run_listen(
                    &me,
                    &recycler,
                    blockstore.as_ref(),
                    &requests_receiver,
                    &response_sender,
                );
                match result {
                    Err(Error::RecvTimeoutError(_)) | Ok(_) => {}
                    Err(err) => info!("repair listener error: {:?}", err),
                };
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                thread_mem_usage::datapoint("solana-repair-listen");
            })
            .unwrap()
    }

    fn handle_packets(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        blockstore: Option<&Arc<Blockstore>>,
        packets: Packets,
        response_sender: &PacketSender,
    ) {
        // iter over the packets, collect pulls separately and process everything else
        let allocated = thread_mem_usage::Allocatedp::default();
        packets.packets.iter().for_each(|packet| {
            let start = allocated.get();
            let from_addr = packet.meta.addr();
            limited_deserialize(&packet.data[..packet.meta.size])
                .into_iter()
                .for_each(|request| {
                    let rsp = Self::handle_repair(me, recycler, &from_addr, blockstore, request);
                    if let Some(rsp) = rsp {
                        let _ignore_disconnect = response_sender.send(rsp);
                    }
                });
            datapoint_debug!(
                "solana-serve-repair-memory",
                ("serve_repair", (allocated.get() - start) as i64, i64),
            );
        });
    }

    fn window_index_request_bytes(&self, slot: Slot, shred_index: u64) -> Result<Vec<u8>> {
        let req = RepairProtocol::WindowIndex(self.my_info.clone(), slot, shred_index);
        let out = serialize(&req)?;
        Ok(out)
    }

    fn window_highest_index_request_bytes(&self, slot: Slot, shred_index: u64) -> Result<Vec<u8>> {
        let req = RepairProtocol::HighestWindowIndex(self.my_info.clone(), slot, shred_index);
        let out = serialize(&req)?;
        Ok(out)
    }

    fn orphan_bytes(&self, slot: Slot) -> Result<Vec<u8>> {
        let req = RepairProtocol::Orphan(self.my_info.clone(), slot);
        let out = serialize(&req)?;
        Ok(out)
    }

    pub fn repair_request(&self, repair_request: &RepairType) -> Result<(SocketAddr, Vec<u8>)> {
        // find a peer that appears to be accepting replication and has the desired slot, as indicated
        // by a valid tvu port location
        let valid: Vec<_> = self
            .cluster_info
            .read()
            .unwrap()
            .repair_peers(repair_request.slot());
        if valid.is_empty() {
            return Err(ClusterInfoError::NoPeers.into());
        }
        let n = thread_rng().gen::<usize>() % valid.len();
        let addr = valid[n].serve_repair; // send the request to the peer's serve_repair port
        let out = self.map_repair_request(repair_request)?;

        Ok((addr, out))
    }

    pub fn map_repair_request(&self, repair_request: &RepairType) -> Result<Vec<u8>> {
        match repair_request {
            RepairType::Shred(slot, shred_index) => {
                datapoint_debug!(
                    "serve_repair-repair",
                    ("repair-slot", *slot, i64),
                    ("repair-ix", *shred_index, i64)
                );
                Ok(self.window_index_request_bytes(*slot, *shred_index)?)
            }
            RepairType::HighestShred(slot, shred_index) => {
                datapoint_debug!(
                    "serve_repair-repair_highest",
                    ("repair-highest-slot", *slot, i64),
                    ("repair-highest-ix", *shred_index, i64)
                );
                Ok(self.window_highest_index_request_bytes(*slot, *shred_index)?)
            }
            RepairType::Orphan(slot) => {
                datapoint_debug!("serve_repair-repair_orphan", ("repair-orphan", *slot, i64));
                Ok(self.orphan_bytes(*slot)?)
            }
        }
    }

    fn run_window_request(
        recycler: &PacketsRecycler,
        from: &ContactInfo,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        me: &ContactInfo,
        slot: Slot,
        shred_index: u64,
    ) -> Option<Packets> {
        if let Some(blockstore) = blockstore {
            // Try to find the requested index in one of the slots
            let packet = Self::get_data_shred_as_packet(blockstore, slot, shred_index, from_addr);

            if let Ok(Some(packet)) = packet {
                inc_new_counter_debug!("serve_repair-window-request-ledger", 1);
                return Some(Packets::new_with_recycler_data(
                    recycler,
                    "run_window_request",
                    vec![packet],
                ));
            }
        }

        inc_new_counter_debug!("serve_repair-window-request-fail", 1);
        trace!(
            "{}: failed WindowIndex {} {} {}",
            me.id,
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
    ) -> Option<Packets> {
        let blockstore = blockstore?;
        // Try to find the requested index in one of the slots
        let meta = blockstore.meta(slot).ok()??;
        if meta.received > highest_index {
            // meta.received must be at least 1 by this point
            let packet =
                Self::get_data_shred_as_packet(blockstore, slot, meta.received - 1, from_addr)
                    .ok()??;
            return Some(Packets::new_with_recycler_data(
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
    ) -> Option<Packets> {
        let mut res = Packets::new_with_recycler(recycler.clone(), 64, "run_orphan");
        if let Some(blockstore) = blockstore {
            // Try to find the next "n" parent slots of the input slot
            while let Ok(Some(meta)) = blockstore.meta(slot) {
                if meta.received == 0 {
                    break;
                }
                let packet =
                    Self::get_data_shred_as_packet(blockstore, slot, meta.received - 1, from_addr);
                if let Ok(Some(packet)) = packet {
                    res.packets.push(packet);
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

    fn get_data_shred_as_packet(
        blockstore: &Arc<Blockstore>,
        slot: Slot,
        shred_index: u64,
        dest: &SocketAddr,
    ) -> Result<Option<Packet>> {
        let data = blockstore.get_data_shred(slot, shred_index)?;
        Ok(data.map(|data| {
            let mut packet = Packet::default();
            packet.meta.size = data.len();
            packet.meta.set_addr(dest);
            packet.data.copy_from_slice(&data);
            packet
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Error;
    use solana_ledger::get_tmp_ledger_path;
    use solana_ledger::{
        blockstore::make_many_slot_entries,
        blockstore_processor::fill_blockstore_slot_with_ticks,
        shred::{
            max_ticks_per_n_shreds, CodingShredHeader, DataShredHeader, Shred, ShredCommonHeader,
        },
    };
    use solana_sdk::{hash::Hash, pubkey::Pubkey, timing::timestamp};

    /// test run_window_requestwindow requests respond with the right shred, and do not overrun
    #[test]
    fn run_highest_window_request() {
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
            );
            assert!(rv.is_none());

            let _ = fill_blockstore_slot_with_ticks(
                &blockstore,
                max_ticks_per_n_shreds(1) + 1,
                2,
                1,
                Hash::default(),
            );

            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                2,
                1,
            );
            let rv: Vec<Shred> = rv
                .expect("packets")
                .packets
                .into_iter()
                .filter_map(|b| Shred::new_from_serialized_shred(b.data.to_vec()).ok())
                .collect();
            assert!(!rv.is_empty());
            let index = blockstore.meta(2).unwrap().unwrap().received - 1;
            assert_eq!(rv[0].index(), index as u32);
            assert_eq!(rv[0].slot(), 2);

            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                2,
                index + 1,
            );
            assert!(rv.is_none());
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    /// test window requests respond with the right shred, and do not overrun
    #[test]
    fn run_window_request() {
        let recycler = PacketsRecycler::default();
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let me = ContactInfo::new(
                &Pubkey::new_rand(),
                socketaddr!("127.0.0.1:1234"),
                socketaddr!("127.0.0.1:1235"),
                socketaddr!("127.0.0.1:1236"),
                socketaddr!("127.0.0.1:1237"),
                socketaddr!("127.0.0.1:1238"),
                socketaddr!("127.0.0.1:1239"),
                socketaddr!("127.0.0.1:1240"),
                socketaddr!("127.0.0.1:1241"),
                socketaddr!("127.0.0.1:1242"),
                socketaddr!("127.0.0.1:1243"),
                0,
            );
            let rv = ServeRepair::run_window_request(
                &recycler,
                &me,
                &socketaddr_any!(),
                Some(&blockstore),
                &me,
                0,
                0,
            );
            assert!(rv.is_none());
            let mut common_header = ShredCommonHeader::default();
            common_header.slot = 2;
            common_header.index = 1;
            let mut data_header = DataShredHeader::default();
            data_header.parent_offset = 1;
            let shred_info = Shred::new_empty_from_header(
                common_header,
                data_header,
                CodingShredHeader::default(),
            );

            blockstore
                .insert_shreds(vec![shred_info], None, false)
                .expect("Expect successful ledger write");

            let rv = ServeRepair::run_window_request(
                &recycler,
                &me,
                &socketaddr_any!(),
                Some(&blockstore),
                &me,
                2,
                1,
            );
            assert!(!rv.is_none());
            let rv: Vec<Shred> = rv
                .expect("packets")
                .packets
                .into_iter()
                .filter_map(|b| Shred::new_from_serialized_shred(b.data.to_vec()).ok())
                .collect();
            assert_eq!(rv[0].index(), 1);
            assert_eq!(rv[0].slot(), 2);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn window_index_request() {
        let me = ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(me)));
        let serve_repair = ServeRepair::new(cluster_info.clone());
        let rv = serve_repair.repair_request(&RepairType::Shred(0, 0));
        assert_matches!(rv, Err(Error::ClusterInfoError(ClusterInfoError::NoPeers)));

        let serve_repair_addr = socketaddr!([127, 0, 0, 1], 1243);
        let nxt = ContactInfo::new(
            &Pubkey::new_rand(),
            socketaddr!([127, 0, 0, 1], 1234),
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
            socketaddr!([127, 0, 0, 1], 1238),
            socketaddr!([127, 0, 0, 1], 1239),
            socketaddr!([127, 0, 0, 1], 1240),
            socketaddr!([127, 0, 0, 1], 1241),
            socketaddr!([127, 0, 0, 1], 1242),
            serve_repair_addr,
            0,
        );
        cluster_info.write().unwrap().insert_info(nxt.clone());
        let rv = serve_repair
            .repair_request(&RepairType::Shred(0, 0))
            .unwrap();
        assert_eq!(nxt.serve_repair, serve_repair_addr);
        assert_eq!(rv.0, nxt.serve_repair);

        let serve_repair_addr2 = socketaddr!([127, 0, 0, 2], 1243);
        let nxt = ContactInfo::new(
            &Pubkey::new_rand(),
            socketaddr!([127, 0, 0, 1], 1234),
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
            socketaddr!([127, 0, 0, 1], 1238),
            socketaddr!([127, 0, 0, 1], 1239),
            socketaddr!([127, 0, 0, 1], 1240),
            socketaddr!([127, 0, 0, 1], 1241),
            socketaddr!([127, 0, 0, 1], 1242),
            serve_repair_addr2,
            0,
        );
        cluster_info.write().unwrap().insert_info(nxt);
        let mut one = false;
        let mut two = false;
        while !one || !two {
            //this randomly picks an option, so eventually it should pick both
            let rv = serve_repair
                .repair_request(&RepairType::Shred(0, 0))
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
    fn run_orphan() {
        solana_logger::setup();
        let recycler = PacketsRecycler::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let rv =
                ServeRepair::run_orphan(&recycler, &socketaddr_any!(), Some(&blockstore), 2, 0);
            assert!(rv.is_none());

            // Create slots 1, 2, 3 with 5 shreds apiece
            let (shreds, _) = make_many_slot_entries(1, 3, 5);

            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");

            // We don't have slot 4, so we don't know how to service this requeset
            let rv =
                ServeRepair::run_orphan(&recycler, &socketaddr_any!(), Some(&blockstore), 4, 5);
            assert!(rv.is_none());

            // For slot 3, we should return the highest shreds from slots 3, 2, 1 respectively
            // for this request
            let rv: Vec<_> =
                ServeRepair::run_orphan(&recycler, &socketaddr_any!(), Some(&blockstore), 3, 5)
                    .expect("run_orphan packets")
                    .packets
                    .iter()
                    .map(|b| b.clone())
                    .collect();
            let expected: Vec<_> = (1..=3)
                .rev()
                .map(|slot| {
                    let index = blockstore.meta(slot).unwrap().unwrap().received - 1;
                    ServeRepair::get_data_shred_as_packet(
                        &blockstore,
                        slot,
                        index,
                        &socketaddr_any!(),
                    )
                    .unwrap()
                    .unwrap()
                })
                .collect();
            assert_eq!(rv, expected)
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
