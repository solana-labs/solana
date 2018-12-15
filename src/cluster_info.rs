//! The `cluster_info` module defines a data structure that is shared by all the nodes in the network over
//! a gossip control plane.  The goal is to share small bits of off-chain information and detect and
//! repair partitions.
//!
//! This CRDT only supports a very limited set of types.  A map of Pubkey -> Versioned Struct.
//! The last version is always picked during an update.
//!
//! The network is arranged in layers:
//!
//! * layer 0 - Leader.
//! * layer 1 - As many nodes as we can fit
//! * layer 2 - Everyone else, if layer 1 is `2^10`, layer 2 should be able to fit `2^20` number of nodes.
//!
//! Bank needs to provide an interface for us to query the stake weight
use crate::bloom::Bloom;
use crate::contact_info::ContactInfo;
use crate::counter::Counter;
use crate::crds_gossip::CrdsGossip;
use crate::crds_gossip_error::CrdsGossipError;
use crate::crds_gossip_pull::CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
use crate::crds_value::{CrdsValue, CrdsValueLabel, LeaderId};
use crate::db_ledger::{DbLedger, LedgerColumnFamily, MetaCf, DEFAULT_SLOT_HEIGHT};
use crate::packet::{to_blob, Blob, SharedBlob, BLOB_SIZE};
use crate::result::Result;
use crate::rpc::RPC_PORT;
use crate::streamer::{BlobReceiver, BlobSender};
use crate::window::{SharedWindow, WindowIndex};
use bincode::{deserialize, serialize};
use hashbrown::HashMap;
use log::Level;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana_metrics::{influxdb, submit};
use solana_netutil::{bind_in_range, bind_to, find_available_port_in_range, multi_bind_in_range};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signable, Signature};
use solana_sdk::timing::{duration_as_ms, timestamp};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};

pub type NodeInfo = ContactInfo;

pub const FULLNODE_PORT_RANGE: (u16, u16) = (8000, 10_000);

/// milliseconds we sleep for between gossip requests
const GOSSIP_SLEEP_MILLIS: u64 = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum ClusterInfoError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadNodeInfo,
    BadGossipAddress,
}

pub struct ClusterInfo {
    /// The network
    pub gossip: CrdsGossip,
    /// set the keypair that will be used to sign crds values generated. It is unset only in tests.
    pub(crate) keypair: Arc<Keypair>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PruneData {
    /// Pubkey of the node that sent this prune data
    pub pubkey: Pubkey,
    /// Pubkeys of nodes that should be pruned
    pub prunes: Vec<Pubkey>,
    /// Signature of this Prune Message
    pub signature: Signature,
    /// The Pubkey of the intended node/destination for this message
    pub destination: Pubkey,
    /// Wallclock of the node that generated this message
    pub wallclock: u64,
}

impl Signable for PruneData {
    fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    fn signable_data(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct SignData {
            pubkey: Pubkey,
            prunes: Vec<Pubkey>,
            destination: Pubkey,
            wallclock: u64,
        }
        let data = SignData {
            pubkey: self.pubkey,
            prunes: self.prunes.clone(),
            destination: self.destination,
            wallclock: self.wallclock,
        };
        serialize(&data).expect("serialize PruneData")
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature
    }
}

// TODO These messages should go through the gpu pipeline for spam filtering
#[derive(Serialize, Deserialize, Debug)]
#[allow(clippy::large_enum_variant)]
enum Protocol {
    /// Gossip protocol messages
    PullRequest(Bloom<Hash>, CrdsValue),
    PullResponse(Pubkey, Vec<CrdsValue>),
    PushMessage(Pubkey, Vec<CrdsValue>),
    PruneMessage(Pubkey, PruneData),

    /// Window protocol messages
    /// TODO: move this message to a different module
    RequestWindowIndex(NodeInfo, u64),
}

impl ClusterInfo {
    pub fn new(node_info: NodeInfo) -> Self {
        //Without a keypair, gossip will not function. Only useful for tests.
        ClusterInfo::new_with_keypair(node_info, Arc::new(Keypair::new()))
    }
    pub fn new_with_keypair(node_info: NodeInfo, keypair: Arc<Keypair>) -> Self {
        let mut me = ClusterInfo {
            gossip: CrdsGossip::default(),
            keypair,
        };
        let id = node_info.id;
        me.gossip.set_self(id);
        me.insert_info(node_info);
        me.push_self();
        me
    }
    pub fn push_self(&mut self) {
        let mut my_data = self.my_data();
        let now = timestamp();
        my_data.wallclock = now;
        let mut entry = CrdsValue::ContactInfo(my_data);
        entry.sign(&self.keypair);
        self.gossip.refresh_push_active_set();
        self.gossip.process_push_message(&[entry], now);
    }
    pub fn insert_info(&mut self, node_info: NodeInfo) {
        let mut value = CrdsValue::ContactInfo(node_info);
        value.sign(&self.keypair);
        let _ = self.gossip.crds.insert(value, timestamp());
    }
    pub fn id(&self) -> Pubkey {
        self.gossip.id
    }
    pub fn lookup(&self, id: Pubkey) -> Option<&NodeInfo> {
        let entry = CrdsValueLabel::ContactInfo(id);
        self.gossip
            .crds
            .lookup(&entry)
            .and_then(|x| x.contact_info())
    }
    pub fn my_data(&self) -> NodeInfo {
        self.lookup(self.id()).cloned().unwrap()
    }
    pub fn leader_id(&self) -> Pubkey {
        let entry = CrdsValueLabel::LeaderId(self.id());
        self.gossip
            .crds
            .lookup(&entry)
            .and_then(|v| v.leader_id())
            .map(|x| x.leader_id)
            .unwrap_or_default()
    }
    pub fn leader_data(&self) -> Option<&NodeInfo> {
        let leader_id = self.leader_id();
        if leader_id == Pubkey::default() {
            return None;
        }
        self.lookup(leader_id)
    }
    pub fn node_info_trace(&self) -> String {
        let leader_id = self.leader_id();
        let nodes: Vec<_> = self
            .rpc_peers()
            .into_iter()
            .map(|node| {
                format!(
                    " gossip: {:20} | {}{}\n \
                     tpu: {:20} |\n \
                     rpc: {:20} |\n",
                    node.gossip.to_string(),
                    node.id,
                    if node.id == leader_id {
                        " <==== leader"
                    } else {
                        ""
                    },
                    node.tpu.to_string(),
                    node.rpc.to_string()
                )
            })
            .collect();

        format!(
            " NodeInfo.contact_info     | Node identifier\n\
             ---------------------------+------------------\n\
             {}\n \
             Nodes: {}",
            nodes.join(""),
            nodes.len()
        )
    }

    pub fn set_leader(&mut self, key: Pubkey) {
        let prev = self.leader_id();
        let self_id = self.gossip.id;
        let now = timestamp();
        let leader = LeaderId::new(self_id, key, now);
        let mut entry = CrdsValue::LeaderId(leader);
        warn!("{}: LEADER_UPDATE TO {} from {}", self_id, key, prev);
        entry.sign(&self.keypair);
        self.gossip.process_push_message(&[entry], now);
    }

    pub fn purge(&mut self, now: u64) {
        self.gossip.purge(now);
    }
    pub fn convergence(&self) -> usize {
        self.gossip_peers().len() + 1
    }
    pub fn rpc_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me)
            .filter(|x| ContactInfo::is_valid_address(&x.rpc))
            .cloned()
            .collect()
    }

    pub fn gossip_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me)
            .filter(|x| ContactInfo::is_valid_address(&x.gossip))
            .cloned()
            .collect()
    }

    /// compute broadcast table
    pub fn tvu_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me)
            .filter(|x| ContactInfo::is_valid_address(&x.tvu))
            .cloned()
            .collect()
    }

    /// all peers that have a valid tvu except the leader
    pub fn retransmit_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me && x.id != self.leader_id())
            .filter(|x| ContactInfo::is_valid_address(&x.tvu))
            .cloned()
            .collect()
    }

    /// all tvu peers with valid gossip addrs
    pub fn repair_peers(&self) -> Vec<NodeInfo> {
        ClusterInfo::tvu_peers(self)
            .into_iter()
            .filter(|x| ContactInfo::is_valid_address(&x.gossip))
            .collect()
    }

    /// compute broadcast table
    pub fn tpu_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me)
            .filter(|x| ContactInfo::is_valid_address(&x.tpu))
            .cloned()
            .collect()
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn broadcast(
        contains_last_tick: bool,
        leader_id: Pubkey,
        me: &NodeInfo,
        broadcast_table: &[NodeInfo],
        window: &SharedWindow,
        s: &UdpSocket,
        transmit_index: &mut WindowIndex,
        received_index: u64,
    ) -> Result<()> {
        if broadcast_table.is_empty() {
            debug!("{}:not enough peers in cluster_info table", me.id);
            inc_new_counter_info!("cluster_info-broadcast-not_enough_peers_error", 1);
            Err(ClusterInfoError::NoPeers)?;
        }
        trace!(
            "{} transmit_index: {:?} received_index: {} broadcast_len: {}",
            me.id,
            *transmit_index,
            received_index,
            broadcast_table.len()
        );

        let old_transmit_index = transmit_index.data;

        let orders = Self::create_broadcast_orders(
            contains_last_tick,
            window,
            broadcast_table,
            transmit_index,
            received_index,
            me,
        );
        trace!("broadcast orders table {}", orders.len());

        let errs = Self::send_orders(s, orders, me, leader_id);

        for e in errs {
            if let Err(e) = &e {
                trace!("broadcast result {:?}", e);
            }
            e?;
            if transmit_index.data < received_index {
                transmit_index.data += 1;
            }
        }
        inc_new_counter_info!(
            "cluster_info-broadcast-max_idx",
            (transmit_index.data - old_transmit_index) as usize
        );
        transmit_index.coding = transmit_index.data;

        Ok(())
    }

    /// retransmit messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn retransmit(obj: &Arc<RwLock<Self>>, blob: &SharedBlob, s: &UdpSocket) -> Result<()> {
        let (me, orders): (NodeInfo, Vec<NodeInfo>) = {
            // copy to avoid locking during IO
            let s = obj.read().expect("'obj' read lock in pub fn retransmit");
            (s.my_data().clone(), s.retransmit_peers())
        };
        blob.write()
            .unwrap()
            .set_id(&me.id)
            .expect("set_id in pub fn retransmit");
        let rblob = blob.read().unwrap();
        trace!("retransmit orders {}", orders.len());
        let errs: Vec<_> = orders
            .par_iter()
            .map(|v| {
                debug!(
                    "{}: retransmit blob {} to {} {}",
                    me.id,
                    rblob.index().unwrap(),
                    v.id,
                    v.tvu,
                );
                //TODO profile this, may need multiple sockets for par_iter
                assert!(rblob.meta.size <= BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.tvu)
            })
            .collect();
        for e in errs {
            if let Err(e) = &e {
                inc_new_counter_info!("cluster_info-retransmit-send_to_error", 1, 1);
                error!("retransmit result {:?}", e);
            }
            e?;
        }
        Ok(())
    }

    fn send_orders(
        s: &UdpSocket,
        orders: Vec<(Option<SharedBlob>, Vec<&NodeInfo>)>,
        me: &NodeInfo,
        leader_id: Pubkey,
    ) -> Vec<io::Result<usize>> {
        orders
            .into_iter()
            .flat_map(|(b, vs)| {
                // only leader should be broadcasting
                assert!(vs.iter().find(|info| info.id == leader_id).is_none());
                let bl = b.unwrap();
                let blob = bl.read().unwrap();
                //TODO profile this, may need multiple sockets for par_iter
                let ids_and_tvus = if log_enabled!(Level::Trace) {
                    let v_ids = vs.iter().map(|v| v.id);
                    let tvus = vs.iter().map(|v| v.tvu);
                    let ids_and_tvus = v_ids.zip(tvus).collect();

                    trace!(
                        "{}: BROADCAST idx: {} sz: {} to {:?} coding: {}",
                        me.id,
                        blob.index().unwrap(),
                        blob.meta.size,
                        ids_and_tvus,
                        blob.is_coding()
                    );

                    ids_and_tvus
                } else {
                    vec![]
                };

                assert!(blob.meta.size <= BLOB_SIZE);
                let send_errs_for_blob: Vec<_> = vs
                    .iter()
                    .map(move |v| {
                        let e = s.send_to(&blob.data[..blob.meta.size], &v.tvu);
                        trace!(
                            "{}: done broadcast {} to {:?}",
                            me.id,
                            blob.meta.size,
                            ids_and_tvus
                        );
                        e
                    })
                    .collect();
                send_errs_for_blob
            })
            .collect()
    }

    fn create_broadcast_orders<'a>(
        contains_last_tick: bool,
        window: &SharedWindow,
        broadcast_table: &'a [NodeInfo],
        transmit_index: &mut WindowIndex,
        received_index: u64,
        me: &NodeInfo,
    ) -> Vec<(Option<SharedBlob>, Vec<&'a NodeInfo>)> {
        // enumerate all the blobs in the window, those are the indices
        // transmit them to nodes, starting from a different node.
        let mut orders = Vec::with_capacity((received_index - transmit_index.data) as usize);
        let window_l = window.read().unwrap();
        let mut br_idx = transmit_index.data as usize % broadcast_table.len();

        for idx in transmit_index.data..received_index {
            let w_idx = idx as usize % window_l.len();

            trace!(
                "{} broadcast order data w_idx {} br_idx {}",
                me.id,
                w_idx,
                br_idx
            );

            // Broadcast the last tick to everyone on the network so it doesn't get dropped
            // (Need to maximize probability the next leader in line sees this handoff tick
            // despite packet drops)
            let target = if idx == received_index - 1 && contains_last_tick {
                // If we see a tick at max_tick_height, then we know it must be the last
                // Blob in the window, at index == received_index. There cannot be an entry
                // that got sent after the last tick, guaranteed by the PohService).
                assert!(window_l[w_idx].data.is_some());
                (
                    window_l[w_idx].data.clone(),
                    broadcast_table.iter().collect(),
                )
            } else {
                (window_l[w_idx].data.clone(), vec![&broadcast_table[br_idx]])
            };

            orders.push(target);
            br_idx += 1;
            br_idx %= broadcast_table.len();
        }

        for idx in transmit_index.coding..received_index {
            let w_idx = idx as usize % window_l.len();

            // skip over empty slots
            if window_l[w_idx].coding.is_none() {
                continue;
            }

            trace!(
                "{} broadcast order coding w_idx: {} br_idx  :{}",
                me.id,
                w_idx,
                br_idx,
            );

            orders.push((
                window_l[w_idx].coding.clone(),
                vec![&broadcast_table[br_idx]],
            ));
            br_idx += 1;
            br_idx %= broadcast_table.len();
        }

        orders
    }

    pub fn window_index_request(&self, ix: u64) -> Result<(SocketAddr, Vec<u8>)> {
        // find a peer that appears to be accepting replication, as indicated
        //  by a valid tvu port location
        let valid: Vec<_> = self.repair_peers();
        if valid.is_empty() {
            Err(ClusterInfoError::NoPeers)?;
        }
        let n = thread_rng().gen::<usize>() % valid.len();
        let addr = valid[n].gossip; // send the request to the peer's gossip port
        let req = Protocol::RequestWindowIndex(self.my_data().clone(), ix);
        let out = serialize(&req)?;

        submit(
            influxdb::Point::new("cluster-info")
                .add_field("repair-ix", influxdb::Value::Integer(ix as i64))
                .to_owned(),
        );

        Ok((addr, out))
    }
    fn new_pull_requests(&mut self) -> Vec<(SocketAddr, Protocol)> {
        let now = timestamp();
        let pulls: Vec<_> = self.gossip.new_pull_request(now).ok().into_iter().collect();

        let pr: Vec<_> = pulls
            .into_iter()
            .filter_map(|(peer, filter, self_info)| {
                let peer_label = CrdsValueLabel::ContactInfo(peer);
                self.gossip
                    .crds
                    .lookup(&peer_label)
                    .and_then(|v| v.contact_info())
                    .map(|peer_info| (peer, filter, peer_info.gossip, self_info))
            })
            .collect();
        pr.into_iter()
            .map(|(peer, filter, gossip, self_info)| {
                self.gossip.mark_pull_request_creation_time(peer, now);
                (gossip, Protocol::PullRequest(filter, self_info))
            })
            .collect()
    }
    fn new_push_requests(&mut self) -> Vec<(SocketAddr, Protocol)> {
        let self_id = self.gossip.id;
        let (_, peers, msgs) = self.gossip.new_push_messages(timestamp());
        peers
            .into_iter()
            .filter_map(|p| {
                let peer_label = CrdsValueLabel::ContactInfo(p);
                self.gossip
                    .crds
                    .lookup(&peer_label)
                    .and_then(|v| v.contact_info())
                    .map(|p| p.gossip)
            })
            .map(|peer| (peer, Protocol::PushMessage(self_id, msgs.clone())))
            .collect()
    }

    fn gossip_request(&mut self) -> Vec<(SocketAddr, Protocol)> {
        let pulls: Vec<_> = self.new_pull_requests();
        let pushes: Vec<_> = self.new_push_requests();
        vec![pulls, pushes].into_iter().flat_map(|x| x).collect()
    }

    /// At random pick a node and try to get updated changes from them
    fn run_gossip(obj: &Arc<RwLock<Self>>, blob_sender: &BlobSender) -> Result<()> {
        let reqs = obj.write().unwrap().gossip_request();
        let blobs = reqs
            .into_iter()
            .filter_map(|(remote_gossip_addr, req)| to_blob(req, remote_gossip_addr).ok())
            .collect();
        blob_sender.send(blobs)?;
        Ok(())
    }

    pub fn get_gossip_top_leader(&self) -> Option<&NodeInfo> {
        let mut table = HashMap::new();
        let def = Pubkey::default();
        let cur = self
            .gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.leader_id())
            .filter(|x| x.leader_id != def);
        for v in cur {
            let cnt = table.entry(&v.leader_id).or_insert(0);
            *cnt += 1;
            trace!("leader {} {}", v.leader_id, *cnt);
        }
        let mut sorted: Vec<(&Pubkey, usize)> = table.into_iter().collect();
        for x in &sorted {
            trace!("{}: sorted leaders {} votes: {}", self.gossip.id, x.0, x.1);
        }
        sorted.sort_by_key(|a| a.1);
        let top_leader = sorted.last().map(|a| *a.0);

        top_leader
            .and_then(|x| {
                let leader_label = CrdsValueLabel::ContactInfo(x);
                self.gossip.crds.lookup(&leader_label)
            })
            .and_then(|x| x.contact_info())
    }

    /// randomly pick a node and ask them for updates asynchronously
    pub fn gossip(
        obj: Arc<RwLock<Self>>,
        blob_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solana-gossip".to_string())
            .spawn(move || {
                let mut last_push = timestamp();
                loop {
                    let start = timestamp();
                    let _ = Self::run_gossip(&obj, &blob_sender);
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    obj.write().unwrap().purge(timestamp());
                    //TODO: possibly tune this parameter
                    //we saw a deadlock passing an obj.read().unwrap().timeout into sleep
                    if start - last_push > CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS / 2 {
                        obj.write().unwrap().push_self();
                        last_push = timestamp();
                    }
                    let elapsed = timestamp() - start;
                    if GOSSIP_SLEEP_MILLIS > elapsed {
                        let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                        sleep(Duration::from_millis(time_left));
                    }
                }
            })
            .unwrap()
    }
    fn run_window_request(
        from: &NodeInfo,
        from_addr: &SocketAddr,
        db_ledger: Option<&Arc<RwLock<DbLedger>>>,
        me: &NodeInfo,
        ix: u64,
    ) -> Vec<SharedBlob> {
        if let Some(db_ledger) = db_ledger {
            let meta = {
                let r_db = db_ledger.read().unwrap();

                r_db.meta_cf
                    .get(&r_db.db, &MetaCf::key(DEFAULT_SLOT_HEIGHT))
            };

            if let Ok(Some(meta)) = meta {
                let max_slot = meta.received_slot;
                // Try to find the requested index in one of the slots
                for i in 0..=max_slot {
                    let get_result = {
                        let r_db = db_ledger.read().unwrap();
                        r_db.data_cf.get_by_slot_index(&r_db.db, i, ix)
                    };

                    if let Ok(Some(blob_data)) = get_result {
                        inc_new_counter_info!("cluster_info-window-request-ledger", 1);
                        let mut blob = Blob::new(&blob_data);
                        blob.set_index(ix).expect("set_index()");
                        blob.set_id(&me.id).expect("set_id()"); // causes retransmission if I'm the leader
                        blob.meta.set_addr(from_addr);

                        return vec![Arc::new(RwLock::new(blob))];
                    }
                }
            }
        }

        inc_new_counter_info!("cluster_info-window-request-fail", 1);
        trace!("{}: failed RequestWindowIndex {} {}", me.id, from.id, ix,);

        vec![]
    }

    //TODO we should first coalesce all the requests
    fn handle_blob(
        obj: &Arc<RwLock<Self>>,
        db_ledger: Option<&Arc<RwLock<DbLedger>>>,
        blob: &Blob,
    ) -> Vec<SharedBlob> {
        deserialize(&blob.data[..blob.meta.size])
            .into_iter()
            .flat_map(|request| {
                ClusterInfo::handle_protocol(obj, &blob.meta.addr(), db_ledger, request)
            })
            .collect()
    }

    fn handle_pull_request(
        me: &Arc<RwLock<Self>>,
        filter: Bloom<Hash>,
        caller: CrdsValue,
        from_addr: &SocketAddr,
    ) -> Vec<SharedBlob> {
        let self_id = me.read().unwrap().gossip.id;
        inc_new_counter_info!("cluster_info-pull_request", 1);
        if caller.contact_info().is_none() {
            return vec![];
        }
        let mut from = caller.contact_info().cloned().unwrap();
        if from.id == self_id {
            warn!(
                "PullRequest ignored, I'm talking to myself: me={} remoteme={}",
                self_id, from.id
            );
            inc_new_counter_info!("cluster_info-window-request-loopback", 1);
            return vec![];
        }
        let now = timestamp();
        let data = me
            .write()
            .unwrap()
            .gossip
            .process_pull_request(caller, filter, now);
        let len = data.len();
        trace!("get updates since response {}", len);
        if data.is_empty() {
            trace!("no updates me {}", self_id);
            vec![]
        } else {
            let rsp = Protocol::PullResponse(self_id, data);
            // the remote side may not know his public IP:PORT, record what he looks like to us
            //  this may or may not be correct for everybody but it's better than leaving him with
            //  an unspecified address in our table
            if from.gossip.ip().is_unspecified() {
                inc_new_counter_info!("cluster_info-window-request-updates-unspec-gossip", 1);
                from.gossip = *from_addr;
            }
            inc_new_counter_info!("cluster_info-pull_request-rsp", len);
            to_blob(rsp, from.gossip).ok().into_iter().collect()
        }
    }
    fn handle_pull_response(me: &Arc<RwLock<Self>>, from: Pubkey, data: Vec<CrdsValue>) {
        let len = data.len();
        let now = Instant::now();
        let self_id = me.read().unwrap().gossip.id;
        trace!("PullResponse me: {} len={}", self_id, len);
        me.write()
            .unwrap()
            .gossip
            .process_pull_response(from, data, timestamp());
        inc_new_counter_info!("cluster_info-pull_request_response", 1);
        inc_new_counter_info!("cluster_info-pull_request_response-size", len);

        report_time_spent("ReceiveUpdates", &now.elapsed(), &format!(" len: {}", len));
    }
    fn handle_push_message(
        me: &Arc<RwLock<Self>>,
        from: Pubkey,
        data: &[CrdsValue],
    ) -> Vec<SharedBlob> {
        let self_id = me.read().unwrap().gossip.id;
        inc_new_counter_info!("cluster_info-push_message", 1);
        let prunes: Vec<_> = me
            .write()
            .unwrap()
            .gossip
            .process_push_message(&data, timestamp());
        if !prunes.is_empty() {
            inc_new_counter_info!("cluster_info-push_message-prunes", prunes.len());
            let ci = me.read().unwrap().lookup(from).cloned();
            let pushes: Vec<_> = me.write().unwrap().new_push_requests();
            inc_new_counter_info!("cluster_info-push_message-pushes", pushes.len());
            let mut rsp: Vec<_> = ci
                .and_then(|ci| {
                    let mut prune_msg = PruneData {
                        pubkey: self_id,
                        prunes,
                        signature: Signature::default(),
                        destination: from,
                        wallclock: timestamp(),
                    };
                    prune_msg.sign(&me.read().unwrap().keypair);
                    let rsp = Protocol::PruneMessage(self_id, prune_msg);
                    to_blob(rsp, ci.gossip).ok()
                })
                .into_iter()
                .collect();
            let mut blobs: Vec<_> = pushes
                .into_iter()
                .filter_map(|(remote_gossip_addr, req)| to_blob(req, remote_gossip_addr).ok())
                .collect();
            rsp.append(&mut blobs);
            rsp
        } else {
            vec![]
        }
    }
    fn handle_request_window_index(
        me: &Arc<RwLock<Self>>,
        from: &ContactInfo,
        db_ledger: Option<&Arc<RwLock<DbLedger>>>,
        ix: u64,
        from_addr: &SocketAddr,
    ) -> Vec<SharedBlob> {
        let now = Instant::now();

        //TODO this doesn't depend on cluster_info module, could be moved
        //but we are using the listen thread to service these request
        //TODO verify from is signed

        let self_id = me.read().unwrap().gossip.id;
        if from.id == me.read().unwrap().gossip.id {
            warn!(
                "{}: Ignored received RequestWindowIndex from ME {} {} ",
                self_id, from.id, ix,
            );
            inc_new_counter_info!("cluster_info-window-request-address-eq", 1);
            return vec![];
        }

        me.write().unwrap().insert_info(from.clone());
        let my_info = me.read().unwrap().my_data().clone();
        inc_new_counter_info!("cluster_info-window-request-recv", 1);
        trace!(
            "{}: received RequestWindowIndex {} {} ",
            self_id,
            from.id,
            ix,
        );
        let res = Self::run_window_request(&from, &from_addr, db_ledger, &my_info, ix);
        report_time_spent(
            "RequestWindowIndex",
            &now.elapsed(),
            &format!(" ix: {}", ix),
        );
        res
    }
    fn handle_protocol(
        me: &Arc<RwLock<Self>>,
        from_addr: &SocketAddr,
        db_ledger: Option<&Arc<RwLock<DbLedger>>>,
        request: Protocol,
    ) -> Vec<SharedBlob> {
        match request {
            // TODO verify messages faster
            Protocol::PullRequest(filter, caller) => {
                //Pulls don't need to be verified
                Self::handle_pull_request(me, filter, caller, from_addr)
            }
            Protocol::PullResponse(from, mut data) => {
                data.retain(|v| v.verify());
                Self::handle_pull_response(me, from, data);
                vec![]
            }
            Protocol::PushMessage(from, mut data) => {
                data.retain(|v| v.verify());
                Self::handle_push_message(me, from, &data)
            }
            Protocol::PruneMessage(from, data) => {
                if data.verify() {
                    inc_new_counter_info!("cluster_info-prune_message", 1);
                    inc_new_counter_info!("cluster_info-prune_message-size", data.prunes.len());
                    match me.write().unwrap().gossip.process_prune_msg(
                        from,
                        data.destination,
                        &data.prunes,
                        data.wallclock,
                        timestamp(),
                    ) {
                        Err(CrdsGossipError::PruneMessageTimeout) => {
                            inc_new_counter_info!("cluster_info-prune_message_timeout", 1)
                        }
                        Err(CrdsGossipError::BadPruneDestination) => {
                            inc_new_counter_info!("cluster_info-bad_prune_destination", 1)
                        }
                        Err(_) => (),
                        Ok(_) => (),
                    }
                }
                vec![]
            }
            Protocol::RequestWindowIndex(from, ix) => {
                Self::handle_request_window_index(me, &from, db_ledger, ix, from_addr)
            }
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        db_ledger: Option<&Arc<RwLock<DbLedger>>>,
        requests_receiver: &BlobReceiver,
        response_sender: &BlobSender,
    ) -> Result<()> {
        //TODO cache connections
        let timeout = Duration::new(1, 0);
        let mut reqs = requests_receiver.recv_timeout(timeout)?;
        while let Ok(mut more) = requests_receiver.try_recv() {
            reqs.append(&mut more);
        }
        let mut resps = Vec::new();
        for req in reqs {
            let mut resp = Self::handle_blob(obj, db_ledger, &req.read().unwrap());
            resps.append(&mut resp);
        }
        response_sender.send(resps)?;
        Ok(())
    }
    pub fn listen(
        me: Arc<RwLock<Self>>,
        db_ledger: Option<Arc<RwLock<DbLedger>>>,
        requests_receiver: BlobReceiver,
        response_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solana-listen".to_string())
            .spawn(move || loop {
                let e = Self::run_listen(
                    &me,
                    db_ledger.as_ref(),
                    &requests_receiver,
                    &response_sender,
                );
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                if e.is_err() {
                    let me = me.read().unwrap();
                    debug!(
                        "{}: run_listen timeout, table size: {}",
                        me.gossip.id,
                        me.gossip.crds.table.len()
                    );
                }
            })
            .unwrap()
    }

    pub fn spy_node() -> (NodeInfo, UdpSocket) {
        let (_, gossip_socket) = bind_in_range(FULLNODE_PORT_RANGE).unwrap();
        let pubkey = Keypair::new().pubkey();
        let daddr = socketaddr_any!();

        let node = NodeInfo::new(
            pubkey,
            daddr,
            daddr,
            daddr,
            daddr,
            daddr,
            daddr,
            timestamp(),
        );
        (node, gossip_socket)
    }
}

#[derive(Debug)]
pub struct Sockets {
    pub gossip: UdpSocket,
    pub tvu: Vec<UdpSocket>,
    pub tpu: Vec<UdpSocket>,
    pub broadcast: UdpSocket,
    pub repair: UdpSocket,
    pub retransmit: UdpSocket,
}

#[derive(Debug)]
pub struct Node {
    pub info: NodeInfo,
    pub sockets: Sockets,
}

impl Node {
    pub fn new_localhost() -> Self {
        let pubkey = Keypair::new().pubkey();
        Self::new_localhost_with_pubkey(pubkey)
    }
    pub fn new_localhost_with_pubkey(pubkey: Pubkey) -> Self {
        let tpu = UdpSocket::bind("127.0.0.1:0").unwrap();
        let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tvu = UdpSocket::bind("127.0.0.1:0").unwrap();
        let repair = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rpc_port = find_available_port_in_range((1024, 65535)).unwrap();
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), rpc_port);
        let rpc_pubsub_port = find_available_port_in_range((1024, 65535)).unwrap();
        let rpc_pubsub_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), rpc_pubsub_port);

        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        let storage = UdpSocket::bind("0.0.0.0:0").unwrap();
        let info = NodeInfo::new(
            pubkey,
            gossip.local_addr().unwrap(),
            tvu.local_addr().unwrap(),
            tpu.local_addr().unwrap(),
            storage.local_addr().unwrap(),
            rpc_addr,
            rpc_pubsub_addr,
            timestamp(),
        );
        Node {
            info,
            sockets: Sockets {
                gossip,
                tvu: vec![tvu],
                tpu: vec![tpu],
                broadcast,
                repair,
                retransmit,
            },
        }
    }
    pub fn new_with_external_ip(pubkey: Pubkey, gossip_addr: &SocketAddr) -> Node {
        fn bind() -> (u16, UdpSocket) {
            bind_in_range(FULLNODE_PORT_RANGE).expect("Failed to bind")
        };

        let (gossip_port, gossip) = if gossip_addr.port() != 0 {
            (
                gossip_addr.port(),
                bind_to(gossip_addr.port(), false).expect("gossip_addr bind"),
            )
        } else {
            bind()
        };

        let (tvu_port, tvu_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 8).expect("tvu multi_bind");

        let (tpu_port, tpu_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 32).expect("tpu multi_bind");

        let (_, repair) = bind();
        let (_, broadcast) = bind();
        let (_, retransmit) = bind();
        let (storage_port, _) = bind();

        let info = NodeInfo::new(
            pubkey,
            SocketAddr::new(gossip_addr.ip(), gossip_port),
            SocketAddr::new(gossip_addr.ip(), tvu_port),
            SocketAddr::new(gossip_addr.ip(), tpu_port),
            SocketAddr::new(gossip_addr.ip(), storage_port),
            SocketAddr::new(gossip_addr.ip(), RPC_PORT),
            SocketAddr::new(gossip_addr.ip(), RPC_PORT + 1),
            0,
        );
        trace!("new NodeInfo: {:?}", info);

        Node {
            info,
            sockets: Sockets {
                gossip,
                tvu: tvu_sockets,
                tpu: tpu_sockets,
                broadcast,
                repair,
                retransmit,
            },
        }
    }
}

fn report_time_spent(label: &str, time: &Duration, extra: &str) {
    let count = duration_as_ms(time);
    if count > 5 {
        info!("{} took: {} ms {}", label, count, extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crds_value::CrdsValueLabel;
    use crate::db_ledger::DbLedger;
    use crate::ledger::get_tmp_ledger_path;

    use crate::packet::BLOB_HEADER_SIZE;
    use crate::result::Error;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_cluster_spy_gossip() {
        //check that gossip doesn't try to push to invalid addresses
        let node = Node::new_localhost();
        let (spy, _) = ClusterInfo::spy_node();
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(node.info)));
        cluster_info.write().unwrap().insert_info(spy);
        cluster_info
            .write()
            .unwrap()
            .gossip
            .refresh_push_active_set();
        let reqs = cluster_info.write().unwrap().gossip_request();
        //assert none of the addrs are invalid.
        reqs.iter().all(|(addr, _)| {
            let res = ContactInfo::is_valid_address(addr);
            assert!(res);
            res
        });
    }

    #[test]
    fn test_cluster_info_new() {
        let d = NodeInfo::new_localhost(Keypair::new().pubkey(), timestamp());
        let cluster_info = ClusterInfo::new(d.clone());
        assert_eq!(d.id, cluster_info.my_data().id);
    }

    #[test]
    fn insert_info_test() {
        let d = NodeInfo::new_localhost(Keypair::new().pubkey(), timestamp());
        let mut cluster_info = ClusterInfo::new(d);
        let d = NodeInfo::new_localhost(Keypair::new().pubkey(), timestamp());
        let label = CrdsValueLabel::ContactInfo(d.id);
        cluster_info.insert_info(d);
        assert!(cluster_info.gossip.crds.lookup(&label).is_some());
    }
    #[test]
    fn window_index_request() {
        let me = NodeInfo::new_localhost(Keypair::new().pubkey(), timestamp());
        let mut cluster_info = ClusterInfo::new(me);
        let rv = cluster_info.window_index_request(0);
        assert_matches!(rv, Err(Error::ClusterInfoError(ClusterInfoError::NoPeers)));

        let gossip_addr = socketaddr!([127, 0, 0, 1], 1234);
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            gossip_addr,
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
            socketaddr!([127, 0, 0, 1], 1238),
            socketaddr!([127, 0, 0, 1], 1239),
            0,
        );
        cluster_info.insert_info(nxt.clone());
        let rv = cluster_info.window_index_request(0).unwrap();
        assert_eq!(nxt.gossip, gossip_addr);
        assert_eq!(rv.0, nxt.gossip);

        let gossip_addr2 = socketaddr!([127, 0, 0, 2], 1234);
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            gossip_addr2,
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
            socketaddr!([127, 0, 0, 1], 1238),
            socketaddr!([127, 0, 0, 1], 1239),
            0,
        );
        cluster_info.insert_info(nxt);
        let mut one = false;
        let mut two = false;
        while !one || !two {
            //this randomly picks an option, so eventually it should pick both
            let rv = cluster_info.window_index_request(0).unwrap();
            if rv.0 == gossip_addr {
                one = true;
            }
            if rv.0 == gossip_addr2 {
                two = true;
            }
        }
        assert!(one && two);
    }

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path("run_window_request");
        {
            let db_ledger = Arc::new(RwLock::new(DbLedger::open(&ledger_path).unwrap()));
            let me = NodeInfo::new(
                Keypair::new().pubkey(),
                socketaddr!("127.0.0.1:1234"),
                socketaddr!("127.0.0.1:1235"),
                socketaddr!("127.0.0.1:1236"),
                socketaddr!("127.0.0.1:1237"),
                socketaddr!("127.0.0.1:1238"),
                socketaddr!("127.0.0.1:1239"),
                0,
            );
            let rv =
                ClusterInfo::run_window_request(&me, &socketaddr_any!(), Some(&db_ledger), &me, 0);
            assert!(rv.is_empty());
            let data_size = 1;
            let blob = SharedBlob::default();
            {
                let mut w_blob = blob.write().unwrap();
                w_blob.set_size(data_size);
                w_blob.set_index(1).expect("set_index()");
                w_blob.set_slot(2).expect("set_slot()");
                w_blob.meta.size = data_size + BLOB_HEADER_SIZE;
            }

            {
                let mut w_ledger = db_ledger.write().unwrap();
                w_ledger
                    .write_shared_blobs(vec![&blob])
                    .expect("Expect successful ledger write");
            }

            let rv =
                ClusterInfo::run_window_request(&me, &socketaddr_any!(), Some(&db_ledger), &me, 1);
            assert!(!rv.is_empty());
            let v = rv[0].clone();
            assert_eq!(v.read().unwrap().index().unwrap(), 1);
            assert_eq!(v.read().unwrap().slot().unwrap(), 2);
            assert_eq!(v.read().unwrap().meta.size, BLOB_HEADER_SIZE + data_size);
        }

        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_default_leader() {
        solana_logger::setup();
        let node_info = NodeInfo::new_localhost(Keypair::new().pubkey(), 0);
        let mut cluster_info = ClusterInfo::new(node_info);
        let network_entry_point = NodeInfo::new_entry_point(&socketaddr!("127.0.0.1:1239"));
        cluster_info.insert_info(network_entry_point);
        assert!(cluster_info.leader_data().is_none());
    }

    #[test]
    fn new_with_external_ip_test_random() {
        let ip = Ipv4Addr::from(0);
        let node = Node::new_with_external_ip(Keypair::new().pubkey(), &socketaddr!(ip, 0));
        assert_eq!(node.sockets.gossip.local_addr().unwrap().ip(), ip);
        assert!(node.sockets.tvu.len() > 1);
        for tx_socket in node.sockets.tvu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert!(node.sockets.tpu.len() > 1);
        for tx_socket in node.sockets.tpu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert_eq!(node.sockets.repair.local_addr().unwrap().ip(), ip);

        assert!(node.sockets.gossip.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.gossip.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
        let tx_port = node.sockets.tvu[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.tvu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        let tx_port = node.sockets.tpu[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.tpu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        assert!(node.sockets.repair.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.repair.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
    }

    #[test]
    fn new_with_external_ip_test_gossip() {
        let ip = IpAddr::V4(Ipv4Addr::from(0));
        let node = Node::new_with_external_ip(Keypair::new().pubkey(), &socketaddr!(0, 8050));
        assert_eq!(node.sockets.gossip.local_addr().unwrap().ip(), ip);
        assert!(node.sockets.tvu.len() > 1);
        for tx_socket in node.sockets.tvu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert!(node.sockets.tpu.len() > 1);
        for tx_socket in node.sockets.tpu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert_eq!(node.sockets.repair.local_addr().unwrap().ip(), ip);

        assert_eq!(node.sockets.gossip.local_addr().unwrap().port(), 8050);
        let tx_port = node.sockets.tvu[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.tvu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        let tx_port = node.sockets.tpu[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.tpu.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        assert!(node.sockets.repair.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.repair.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
    }

    //test that all cluster_info objects only generate signed messages
    //when constructed with keypairs
    #[test]
    fn test_gossip_signature_verification() {
        //create new cluster info, leader, and peer
        let keypair = Keypair::new();
        let peer_keypair = Keypair::new();
        let leader_keypair = Keypair::new();
        let node_info = NodeInfo::new_localhost(keypair.pubkey(), 0);
        let leader = NodeInfo::new_localhost(leader_keypair.pubkey(), 0);
        let peer = NodeInfo::new_localhost(peer_keypair.pubkey(), 0);
        let mut cluster_info = ClusterInfo::new_with_keypair(node_info.clone(), Arc::new(keypair));
        cluster_info.set_leader(leader.id);
        cluster_info.insert_info(peer.clone());
        //check that all types of gossip messages are signed correctly
        let (_, _, vals) = cluster_info.gossip.new_push_messages(timestamp());
        // there should be some pushes ready
        assert!(vals.len() > 0);
        vals.par_iter().for_each(|v| assert!(v.verify()));

        let (_, _, val) = cluster_info
            .gossip
            .new_pull_request(timestamp())
            .ok()
            .unwrap();
        assert!(val.verify());
    }
}
