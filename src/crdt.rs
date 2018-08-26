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
use byteorder::{LittleEndian, ReadBytesExt};
use choose_gossip_peer_strategy::{ChooseGossipPeerStrategy, ChooseWeightedPeerStrategy};
use counter::Counter;
use hash::Hash;
use ledger::LedgerWindow;
use log::Level;
use nat::udp_random_bind;
use packet::{to_blob, Blob, BlobRecycler, SharedBlob, BLOB_SIZE};
use pnet_datalink as datalink;
use rand::{thread_rng, RngCore};
use rayon::prelude::*;
use result::{Error, Result};
use signature::{Keypair, KeypairUtil, Pubkey};
use std;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::{BlobReceiver, BlobSender};
use timing::{duration_as_ms, timestamp};
use transaction::Vote;
use window::{SharedWindow, WindowIndex};

/// milliseconds we sleep for between gossip requests
const GOSSIP_SLEEP_MILLIS: u64 = 100;
const GOSSIP_PURGE_MILLIS: u64 = 15000;

/// minimum membership table size before we start purging dead nodes
const MIN_TABLE_SIZE: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum CrdtError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadNodeInfo,
    BadGossipAddress,
}

pub fn parse_port_or_addr(optstr: Option<String>) -> SocketAddr {
    let daddr: SocketAddr = "0.0.0.0:8000".parse().expect("default socket address");
    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = daddr;
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

fn make_debug_id(key: &Pubkey) -> u64 {
    let buf: &[u8] = &key.as_ref();
    let mut rdr = Cursor::new(&buf[..8]);
    rdr.read_u64::<LittleEndian>()
        .expect("rdr.read_u64 in fn debug_id")
}

impl NodeInfo {
    pub fn new(
        id: Pubkey,
        ncp: SocketAddr,
        tvu: SocketAddr,
        rpu: SocketAddr,
        tpu: SocketAddr,
        tvu_window: SocketAddr,
    ) -> Self {
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
            leader_id: Pubkey::default(),
            ledger_state: LedgerState {
                last_id: Hash::default(),
            },
        }
    }
    #[cfg(test)]
    /// NodeInfo with unspecified addresses for adversarial testing.
    pub fn new_unspecified() -> Self {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        assert!(addr.ip().is_unspecified());
        Self::new(
            Keypair::new().pubkey(),
            addr.clone(),
            addr.clone(),
            addr.clone(),
            addr.clone(),
            addr.clone(),
        )
    }
    #[cfg(test)]
    /// NodeInfo with multicast addresses for adversarial testing.
    pub fn new_multicast() -> Self {
        let addr: SocketAddr = "224.0.1.255:1000".parse().unwrap();
        assert!(addr.ip().is_multicast());
        Self::new(
            Keypair::new().pubkey(),
            addr.clone(),
            addr.clone(),
            addr.clone(),
            addr.clone(),
            addr.clone(),
        )
    }
    pub fn debug_id(&self) -> u64 {
        make_debug_id(&self.id)
    }
    fn next_port(addr: &SocketAddr, nxt: u16) -> SocketAddr {
        let mut nxt_addr = *addr;
        nxt_addr.set_port(addr.port() + nxt);
        nxt_addr
    }
    pub fn new_leader_with_pubkey(pubkey: Pubkey, bind_addr: &SocketAddr) -> Self {
        let transactions_addr = *bind_addr;
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
        let keypair = Keypair::new();
        Self::new_leader_with_pubkey(keypair.pubkey(), bind_addr)
    }
    pub fn new_entry_point(gossip_addr: SocketAddr) -> Self {
        let daddr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        NodeInfo::new(Pubkey::default(), gossip_addr, daddr, daddr, daddr, daddr)
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
    pub me: Pubkey,
    /// last time we heard from anyone getting a message fro this public key
    /// these are rumers and shouldn't be trusted directly
    external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>>,
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
    pub fn new(me: NodeInfo) -> Result<Crdt> {
        if me.version != 0 {
            return Err(Error::CrdtError(CrdtError::BadNodeInfo));
        }
        if me.contact_info.ncp.ip().is_unspecified()
            || me.contact_info.ncp.port() == 0
            || me.contact_info.ncp.ip().is_multicast()
        {
            return Err(Error::CrdtError(CrdtError::BadGossipAddress));
        }
        for addr in &[
            me.contact_info.tvu,
            me.contact_info.rpu,
            me.contact_info.tpu,
            me.contact_info.tvu_window,
        ] {
            //dummy address is allowed, services will filter them
            if addr.ip().is_unspecified() && addr.port() == 0 {
                continue;
            }
            //if addr is not a dummy address, than it must be valid
            if addr.ip().is_unspecified() || addr.port() == 0 || addr.ip().is_multicast() {
                return Err(Error::CrdtError(CrdtError::BadContactInfo));
            }
        }
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
        Ok(g)
    }
    pub fn debug_id(&self) -> u64 {
        make_debug_id(&self.me)
    }
    pub fn my_data(&self) -> &NodeInfo {
        &self.table[&self.me]
    }
    pub fn leader_data(&self) -> Option<&NodeInfo> {
        let leader_id = self.table[&self.me].leader_id;

        // leader_id can be 0s from network entry point
        if leader_id == Pubkey::default() {
            return None;
        }

        self.table.get(&leader_id)
    }

    pub fn set_leader(&mut self, key: Pubkey) -> () {
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

    pub fn get_external_liveness_entry(&self, key: &Pubkey) -> Option<&HashMap<Pubkey, u64>> {
        self.external_liveness.get(key)
    }

    pub fn insert_vote(&mut self, pubkey: &Pubkey, v: &Vote, last_id: Hash) {
        if self.table.get(pubkey).is_none() {
            warn!(
                "{:x}: VOTE for unknown id: {:x}",
                self.debug_id(),
                make_debug_id(pubkey)
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
        if *pubkey == self.my_data().leader_id {
            info!(
                "{:x}: LEADER_VOTED! {:x}",
                self.debug_id(),
                make_debug_id(&pubkey)
            );
            inc_new_counter_info!("crdt-insert_vote-leader_voted", 1);
        }

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
            self.update_liveness(data.id);
            self.insert(&data);
        }
    }
    pub fn insert_votes(&mut self, votes: &[(Pubkey, Vote, Hash)]) {
        inc_new_counter_info!("crdt-vote-count", votes.len());
        if !votes.is_empty() {
            info!("{:x}: INSERTING VOTES {}", self.debug_id(), votes.len());
        }
        for v in votes {
            self.insert_vote(&v.0, &v.1, v.2);
        }
    }

    pub fn insert(&mut self, v: &NodeInfo) -> usize {
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
                "{:x}: INSERT FAILED data: {:x} new.version: {} me.version: {}",
                self.debug_id(),
                v.debug_id(),
                v.version,
                self.table[&v.id].version
            );
            0
        }
    }

    fn update_liveness(&mut self, id: Pubkey) {
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
                if k != self.me && (now - v) > limit {
                    Some(k)
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

        inc_new_counter_info!("crdt-purge-count", dead_ids.len());

        for id in &dead_ids {
            self.alive.remove(id);
            self.table.remove(id);
            self.remote.remove(id);
            self.local.remove(id);
            self.external_liveness.remove(id);
            info!("{:x}: PURGE {:x}", self.debug_id(), make_debug_id(id));
            for map in self.external_liveness.values_mut() {
                map.remove(id);
            }
            if *id == leader_id {
                info!(
                    "{:x}: PURGE LEADER {:x}",
                    self.debug_id(),
                    make_debug_id(id),
                );
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
        let me = &self.table[&self.me];
        let cloned_table: Vec<NodeInfo> = live
            .iter()
            .map(|x| &self.table[x.0])
            .filter(|v| {
                if me.id == v.id {
                    //filter myself
                    false
                } else if !(Self::is_valid_address(v.contact_info.tvu)) {
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
        broadcast_table: &[NodeInfo],
        window: &SharedWindow,
        s: &UdpSocket,
        transmit_index: &mut WindowIndex,
        received_index: u64,
    ) -> Result<()> {
        if broadcast_table.is_empty() {
            warn!("{:x}:not enough peers in crdt table", me.debug_id());
            inc_new_counter_info!("crdt-broadcast-not_enough_peers_error", 1);
            Err(CrdtError::NoPeers)?;
        }
        trace!(
            "{:x} transmit_index: {:?} received_index: {} broadcast_len: {}",
            me.debug_id(),
            *transmit_index,
            received_index,
            broadcast_table.len()
        );

        let old_transmit_index = transmit_index.data;

        // enumerate all the blobs in the window, those are the indices
        // transmit them to nodes, starting from a different node
        let mut orders = Vec::with_capacity((received_index - transmit_index.data) as usize);
        let window_l = window.write().unwrap();

        let mut br_idx = transmit_index.data as usize % broadcast_table.len();

        for idx in transmit_index.data..received_index {
            let w_idx = idx as usize % window_l.len();

            trace!(
                "{:x} broadcast order data w_idx {} br_idx {}",
                me.debug_id(),
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
                "{:x} broadcast order coding w_idx: {} br_idx  :{}",
                me.debug_id(),
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
                assert!(blob.meta.size <= BLOB_SIZE);
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
                eprintln!("broadcast result {:?}", e);
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
            (s.table[&s.me].clone(), s.table.values().cloned().collect())
        };
        blob.write()
            .unwrap()
            .set_id(me.id)
            .expect("set_id in pub fn retransmit");
        let rblob = blob.read().unwrap();
        let orders: Vec<_> = table
            .iter()
            .filter(|v| {
                if me.id == v.id {
                    false
                } else if me.leader_id == v.id {
                    trace!("skip retransmit to leader {:?}", v.id);
                    false
                } else if !(Self::is_valid_address(v.contact_info.tvu)) {
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
                assert!(rblob.meta.size <= BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.contact_info.tvu)
            })
            .collect();
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

    fn random() -> u64 {
        thread_rng().next_u64()
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
        let id = self.me;
        let ups = self.update_index;
        (id, ups, data)
    }

    pub fn valid_last_ids(&self) -> Vec<Hash> {
        self.table
            .values()
            .filter(|r| {
                r.id != Pubkey::default()
                    && (Self::is_valid_address(r.contact_info.tpu)
                        || Self::is_valid_address(r.contact_info.tvu))
            })
            .map(|x| x.ledger_state.last_id)
            .collect()
    }

    pub fn window_index_request(&self, ix: u64) -> Result<(SocketAddr, Vec<u8>)> {
        let valid: Vec<_> = self
            .table
            .values()
            .filter(|r| r.id != self.me && Self::is_valid_address(r.contact_info.tvu_window))
            .collect();
        if valid.is_empty() {
            Err(CrdtError::NoPeers)?;
        }
        let n = (Self::random() as usize) % valid.len();
        let addr = valid[n].contact_info.ncp;
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
        let options: Vec<_> = self
            .table
            .values()
            .filter(|v| {
                v.id != self.me
                    && !v.contact_info.ncp.ip().is_unspecified()
                    && !v.contact_info.ncp.ip().is_multicast()
            })
            .collect();

        let choose_peer_strategy = ChooseWeightedPeerStrategy::new(
            &self.remote,
            &self.external_liveness,
            &Self::get_stake,
        );

        let choose_peer_result = choose_peer_strategy.choose_peer(options);

        if let Err(Error::CrdtError(CrdtError::NoPeers)) = &choose_peer_result {
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
        let mut q: VecDeque<SharedBlob> = VecDeque::new();
        q.push_back(blob);
        blob_sender.send(q)?;
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
            trace!("leader {:x} {}", make_debug_id(&v.leader_id), *cnt);
        }
        let mut sorted: Vec<(&Pubkey, usize)> = table.into_iter().collect();
        let my_id = self.debug_id();
        for x in &sorted {
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
                obj.write().unwrap().update_leader();
                let elapsed = timestamp() - start;
                if GOSSIP_SLEEP_MILLIS > elapsed {
                    let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                    sleep(Duration::from_millis(time_left));
                }
            })
            .unwrap()
    }
    fn run_window_request(
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        me: &NodeInfo,
        from: &NodeInfo,
        ix: u64,
        blob_recycler: &BlobRecycler,
    ) -> Option<SharedBlob> {
        let pos = (ix as usize) % window.read().unwrap().len();
        if let Some(blob) = &window.read().unwrap()[pos].data {
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
                    Some(&from.contact_info.tvu_window),
                );

                return Some(out);
            }
        }

        inc_new_counter_info!("crdt-window-request-fail", 1);
        trace!(
            "{:x}: failed RequestWindowIndex {:x} {} {}",
            me.debug_id(),
            from.debug_id(),
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
            Ok(request) => {
                Crdt::handle_protocol(request, obj, window, ledger_window, blob_recycler)
            }
            Err(_) => {
                warn!("deserialize crdt packet failed");
                None
            }
        }
    }

    fn handle_protocol(
        request: Protocol,
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        blob_recycler: &BlobRecycler,
    ) -> Option<SharedBlob> {
        match request {
            // TODO sigverify these
            Protocol::RequestUpdates(v, from_rd) => {
                let addr = from_rd.contact_info.ncp;
                trace!("RequestUpdates {} from {}", v, addr);
                let me = obj.read().unwrap();
                if addr == me.table[&me.me].contact_info.ncp {
                    warn!(
                        "RequestUpdates ignored, I'm talking to myself: me={:x} remoteme={:x}",
                        me.debug_id(),
                        make_debug_id(&from_rd.id)
                    );
                    inc_new_counter_info!("crdt-window-request-loopback", 1);
                    return None;
                }
                // only lock for these two calls, dont lock during IO `sock.send_to` or `sock.recv_from`
                let (from, ups, data) = me.get_updates_since(v);
                let external_liveness = me.remote.iter().map(|(k, v)| (*k, *v)).collect();
                drop(me);
                trace!("get updates since response {} {}", v, data.len());
                let len = data.len();
                let rsp = Protocol::ReceiveUpdates(from, ups, data, external_liveness);
                {
                    let mut me = obj.write().unwrap();
                    me.insert(&from_rd);
                    me.update_liveness(from_rd.id);
                }
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
            Protocol::ReceiveUpdates(from, update_index, data, external_liveness) => {
                let now = Instant::now();
                trace!(
                    "ReceivedUpdates from={:x} update_index={} len={}",
                    make_debug_id(&from),
                    update_index,
                    data.len()
                );
                obj.write()
                    .expect("'obj' write lock in ReceiveUpdates")
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
                //TODO this doesn't depend on CRDT module, can be moved
                //but we are using the listen thread to service these request
                //TODO verify from is signed
                obj.write().unwrap().insert(&from);
                let me = obj.read().unwrap().my_data().clone();
                inc_new_counter_info!("crdt-window-request-recv", 1);
                trace!(
                    "{:x}:received RequestWindowIndex {:x} {} ",
                    me.debug_id(),
                    from.debug_id(),
                    ix,
                );
                if from.contact_info.tvu_window == me.contact_info.tvu_window {
                    warn!(
                        "Ignored {:x}:received RequestWindowIndex from ME {:x} {} ",
                        me.debug_id(),
                        from.debug_id(),
                        ix,
                    );
                    inc_new_counter_info!("crdt-window-request-address-eq", 1);
                    return None;
                }
                let res =
                    Self::run_window_request(&window, ledger_window, &me, &from, ix, blob_recycler);
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
        let mut resps = VecDeque::new();
        while let Some(req) = reqs.pop_front() {
            if let Some(resp) = Self::handle_blob(
                obj,
                window,
                ledger_window,
                blob_recycler,
                &req.read().unwrap(),
            ) {
                resps.push_back(resp);
            }
            blob_recycler.recycle(req);
        }
        response_sender.send(resps)?;
        Ok(())
    }
    pub fn listen(
        obj: Arc<RwLock<Self>>,
        window: SharedWindow,
        ledger_path: Option<&str>,
        blob_recycler: BlobRecycler,
        requests_receiver: BlobReceiver,
        response_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let debug_id = obj.read().unwrap().debug_id();

        let mut ledger_window = ledger_path.map(|p| LedgerWindow::open(p).unwrap());

        Builder::new()
            .name("solana-listen".to_string())
            .spawn(move || loop {
                let e = Self::run_listen(
                    &obj,
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
                    debug!(
                        "{:x}: run_listen timeout, table size: {}",
                        debug_id,
                        obj.read().unwrap().table.len()
                    );
                }
            })
            .unwrap()
    }

    fn is_valid_ip_internal(addr: IpAddr, cfg_test: bool) -> bool {
        !(addr.is_unspecified() || addr.is_multicast() || (addr.is_loopback() && !cfg_test))
    }
    pub fn is_valid_ip(addr: IpAddr) -> bool {
        Self::is_valid_ip_internal(addr, cfg!(test) || cfg!(feature = "test"))
    }
    /// port must not be 0
    /// ip must be specified and not mulitcast
    /// loopback ip is only allowed in tests
    pub fn is_valid_address(addr: SocketAddr) -> bool {
        (addr.port() != 0) && Self::is_valid_ip(addr.ip())
    }

    pub fn spy_node(addr: IpAddr) -> (NodeInfo, UdpSocket, UdpSocket) {
        let gossip_socket = udp_random_bind(8000, 10000, 5).unwrap();
        let gossip_send_socket = udp_random_bind(8000, 10000, 5).unwrap();
        let gossip_addr = SocketAddr::new(addr, gossip_socket.local_addr().unwrap().port());
        let pubkey = Keypair::new().pubkey();
        let daddr = "0.0.0.0:0".parse().unwrap();
        let node = NodeInfo::new(pubkey, gossip_addr, daddr, daddr, daddr, daddr);
        (node, gossip_socket, gossip_send_socket)
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
            data,
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
        let mut local_gossip_addr = bind_addr;
        local_gossip_addr.set_port(data.contact_info.ncp.port());

        let mut local_replicate_addr = bind_addr;
        local_replicate_addr.set_port(data.contact_info.tvu.port());

        let mut local_requests_addr = bind_addr;
        local_requests_addr.set_port(data.contact_info.rpu.port());

        let mut local_transactions_addr = bind_addr;
        local_transactions_addr.set_port(data.contact_info.tpu.port());

        let mut local_repair_addr = bind_addr;
        local_repair_addr.set_port(data.contact_info.tvu_window.port());

        fn bind(addr: SocketAddr) -> UdpSocket {
            match UdpSocket::bind(addr) {
                Ok(socket) => socket,
                Err(err) => {
                    panic!("Failed to bind to {:?}: {:?}", addr, err);
                }
            }
        };

        let transaction = bind(local_transactions_addr);
        let gossip = bind(local_gossip_addr);
        let replicate = bind(local_replicate_addr);
        let repair = bind(local_repair_addr);
        let requests = bind(local_requests_addr);

        // Responses are sent from the same Udp port as requests are received
        // from, in hopes that a NAT sitting in the middle will route the
        // response Udp packet correctly back to the requester.
        let respond = requests.try_clone().unwrap();

        let gossip_send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        TestNode {
            data,
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
    pub fn new_with_external_ip(
        pubkey: Pubkey,
        ip: IpAddr,
        port_range: (u16, u16),
        ncp_port: u16,
    ) -> TestNode {
        fn bind(port_range: (u16, u16)) -> (u16, UdpSocket) {
            match udp_random_bind(port_range.0, port_range.1, 5) {
                Ok(socket) => (socket.local_addr().unwrap().port(), socket),
                Err(err) => {
                    panic!("Failed to bind to {:?}", err);
                }
            }
        };

        fn bind_to(port: u16) -> UdpSocket {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
            match UdpSocket::bind(addr) {
                Ok(socket) => socket,
                Err(err) => {
                    panic!("Failed to bind to {:?}: {:?}", addr, err);
                }
            }
        };

        let (gossip_port, gossip) = if ncp_port != 0 {
            (ncp_port, bind_to(ncp_port))
        } else {
            bind(port_range)
        };
        let (replicate_port, replicate) = bind(port_range);
        let (requests_port, requests) = bind(port_range);
        let (transaction_port, transaction) = bind(port_range);
        let (repair_port, repair) = bind(port_range);

        // Responses are sent from the same Udp port as requests are received
        // from, in hopes that a NAT sitting in the middle will route the
        // response Udp packet correctly back to the requester.
        let respond = requests.try_clone().unwrap();

        let gossip_send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();

        let node_info = NodeInfo::new(
            pubkey,
            SocketAddr::new(ip, gossip_port),
            SocketAddr::new(ip, replicate_port),
            SocketAddr::new(ip, requests_port),
            SocketAddr::new(ip, transaction_port),
            SocketAddr::new(ip, repair_port),
        );

        TestNode {
            data: node_info,
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

fn report_time_spent(label: &str, time: &Duration, extra: &str) {
    let count = duration_as_ms(time);
    if count > 5 {
        info!("{} took: {} ms {}", label, count, extra);
    }
}

#[cfg(test)]
mod tests {
    use crdt::{
        parse_port_or_addr, Crdt, CrdtError, NodeInfo, Protocol, TestNode, GOSSIP_PURGE_MILLIS,
        GOSSIP_SLEEP_MILLIS, MIN_TABLE_SIZE,
    };
    use entry::Entry;
    use hash::{hash, Hash};
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
    use transaction::Vote;
    use window::default_window;

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
    fn test_bad_address() {
        let d1 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.0:1234".parse().unwrap(),
            "0.0.0.0:1235".parse().unwrap(),
            "0.0.0.0:1236".parse().unwrap(),
            "0.0.0.0:1237".parse().unwrap(),
            "0.0.0.0:1238".parse().unwrap(),
        );
        assert_matches!(
            Crdt::new(d1).err(),
            Some(Error::CrdtError(CrdtError::BadGossipAddress))
        );
        let d1_1 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.1:1234".parse().unwrap(),
            "0.0.0.0:1235".parse().unwrap(),
            "0.0.0.0:1236".parse().unwrap(),
            "0.0.0.0:1237".parse().unwrap(),
            "0.0.0.0:1238".parse().unwrap(),
        );
        assert_matches!(
            Crdt::new(d1_1).err(),
            Some(Error::CrdtError(CrdtError::BadContactInfo))
        );
        let d2 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
        );
        assert_matches!(
            Crdt::new(d2).err(),
            Some(Error::CrdtError(CrdtError::BadGossipAddress))
        );
        let d2_1 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.1:1234".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.1:0".parse().unwrap(),
        );
        assert_matches!(
            Crdt::new(d2_1).err(),
            Some(Error::CrdtError(CrdtError::BadContactInfo))
        );
        let d3 = NodeInfo::new_unspecified();
        assert_matches!(
            Crdt::new(d3).err(),
            Some(Error::CrdtError(CrdtError::BadGossipAddress))
        );
        let d4 = NodeInfo::new_multicast();
        assert_matches!(
            Crdt::new(d4).err(),
            Some(Error::CrdtError(CrdtError::BadGossipAddress))
        );
        let mut d5 = NodeInfo::new_multicast();
        d5.version = 1;
        assert_matches!(
            Crdt::new(d5).err(),
            Some(Error::CrdtError(CrdtError::BadNodeInfo))
        );
        let d6 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.0:1234".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
        );
        assert_matches!(
            Crdt::new(d6).err(),
            Some(Error::CrdtError(CrdtError::BadGossipAddress))
        );
        let d7 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.1:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
        );
        assert_matches!(
            Crdt::new(d7).err(),
            Some(Error::CrdtError(CrdtError::BadGossipAddress))
        );
        let d8 = NodeInfo::new(
            Keypair::new().pubkey(),
            "0.0.0.1:1234".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
        );
        assert_eq!(Crdt::new(d8).is_ok(), true);
    }

    #[test]
    fn insert_test() {
        let mut d = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
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
        let d = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone()).unwrap();
        assert_eq!(crdt.table[&d.id].version, 0);
        let leader = NodeInfo::new_leader(&"127.0.0.2:1235".parse().unwrap());
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
        let d = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
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
    fn replicated_data_new_leader_with_pubkey() {
        let keypair = Keypair::new();
        let d1 = NodeInfo::new_leader_with_pubkey(
            keypair.pubkey().clone(),
            &"127.0.0.1:1234".parse().unwrap(),
        );
        assert_eq!(d1.id, keypair.pubkey());
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
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let d2 = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let d3 = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
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
        let d4 = NodeInfo::new_entry_point("127.0.0.4:1234".parse().unwrap());
        crdt.insert(&d4);
        let (_key, _ix, ups) = crdt.get_updates_since(0);
        assert_eq!(sorted(&ups), sorted(&vec![d2.clone(), d1, d3]));
    }
    #[test]
    fn window_index_request() {
        let me = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let rv = crdt.window_index_request(0);
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
        );
        crdt.insert(&nxt);
        let rv = crdt.window_index_request(0);
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));
        let nxt = NodeInfo::new(
            Keypair::new().pubkey(),
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
            Keypair::new().pubkey(),
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

    #[test]
    fn gossip_request_bad_addr() {
        let me = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:127".parse().unwrap(),
            "127.0.0.1:127".parse().unwrap(),
            "127.0.0.1:127".parse().unwrap(),
            "127.0.0.1:127".parse().unwrap(),
            "127.0.0.1:127".parse().unwrap(),
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
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let rv = crdt.gossip_request();
        assert_matches!(rv, Err(Error::CrdtError(CrdtError::NoPeers)));
        let nxt1 = NodeInfo::new(
            Keypair::new().pubkey(),
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
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
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

        let mut nxt2 = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
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
        let me = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let nxt = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
        assert_ne!(me.id, nxt.id);
        crdt.insert(&nxt);
        crdt.set_leader(nxt.id);
        let now = crdt.alive[&nxt.id];
        let mut nxt2 = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
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
        let window = default_window();
        let me = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let recycler = BlobRecycler::default();
        let rv = Crdt::run_window_request(&window, &mut None, &me, &me, 0, &recycler);
        assert!(rv.is_none());
        let out = recycler.allocate();
        out.write().unwrap().meta.size = 200;
        window.write().unwrap()[0].data = Some(out);
        let rv = Crdt::run_window_request(&window, &mut None, &me, &me, 0, &recycler);
        assert!(rv.is_some());
        let v = rv.unwrap();
        //test we copied the blob
        assert_eq!(v.read().unwrap().meta.size, 200);
        let len = window.read().unwrap().len() as u64;
        let rv = Crdt::run_window_request(&window, &mut None, &me, &me, len, &recycler);
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
            &window,
            &mut Some(&mut ledger_window),
            &me,
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
        let window = default_window();

        let mut me = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        me.leader_id = me.id;

        let mock_peer = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());

        let recycler = BlobRecycler::default();

        // Simulate handling a repair request from mock_peer
        let rv = Crdt::run_window_request(&window, &mut None, &me, &mock_peer, 0, &recycler);
        assert!(rv.is_none());
        let blob = recycler.allocate();
        let blob_size = 200;
        blob.write().unwrap().meta.size = blob_size;
        window.write().unwrap()[0].data = Some(blob);

        let num_requests: u32 = 64;
        for i in 0..num_requests {
            let shared_blob = Crdt::run_window_request(
                &window, &mut None, &me, &mock_peer, 0, &recycler,
            ).unwrap();
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
        let mut crdt = Crdt::new(me.clone()).expect("Crdt::new");
        assert_eq!(crdt.top_leader(), None);
        crdt.set_leader(leader0.id);
        assert_eq!(crdt.top_leader().unwrap(), leader0.id);
        //add a bunch of nodes with a new leader
        for _ in 0..10 {
            let mut dum = NodeInfo::new_entry_point("127.0.0.1:1234".parse().unwrap());
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
        let mut leader0 = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
        leader0.ledger_state.last_id = hash(b"0");
        let mut leader1 = NodeInfo::new_multicast();
        leader1.ledger_state.last_id = hash(b"1");
        let mut leader2 =
            NodeInfo::new_leader_with_pubkey(Pubkey::default(), &"127.0.0.2:1234".parse().unwrap());
        leader2.ledger_state.last_id = hash(b"2");
        // test that only valid tvu or tpu are retured as nodes
        let mut leader3 = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
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
        let window = default_window();
        let recycler = BlobRecycler::default();

        let node = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let node_with_same_addr = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_ne!(node.id, node_with_same_addr.id);
        let node_with_diff_addr = NodeInfo::new_leader(&"127.0.0.1:4321".parse().unwrap());

        let crdt = Crdt::new(node.clone()).expect("Crdt::new");
        assert_eq!(crdt.alive.len(), 0);

        let obj = Arc::new(RwLock::new(crdt));

        let request = Protocol::RequestUpdates(1, node.clone());
        assert!(Crdt::handle_protocol(request, &obj, &window, &mut None, &recycler).is_none());

        let request = Protocol::RequestUpdates(1, node_with_same_addr.clone());
        assert!(Crdt::handle_protocol(request, &obj, &window, &mut None, &recycler).is_none());

        let request = Protocol::RequestUpdates(1, node_with_diff_addr.clone());
        Crdt::handle_protocol(request, &obj, &window, &mut None, &recycler);

        let me = obj.write().unwrap();

        // |node| and |node_with_same_addr| should not be in me.alive, but
        // |node_with_diff_addr| should now be.
        assert!(!me.alive.contains_key(&node.id));
        assert!(!me.alive.contains_key(&node_with_same_addr.id));
        assert!(me.alive[&node_with_diff_addr.id] > 0);
    }

    #[test]
    fn test_is_valid_address() {
        assert!(cfg!(test));
        let bad_address_port = "127.0.0.1:0".parse().unwrap();
        assert!(!Crdt::is_valid_address(bad_address_port));
        let bad_address_unspecified = "0.0.0.0:1234".parse().unwrap();
        assert!(!Crdt::is_valid_address(bad_address_unspecified));
        let bad_address_multicast = "224.254.0.0:1234".parse().unwrap();
        assert!(!Crdt::is_valid_address(bad_address_multicast));
        let loopback = "127.0.0.1:1234".parse().unwrap();
        assert!(Crdt::is_valid_address(loopback));
        assert!(!Crdt::is_valid_ip_internal(loopback.ip(), false));
    }

    #[test]
    fn test_default_leader() {
        logger::setup();
        let node_info = NodeInfo::new(
            Keypair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let mut crdt = Crdt::new(node_info).unwrap();
        let network_entry_point = NodeInfo::new_entry_point("127.0.0.1:1239".parse().unwrap());
        crdt.insert(&network_entry_point);
        assert!(crdt.leader_data().is_none());
    }

    #[test]
    fn new_with_external_ip_test_random() {
        let sockaddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8080);
        let node =
            TestNode::new_with_external_ip(Keypair::new().pubkey(), sockaddr.ip(), (8100, 8200), 0);

        assert_eq!(
            node.sockets.gossip.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.replicate.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.requests.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.transaction.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.repair.local_addr().unwrap().ip(),
            sockaddr.ip()
        );

        assert!(node.sockets.gossip.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.gossip.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.replicate.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.replicate.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.requests.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.requests.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.transaction.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.transaction.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.repair.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.repair.local_addr().unwrap().port() <= 8200);
    }

    #[test]
    fn new_with_external_ip_test_gossip() {
        let sockaddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8080);
        let node = TestNode::new_with_external_ip(
            Keypair::new().pubkey(),
            sockaddr.ip(),
            (8100, 8200),
            8050,
        );
        assert_eq!(
            node.sockets.gossip.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.replicate.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.requests.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.transaction.local_addr().unwrap().ip(),
            sockaddr.ip()
        );
        assert_eq!(
            node.sockets.repair.local_addr().unwrap().ip(),
            sockaddr.ip()
        );

        assert_eq!(node.sockets.gossip.local_addr().unwrap().port(), 8050);
        assert!(node.sockets.replicate.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.replicate.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.requests.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.requests.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.transaction.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.transaction.local_addr().unwrap().port() <= 8200);
        assert!(node.sockets.repair.local_addr().unwrap().port() >= 8100);
        assert!(node.sockets.repair.local_addr().unwrap().port() <= 8200);
    }
}
