//! The `crdt` module defines a data structure that is shared by all the nodes in the network over
//! a gossip control plane.  The goal is to share small bits of off-chain information and detect and
//! repair partitions.
//!
//! This CRDT only supports a very limited set of types.  A map of PublicKey -> Versioned Struct.
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
use byteorder::{LittleEndian, ReadBytesExt};
use choose_gossip_peer_strategy::{ChooseGossipPeerStrategy, ChooseWeightedPeerStrategy};
use counter::Counter;
use hash::Hash;
use packet::{to_blob, Blob, BlobRecycler, SharedBlob, BLOB_SIZE};
use pnet_datalink as datalink;
use rand::{thread_rng, RngCore};
use rayon::prelude::*;
use result::{Error, Result};
use signature::{KeyPair, KeyPairUtil, PublicKey};
use std;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::Duration;
use streamer::{BlobReceiver, BlobSender, Window};
use timing::timestamp;
use transaction::Vote;

const LOG_RATE: usize = 10;

/// milliseconds we sleep for between gossip requests
const GOSSIP_SLEEP_MILLIS: u64 = 100;
const GOSSIP_PURGE_MILLIS: u64 = 15000;

/// minimum membership table size before we start purging dead nodes
const MIN_TABLE_SIZE: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum CrdtError {
    TooSmall,
    NoLeader,
}

pub fn parse_port_or_addr(optstr: Option<String>) -> SocketAddr {
    let daddr: SocketAddr = "0.0.0.0:8000".parse().expect("default socket address");
    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = daddr.clone();
            addr.set_port(port);
            addr
        } else if let Ok(addr) = addrstr.parse() {
            addr
        } else {
            daddr
        }
    } else {
        daddr
    }
}

pub fn get_ip_addr() -> Option<IpAddr> {
    for iface in datalink::interfaces() {
        for p in iface.ips {
            if !p.ip().is_loopback() && !p.ip().is_multicast() {
                match p.ip() {
                    IpAddr::V4(addr) => {
                        if !addr.is_link_local() {
                            return Some(p.ip());
                        }
                    }
                    IpAddr::V6(_addr) => {
                        // Select an ipv6 address if the config is selected
                        #[cfg(feature = "ipv6")]
                        {
                            return Some(p.ip());
                        }
                    }
                }
            }
        }
    }
    None
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
    /// repair address, we use this to jump ahead of the packets
    /// destined to the replciate_addr
    pub tvu_window: SocketAddr,
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
    /// last verified entry count, always increasing
    pub entry_height: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NodeInfo {
    pub id: PublicKey,
    /// If any of the bits change, update increment this value
    pub version: u64,
    /// network addresses
    pub contact_info: ContactInfo,
    /// current leader identity
    pub leader_id: PublicKey,
    /// information about the state of the ledger
    ledger_state: LedgerState,
}

fn make_debug_id(buf: &[u8]) -> u64 {
    let mut rdr = Cursor::new(&buf[..8]);
    rdr.read_u64::<LittleEndian>()
        .expect("rdr.read_u64 in fn debug_id")
}

impl NodeInfo {
    pub fn new(
        id: PublicKey,
        ncp: SocketAddr,
        tvu: SocketAddr,
        rpu: SocketAddr,
        tpu: SocketAddr,
        tvu_window: SocketAddr,
    ) -> NodeInfo {
        NodeInfo {
            id,
            version: 0,
            contact_info: ContactInfo {
                ncp,
                tvu,
                rpu,
                tpu,
                tvu_window,
                version: 0,
            },
            leader_id: PublicKey::default(),
            ledger_state: LedgerState {
                last_id: Hash::default(),
                entry_height: 0,
            },
        }
    }
    pub fn debug_id(&self) -> u64 {
        make_debug_id(&self.id)
    }
    fn next_port(addr: &SocketAddr, nxt: u16) -> SocketAddr {
        let mut nxt_addr = addr.clone();
        nxt_addr.set_port(addr.port() + nxt);
        nxt_addr
    }
    pub fn new_leader_with_pubkey(pubkey: PublicKey, bind_addr: &SocketAddr) -> Self {
        let transactions_addr = bind_addr.clone();
        let gossip_addr = Self::next_port(&bind_addr, 1);
        let replicate_addr = Self::next_port(&bind_addr, 2);
        let requests_addr = Self::next_port(&bind_addr, 3);
        let repair_addr = Self::next_port(&bind_addr, 4);
        NodeInfo::new(
            pubkey,
            gossip_addr,
            replicate_addr,
            requests_addr,
            transactions_addr,
            repair_addr,
        )
    }
    pub fn new_leader(bind_addr: &SocketAddr) -> Self {
        let keypair = KeyPair::new();
        Self::new_leader_with_pubkey(keypair.pubkey(), bind_addr)
    }
    pub fn new_entry_point(gossip_addr: SocketAddr) -> Self {
        let daddr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        NodeInfo::new(
            PublicKey::default(),
            gossip_addr,
            daddr.clone(),
            daddr.clone(),
            daddr.clone(),
            daddr,
        )
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
    pub table: HashMap<PublicKey, NodeInfo>,
    /// Value of my update index when entry in table was updated.
    /// Nodes will ask for updates since `update_index`, and this node
    /// should respond with all the identities that are greater then the
    /// request's `update_index` in this list
    local: HashMap<PublicKey, u64>,
    /// The value of the remote update index that I have last seen
    /// This Node will ask external nodes for updates since the value in this list
    pub remote: HashMap<PublicKey, u64>,
    /// last time the public key had sent us a message
    pub alive: HashMap<PublicKey, u64>,
    pub update_index: u64,
    pub me: PublicKey,
    /// last time we heard from anyone getting a message fro this public key
    /// these are rumers and shouldn't be trusted directly
    external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>>,
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
    ReceiveUpdates(PublicKey, u64, Vec<NodeInfo>, Vec<(PublicKey, u64)>),
    /// ask for a missing index
    /// (my replicated data to keep alive, missing window index)
    RequestWindowIndex(NodeInfo, u64),
}

impl Crdt {
    pub fn new(me: NodeInfo) -> Crdt {
        assert_eq!(me.version, 0);
        let mut g = Crdt {
            table: HashMap::new(),
            local: HashMap::new(),
            remote: HashMap::new(),
            alive: HashMap::new(),
            external_liveness: HashMap::new(),
            me: me.id,
            update_index: 1,
        };
        g.local.insert(me.id, g.update_index);
        g.table.insert(me.id, me);
        g
    }
    pub fn debug_id(&self) -> u64 {
        make_debug_id(&self.me)
    }
    pub fn my_data(&self) -> &NodeInfo {
        &self.table[&self.me]
    }
    pub fn leader_data(&self) -> Option<&NodeInfo> {
        self.table.get(&(self.table[&self.me].leader_id))
    }

    pub fn set_leader(&mut self, key: PublicKey) -> () {
        let mut me = self.my_data().clone();
        warn!(
            "{:x}: LEADER_UPDATE TO {:x} from {:x}",
            me.debug_id(),
            make_debug_id(&key),
            make_debug_id(&me.leader_id),
        );
        me.leader_id = key;
        me.version += 1;
        self.insert(&me);
    }

    pub fn get_external_liveness_entry(&self, key: &PublicKey) -> Option<&HashMap<PublicKey, u64>> {
        self.external_liveness.get(key)
    }

    pub fn insert_vote(&mut self, pubkey: &PublicKey, v: &Vote, last_id: Hash) {
        if self.table.get(pubkey).is_none() {
            warn!(
                "{:x}: VOTE for unknown id: {:x}",
                self.debug_id(),
                make_debug_id(&pubkey)
            );
            return;
        }
        if v.contact_info_version > self.table[pubkey].contact_info.version {
            warn!(
                "{:x}: VOTE for new address version from: {:x} ours: {} vote: {:?}",
                self.debug_id(),
                make_debug_id(pubkey),
                self.table[pubkey].contact_info.version,
                v,
            );
            return;
        }
        self.update_leader_liveness();
        if v.version <= self.table[pubkey].version {
            debug!(
                "{:x}: VOTE for old version: {:x}",
                self.debug_id(),
                make_debug_id(&pubkey)
            );
            self.update_liveness(*pubkey);
            return;
        } else {
            let mut data = self.table[pubkey].clone();
            data.version = v.version;
            data.ledger_state.last_id = last_id;
            debug!(
                "{:x}: INSERTING VOTE! for {:x}",
                self.debug_id(),
                data.debug_id()
            );
            self.insert(&data);
        }
    }
    fn update_leader_liveness(&mut self) {
        //TODO: (leaders should vote)
        //until then we pet their liveness every time we see some votes from anyone
        let ld = self.leader_data().map(|x| x.id.clone());
        trace!("leader_id {:?}", ld);
        if let Some(leader_id) = ld {
            self.update_liveness(leader_id);
        }
    }
    pub fn insert_votes(&mut self, votes: Vec<(PublicKey, Vote, Hash)>) {
        static mut COUNTER_VOTE: Counter = create_counter!("crdt-vote-count", LOG_RATE);
        inc_counter!(COUNTER_VOTE, votes.len());
        if votes.len() > 0 {
            info!("{:x}: INSERTING VOTES {}", self.debug_id(), votes.len());
        }
        for v in &votes {
            self.insert_vote(&v.0, &v.1, v.2);
        }
    }
    pub fn insert(&mut self, v: &NodeInfo) {
        // TODO check that last_verified types are always increasing
        //update the peer table
        if self.table.get(&v.id).is_none() || (v.version > self.table[&v.id].version) {
            //somehow we signed a message for our own identity with a higher version that
            // we have stored ourselves
            trace!(
                "{:x}: insert v.id: {:x} version: {}",
                self.debug_id(),
                v.debug_id(),
                v.version
            );

            self.update_index += 1;
            let _ = self.table.insert(v.id.clone(), v.clone());
            let _ = self.local.insert(v.id, self.update_index);
            static mut COUNTER_UPDATE: Counter = create_counter!("crdt-update-count", LOG_RATE);
            inc_counter!(COUNTER_UPDATE, 1);
        } else {
            trace!(
                "{:x}: INSERT FAILED data: {:x} new.version: {} me.version: {}",
                self.debug_id(),
                v.debug_id(),
                v.version,
                self.table[&v.id].version
            );
        }
        self.update_liveness(v.id);
    }

    fn update_liveness(&mut self, id: PublicKey) {
        //update the liveness table
        let now = timestamp();
        trace!(
            "{:x} updating liveness {:x} to {}",
            self.debug_id(),
            make_debug_id(&id),
            now
        );
        *self.alive.entry(id).or_insert(now) = now;
    }
    /// purge old validators
    /// TODO: we need a robust membership protocol
    /// http://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf
    /// challenging part is that we are on a permissionless network
    pub fn purge(&mut self, now: u64) {
        if self.table.len() <= MIN_TABLE_SIZE {
            return;
        }
        if self.leader_data().is_none() {
            return;
        }
        let leader_id = self.leader_data().unwrap().id;

        let limit = GOSSIP_PURGE_MILLIS;
        let dead_ids: Vec<PublicKey> = self.alive
            .iter()
            .filter_map(|(&k, v)| {
                if k != self.me && (now - v) > limit {
                    if leader_id == k {
                        info!(
                            "{:x}: PURGE LEADER {:x} {}",
                            self.debug_id(),
                            make_debug_id(&k),
                            now - v
                        );
                        Some(k)
                    } else {
                        info!(
                            "{:x}: PURGE {:x} {}",
                            self.debug_id(),
                            make_debug_id(&k),
                            now - v
                        );
                        Some(k)
                    }
                } else {
                    trace!(
                        "{:x} purge skipped {:x} {} {}",
                        self.debug_id(),
                        make_debug_id(&k),
                        now - v,
                        limit
                    );
                    None
                }
            })
            .collect();

        static mut COUNTER_PURGE: Counter = create_counter!("crdt-purge-count", LOG_RATE);
        inc_counter!(COUNTER_PURGE, dead_ids.len());

        for id in dead_ids.iter() {
            self.alive.remove(id);
            self.table.remove(id);
            self.remote.remove(id);
            self.local.remove(id);
            self.external_liveness.remove(id);
            for map in self.external_liveness.values_mut() {
                map.remove(id);
            }
        }
    }

    pub fn index_blobs(
        me: &NodeInfo,
        blobs: &Vec<SharedBlob>,
        receive_index: &mut u64,
    ) -> Result<()> {
        // enumerate all the blobs, those are the indices
        trace!("{:x}: INDEX_BLOBS {}", me.debug_id(), blobs.len());
        for (i, b) in blobs.iter().enumerate() {
            // only leader should be broadcasting
            let mut blob = b.write().expect("'blob' write lock in crdt::index_blobs");
            blob.set_id(me.id).expect("set_id in pub fn broadcast");
            blob.set_index(*receive_index + i as u64)
                .expect("set_index in pub fn broadcast");
        }

        Ok(())
    }
    /// compute broadcast table
    /// # Remarks
    pub fn compute_broadcast_table(&self) -> Vec<NodeInfo> {
        let live: Vec<_> = self.alive.iter().collect();
        //thread_rng().shuffle(&mut live);
        let daddr = "0.0.0.0:0".parse().unwrap();
        let me = &self.table[&self.me];
        let cloned_table: Vec<NodeInfo> = live.iter()
            .map(|x| &self.table[x.0])
            .filter(|v| {
                if me.id == v.id {
                    //filter myself
                    false
                } else if v.contact_info.tvu == daddr {
                    trace!(
                        "{:x}:broadcast skip not listening {:x}",
                        me.debug_id(),
                        v.debug_id()
                    );
                    false
                } else {
                    trace!(
                        "{:x}:broadcast node {:x} {}",
                        me.debug_id(),
                        v.debug_id(),
                        v.contact_info.tvu
                    );
                    true
                }
            })
            .cloned()
            .collect();
        cloned_table
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn broadcast(
        me: &NodeInfo,
        broadcast_table: &Vec<NodeInfo>,
        window: &Window,
        s: &UdpSocket,
        transmit_index: &mut u64,
        received_index: u64,
    ) -> Result<()> {
        if broadcast_table.len() < 1 {
            warn!("{:x}:not enough peers in crdt table", me.debug_id());
            Err(CrdtError::TooSmall)?;
        }
        trace!("broadcast nodes {}", broadcast_table.len());

        // enumerate all the blobs in the window, those are the indices
        // transmit them to nodes, starting from a different node
        let mut orders = Vec::new();
        let window_l = window.write().unwrap();
        for i in *transmit_index..received_index {
            let is = i as usize;
            let k = is % window_l.len();
            assert!(window_l[k].is_some());

            let pos = is % broadcast_table.len();
            orders.push((window_l[k].clone(), &broadcast_table[pos]));
        }

        trace!("broadcast orders table {}", orders.len());
        let errs: Vec<_> = orders
            .into_iter()
            .map(|(b, v)| {
                // only leader should be broadcasting
                assert!(me.leader_id != v.id);
                let bl = b.unwrap();
                let blob = bl.read().expect("blob read lock in streamer::broadcast");
                //TODO profile this, may need multiple sockets for par_iter
                trace!(
                    "{:x}: BROADCAST idx: {} sz: {} to {:x},{} coding: {}",
                    me.debug_id(),
                    blob.get_index().unwrap(),
                    blob.meta.size,
                    v.debug_id(),
                    v.contact_info.tvu,
                    blob.is_coding()
                );
                assert!(blob.meta.size < BLOB_SIZE);
                let e = s.send_to(&blob.data[..blob.meta.size], &v.contact_info.tvu);
                trace!(
                    "{:x}: done broadcast {} to {:x} {}",
                    me.debug_id(),
                    blob.meta.size,
                    v.debug_id(),
                    v.contact_info.tvu
                );
                e
            })
            .collect();
        trace!("broadcast results {}", errs.len());
        for e in errs {
            if let Err(e) = &e {
                error!("broadcast result {:?}", e);
            }
            e?;
            *transmit_index += 1;
        }
        Ok(())
    }

    /// retransmit messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn retransmit(obj: &Arc<RwLock<Self>>, blob: &SharedBlob, s: &UdpSocket) -> Result<()> {
        let (me, table): (NodeInfo, Vec<NodeInfo>) = {
            // copy to avoid locking during IO
            let s = obj.read().expect("'obj' read lock in pub fn retransmit");
            (s.table[&s.me].clone(), s.table.values().cloned().collect())
        };
        blob.write()
            .unwrap()
            .set_id(me.id)
            .expect("set_id in pub fn retransmit");
        let rblob = blob.read().unwrap();
        let daddr = "0.0.0.0:0".parse().unwrap();
        let orders: Vec<_> = table
            .iter()
            .filter(|v| {
                if me.id == v.id {
                    false
                } else if me.leader_id == v.id {
                    trace!("skip retransmit to leader {:?}", v.id);
                    false
                } else if v.contact_info.tvu == daddr {
                    trace!("skip nodes that are not listening {:?}", v.id);
                    false
                } else {
                    true
                }
            })
            .collect();
        trace!("retransmit orders {}", orders.len());
        let errs: Vec<_> = orders
            .par_iter()
            .map(|v| {
                debug!(
                    "{:x}: retransmit blob {} to {:x}",
                    me.debug_id(),
                    rblob.get_index().unwrap(),
                    v.debug_id(),
                );
                //TODO profile this, may need multiple sockets for par_iter
                assert!(rblob.meta.size < BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.contact_info.tvu)
            })
            .collect();
        for e in errs {
            if let Err(e) = &e {
                error!("broadcast result {:?}", e);
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

    fn random() -> u64 {
        thread_rng().next_u64()
    }

    // TODO: fill in with real implmentation once staking is implemented
    fn get_stake(_id: PublicKey) -> f64 {
        1.0
    }

    fn get_updates_since(&self, v: u64) -> (PublicKey, u64, Vec<NodeInfo>) {
        //trace!("get updates since {}", v);
        let data = self.table
            .values()
            .filter(|x| x.id != PublicKey::default() && self.local[&x.id] > v)
            .cloned()
            .collect();
        let id = self.me;
        let ups = self.update_index;
        (id, ups, data)
    }

    pub fn window_index_request(&self, ix: u64) -> Result<(SocketAddr, Vec<u8>)> {
        let daddr = "0.0.0.0:0".parse().unwrap();
        let valid: Vec<_> = self.table
            .values()
            .filter(|r| r.id != self.me && r.contact_info.tvu_window != daddr)
            .collect();
        if valid.is_empty() {
            Err(CrdtError::TooSmall)?;
        }
        let n = (Self::random() as usize) % valid.len();
        let addr = valid[n].contact_info.ncp.clone();
        let req = Protocol::RequestWindowIndex(self.table[&self.me].clone(), ix);
        let out = serialize(&req)?;
        Ok((addr, out))
    }

    /// Create a random gossip request
    /// # Returns
    /// (A,B)
    /// * A - Address to send to
    /// * B - RequestUpdates protocol message
    fn gossip_request(&self) -> Result<(SocketAddr, Protocol)> {
        let options: Vec<_> = self.table.values().filter(|v| v.id != self.me).collect();

        let choose_peer_strategy = ChooseWeightedPeerStrategy::new(
            &self.remote,
            &self.external_liveness,
            &Self::get_stake,
        );

        let choose_peer_result = choose_peer_strategy.choose_peer(options);

        if let Err(Error::CrdtError(CrdtError::TooSmall)) = &choose_peer_result {
            trace!(
                "crdt too small for gossip {:x} {}",
                self.debug_id(),
                self.table.len()
            );
        };
        let v = choose_peer_result?;

        let remote_update_index = *self.remote.get(&v.id).unwrap_or(&0);
        let req = Protocol::RequestUpdates(remote_update_index, self.table[&self.me].clone());
        trace!(
            "created gossip request from {:x} to {:x} {}",
            self.debug_id(),
            v.debug_id(),
            v.contact_info.ncp
        );

        Ok((v.contact_info.ncp, req))
    }

    pub fn new_vote(&mut self, height: u64, last_id: Hash) -> Result<(Vote, SocketAddr)> {
        let mut me = self.my_data().clone();
        let leader = self.leader_data().ok_or(CrdtError::NoLeader)?.clone();
        me.version += 1;
        me.ledger_state.last_id = last_id;
        me.ledger_state.entry_height = height;
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
        let (remote_gossip_addr, req) = obj.read()
            .expect("'obj' read lock in fn run_gossip")
            .gossip_request()?;

        // TODO this will get chatty, so we need to first ask for number of updates since
        // then only ask for specific data that we dont have
        let blob = to_blob(req, remote_gossip_addr, blob_recycler)?;
        let mut q: VecDeque<SharedBlob> = VecDeque::new();
        q.push_back(blob);
        blob_sender.send(q)?;
        Ok(())
    }
    /// TODO: This is obviously the wrong way to do this. Need to implement leader selection
    fn top_leader(&self) -> Option<PublicKey> {
        let mut table = HashMap::new();
        let def = PublicKey::default();
        let cur = self.table.values().filter(|x| x.leader_id != def);
        for v in cur {
            let cnt = table.entry(&v.leader_id).or_insert(0);
            *cnt += 1;
            trace!("leader {:x} {}", make_debug_id(&v.leader_id), *cnt);
        }
        let mut sorted: Vec<(&PublicKey, usize)> = table.into_iter().collect();
        let my_id = self.debug_id();
        for x in sorted.iter() {
            trace!(
                "{:x}: sorted leaders {:x} votes: {}",
                my_id,
                make_debug_id(&x.0),
                x.1
            );
        }
        sorted.sort_by_key(|a| a.1);
        sorted.last().map(|a| *a.0)
    }

    /// TODO: This is obviously the wrong way to do this. Need to implement leader selection
    /// A t-shirt for the first person to actually use this bad behavior to attack the alpha testnet
    fn update_leader(&mut self) {
        if let Some(leader_id) = self.top_leader() {
            if self.my_data().leader_id != leader_id {
                if self.table.get(&leader_id).is_some() {
                    self.set_leader(leader_id);
                }
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
        from: PublicKey,
        update_index: u64,
        data: &[NodeInfo],
        external_liveness: &[(PublicKey, u64)],
    ) {
        trace!("got updates {}", data.len());
        // TODO we need to punish/spam resist here
        // sig verify the whole update and slash anyone who sends a bad update
        for v in data {
            self.insert(&v);
        }

        for (pk, external_remote_index) in external_liveness {
            let remote_entry = if let Some(v) = self.remote.get(pk) {
                *v
            } else {
                0
            };

            if remote_entry >= *external_remote_index {
                continue;
            }

            let liveness_entry = self.external_liveness.entry(*pk).or_insert(HashMap::new());
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
        blob_recycler: BlobRecycler,
        blob_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
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
                let _ = obj.write().unwrap().update_leader();
                let elapsed = timestamp() - start;
                if GOSSIP_SLEEP_MILLIS > elapsed {
                    let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                    sleep(Duration::from_millis(time_left));
                }
            })
            .unwrap()
    }
    fn run_window_request(
        window: &Window,
        me: &NodeInfo,
        from: &NodeInfo,
        ix: u64,
        blob_recycler: &BlobRecycler,
    ) -> Option<SharedBlob> {
        let pos = (ix as usize) % window.read().unwrap().len();
        if let Some(blob) = &window.read().unwrap()[pos] {
            let mut wblob = blob.write().unwrap();
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
                    let mut outblob = out.write().unwrap();
                    let sz = wblob.meta.size;
                    outblob.meta.size = sz;
                    outblob.data[..sz].copy_from_slice(&wblob.data[..sz]);
                    outblob.meta.set_addr(&from.contact_info.tvu_window);
                    outblob.set_id(sender_id).expect("blob set_id");
                }
                static mut COUNTER_REQ_WINDOW_PASS: Counter =
                    create_counter!("crdt-window-request-pass", LOG_RATE);
                inc_counter!(COUNTER_REQ_WINDOW_PASS, 1);

                return Some(out);
            } else {
                static mut COUNTER_REQ_WINDOW_OUTSIDE: Counter =
                    create_counter!("crdt-window-request-outside", LOG_RATE);
                inc_counter!(COUNTER_REQ_WINDOW_OUTSIDE, 1);
                info!(
                    "requested ix {} != blob_ix {}, outside window!",
                    ix, blob_ix
                );
            }
        } else {
            static mut COUNTER_REQ_WINDOW_FAIL: Counter =
                create_counter!("crdt-window-request-fail", LOG_RATE);
            inc_counter!(COUNTER_REQ_WINDOW_FAIL, 1);
            assert!(window.read().unwrap()[pos].is_none());
            info!(
                "{:x}: failed RequestWindowIndex {:x} {} {}",
                me.debug_id(),
                from.debug_id(),
                ix,
                pos,
            );
        }

        None
    }

    //TODO we should first coalesce all the requests
    fn handle_blob(
        obj: &Arc<RwLock<Self>>,
        window: &Window,
        blob_recycler: &BlobRecycler,
        blob: &Blob,
    ) -> Option<SharedBlob> {
        match deserialize(&blob.data[..blob.meta.size]) {
            // TODO sigverify these
            Ok(Protocol::RequestUpdates(v, from_rd)) => {
                trace!("RequestUpdates {}", v);
                let addr = from_rd.contact_info.ncp;
                let me = obj.read().unwrap();
                // only lock for these two calls, dont lock during IO `sock.send_to` or `sock.recv_from`
                let (from, ups, data) = me.get_updates_since(v);
                let external_liveness = me.remote
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                drop(me);
                trace!("get updates since response {} {}", v, data.len());
                let len = data.len();
                let rsp = Protocol::ReceiveUpdates(from, ups, data, external_liveness);
                obj.write().unwrap().insert(&from_rd);
                if len < 1 {
                    let me = obj.read().unwrap();
                    trace!(
                        "no updates me {:x} ix {} since {}",
                        me.debug_id(),
                        me.update_index,
                        v
                    );
                    None
                } else if let Ok(r) = to_blob(rsp, addr, &blob_recycler) {
                    trace!(
                        "sending updates me {:x} len {} to {:x} {}",
                        obj.read().unwrap().debug_id(),
                        len,
                        from_rd.debug_id(),
                        addr,
                    );
                    Some(r)
                } else {
                    warn!("to_blob failed");
                    None
                }
            }
            Ok(Protocol::ReceiveUpdates(from, ups, data, external_liveness)) => {
                trace!(
                    "ReceivedUpdates {:x} {} {}",
                    make_debug_id(&from),
                    ups,
                    data.len()
                );
                obj.write()
                    .expect("'obj' write lock in ReceiveUpdates")
                    .apply_updates(from, ups, &data, &external_liveness);
                None
            }
            Ok(Protocol::RequestWindowIndex(from, ix)) => {
                //TODO this doesn't depend on CRDT module, can be moved
                //but we are using the listen thread to service these request
                //TODO verify from is signed
                obj.write().unwrap().insert(&from);
                let me = obj.read().unwrap().my_data().clone();
                static mut COUNTER_REQ_WINDOW: Counter =
                    create_counter!("crdt-window-request-recv", LOG_RATE);
                inc_counter!(COUNTER_REQ_WINDOW, 1);
                trace!(
                    "{:x}:received RequestWindowIndex {:x} {} ",
                    me.debug_id(),
                    from.debug_id(),
                    ix,
                );
                assert_ne!(from.contact_info.tvu_window, me.contact_info.tvu_window);
                Self::run_window_request(&window, &me, &from, ix, blob_recycler)
            }
            Err(_) => {
                warn!("deserialize crdt packet failed");
                None
            }
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        window: &Window,
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
        let resp: VecDeque<_> = reqs.iter()
            .filter_map(|b| Self::handle_blob(obj, window, blob_recycler, &b.read().unwrap()))
            .collect();
        response_sender.send(resp)?;
        while let Some(r) = reqs.pop_front() {
            blob_recycler.recycle(r);
        }
        Ok(())
    }
    pub fn listen(
        obj: Arc<RwLock<Self>>,
        window: Window,
        blob_recycler: BlobRecycler,
        requests_receiver: BlobReceiver,
        response_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let debug_id = obj.read().unwrap().debug_id();
        Builder::new()
            .name("solana-listen".to_string())
            .spawn(move || loop {
                let e = Self::run_listen(
                    &obj,
                    &window,
                    &blob_recycler,
                    &requests_receiver,
                    &response_sender,
                );
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                if e.is_err() {
                    info!(
                        "{:x}: run_listen timeout, table size: {}",
                        debug_id,
                        obj.read().unwrap().table.len()
                    );
                }
            })
            .unwrap()
    }
}

pub struct Sockets {
    pub gossip: UdpSocket,
    pub gossip_send: UdpSocket,
    pub requests: UdpSocket,
    pub replicate: UdpSocket,
    pub transaction: UdpSocket,
    pub respond: UdpSocket,
    pub broadcast: UdpSocket,
    pub repair: UdpSocket,
    pub retransmit: UdpSocket,
}

pub struct TestNode {
    pub data: NodeInfo,
    pub sockets: Sockets,
}

impl TestNode {
    pub fn new() -> Self {
        let pubkey = KeyPair::new().pubkey();
        Self::new_with_pubkey(pubkey)
    }
    pub fn new_with_pubkey(pubkey: PublicKey) -> Self {
        let transaction = UdpSocket::bind("0.0.0.0:0").unwrap();
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let replicate = UdpSocket::bind("0.0.0.0:0").unwrap();
        let requests = UdpSocket::bind("0.0.0.0:0").unwrap();
        let repair = UdpSocket::bind("0.0.0.0:0").unwrap();
        let gossip_send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let respond = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        let data = NodeInfo::new(
            pubkey,
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            requests.local_addr().unwrap(),
            transaction.local_addr().unwrap(),
            repair.local_addr().unwrap(),
        );
        TestNode {
            data: data,
            sockets: Sockets {
                gossip,
                gossip_send,
                requests,
                replicate,
                transaction,
                respond,
                broadcast,
                repair,
                retransmit,
            },
        }
    }
    pub fn new_with_bind_addr(data: NodeInfo, bind_addr: SocketAddr) -> TestNode {
        let mut local_gossip_addr = bind_addr.clone();
        local_gossip_addr.set_port(data.contact_info.ncp.port());

        let mut local_replicate_addr = bind_addr.clone();
        local_replicate_addr.set_port(data.contact_info.tvu.port());

        let mut local_requests_addr = bind_addr.clone();
        local_requests_addr.set_port(data.contact_info.rpu.port());

        let mut local_transactions_addr = bind_addr.clone();
        local_transactions_addr.set_port(data.contact_info.tpu.port());

        let mut local_repair_addr = bind_addr.clone();
        local_repair_addr.set_port(data.contact_info.tvu_window.port());

        let transaction = UdpSocket::bind(local_transactions_addr).unwrap();
        let gossip = UdpSocket::bind(local_gossip_addr).unwrap();
        let replicate = UdpSocket::bind(local_replicate_addr).unwrap();
        let repair = UdpSocket::bind(local_repair_addr).unwrap();
        let requests = UdpSocket::bind(local_requests_addr).unwrap();

        // Responses are sent from the same Udp port as requests are received
        // from, in hopes that a NAT sitting in the middle will route the
        // response Udp packet correctly back to the requester.
        let respond = requests.try_clone().unwrap();

        let gossip_send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        TestNode {
            data: data,
            sockets: Sockets {
                gossip,
                gossip_send,
                requests,
                replicate,
                transaction,
                respond,
                broadcast,
                repair,
                retransmit,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crdt::{
        parse_port_or_addr, Crdt, CrdtError, NodeInfo, GOSSIP_PURGE_MILLIS,
        GOSSIP_SLEEP_MILLIS, MIN_TABLE_SIZE,
    };
    use hash::Hash;
    use logger;
    use packet::BlobRecycler;
    use result::Error;
    use signature::{KeyPair, KeyPairUtil};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;
    use streamer::default_window;
    use transaction::Vote;

    #[test]
    fn test_parse_port_or_addr() {
        let p1 = parse_port_or_addr(Some("9000".to_string()));
        assert_eq!(p1.port(), 9000);
        let p2 = parse_port_or_addr(Some("127.0.0.1:7000".to_string()));
        assert_eq!(p2.port(), 7000);
        let p3 = parse_port_or_addr(None);
        assert_eq!(p3.port(), 8000);
    }
    #[test]
    fn insert_test() {
        let mut d = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone());
        assert_eq!(crdt.table[&d.id].version, 0);
        d.version = 2;
        crdt.insert(&d);
        assert_eq!(crdt.table[&d.id].version, 2);
        d.version = 1;
        crdt.insert(&d);
        assert_eq!(crdt.table[&d.id].version, 2);
    }
    #[test]
    fn test_new_vote() {
        let d = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone());
        assert_eq!(crdt.table[&d.id].version, 0);
        let leader = NodeInfo::new_leader(&"127.0.0.2:1235".parse().unwrap());
        assert_ne!(d.id, leader.id);
        assert_matches!(
            crdt.new_vote(0, Hash::default()).err(),
            Some(Error::CrdtError(CrdtError::NoLeader))
        );
        crdt.insert(&leader);
        assert_matches!(
            crdt.new_vote(0, Hash::default()).err(),
            Some(Error::CrdtError(CrdtError::NoLeader))
        );
        crdt.set_leader(leader.id);
        assert_eq!(crdt.table[&d.id].version, 1);
        let v = Vote {
            version: 2, //version shoud increase when we vote
            contact_info_version: 0,
        };
        let expected = (v, crdt.table[&leader.id].contact_info.tpu);
        assert_eq!(crdt.new_vote(0, Hash::default()).unwrap(), expected);
    }

    #[test]
    fn test_insert_vote() {
        let d = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone());
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

    #[test]
    fn test_insert_vote_leader_liveness() {
        logger::setup();
        // TODO: remove this test once leaders vote
        let d = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone());
        let leader = NodeInfo::new_leader(&"127.0.0.2:1235".parse().unwrap());
        assert_ne!(d.id, leader.id);
        crdt.insert(&leader);
        crdt.set_leader(leader.id);
        let live: u64 = crdt.alive[&leader.id];
        trace!("{:x} live {}", leader.debug_id(), live);
        let vote_new_version_old_addrs = Vote {
            version: d.version + 1,
            contact_info_version: 0,
        };
        sleep(Duration::from_millis(100));
        let votes = vec![(d.id.clone(), vote_new_version_old_addrs, Hash::default())];
        crdt.insert_votes(votes);
        let updated = crdt.alive[&leader.id];
        //should be accepted, since the update is for the same address field as the one we know
        assert_eq!(crdt.table[&d.id].version, 1);
        trace!("{:x} {} {}", leader.debug_id(), updated, live);
        assert!(updated > live);
    }

    fn sorted(ls: &Vec<NodeInfo>) -> Vec<NodeInfo> {
        let mut copy: Vec<_> = ls.iter().cloned().collect();
        copy.sort_by(|x, y| x.id.cmp(&y.id));
        copy
    }
    #[test]
    fn replicated_data_new_leader_with_pubkey() {
        let kp = KeyPair::new();
        let d1 = NodeInfo::new_leader_with_pubkey(
            kp.pubkey().clone(),
            &"127.0.0.1:1234".parse().unwrap(),
        );
        assert_eq!(d1.id, kp.pubkey());
        assert_eq!(d1.contact_info.ncp, "127.0.0.1:1235".parse().unwrap());
        assert_eq!(d1.contact_info.tvu, "127.0.0.1:1236".parse().unwrap());
        assert_eq!(d1.contact_info.rpu, "127.0.0.1:1237".parse().unwrap());
        assert_eq!(d1.contact_info.tpu, "127.0.0.1:1234".parse().unwrap());
        assert_eq!(
            d1.contact_info.tvu_window,
            "127.0.0.1:1238".parse().unwrap()
        );
    }
    #[test]
    fn update_test() {
        let d1 = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let d2 = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let d3 = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let mut crdt = Crdt::new(d1.clone());
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
        let mut crdt2 = Crdt::new(d2.clone());
        crdt2.apply_updates(key, ix, &ups, &vec![]);
        assert_eq!(crdt2.table.values().len(), 3);
        assert_eq!(
            sorted(&crdt2.table.values().map(|x| x.clone()).collect()),
            sorted(&crdt.table.values().map(|x| x.clone()).collect())
        );
        let d4 = NodeInfo::new_entry_point("127.0.0.4:1234".parse().unwrap());
        crdt.insert(&d4);
        let (_key, _ix, ups) = crdt.get_updates_since(0);
        assert_eq!(sorted(&ups), sorted(&vec![d2.clone(), d1, d3]));
    }
    #[test]
    fn window_index_request() {
        let me = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let mut crdt = Crdt::new(me.clone());
        let rv = crdt.window_index_request(0);
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::TooSmall)));
        let nxt = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
        );
        crdt.insert(&nxt);
        let rv = crdt.window_index_request(0);
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::TooSmall)));
        let nxt = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.2:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        crdt.insert(&nxt);
        let rv = crdt.window_index_request(0).unwrap();
        assert_eq!(nxt.contact_info.ncp, "127.0.0.2:1234".parse().unwrap());
        assert_eq!(rv.0, "127.0.0.2:1234".parse().unwrap());

        let nxt = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.3:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        crdt.insert(&nxt);
        let mut one = false;
        let mut two = false;
        while !one || !two {
            //this randomly picks an option, so eventually it should pick both
            let rv = crdt.window_index_request(0).unwrap();
            if rv.0 == "127.0.0.2:1234".parse().unwrap() {
                one = true;
            }
            if rv.0 == "127.0.0.3:1234".parse().unwrap() {
                two = true;
            }
        }
        assert!(one && two);
    }

    /// test that gossip requests are eventually generated for all nodes
    #[test]
    fn gossip_request() {
        let me = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let mut crdt = Crdt::new(me.clone());
        let rv = crdt.gossip_request();
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::TooSmall)));
        let nxt1 = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.2:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );

        crdt.insert(&nxt1);

        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt1.contact_info.ncp);

        let nxt2 = NodeInfo::new_entry_point("127.0.0.3:1234".parse().unwrap());
        crdt.insert(&nxt2);
        // check that the service works
        // and that it eventually produces a request for both nodes
        let (sender, reader) = channel();
        let recycler = BlobRecycler::default();
        let exit = Arc::new(AtomicBool::new(false));
        let obj = Arc::new(RwLock::new(crdt));
        let thread = Crdt::gossip(obj, recycler, sender, exit.clone());
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
                if i.read().unwrap().meta.addr() == nxt1.contact_info.ncp {
                    one = true;
                } else if i.read().unwrap().meta.addr() == nxt2.contact_info.ncp {
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
        let me = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let mut crdt = Crdt::new(me.clone());
        let nxt = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
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

        let nxt2 = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
        assert_ne!(me.id, nxt2.id);
        assert_ne!(nxt.id, nxt2.id);
        crdt.insert(&nxt2);
        while now == crdt.alive[&nxt2.id] {
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
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

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request() {
        let window = default_window();
        let me = NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let recycler = BlobRecycler::default();
        let rv = Crdt::run_window_request(&window, &me, &me, 0, &recycler);
        assert!(rv.is_none());
        let out = recycler.allocate();
        out.write().unwrap().meta.size = 200;
        window.write().unwrap()[0] = Some(out);
        let rv = Crdt::run_window_request(&window, &me, &me, 0, &recycler);
        assert!(rv.is_some());
        let v = rv.unwrap();
        //test we copied the blob
        assert_eq!(v.read().unwrap().meta.size, 200);
        let len = window.read().unwrap().len() as u64;
        let rv = Crdt::run_window_request(&window, &me, &me, len, &recycler);
        assert!(rv.is_none());
    }

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request_with_backoff() {
        let window = default_window();

        let mut me = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        me.leader_id = me.id;

        let mock_peer = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());

        let recycler = BlobRecycler::default();

        // Simulate handling a repair request from mock_peer
        let rv = Crdt::run_window_request(&window, &me, &mock_peer, 0, &recycler);
        assert!(rv.is_none());
        let blob = recycler.allocate();
        let blob_size = 200;
        blob.write().unwrap().meta.size = blob_size;
        window.write().unwrap()[0] = Some(blob);

        let num_requests: u32 = 64;
        for i in 0..num_requests {
            let shared_blob =
                Crdt::run_window_request(&window, &me, &mock_peer, 0, &recycler).unwrap();
            let blob = shared_blob.read().unwrap();
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
        let me = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let leader0 = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let leader1 = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let mut crdt = Crdt::new(me.clone());
        assert_eq!(crdt.top_leader(), None);
        crdt.set_leader(leader0.id);
        assert_eq!(crdt.top_leader().unwrap(), leader0.id);
        //add a bunch of nodes with a new leader
        for _ in 0..10 {
            let mut dum = NodeInfo::new_entry_point("127.0.0.1:1234".parse().unwrap());
            dum.id = KeyPair::new().pubkey();
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
}
