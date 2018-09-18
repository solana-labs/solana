//! The `crdt` module defines a data structure that is shared by all the nodes in the network over
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
use choose_gossip_peer_strategy::{ChooseGossipPeerStrategy, ChooseWeightedPeerStrategy};
use counter::Counter;
use hash::Hash;
use instruction::Vote;
use ledger::LedgerWindow;
use log::Level;
use netutil::{bind_in_range, bind_to, multi_bind_in_range};
use packet::{to_blob, Blob, BlobRecycler, SharedBlob, BLOB_SIZE};
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use result::{Error, Result};
use signature::{Keypair, KeypairUtil, Pubkey};
use std;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::{BlobReceiver, BlobSender};
use timing::{duration_as_ms, timestamp};
use window::{SharedWindow, WindowIndex};

pub const FULLNODE_PORT_RANGE: (u16, u16) = (8000, 10_000);

/// milliseconds we sleep for between gossip requests
const GOSSIP_SLEEP_MILLIS: u64 = 100;
const GOSSIP_PURGE_MILLIS: u64 = 15000;

/// minimum membership table size before we start purging dead nodes
const MIN_TABLE_SIZE: usize = 2;

#[macro_export]
macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: SocketAddr = $str.parse().unwrap();
        a
    }};
}
#[macro_export]
macro_rules! socketaddr_any {
    () => {
        socketaddr!(0, 0)
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum CrdtError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadNodeInfo,
    BadGossipAddress,
}

/// Structure to be replicated by the network
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContactInfo {
    /// gossip address
    pub ncp: SocketAddr,
    /// address to connect to for replication
    pub tvu: SocketAddr,
    /// address to connect to when this node is leader
    pub rpu: SocketAddr,
    /// transactions address
    pub tpu: SocketAddr,
    /// if this struture changes update this value as well
    /// Always update `NodeInfo` version too
    /// This separate version for addresses allows us to use the `Vote`
    /// as means of updating the `NodeInfo` table without touching the
    /// addresses if they haven't changed.
    pub version: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LedgerState {
    /// last verified hash that was submitted to the leader
    pub last_id: Hash,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NodeInfo {
    pub id: Pubkey,
    /// If any of the bits change, update increment this value
    pub version: u64,
    /// network addresses
    pub contact_info: ContactInfo,
    /// current leader identity
    pub leader_id: Pubkey,
    /// information about the state of the ledger
    pub ledger_state: LedgerState,
}

impl NodeInfo {
    pub fn new(
        id: Pubkey,
        ncp: SocketAddr,
        tvu: SocketAddr,
        rpu: SocketAddr,
        tpu: SocketAddr,
    ) -> Self {
        NodeInfo {
            id,
            version: 0,
            contact_info: ContactInfo {
                ncp,
                tvu,
                rpu,
                tpu,
                version: 0,
            },
            leader_id: Pubkey::default(),
            ledger_state: LedgerState {
                last_id: Hash::default(),
            },
        }
    }

    #[cfg(test)]
    /// NodeInfo with unspecified addresses for adversarial testing.
    pub fn new_unspecified() -> Self {
        let addr = socketaddr!(0, 0);
        assert!(addr.ip().is_unspecified());
        Self::new(Keypair::new().pubkey(), addr, addr, addr, addr)
    }
    #[cfg(test)]
    /// NodeInfo with multicast addresses for adversarial testing.
    pub fn new_multicast() -> Self {
        let addr = socketaddr!("224.0.1.255:1000");
        assert!(addr.ip().is_multicast());
        Self::new(Keypair::new().pubkey(), addr, addr, addr, addr)
    }
    fn next_port(addr: &SocketAddr, nxt: u16) -> SocketAddr {
        let mut nxt_addr = *addr;
        nxt_addr.set_port(addr.port() + nxt);
        nxt_addr
    }
    pub fn new_with_pubkey_socketaddr(pubkey: Pubkey, bind_addr: &SocketAddr) -> Self {
        let transactions_addr = *bind_addr;
        let gossip_addr = Self::next_port(&bind_addr, 1);
        let replicate_addr = Self::next_port(&bind_addr, 2);
        let requests_addr = Self::next_port(&bind_addr, 3);
        NodeInfo::new(
            pubkey,
            gossip_addr,
            replicate_addr,
            requests_addr,
            transactions_addr,
        )
    }
    pub fn new_with_socketaddr(bind_addr: &SocketAddr) -> Self {
        let keypair = Keypair::new();
        Self::new_with_pubkey_socketaddr(keypair.pubkey(), bind_addr)
    }
    //
    pub fn new_entry_point(gossip_addr: &SocketAddr) -> Self {
        let daddr: SocketAddr = socketaddr!("0.0.0.0:0");
        NodeInfo::new(Pubkey::default(), *gossip_addr, daddr, daddr, daddr)
    }
}

/// `Crdt` structure keeps a table of `NodeInfo` structs
/// # Properties
/// * `table` - map of public id's to versioned and signed NodeInfo structs
/// * `local` - map of public id's to what `self.update_index` `self.table` was updated
/// * `remote` - map of public id's to the `remote.update_index` was sent
/// * `update_index` - my update index
/// # Remarks
/// This implements two services, `gossip` and `listen`.
/// * `gossip` - asynchronously ask nodes to send updates
/// * `listen` - listen for requests and responses
/// No attempt to keep track of timeouts or dropped requests is made, or should be.
pub struct Crdt {
    /// table of everyone in the network
    pub table: HashMap<Pubkey, NodeInfo>,
    /// Value of my update index when entry in table was updated.
    /// Nodes will ask for updates since `update_index`, and this node
    /// should respond with all the identities that are greater then the
    /// request's `update_index` in this list
    local: HashMap<Pubkey, u64>,
    /// The value of the remote update index that I have last seen
    /// This Node will ask external nodes for updates since the value in this list
    pub remote: HashMap<Pubkey, u64>,
    /// last time the public key had sent us a message
    pub alive: HashMap<Pubkey, u64>,
    pub update_index: u64,
    pub id: Pubkey,
    /// last time we heard from anyone getting a message fro this public key
    /// these are rumers and shouldn't be trusted directly
    external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>>,
    /// TODO: Clearly not the correct implementation of this, but a temporary abstraction
    /// for testing
    pub scheduled_leaders: HashMap<u64, Pubkey>,
    // TODO: Is there a better way to do this? We didn't make this a constant because
    // we want to be able to set it in integration tests so that the tests don't time out.
    pub leader_rotation_interval: u64,
}

// TODO These messages should be signed, and go through the gpu pipeline for spam filtering
#[derive(Serialize, Deserialize, Debug)]
enum Protocol {
    /// forward your own latest data structure when requesting an update
    /// this doesn't update the `remote` update index, but it allows the
    /// recepient of this request to add knowledge of this node to the network
    /// (last update index i saw from you, my replicated data)
    RequestUpdates(u64, NodeInfo),
    //TODO might need a since?
    /// from id, form's last update index, NodeInfo
    ReceiveUpdates(Pubkey, u64, Vec<NodeInfo>, Vec<(Pubkey, u64)>),
    /// ask for a missing index
    /// (my replicated data to keep alive, missing window index)
    RequestWindowIndex(NodeInfo, u64),
}

impl Crdt {
    pub fn new(node_info: NodeInfo) -> Result<Crdt> {
        if node_info.version != 0 {
            return Err(Error::CrdtError(CrdtError::BadNodeInfo));
        }
        let mut me = Crdt {
            table: HashMap::new(),
            local: HashMap::new(),
            remote: HashMap::new(),
            alive: HashMap::new(),
            external_liveness: HashMap::new(),
            id: node_info.id,
            update_index: 1,
            scheduled_leaders: HashMap::new(),
            leader_rotation_interval: 100,
        };
        me.local.insert(node_info.id, me.update_index);
        me.table.insert(node_info.id, node_info);
        Ok(me)
    }
    pub fn my_data(&self) -> &NodeInfo {
        &self.table[&self.id]
    }
    pub fn leader_data(&self) -> Option<&NodeInfo> {
        let leader_id = self.table[&self.id].leader_id;

        // leader_id can be 0s from network entry point
        if leader_id == Pubkey::default() {
            return None;
        }

        self.table.get(&leader_id)
    }

    pub fn node_info_trace(&self) -> String {
        let leader_id = self.table[&self.id].leader_id;

        let nodes: Vec<_> = self
            .table
            .values()
            .filter(|n| Self::is_valid_address(&n.contact_info.rpu))
            .cloned()
            .map(|node| {
                format!(
                    " ncp: {:20} | {}{}\n \
                     rpu: {:20} |\n \
                     tpu: {:20} |\n",
                    node.contact_info.ncp.to_string(),
                    node.id,
                    if node.id == leader_id {
                        " <==== leader"
                    } else {
                        ""
                    },
                    node.contact_info.rpu.to_string(),
                    node.contact_info.tpu.to_string()
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
        let mut me = self.my_data().clone();
        warn!("{}: LEADER_UPDATE TO {} from {}", me.id, key, me.leader_id);
        me.leader_id = key;
        me.version += 1;
        self.insert(&me);
    }

    // TODO: Dummy leader scheduler, need to implement actual leader scheduling.
    pub fn get_scheduled_leader(&self, entry_height: u64) -> Option<Pubkey> {
        match self.scheduled_leaders.get(&entry_height) {
            Some(x) => Some(x.clone()),
            None => Some(self.my_data().leader_id),
        }
    }

    pub fn set_leader_rotation_interval(&mut self, leader_rotation_interval: u64) {
        self.leader_rotation_interval = leader_rotation_interval;
    }

    pub fn get_leader_rotation_interval(&self) -> u64 {
        self.leader_rotation_interval
    }

    // TODO: Dummy leader schedule setter, need to implement actual leader scheduling.
    pub fn set_scheduled_leader(&mut self, entry_height: u64, new_leader_id: Pubkey) -> () {
        self.scheduled_leaders.insert(entry_height, new_leader_id);
    }

    pub fn get_valid_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.table
            .values()
            .into_iter()
            .filter(|x| x.id != me)
            .filter(|x| Crdt::is_valid_address(&x.contact_info.rpu))
            .cloned()
            .collect()
    }

    pub fn get_external_liveness_entry(&self, key: &Pubkey) -> Option<&HashMap<Pubkey, u64>> {
        self.external_liveness.get(key)
    }

    pub fn insert_vote(&mut self, pubkey: &Pubkey, v: &Vote, last_id: Hash) {
        if self.table.get(pubkey).is_none() {
            warn!("{}: VOTE for unknown id: {}", self.id, pubkey);
            return;
        }
        if v.contact_info_version > self.table[pubkey].contact_info.version {
            warn!(
                "{}: VOTE for new address version from: {} ours: {} vote: {:?}",
                self.id, pubkey, self.table[pubkey].contact_info.version, v,
            );
            return;
        }
        if *pubkey == self.my_data().leader_id {
            info!("{}: LEADER_VOTED! {}", self.id, pubkey);
            inc_new_counter_info!("crdt-insert_vote-leader_voted", 1);
        }

        if v.version <= self.table[pubkey].version {
            debug!("{}: VOTE for old version: {}", self.id, pubkey);
            self.update_liveness(*pubkey);
            return;
        } else {
            let mut data = self.table[pubkey].clone();
            data.version = v.version;
            data.ledger_state.last_id = last_id;

            debug!("{}: INSERTING VOTE! for {}", self.id, data.id);
            self.update_liveness(data.id);
            self.insert(&data);
        }
    }
    pub fn insert_votes(&mut self, votes: &[(Pubkey, Vote, Hash)]) {
        inc_new_counter_info!("crdt-vote-count", votes.len());
        if !votes.is_empty() {
            info!("{}: INSERTING VOTES {}", self.id, votes.len());
        }
        for v in votes {
            self.insert_vote(&v.0, &v.1, v.2);
        }
    }

    pub fn insert(&mut self, v: &NodeInfo) -> usize {
        // TODO check that last_verified types are always increasing
        // update the peer table
        if self.table.get(&v.id).is_none() || (v.version > self.table[&v.id].version) {
            //somehow we signed a message for our own identity with a higher version than
            // we have stored ourselves
            trace!("{}: insert v.id: {} version: {}", self.id, v.id, v.version);
            if self.table.get(&v.id).is_none() {
                inc_new_counter_info!("crdt-insert-new_entry", 1, 1);
            }

            self.update_index += 1;
            let _ = self.table.insert(v.id, v.clone());
            let _ = self.local.insert(v.id, self.update_index);
            self.update_liveness(v.id);
            1
        } else {
            trace!(
                "{}: INSERT FAILED data: {} new.version: {} me.version: {}",
                self.id,
                v.id,
                v.version,
                self.table[&v.id].version
            );
            0
        }
    }

    fn update_liveness(&mut self, id: Pubkey) {
        //update the liveness table
        let now = timestamp();
        trace!("{} updating liveness {} to {}", self.id, id, now);
        *self.alive.entry(id).or_insert(now) = now;
    }
    /// purge old validators
    /// TODO: we need a robust membership protocol
    /// http://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf
    /// challenging part is that we are on a permissionless network
    pub fn purge(&mut self, now: u64) {
        if self.table.len() <= MIN_TABLE_SIZE {
            trace!("purge: skipped: table too small: {}", self.table.len());
            return;
        }
        if self.leader_data().is_none() {
            trace!("purge: skipped: no leader_data");
            return;
        }
        let leader_id = self.leader_data().unwrap().id;
        let limit = GOSSIP_PURGE_MILLIS;
        let dead_ids: Vec<Pubkey> = self
            .alive
            .iter()
            .filter_map(|(&k, v)| {
                if k != self.id && (now - v) > limit {
                    Some(k)
                } else {
                    trace!("{} purge skipped {} {} {}", self.id, k, now - v, limit);
                    None
                }
            }).collect();

        inc_new_counter_info!("crdt-purge-count", dead_ids.len());

        for id in &dead_ids {
            self.alive.remove(id);
            self.table.remove(id);
            self.remote.remove(id);
            self.local.remove(id);
            self.external_liveness.remove(id);
            info!("{}: PURGE {}", self.id, id);
            for map in self.external_liveness.values_mut() {
                map.remove(id);
            }
            if *id == leader_id {
                info!("{}: PURGE LEADER {}", self.id, id,);
                inc_new_counter_info!("crdt-purge-purged_leader", 1, 1);
                self.set_leader(Pubkey::default());
            }
        }
    }

    /// compute broadcast table
    /// # Remarks
    pub fn compute_broadcast_table(&self) -> Vec<NodeInfo> {
        let live: Vec<_> = self.alive.iter().collect();
        //thread_rng().shuffle(&mut live);
        let me = &self.table[&self.id];
        let cloned_table: Vec<NodeInfo> = live
            .iter()
            .map(|x| &self.table[x.0])
            .filter(|v| {
                if me.id == v.id {
                    //filter myself
                    false
                } else if !(Self::is_valid_address(&v.contact_info.tvu)) {
                    trace!(
                        "{}:broadcast skip not listening {} {}",
                        me.id,
                        v.id,
                        v.contact_info.tvu,
                    );
                    false
                } else {
                    trace!("{}:broadcast node {} {}", me.id, v.id, v.contact_info.tvu);
                    true
                }
            }).cloned()
            .collect();
        cloned_table
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn broadcast(
        me: &NodeInfo,
        broadcast_table: &[NodeInfo],
        window: &SharedWindow,
        s: &UdpSocket,
        transmit_index: &mut WindowIndex,
        received_index: u64,
    ) -> Result<()> {
        if broadcast_table.is_empty() {
            warn!("{}:not enough peers in crdt table", me.id);
            inc_new_counter_info!("crdt-broadcast-not_enough_peers_error", 1);
            Err(CrdtError::NoPeers)?;
        }
        trace!(
            "{} transmit_index: {:?} received_index: {} broadcast_len: {}",
            me.id,
            *transmit_index,
            received_index,
            broadcast_table.len()
        );

        let old_transmit_index = transmit_index.data;

        // enumerate all the blobs in the window, those are the indices
        // transmit them to nodes, starting from a different node
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

            orders.push((window_l[w_idx].data.clone(), &broadcast_table[br_idx]));
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

            orders.push((window_l[w_idx].coding.clone(), &broadcast_table[br_idx]));
            br_idx += 1;
            br_idx %= broadcast_table.len();
        }

        trace!("broadcast orders table {}", orders.len());
        let errs: Vec<_> = orders
            .into_iter()
            .map(|(b, v)| {
                // only leader should be broadcasting
                assert!(me.leader_id != v.id);
                let bl = b.unwrap();
                let blob = bl.read();
                //TODO profile this, may need multiple sockets for par_iter
                trace!(
                    "{}: BROADCAST idx: {} sz: {} to {},{} coding: {}",
                    me.id,
                    blob.get_index().unwrap(),
                    blob.meta.size,
                    v.id,
                    v.contact_info.tvu,
                    blob.is_coding()
                );
                assert!(blob.meta.size <= BLOB_SIZE);
                let e = s.send_to(&blob.data[..blob.meta.size], &v.contact_info.tvu);
                trace!(
                    "{}: done broadcast {} to {} {}",
                    me.id,
                    blob.meta.size,
                    v.id,
                    v.contact_info.tvu
                );
                e
            }).collect();

        trace!("broadcast results {}", errs.len());
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
            "crdt-broadcast-max_idx",
            (transmit_index.data - old_transmit_index) as usize
        );
        transmit_index.coding = transmit_index.data;

        Ok(())
    }

    /// retransmit messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn retransmit(obj: &Arc<RwLock<Self>>, blob: &SharedBlob, s: &UdpSocket) -> Result<()> {
        let (me, table): (NodeInfo, Vec<NodeInfo>) = {
            // copy to avoid locking during IO
            let s = obj.read().expect("'obj' read lock in pub fn retransmit");
            (s.my_data().clone(), s.table.values().cloned().collect())
        };
        blob.write()
            .set_id(me.id)
            .expect("set_id in pub fn retransmit");
        let rblob = blob.read();
        let orders: Vec<_> = table
            .iter()
            .filter(|v| {
                if me.id == v.id {
                    trace!("skip retransmit to self {:?}", v.id);
                    false
                } else if me.leader_id == v.id {
                    trace!("skip retransmit to leader {:?}", v.id);
                    false
                } else if !(Self::is_valid_address(&v.contact_info.tvu)) {
                    trace!(
                        "skip nodes that are not listening {:?} {}",
                        v.id,
                        v.contact_info.tvu
                    );
                    false
                } else {
                    true
                }
            }).collect();
        trace!("retransmit orders {}", orders.len());
        let errs: Vec<_> = orders
            .par_iter()
            .map(|v| {
                debug!(
                    "{}: retransmit blob {} to {} {}",
                    me.id,
                    rblob.get_index().unwrap(),
                    v.id,
                    v.contact_info.tvu,
                );
                //TODO profile this, may need multiple sockets for par_iter
                assert!(rblob.meta.size <= BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.contact_info.tvu)
            }).collect();
        for e in errs {
            if let Err(e) = &e {
                inc_new_counter_info!("crdt-retransmit-send_to_error", 1, 1);
                error!("retransmit result {:?}", e);
            }
            e?;
        }
        Ok(())
    }

    // max number of nodes that we could be converged to
    pub fn convergence(&self) -> u64 {
        let max = self.remote.values().len() as u64 + 1;
        self.remote.values().fold(max, |a, b| std::cmp::min(a, *b))
    }

    // TODO: fill in with real implmentation once staking is implemented
    fn get_stake(_id: Pubkey) -> f64 {
        1.0
    }

    fn get_updates_since(&self, v: u64) -> (Pubkey, u64, Vec<NodeInfo>) {
        //trace!("get updates since {}", v);
        let data = self
            .table
            .values()
            .filter(|x| x.id != Pubkey::default() && self.local[&x.id] > v)
            .cloned()
            .collect();
        let id = self.id;
        let ups = self.update_index;
        (id, ups, data)
    }

    pub fn valid_last_ids(&self) -> Vec<Hash> {
        self.table
            .values()
            .filter(|r| {
                r.id != Pubkey::default()
                    && (Self::is_valid_address(&r.contact_info.tpu)
                        || Self::is_valid_address(&r.contact_info.tvu))
            }).map(|x| x.ledger_state.last_id)
            .collect()
    }

    pub fn window_index_request(&self, ix: u64) -> Result<(SocketAddr, Vec<u8>)> {
        // find a peer that appears to be accepting replication, as indicated
        //  by a valid tvu port location
        let valid: Vec<_> = self
            .table
            .values()
            .filter(|r| r.id != self.id && Self::is_valid_address(&r.contact_info.tvu))
            .collect();
        if valid.is_empty() {
            Err(CrdtError::NoPeers)?;
        }
        let n = thread_rng().gen::<usize>() % valid.len();
        let addr = valid[n].contact_info.ncp; // send the request to the peer's gossip port
        let req = Protocol::RequestWindowIndex(self.my_data().clone(), ix);
        let out = serialize(&req)?;
        Ok((addr, out))
    }

    /// Create a random gossip request
    /// # Returns
    /// (A,B)
    /// * A - Address to send to
    /// * B - RequestUpdates protocol message
    fn gossip_request(&self) -> Result<(SocketAddr, Protocol)> {
        let options: Vec<_> = self
            .table
            .values()
            .filter(|v| {
                v.id != self.id
                    && !v.contact_info.ncp.ip().is_unspecified()
                    && !v.contact_info.ncp.ip().is_multicast()
            }).collect();

        let choose_peer_strategy = ChooseWeightedPeerStrategy::new(
            &self.remote,
            &self.external_liveness,
            &Self::get_stake,
        );

        let choose_peer_result = choose_peer_strategy.choose_peer(options);

        if let Err(Error::CrdtError(CrdtError::NoPeers)) = &choose_peer_result {
            trace!("crdt too small for gossip {} {}", self.id, self.table.len());
        };
        let v = choose_peer_result?;

        let remote_update_index = *self.remote.get(&v.id).unwrap_or(&0);
        let req = Protocol::RequestUpdates(remote_update_index, self.my_data().clone());
        trace!(
            "created gossip request from {} {:?} to {} {}",
            self.id,
            self.my_data(),
            v.id,
            v.contact_info.ncp
        );

        Ok((v.contact_info.ncp, req))
    }

    pub fn new_vote(&mut self, last_id: Hash) -> Result<(Vote, SocketAddr)> {
        let mut me = self.my_data().clone();
        let leader = self.leader_data().ok_or(CrdtError::NoLeader)?.clone();
        me.version += 1;
        me.ledger_state.last_id = last_id;
        let vote = Vote {
            version: me.version,
            contact_info_version: me.contact_info.version,
        };
        self.insert(&me);
        Ok((vote, leader.contact_info.tpu))
    }

    /// At random pick a node and try to get updated changes from them
    fn run_gossip(
        obj: &Arc<RwLock<Self>>,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
    ) -> Result<()> {
        //TODO we need to keep track of stakes and weight the selection by stake size
        //TODO cache sockets

        // Lock the object only to do this operation and not for any longer
        // especially not when doing the `sock.send_to`
        let (remote_gossip_addr, req) = obj
            .read()
            .expect("'obj' read lock in fn run_gossip")
            .gossip_request()?;

        // TODO this will get chatty, so we need to first ask for number of updates since
        // then only ask for specific data that we dont have
        let blob = to_blob(req, remote_gossip_addr, blob_recycler)?;
        blob_sender.send(vec![blob])?;
        Ok(())
    }
    /// TODO: This is obviously the wrong way to do this. Need to implement leader selection
    fn top_leader(&self) -> Option<Pubkey> {
        let mut table = HashMap::new();
        let def = Pubkey::default();
        let cur = self.table.values().filter(|x| x.leader_id != def);
        for v in cur {
            let cnt = table.entry(&v.leader_id).or_insert(0);
            *cnt += 1;
            trace!("leader {} {}", v.leader_id, *cnt);
        }
        let mut sorted: Vec<(&Pubkey, usize)> = table.into_iter().collect();
        for x in &sorted {
            trace!("{}: sorted leaders {} votes: {}", self.id, x.0, x.1);
        }
        sorted.sort_by_key(|a| a.1);
        sorted.last().map(|a| *a.0)
    }

    /// TODO: This is obviously the wrong way to do this. Need to implement leader selection
    /// A t-shirt for the first person to actually use this bad behavior to attack the alpha testnet
    fn update_leader(&mut self) {
        if let Some(leader_id) = self.top_leader() {
            if self.my_data().leader_id != leader_id && self.table.get(&leader_id).is_some() {
                self.set_leader(leader_id);
            }
        }
    }

    /// Apply updates that we received from the identity `from`
    /// # Arguments
    /// * `from` - identity of the sender of the updates
    /// * `update_index` - the number of updates that `from` has completed and this set of `data` represents
    /// * `data` - the update data
    fn apply_updates(
        &mut self,
        from: Pubkey,
        update_index: u64,
        data: &[NodeInfo],
        external_liveness: &[(Pubkey, u64)],
    ) {
        trace!("got updates {}", data.len());
        // TODO we need to punish/spam resist here
        // sigverify the whole update and slash anyone who sends a bad update
        let mut insert_total = 0;
        for v in data {
            insert_total += self.insert(&v);
        }
        inc_new_counter_info!("crdt-update-count", insert_total);

        for (pubkey, external_remote_index) in external_liveness {
            let remote_entry = if let Some(v) = self.remote.get(pubkey) {
                *v
            } else {
                0
            };

            if remote_entry >= *external_remote_index {
                continue;
            }

            let liveness_entry = self
                .external_liveness
                .entry(*pubkey)
                .or_insert_with(HashMap::new);
            let peer_index = *liveness_entry.entry(from).or_insert(*external_remote_index);
            if *external_remote_index > peer_index {
                liveness_entry.insert(from, *external_remote_index);
            }
        }

        *self.remote.entry(from).or_insert(update_index) = update_index;

        // Clear the remote liveness table for this node, b/c we've heard directly from them
        // so we don't need to rely on rumors
        self.external_liveness.remove(&from);
    }

    /// randomly pick a node and ask them for updates asynchronously
    pub fn gossip(
        obj: Arc<RwLock<Self>>,
        blob_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let blob_recycler = BlobRecycler::default();
        Builder::new()
            .name("solana-gossip".to_string())
            .spawn(move || loop {
                let start = timestamp();
                let _ = Self::run_gossip(&obj, &blob_sender, &blob_recycler);
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                obj.write().unwrap().purge(timestamp());
                //TODO: possibly tune this parameter
                //we saw a deadlock passing an obj.read().unwrap().timeout into sleep
                obj.write().unwrap().update_leader();
                let elapsed = timestamp() - start;
                if GOSSIP_SLEEP_MILLIS > elapsed {
                    let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                    sleep(Duration::from_millis(time_left));
                }
            }).unwrap()
    }
    fn run_window_request(
        from: &NodeInfo,
        from_addr: &SocketAddr,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        me: &NodeInfo,
        ix: u64,
        blob_recycler: &BlobRecycler,
    ) -> Option<SharedBlob> {
        let pos = (ix as usize) % window.read().unwrap().len();
        if let Some(ref mut blob) = &mut window.write().unwrap()[pos].data {
            let mut wblob = blob.write();
            let blob_ix = wblob.get_index().expect("run_window_request get_index");
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
                if me.leader_id == me.id
                    && (num_retransmits == 0 || num_retransmits.is_power_of_two())
                {
                    sender_id = me.id
                }

                let out = blob_recycler.allocate();

                // copy to avoid doing IO inside the lock
                {
                    let mut outblob = out.write();
                    let sz = wblob.meta.size;
                    outblob.meta.size = sz;
                    outblob.data[..sz].copy_from_slice(&wblob.data[..sz]);
                    outblob.meta.set_addr(from_addr);
                    outblob.set_id(sender_id).expect("blob set_id");
                }
                inc_new_counter_info!("crdt-window-request-pass", 1);

                return Some(out);
            } else {
                inc_new_counter_info!("crdt-window-request-outside", 1);
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
                inc_new_counter_info!("crdt-window-request-ledger", 1);

                let out = entry.to_blob(
                    blob_recycler,
                    Some(ix),
                    Some(me.id), // causes retransmission if I'm the leader
                    Some(from_addr),
                );

                return Some(out);
            }
        }

        inc_new_counter_info!("crdt-window-request-fail", 1);
        trace!(
            "{}: failed RequestWindowIndex {} {} {}",
            me.id,
            from.id,
            ix,
            pos,
        );

        None
    }

    //TODO we should first coalesce all the requests
    fn handle_blob(
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        blob_recycler: &BlobRecycler,
        blob: &Blob,
    ) -> Option<SharedBlob> {
        match deserialize(&blob.data[..blob.meta.size]) {
            Ok(request) => Crdt::handle_protocol(
                obj,
                &blob.meta.addr(),
                request,
                window,
                ledger_window,
                blob_recycler,
            ),
            Err(_) => {
                warn!("deserialize crdt packet failed");
                None
            }
        }
    }

    fn handle_protocol(
        me: &Arc<RwLock<Self>>,
        from_addr: &SocketAddr,
        request: Protocol,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        blob_recycler: &BlobRecycler,
    ) -> Option<SharedBlob> {
        match request {
            // TODO sigverify these
            Protocol::RequestUpdates(version, mut from) => {
                let id = me.read().unwrap().id;

                trace!(
                    "{} RequestUpdates {} from {}, professing to be {}",
                    id,
                    version,
                    from_addr,
                    from.contact_info.ncp
                );

                if from.id == me.read().unwrap().id {
                    warn!(
                        "RequestUpdates ignored, I'm talking to myself: me={} remoteme={}",
                        me.read().unwrap().id,
                        from.id
                    );
                    inc_new_counter_info!("crdt-window-request-loopback", 1);
                    return None;
                }

                // the remote side may not know his public IP:PORT, record what he looks like to us
                //  this may or may not be correct for everybody but it's better than leaving him with
                //  an unspecified address in our table
                if from.contact_info.ncp.ip().is_unspecified() {
                    inc_new_counter_info!("crdt-window-request-updates-unspec-ncp", 1);
                    from.contact_info.ncp = *from_addr;
                }

                let (from_id, ups, data, liveness) = {
                    let me = me.read().unwrap();

                    // only lock for these two calls, dont lock during IO `sock.send_to` or `sock.recv_from`
                    let (from_id, ups, data) = me.get_updates_since(version);

                    (
                        from_id,
                        ups,
                        data,
                        me.remote.iter().map(|(k, v)| (*k, *v)).collect(),
                    )
                };

                // update entry only after collecting liveness
                {
                    let mut me = me.write().unwrap();
                    me.insert(&from);
                    me.update_liveness(from.id);
                }

                trace!("get updates since response {} {}", version, data.len());
                let len = data.len();

                if len < 1 {
                    let me = me.read().unwrap();
                    trace!(
                        "no updates me {} ix {} since {}",
                        id,
                        me.update_index,
                        version
                    );
                    None
                } else {
                    let rsp = Protocol::ReceiveUpdates(from_id, ups, data, liveness);

                    if let Ok(r) = to_blob(rsp, from.contact_info.ncp, &blob_recycler) {
                        trace!(
                            "sending updates me {} len {} to {} {}",
                            id,
                            len,
                            from.id,
                            from.contact_info.ncp,
                        );
                        Some(r)
                    } else {
                        warn!("to_blob failed");
                        None
                    }
                }
            }
            Protocol::ReceiveUpdates(from, update_index, data, external_liveness) => {
                let now = Instant::now();
                trace!(
                    "ReceivedUpdates from={} update_index={} len={}",
                    from,
                    update_index,
                    data.len()
                );
                me.write()
                    .expect("'me' write lock in ReceiveUpdates")
                    .apply_updates(from, update_index, &data, &external_liveness);

                report_time_spent(
                    "ReceiveUpdates",
                    &now.elapsed(),
                    &format!(" len: {}", data.len()),
                );
                None
            }

            Protocol::RequestWindowIndex(from, ix) => {
                let now = Instant::now();

                //TODO this doesn't depend on CRDT module, could be moved
                //but we are using the listen thread to service these request
                //TODO verify from is signed

                if from.id == me.read().unwrap().id {
                    warn!(
                        "{}: Ignored received RequestWindowIndex from ME {} {} ",
                        me.read().unwrap().id,
                        from.id,
                        ix,
                    );
                    inc_new_counter_info!("crdt-window-request-address-eq", 1);
                    return None;
                }

                me.write().unwrap().insert(&from);
                let me = me.read().unwrap().my_data().clone();
                inc_new_counter_info!("crdt-window-request-recv", 1);
                trace!("{}: received RequestWindowIndex {} {} ", me.id, from.id, ix,);
                let res = Self::run_window_request(
                    &from,
                    &from_addr,
                    &window,
                    ledger_window,
                    &me,
                    ix,
                    blob_recycler,
                );
                report_time_spent(
                    "RequestWindowIndex",
                    &now.elapsed(),
                    &format!(" ix: {}", ix),
                );
                res
            }
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        blob_recycler: &BlobRecycler,
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
            if let Some(resp) =
                Self::handle_blob(obj, window, ledger_window, blob_recycler, &req.read())
            {
                resps.push(resp);
            }
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
        let blob_recycler = BlobRecycler::default();

        Builder::new()
            .name("solana-listen".to_string())
            .spawn(move || loop {
                let e = Self::run_listen(
                    &me,
                    &window,
                    &mut ledger_window.as_mut(),
                    &blob_recycler,
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
                        me.id,
                        me.table.len()
                    );
                }
            }).unwrap()
    }

    fn is_valid_ip(addr: IpAddr) -> bool {
        !(addr.is_unspecified() || addr.is_multicast())
        // || (addr.is_loopback() && !cfg_test))
        // TODO: boot loopback in production networks
    }
    /// port must not be 0
    /// ip must be specified and not mulitcast
    /// loopback ip is only allowed in tests
    pub fn is_valid_address(addr: &SocketAddr) -> bool {
        (addr.port() != 0) && Self::is_valid_ip(addr.ip())
    }

    pub fn spy_node() -> (NodeInfo, UdpSocket) {
        let (_, gossip_socket) = bind_in_range(FULLNODE_PORT_RANGE).unwrap();
        let pubkey = Keypair::new().pubkey();
        let daddr = socketaddr_any!();

        let node = NodeInfo::new(pubkey, daddr, daddr, daddr, daddr);
        (node, gossip_socket)
    }
}

pub struct Sockets {
    pub gossip: UdpSocket,
    pub requests: UdpSocket,
    pub replicate: Vec<UdpSocket>,
    pub transaction: Vec<UdpSocket>,
    pub respond: UdpSocket,
    pub broadcast: UdpSocket,
    pub repair: UdpSocket,
    pub retransmit: UdpSocket,
}

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
        let requests = UdpSocket::bind("127.0.0.1:0").unwrap();
        let repair = UdpSocket::bind("127.0.0.1:0").unwrap();

        let respond = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        let info = NodeInfo::new(
            pubkey,
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            requests.local_addr().unwrap(),
            transaction.local_addr().unwrap(),
        );
        Node {
            info,
            sockets: Sockets {
                gossip,
                requests,
                replicate: vec![replicate],
                transaction: vec![transaction],
                respond,
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

        let (requests_port, requests) = bind();

        let (transaction_port, transaction_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 32).expect("tpu multi_bind");

        let (_, repair) = bind();
        let (_, broadcast) = bind();
        let (_, retransmit) = bind();

        // Responses are sent from the same Udp port as requests are received
        // from, in hopes that a NAT sitting in the middle will route the
        // response Udp packet correctly back to the requester.
        let respond = requests.try_clone().unwrap();

        let info = NodeInfo::new(
            pubkey,
            SocketAddr::new(ncp.ip(), gossip_port),
            SocketAddr::new(ncp.ip(), replicate_port),
            SocketAddr::new(ncp.ip(), requests_port),
            SocketAddr::new(ncp.ip(), transaction_port),
        );
        trace!("new NodeInfo: {:?}", info);

        Node {
            info,
            sockets: Sockets {
                gossip,
                requests,
                replicate: replicate_sockets,
                transaction: transaction_sockets,
                respond,
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
    use crdt::{
        Crdt, CrdtError, Node, NodeInfo, Protocol, FULLNODE_PORT_RANGE, GOSSIP_PURGE_MILLIS,
        GOSSIP_SLEEP_MILLIS, MIN_TABLE_SIZE,
    };
    use entry::Entry;
    use hash::{hash, Hash};
    use instruction::Vote;
    use ledger::{LedgerWindow, LedgerWriter};
    use logger;
    use packet::BlobRecycler;
    use result::Error;
    use signature::{Keypair, KeypairUtil, Pubkey};
    use std::fs::remove_dir_all;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;
    use window::default_window;

    #[test]
    fn insert_test() {
        let mut d = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone()).unwrap();
        assert_eq!(crdt.table[&d.id].version, 0);
        assert!(!crdt.alive.contains_key(&d.id));

        d.version = 2;
        crdt.insert(&d);
        let liveness = crdt.alive[&d.id];
        assert_eq!(crdt.table[&d.id].version, 2);

        d.version = 1;
        crdt.insert(&d);
        assert_eq!(crdt.table[&d.id].version, 2);
        assert_eq!(liveness, crdt.alive[&d.id]);

        // Ensure liveness will be updated for version 3
        sleep(Duration::from_millis(1));

        d.version = 3;
        crdt.insert(&d);
        assert_eq!(crdt.table[&d.id].version, 3);
        assert!(liveness < crdt.alive[&d.id]);
    }
    #[test]
    fn test_new_vote() {
        let d = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone()).unwrap();
        assert_eq!(crdt.table[&d.id].version, 0);
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1235"));
        assert_ne!(d.id, leader.id);
        assert_matches!(
            crdt.new_vote(Hash::default()).err(),
            Some(Error::CrdtError(CrdtError::NoLeader))
        );
        crdt.insert(&leader);
        assert_matches!(
            crdt.new_vote(Hash::default()).err(),
            Some(Error::CrdtError(CrdtError::NoLeader))
        );
        crdt.set_leader(leader.id);
        assert_eq!(crdt.table[&d.id].version, 1);
        let v = Vote {
            version: 2, //version should increase when we vote
            contact_info_version: 0,
        };
        let expected = (v, crdt.table[&leader.id].contact_info.tpu);
        assert_eq!(crdt.new_vote(Hash::default()).unwrap(), expected);
    }

    #[test]
    fn test_insert_vote() {
        let d = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone()).unwrap();
        assert_eq!(crdt.table[&d.id].version, 0);
        let vote_same_version = Vote {
            version: d.version,
            contact_info_version: 0,
        };
        crdt.insert_vote(&d.id, &vote_same_version, Hash::default());
        assert_eq!(crdt.table[&d.id].version, 0);

        let vote_new_version_new_addrs = Vote {
            version: d.version + 1,
            contact_info_version: 1,
        };
        crdt.insert_vote(&d.id, &vote_new_version_new_addrs, Hash::default());
        //should be dropped since the address is newer then we know
        assert_eq!(crdt.table[&d.id].version, 0);

        let vote_new_version_old_addrs = Vote {
            version: d.version + 1,
            contact_info_version: 0,
        };
        crdt.insert_vote(&d.id, &vote_new_version_old_addrs, Hash::default());
        //should be accepted, since the update is for the same address field as the one we know
        assert_eq!(crdt.table[&d.id].version, 1);
    }
    fn sorted(ls: &Vec<NodeInfo>) -> Vec<NodeInfo> {
        let mut copy: Vec<_> = ls.iter().cloned().collect();
        copy.sort_by(|x, y| x.id.cmp(&y.id));
        copy
    }
    #[test]
    fn replicated_data_new_with_socketaddr_with_pubkey() {
        let keypair = Keypair::new();
        let d1 = NodeInfo::new_with_pubkey_socketaddr(
            keypair.pubkey().clone(),
            &socketaddr!("127.0.0.1:1234"),
        );
        assert_eq!(d1.id, keypair.pubkey());
        assert_eq!(d1.contact_info.ncp, socketaddr!("127.0.0.1:1235"));
        assert_eq!(d1.contact_info.tvu, socketaddr!("127.0.0.1:1236"));
        assert_eq!(d1.contact_info.rpu, socketaddr!("127.0.0.1:1237"));
        assert_eq!(d1.contact_info.tpu, socketaddr!("127.0.0.1:1234"));
    }
    #[test]
    fn update_test() {
        let d1 = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );
        let d2 = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );
        let d3 = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );
        let mut crdt = Crdt::new(d1.clone()).expect("Crdt::new");
        let (key, ix, ups) = crdt.get_updates_since(0);
        assert_eq!(key, d1.id);
        assert_eq!(ix, 1);
        assert_eq!(ups.len(), 1);
        assert_eq!(sorted(&ups), sorted(&vec![d1.clone()]));
        crdt.insert(&d2);
        let (key, ix, ups) = crdt.get_updates_since(0);
        assert_eq!(key, d1.id);
        assert_eq!(ix, 2);
        assert_eq!(ups.len(), 2);
        assert_eq!(sorted(&ups), sorted(&vec![d1.clone(), d2.clone()]));
        crdt.insert(&d3);
        let (key, ix, ups) = crdt.get_updates_since(0);
        assert_eq!(key, d1.id);
        assert_eq!(ix, 3);
        assert_eq!(ups.len(), 3);
        assert_eq!(
            sorted(&ups),
            sorted(&vec![d1.clone(), d2.clone(), d3.clone()])
        );
        let mut crdt2 = Crdt::new(d2.clone()).expect("Crdt::new");
        crdt2.apply_updates(key, ix, &ups, &vec![]);
        assert_eq!(crdt2.table.values().len(), 3);
        assert_eq!(
            sorted(&crdt2.table.values().map(|x| x.clone()).collect()),
            sorted(&crdt.table.values().map(|x| x.clone()).collect())
        );
        let d4 = NodeInfo::new_entry_point(&socketaddr!("127.0.0.4:1234"));
        crdt.insert(&d4);
        let (_key, _ix, ups) = crdt.get_updates_since(0);
        assert_eq!(sorted(&ups), sorted(&vec![d2.clone(), d1, d3]));
    }
    #[test]
    fn window_index_request() {
        let me = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!([127, 0, 0, 1], 1234),
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
        );
        let mut crdt = Crdt::new(me).expect("Crdt::new");
        let rv = crdt.window_index_request(0);
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));

        let ncp = socketaddr!([127, 0, 0, 1], 1234);
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            ncp,
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
        );
        crdt.insert(&nxt);
        let rv = crdt.window_index_request(0).unwrap();
        assert_eq!(nxt.contact_info.ncp, ncp);
        assert_eq!(rv.0, nxt.contact_info.ncp);

        let ncp2 = socketaddr!([127, 0, 0, 2], 1234);
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            ncp2,
            socketaddr!([127, 0, 0, 1], 1235),
            socketaddr!([127, 0, 0, 1], 1236),
            socketaddr!([127, 0, 0, 1], 1237),
        );
        crdt.insert(&nxt);
        let mut one = false;
        let mut two = false;
        while !one || !two {
            //this randomly picks an option, so eventually it should pick both
            let rv = crdt.window_index_request(0).unwrap();
            if rv.0 == ncp {
                one = true;
            }
            if rv.0 == ncp2 {
                two = true;
            }
        }
        assert!(one && two);
    }

    #[test]
    fn gossip_request_bad_addr() {
        let me = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:127"),
            socketaddr!("127.0.0.1:127"),
            socketaddr!("127.0.0.1:127"),
            socketaddr!("127.0.0.1:127"),
        );

        let mut crdt = Crdt::new(me).expect("Crdt::new");
        let nxt1 = NodeInfo::new_unspecified();
        // Filter out unspecified addresses
        crdt.insert(&nxt1); //<--- attack!
        let rv = crdt.gossip_request();
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));
        let nxt2 = NodeInfo::new_multicast();
        // Filter out multicast addresses
        crdt.insert(&nxt2); //<--- attack!
        let rv = crdt.gossip_request();
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));
    }

    /// test that gossip requests are eventually generated for all nodes
    #[test]
    fn gossip_request() {
        let me = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let rv = crdt.gossip_request();
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));
        let nxt1 = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.2:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );

        crdt.insert(&nxt1);

        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt1.contact_info.ncp);

        let nxt2 = NodeInfo::new_entry_point(&socketaddr!("127.0.0.3:1234"));
        crdt.insert(&nxt2);
        // check that the service works
        // and that it eventually produces a request for both nodes
        let (sender, reader) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let obj = Arc::new(RwLock::new(crdt));
        let thread = Crdt::gossip(obj, sender, exit.clone());
        let mut one = false;
        let mut two = false;
        for _ in 0..30 {
            //50% chance each try that we get a repeat
            let mut rv = reader.recv_timeout(Duration::new(1, 0)).unwrap();
            while let Ok(mut more) = reader.try_recv() {
                rv.append(&mut more);
            }
            assert!(rv.len() > 0);
            for i in rv.iter() {
                if i.read().meta.addr() == nxt1.contact_info.ncp {
                    one = true;
                } else if i.read().meta.addr() == nxt2.contact_info.ncp {
                    two = true;
                } else {
                    //unexpected request
                    assert!(false);
                }
            }
            if one && two {
                break;
            }
        }
        exit.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        //created requests to both
        assert!(one && two);
    }

    #[test]
    fn purge_test() {
        logger::setup();
        let me = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let nxt = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        assert_ne!(me.id, nxt.id);
        crdt.set_leader(me.id);
        crdt.insert(&nxt);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);
        let now = crdt.alive[&nxt.id];
        crdt.purge(now);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);

        crdt.purge(now + GOSSIP_PURGE_MILLIS);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);

        crdt.purge(now + GOSSIP_PURGE_MILLIS + 1);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);

        let mut nxt2 = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        assert_ne!(me.id, nxt2.id);
        assert_ne!(nxt.id, nxt2.id);
        crdt.insert(&nxt2);
        while now == crdt.alive[&nxt2.id] {
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
            nxt2.version += 1;
            crdt.insert(&nxt2);
        }
        let len = crdt.table.len() as u64;
        assert!((MIN_TABLE_SIZE as u64) < len);
        crdt.purge(now + GOSSIP_PURGE_MILLIS);
        assert_eq!(len as usize, crdt.table.len());
        trace!("purging");
        crdt.purge(now + GOSSIP_PURGE_MILLIS + 1);
        assert_eq!(len as usize - 1, crdt.table.len());
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);
    }
    #[test]
    fn purge_leader_test() {
        logger::setup();
        let me = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let nxt = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        assert_ne!(me.id, nxt.id);
        crdt.insert(&nxt);
        crdt.set_leader(nxt.id);
        let now = crdt.alive[&nxt.id];
        let mut nxt2 = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        crdt.insert(&nxt2);
        while now == crdt.alive[&nxt2.id] {
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
            nxt2.version = nxt2.version + 1;
            crdt.insert(&nxt2);
        }
        let len = crdt.table.len() as u64;
        crdt.purge(now + GOSSIP_PURGE_MILLIS + 1);
        assert_eq!(len as usize - 1, crdt.table.len());
        assert_eq!(crdt.my_data().leader_id, Pubkey::default());
        assert!(crdt.leader_data().is_none());
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
        );
        let recycler = BlobRecycler::default();
        let rv = Crdt::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            0,
            &recycler,
        );
        assert!(rv.is_none());
        let out = recycler.allocate();
        out.write().meta.size = 200;
        window.write().unwrap()[0].data = Some(out);
        let rv = Crdt::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            0,
            &recycler,
        );
        assert!(rv.is_some());
        let v = rv.unwrap();
        //test we copied the blob
        assert_eq!(v.read().meta.size, 200);
        let len = window.read().unwrap().len() as u64;
        let rv = Crdt::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            len,
            &recycler,
        );
        assert!(rv.is_none());

        fn tmp_ledger(name: &str) -> String {
            use std::env;
            let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
            let keypair = Keypair::new();

            let path = format!("{}/tmp-ledger-{}-{}", out_dir, name, keypair.pubkey());

            let mut writer = LedgerWriter::open(&path, true).unwrap();
            let zero = Hash::default();
            let one = hash(&zero.as_ref());
            writer
                .write_entries(vec![Entry::new_tick(0, &zero), Entry::new_tick(0, &one)].to_vec())
                .unwrap();
            path
        }

        let ledger_path = tmp_ledger("run_window_request");
        let mut ledger_window = LedgerWindow::open(&ledger_path).unwrap();

        let rv = Crdt::run_window_request(
            &me,
            &socketaddr_any!(),
            &window,
            &mut Some(&mut ledger_window),
            &me,
            1,
            &recycler,
        );
        assert!(rv.is_some());

        remove_dir_all(ledger_path).unwrap();
    }

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request_with_backoff() {
        let window = Arc::new(RwLock::new(default_window()));

        let mut me = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        me.leader_id = me.id;

        let mock_peer = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));

        let recycler = BlobRecycler::default();

        // Simulate handling a repair request from mock_peer
        let rv = Crdt::run_window_request(
            &mock_peer,
            &socketaddr_any!(),
            &window,
            &mut None,
            &me,
            0,
            &recycler,
        );
        assert!(rv.is_none());
        let blob = recycler.allocate();
        let blob_size = 200;
        blob.write().meta.size = blob_size;
        window.write().unwrap()[0].data = Some(blob);

        let num_requests: u32 = 64;
        for i in 0..num_requests {
            let shared_blob = Crdt::run_window_request(
                &mock_peer,
                &socketaddr_any!(),
                &window,
                &mut None,
                &me,
                0,
                &recycler,
            ).unwrap();
            let blob = shared_blob.read();
            // Test we copied the blob
            assert_eq!(blob.meta.size, blob_size);

            let id = if i == 0 || i.is_power_of_two() {
                me.id
            } else {
                mock_peer.id
            };
            assert_eq!(blob.get_id().unwrap(), id);
        }
    }
    /// TODO: This is obviously the wrong way to do this. Need to implement leader selection,
    /// delete this test after leader selection is correctly implemented
    #[test]
    fn test_update_leader() {
        logger::setup();
        let me = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let leader0 = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let leader1 = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        assert_eq!(crdt.top_leader(), None);
        crdt.set_leader(leader0.id);
        assert_eq!(crdt.top_leader().unwrap(), leader0.id);
        //add a bunch of nodes with a new leader
        for _ in 0..10 {
            let mut dum = NodeInfo::new_entry_point(&socketaddr!("127.0.0.1:1234"));
            dum.id = Keypair::new().pubkey();
            dum.leader_id = leader1.id;
            crdt.insert(&dum);
        }
        assert_eq!(crdt.top_leader().unwrap(), leader1.id);
        crdt.update_leader();
        assert_eq!(crdt.my_data().leader_id, leader0.id);
        crdt.insert(&leader1);
        crdt.update_leader();
        assert_eq!(crdt.my_data().leader_id, leader1.id);
    }

    #[test]
    fn test_valid_last_ids() {
        logger::setup();
        let mut leader0 = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        leader0.ledger_state.last_id = hash(b"0");
        let mut leader1 = NodeInfo::new_multicast();
        leader1.ledger_state.last_id = hash(b"1");
        let mut leader2 =
            NodeInfo::new_with_pubkey_socketaddr(Pubkey::default(), &socketaddr!("127.0.0.2:1234"));
        leader2.ledger_state.last_id = hash(b"2");
        // test that only valid tvu or tpu are retured as nodes
        let mut leader3 = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr_any!(),
            socketaddr!("127.0.0.1:1236"),
            socketaddr_any!(),
        );
        leader3.ledger_state.last_id = hash(b"3");
        let mut crdt = Crdt::new(leader0.clone()).expect("Crdt::new");
        crdt.insert(&leader1);
        crdt.insert(&leader2);
        crdt.insert(&leader3);
        assert_eq!(crdt.valid_last_ids(), vec![leader0.ledger_state.last_id]);
    }

    /// Validates the node that sent Protocol::ReceiveUpdates gets its
    /// liveness updated, but not if the node sends Protocol::ReceiveUpdates
    /// to itself.
    #[test]
    fn protocol_requestupdate_alive() {
        logger::setup();
        let window = Arc::new(RwLock::new(default_window()));
        let recycler = BlobRecycler::default();

        let node = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let node_with_same_addr = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        assert_ne!(node.id, node_with_same_addr.id);
        let node_with_diff_addr = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:4321"));

        let crdt = Crdt::new(node.clone()).expect("Crdt::new");
        assert_eq!(crdt.alive.len(), 0);

        let obj = Arc::new(RwLock::new(crdt));

        let request = Protocol::RequestUpdates(1, node.clone());
        assert!(
            Crdt::handle_protocol(
                &obj,
                &node.contact_info.ncp,
                request,
                &window,
                &mut None,
                &recycler
            ).is_none()
        );

        let request = Protocol::RequestUpdates(1, node_with_same_addr.clone());
        assert!(
            Crdt::handle_protocol(
                &obj,
                &node.contact_info.ncp,
                request,
                &window,
                &mut None,
                &recycler
            ).is_none()
        );

        let request = Protocol::RequestUpdates(1, node_with_diff_addr.clone());
        Crdt::handle_protocol(
            &obj,
            &node.contact_info.ncp,
            request,
            &window,
            &mut None,
            &recycler,
        );

        let me = obj.write().unwrap();

        // |node| and |node_with_same_addr| are ok to me in me.alive, should not be in me.alive, but
        assert!(!me.alive.contains_key(&node.id));
        // same addr might very well happen because of NAT
        assert!(me.alive.contains_key(&node_with_same_addr.id));
        // |node_with_diff_addr| should now be.
        assert!(me.alive[&node_with_diff_addr.id] > 0);
    }

    #[test]
    fn test_is_valid_address() {
        assert!(cfg!(test));
        let bad_address_port = socketaddr!("127.0.0.1:0");
        assert!(!Crdt::is_valid_address(&bad_address_port));
        let bad_address_unspecified = socketaddr!(0, 1234);
        assert!(!Crdt::is_valid_address(&bad_address_unspecified));
        let bad_address_multicast = socketaddr!([224, 254, 0, 0], 1234);
        assert!(!Crdt::is_valid_address(&bad_address_multicast));
        let loopback = socketaddr!("127.0.0.1:1234");
        assert!(Crdt::is_valid_address(&loopback));
        //        assert!(!Crdt::is_valid_ip_internal(loopback.ip(), false));
    }

    #[test]
    fn test_default_leader() {
        logger::setup();
        let node_info = NodeInfo::new(
            Keypair::new().pubkey(),
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
        );
        let mut crdt = Crdt::new(node_info).unwrap();
        let network_entry_point = NodeInfo::new_entry_point(&socketaddr!("127.0.0.1:1239"));
        crdt.insert(&network_entry_point);
        assert!(crdt.leader_data().is_none());
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
        assert_eq!(node.sockets.requests.local_addr().unwrap().ip(), ip);
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
        assert!(node.sockets.requests.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.requests.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
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
        assert_eq!(node.sockets.requests.local_addr().unwrap().ip(), ip);
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
        assert!(node.sockets.requests.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.requests.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
        let tx_port = node.sockets.transaction[0].local_addr().unwrap().port();
        assert!(tx_port >= FULLNODE_PORT_RANGE.0);
        assert!(tx_port < FULLNODE_PORT_RANGE.1);
        for tx_socket in node.sockets.transaction.iter() {
            assert_eq!(tx_socket.local_addr().unwrap().port(), tx_port);
        }
        assert!(node.sockets.repair.local_addr().unwrap().port() >= FULLNODE_PORT_RANGE.0);
        assert!(node.sockets.repair.local_addr().unwrap().port() < FULLNODE_PORT_RANGE.1);
    }
}
