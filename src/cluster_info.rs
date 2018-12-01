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
use bincode::{deserialize, serialize};
use bloom::Bloom;
use contact_info::ContactInfo;
use counter::Counter;
use crds_gossip::CrdsGossip;
use crds_gossip_error::CrdsGossipError;
use crds_gossip_pull::CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
use crds_value::{CrdsValue, CrdsValueLabel, LeaderId};
use ledger::LedgerWindow;
use log::Level;
use netutil::{bind_in_range, bind_to, find_available_port_in_range, multi_bind_in_range};
use packet::{to_blob, Blob, SharedBlob, BLOB_SIZE};
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use result::Result;
use rpc::RPC_PORT;
use signature::{Keypair, KeypairUtil, Signable, Signature};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::{duration_as_ms, timestamp};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::{BlobReceiver, BlobSender};
use window::{SharedWindow, WindowIndex};

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
    keypair: Arc<Keypair>,
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
#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
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
                    " ncp: {:20} | {}{}\n \
                     tpu: {:20} |\n \
                     rpc: {:20} |\n",
                    node.ncp.to_string(),
                    node.id,
                    if node.id == leader_id {
                        " <==== leader"
                    } else {
                        ""
                    },
                    node.tpu.to_string(),
                    node.rpc.to_string()
                )
            }).collect();

        format!(
            " NodeInfo.contact_info     | Node identifier\n\
             ---------------------------+------------------\n\
             {}\n \
             Nodes: {}",
            nodes.join(""),
            nodes.len()
        )
    }

    pub fn set_leader(&mut self, key: Pubkey) -> () {
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
        self.ncp_peers().len() + 1
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

    pub fn ncp_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me)
            .filter(|x| ContactInfo::is_valid_address(&x.ncp))
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
            .filter(|x| x.id != me && x.id != self.leader_id())
            .filter(|x| ContactInfo::is_valid_address(&x.tvu))
            .cloned()
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
            (s.my_data().clone(), s.tvu_peers())
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
            }).collect();
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
                    }).collect();
                send_errs_for_blob
            }).collect()
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
        let valid: Vec<_> = self.ncp_peers();
        if valid.is_empty() {
            Err(ClusterInfoError::NoPeers)?;
        }
        let n = thread_rng().gen::<usize>() % valid.len();
        let addr = valid[n].ncp; // send the request to the peer's gossip port
        let req = Protocol::RequestWindowIndex(self.my_data().clone(), ix);
        let out = serialize(&req)?;
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
                    .map(|peer_info| (peer, filter, peer_info.ncp, self_info))
            }).collect();
        pr.into_iter()
            .map(|(peer, filter, ncp, self_info)| {
                self.gossip.mark_pull_request_creation_time(peer, now);
                (ncp, Protocol::PullRequest(filter, self_info))
            }).collect()
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
                    .map(|p| p.ncp)
            }).map(|peer| (peer, Protocol::PushMessage(self_id, msgs.clone())))
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
            }).and_then(|x| x.contact_info())
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
            }).unwrap()
    }
    fn run_window_request(
        from: &NodeInfo,
        from_addr: &SocketAddr,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        me: &NodeInfo,
        leader_id: Pubkey,
        ix: u64,
    ) -> Vec<SharedBlob> {
        let pos = (ix as usize) % window.read().unwrap().len();
        if let Some(ref mut blob) = &mut window.write().unwrap()[pos].data {
            let mut wblob = blob.write().unwrap();
            let blob_ix = wblob.index().expect("run_window_request index");
            if blob_ix == ix {
                let num_retransmits = wblob.meta.num_retransmits;
                wblob.meta.num_retransmits += 1;
                // Setting the sender id to the requester id
                // prevents the requester from retransmitting this response
                // to other peers
                let mut sender_id = from.id;

                // Allow retransmission of this response if the node
                // is the leader and the number of repair requests equals
                // a power of two
                if leader_id == me.id && (num_retransmits == 0 || num_retransmits.is_power_of_two())
                {
                    sender_id = me.id
                }

                let out = SharedBlob::default();

                // copy to avoid doing IO inside the lock
                {
                    let mut outblob = out.write().unwrap();
                    let sz = wblob.meta.size;
                    outblob.meta.size = sz;
                    outblob.data[..sz].copy_from_slice(&wblob.data[..sz]);
                    outblob.meta.set_addr(from_addr);
                    outblob.set_id(&sender_id).expect("blob set_id");
                }
                inc_new_counter_info!("cluster_info-window-request-pass", 1);

                return vec![out];
            } else {
                inc_new_counter_info!("cluster_info-window-request-outside", 1);
                trace!(
                    "requested ix {} != blob_ix {}, outside window!",
                    ix,
                    blob_ix
                );
                // falls through to checking window_ledger
            }
        }

        if let Some(ledger_window) = ledger_window {
            if let Ok(entry) = ledger_window.get_entry(ix) {
                inc_new_counter_info!("cluster_info-window-request-ledger", 1);

                let out = entry.to_blob(
                    Some(ix),
                    Some(me.id), // causes retransmission if I'm the leader
                    Some(from_addr),
                );

                return vec![out];
            }
        }

        inc_new_counter_info!("cluster_info-window-request-fail", 1);
        trace!(
            "{}: failed RequestWindowIndex {} {} {}",
            me.id,
            from.id,
            ix,
            pos,
        );

        vec![]
    }

    //TODO we should first coalesce all the requests
    fn handle_blob(
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        blob: &Blob,
    ) -> Vec<SharedBlob> {
        deserialize(&blob.data[..blob.meta.size])
            .into_iter()
            .flat_map(|request| {
                ClusterInfo::handle_protocol(obj, &blob.meta.addr(), request, window, ledger_window)
            }).collect()
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
            if from.ncp.ip().is_unspecified() {
                inc_new_counter_info!("cluster_info-window-request-updates-unspec-ncp", 1);
                from.ncp = *from_addr;
            }
            inc_new_counter_info!("cluster_info-pull_request-rsp", len);
            to_blob(rsp, from.ncp).ok().into_iter().collect()
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
                    to_blob(rsp, ci.ncp).ok()
                }).into_iter()
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
        ix: u64,
        from_addr: &SocketAddr,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
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
        let leader_id = me.read().unwrap().leader_id();
        let my_info = me.read().unwrap().my_data().clone();
        inc_new_counter_info!("cluster_info-window-request-recv", 1);
        trace!(
            "{}: received RequestWindowIndex {} {} ",
            self_id,
            from.id,
            ix,
        );
        let res = Self::run_window_request(
            &from,
            &from_addr,
            &window,
            ledger_window,
            &my_info,
            leader_id,
            ix,
        );
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
        request: Protocol,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
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
                Self::handle_request_window_index(me, &from, ix, from_addr, window, ledger_window)
            }
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
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
            let mut resp = Self::handle_blob(obj, window, ledger_window, &req.read().unwrap());
            resps.append(&mut resp);
        }
        response_sender.send(resps)?;
        Ok(())
    }
    pub fn listen(
        me: Arc<RwLock<Self>>,
        window: SharedWindow,
        ledger_path: Option<&str>,
        requests_receiver: BlobReceiver,
        response_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let mut ledger_window = ledger_path.map(|p| LedgerWindow::open(p).unwrap());

        Builder::new()
            .name("solana-listen".to_string())
            .spawn(move || loop {
                let e = Self::run_listen(
                    &me,
                    &window,
                    &mut ledger_window.as_mut(),
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
            }).unwrap()
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
    pub replicate: Vec<UdpSocket>,
    pub transaction: Vec<UdpSocket>,
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
        let transaction = UdpSocket::bind("127.0.0.1:0").unwrap();
        let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
        let replicate = UdpSocket::bind("127.0.0.1:0").unwrap();
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
            replicate.local_addr().unwrap(),
            transaction.local_addr().unwrap(),
            storage.local_addr().unwrap(),
            rpc_addr,
            rpc_pubsub_addr,
            timestamp(),
        );
        Node {
            info,
            sockets: Sockets {
                gossip,
                replicate: vec![replicate],
                transaction: vec![transaction],
                broadcast,
                repair,
                retransmit,
            },
        }
    }
    pub fn new_with_external_ip(pubkey: Pubkey, ncp: &SocketAddr) -> Node {
        fn bind() -> (u16, UdpSocket) {
            bind_in_range(FULLNODE_PORT_RANGE).expect("Failed to bind")
        };

        let (gossip_port, gossip) = if ncp.port() != 0 {
            (ncp.port(), bind_to(ncp.port(), false).expect("ncp bind"))
        } else {
            bind()
        };

        let (replicate_port, replicate_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 8).expect("tvu multi_bind");

        let (transaction_port, transaction_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 32).expect("tpu multi_bind");

        let (_, repair) = bind();
        let (_, broadcast) = bind();
        let (_, retransmit) = bind();
        let (storage_port, _) = bind();

        let info = NodeInfo::new(
            pubkey,
            SocketAddr::new(ncp.ip(), gossip_port),
            SocketAddr::new(ncp.ip(), replicate_port),
            SocketAddr::new(ncp.ip(), transaction_port),
            SocketAddr::new(ncp.ip(), storage_port),
            SocketAddr::new(ncp.ip(), RPC_PORT),
            SocketAddr::new(ncp.ip(), RPC_PORT + 1),
            0,
        );
        trace!("new NodeInfo: {:?}", info);

        Node {
            info,
            sockets: Sockets {
                gossip,
                replicate: replicate_sockets,
                transaction: transaction_sockets,
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
    use crds_value::CrdsValueLabel;
    use entry::Entry;
    use ledger::{get_tmp_ledger_path, LedgerWindow, LedgerWriter};
    use logger;
    use packet::SharedBlob;
    use result::Error;
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::hash::{hash, Hash};
    use std::fs::remove_dir_all;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::{Arc, RwLock};
    use window::default_window;

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

        let ncp = socketaddr!([127, 0, 0, 1], 1234);
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            ncp,
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
            socketaddr!([127, 0, 0, 1], 1238),
            socketaddr!([127, 0, 0, 1], 1239),
            0,
        );
        cluster_info.insert_info(nxt.clone());
        let rv = cluster_info.window_index_request(0).unwrap();
        assert_eq!(nxt.ncp, ncp);
        assert_eq!(rv.0, nxt.ncp);

        let ncp2 = socketaddr!([127, 0, 0, 2], 1234);
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            ncp2,
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
            if rv.0 == ncp {
                one = true;
            }
            if rv.0 == ncp2 {
                two = true;
            }
        }
        assert!(one && two);
    }

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request() {
        logger::setup();
        let window = Arc::new(RwLock::new(default_window()));
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
        let leader_id = me.id;
        let rv = ClusterInfo::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            leader_id,
            0,
        );
        assert!(rv.is_empty());
        let out = SharedBlob::default();
        out.write().unwrap().meta.size = 200;
        window.write().unwrap()[0].data = Some(out);
        let rv = ClusterInfo::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            leader_id,
            0,
        );
        assert!(!rv.is_empty());
        let v = rv[0].clone();
        //test we copied the blob
        assert_eq!(v.read().unwrap().meta.size, 200);
        let len = window.read().unwrap().len() as u64;
        let rv = ClusterInfo::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            leader_id,
            len,
        );
        assert!(rv.is_empty());

        fn tmp_ledger(name: &str) -> String {
            let path = get_tmp_ledger_path(name);

            let mut writer = LedgerWriter::open(&path, true).unwrap();
            let zero = Hash::default();
            let one = hash(&zero.as_ref());
            writer
                .write_entries(
                    &vec![
                        Entry::new_tick(&zero, 0, &zero),
                        Entry::new_tick(&one, 0, &one),
                    ].to_vec(),
                ).unwrap();
            path
        }

        let ledger_path = tmp_ledger("run_window_request");
        let mut ledger_window = LedgerWindow::open(&ledger_path).unwrap();

        let rv = ClusterInfo::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut Some(&mut ledger_window),
            &me,
            leader_id,
            1,
        );
        assert!(!rv.is_empty());

        remove_dir_all(ledger_path).unwrap();
    }

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request_with_backoff() {
        let window = Arc::new(RwLock::new(default_window()));

        let me = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let leader_id = me.id;

        let mock_peer = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));

        // Simulate handling a repair request from mock_peer
        let rv = ClusterInfo::run_window_request(
            &mock_peer,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            leader_id,
            0,
        );
        assert!(rv.is_empty());
        let blob = SharedBlob::default();
        let blob_size = 200;
        blob.write().unwrap().meta.size = blob_size;
        window.write().unwrap()[0].data = Some(blob);

        let num_requests: u32 = 64;
        for i in 0..num_requests {
            let shared_blob = ClusterInfo::run_window_request(
                &mock_peer,
                &socketaddr_any!(),
                &window,
                &mut None,
                &me,
                leader_id,
                0,
            )[0].clone();
            let blob = shared_blob.read().unwrap();
            // Test we copied the blob
            assert_eq!(blob.meta.size, blob_size);

            let id = if i == 0 || i.is_power_of_two() {
                me.id
            } else {
                mock_peer.id
            };
            assert_eq!(blob.id().unwrap(), id);
        }
    }

    #[test]
    fn test_default_leader() {
        logger::setup();
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
        assert!(node.sockets.replicate.len() > 1);
        for tx_socket in node.sockets.replicate.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert!(node.sockets.transaction.len() > 1);
        for tx_socket in node.sockets.transaction.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert_eq!(node.sockets.repair.local_addr().unwrap().ip(), ip);

        assert!(node.sockets.gossip.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.gossip.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
        let tx_port = node.sockets.replicate[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.replicate.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        let tx_port = node.sockets.transaction[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.transaction.iter() {
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
        assert!(node.sockets.replicate.len() > 1);
        for tx_socket in node.sockets.replicate.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert!(node.sockets.transaction.len() > 1);
        for tx_socket in node.sockets.transaction.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().ip(), ip);
        }
        assert_eq!(node.sockets.repair.local_addr().unwrap().ip(), ip);

        assert_eq!(node.sockets.gossip.local_addr().unwrap().port(), 8050);
        let tx_port = node.sockets.replicate[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.replicate.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        let tx_port = node.sockets.transaction[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.transaction.iter() {
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
