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
use hash::Hash;
use packet::{to_blob, Blob, BlobRecycler, SharedBlob, BLOB_SIZE};
use pnet_datalink as datalink;
use rayon::prelude::*;
use result::{Error, Result};
use ring::rand::{SecureRandom, SystemRandom};
use signature::{KeyPair, KeyPairUtil, PublicKey, Signature};
use std;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::Duration;
use streamer::{BlobReceiver, BlobSender, Window};
use timing::timestamp;

/// milliseconds we sleep for between gossip requests
const GOSSIP_SLEEP_MILLIS: u64 = 100;
//const GOSSIP_MIN_PURGE_MILLIS: u64 = 15000;

/// minimum membership table size before we start purging dead nodes
const MIN_TABLE_SIZE: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum CrdtError {
    TooSmall,
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
pub struct ReplicatedData {
    pub id: PublicKey,
    sig: Signature,
    /// should always be increasing
    pub version: u64,
    /// address to connect to for gossip
    pub gossip_addr: SocketAddr,
    /// address to connect to for replication
    pub replicate_addr: SocketAddr,
    /// address to connect to when this node is leader
    pub requests_addr: SocketAddr,
    /// transactions address
    pub transactions_addr: SocketAddr,
    /// repair address, we use this to jump ahead of the packets
    /// destined to the replciate_addr
    pub repair_addr: SocketAddr,
    /// current leader identity
    pub current_leader_id: PublicKey,
    /// last verified hash that was submitted to the leader
    last_verified_hash: Hash,
    /// last verified count, always increasing
    last_verified_count: u64,
}

fn make_debug_id(buf: &[u8]) -> u64 {
    let mut rdr = Cursor::new(&buf[..8]);
    rdr.read_u64::<LittleEndian>()
        .expect("rdr.read_u64 in fn debug_id")
}

impl ReplicatedData {
    pub fn new(
        id: PublicKey,
        gossip_addr: SocketAddr,
        replicate_addr: SocketAddr,
        requests_addr: SocketAddr,
        transactions_addr: SocketAddr,
        repair_addr: SocketAddr,
    ) -> ReplicatedData {
        ReplicatedData {
            id,
            sig: Signature::default(),
            version: 0,
            gossip_addr,
            replicate_addr,
            requests_addr,
            transactions_addr,
            repair_addr,
            current_leader_id: PublicKey::default(),
            last_verified_hash: Hash::default(),
            last_verified_count: 0,
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

    pub fn new_leader(bind_addr: &SocketAddr) -> Self {
        let transactions_addr = bind_addr.clone();
        let gossip_addr = Self::next_port(&bind_addr, 1);
        let replicate_addr = Self::next_port(&bind_addr, 2);
        let requests_addr = Self::next_port(&bind_addr, 3);
        let repair_addr = Self::next_port(&bind_addr, 4);
        let pubkey = KeyPair::new().pubkey();
        ReplicatedData::new(
            pubkey,
            gossip_addr,
            replicate_addr,
            requests_addr,
            transactions_addr,
            repair_addr,
        )
    }
    pub fn new_entry_point(gossip_addr: SocketAddr) -> Self {
        let daddr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        ReplicatedData::new(
            PublicKey::default(),
            gossip_addr,
            daddr.clone(),
            daddr.clone(),
            daddr.clone(),
            daddr,
        )
    }
}

/// `Crdt` structure keeps a table of `ReplicatedData` structs
/// # Properties
/// * `table` - map of public id's to versioned and signed ReplicatedData structs
/// * `local` - map of public id's to what `self.update_index` `self.table` was updated
/// * `remote` - map of public id's to the `remote.update_index` was sent
/// * `update_index` - my update index
/// # Remarks
/// This implements two services, `gossip` and `listen`.
/// * `gossip` - asynchronously ask nodes to send updates
/// * `listen` - listen for requests and responses
/// No attempt to keep track of timeouts or dropped requests is made, or should be.
pub struct Crdt {
    pub table: HashMap<PublicKey, ReplicatedData>,
    /// Value of my update index when entry in table was updated.
    /// Nodes will ask for updates since `update_index`, and this node
    /// should respond with all the identities that are greater then the
    /// request's `update_index` in this list
    local: HashMap<PublicKey, u64>,
    /// The value of the remote update index that I have last seen
    /// This Node will ask external nodes for updates since the value in this list
    pub remote: HashMap<PublicKey, u64>,
    pub alive: HashMap<PublicKey, u64>,
    pub update_index: u64,
    pub me: PublicKey,
    external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>>,
}
// TODO These messages should be signed, and go through the gpu pipeline for spam filtering
#[derive(Serialize, Deserialize, Debug)]
enum Protocol {
    /// forward your own latest data structure when requesting an update
    /// this doesn't update the `remote` update index, but it allows the
    /// recepient of this request to add knowledge of this node to the network
    RequestUpdates(u64, ReplicatedData),
    //TODO might need a since?
    /// from id, form's last update index, ReplicatedData
    ReceiveUpdates(PublicKey, u64, Vec<ReplicatedData>, Vec<(PublicKey, u64)>),
    /// ask for a missing index
    RequestWindowIndex(ReplicatedData, u64),
}

impl Crdt {
    pub fn new(me: ReplicatedData) -> Crdt {
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
    pub fn my_data(&self) -> &ReplicatedData {
        &self.table[&self.me]
    }
    pub fn leader_data(&self) -> Option<&ReplicatedData> {
        self.table.get(&(self.table[&self.me].current_leader_id))
    }

    pub fn set_leader(&mut self, key: PublicKey) -> () {
        let mut me = self.my_data().clone();
        info!(
            "{:x}: setting leader to {:x} from {:x}",
            me.debug_id(),
            make_debug_id(&key),
            make_debug_id(&me.current_leader_id),
        );
        me.current_leader_id = key;
        me.version += 1;
        self.insert(&me);
    }

    pub fn get_external_liveness_entry(&self, key: &PublicKey) -> Option<&HashMap<PublicKey, u64>> {
        self.external_liveness.get(key)
    }

    pub fn insert(&mut self, v: &ReplicatedData) {
        // TODO check that last_verified types are always increasing
        if self.table.get(&v.id).is_none() || (v.version > self.table[&v.id].version) {
            //somehow we signed a message for our own identity with a higher version that
            // we have stored ourselves
            trace!(
                "me: {:x} v.id: {:x} version: {}",
                self.debug_id(),
                v.debug_id(),
                v.version
            );
            self.update_index += 1;
            let _ = self.table.insert(v.id.clone(), v.clone());
            let _ = self.local.insert(v.id, self.update_index);
        } else {
            trace!(
                "INSERT FAILED me: {:x} data: {:?} new.version: {} me.version: {}",
                self.debug_id(),
                v.debug_id(),
                v.version,
                self.table[&v.id].version
            );
        }
        //update the liveness table
        let now = timestamp();
        *self.alive.entry(v.id).or_insert(now) = now;
    }

    /// purge old validators
    /// TODO: we need a robust membership protocol
    /// http://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf
    /// challenging part is that we are on a permissionless network
    pub fn purge(&mut self, now: u64) {
        if self.table.len() <= MIN_TABLE_SIZE {
            return;
        }

        //wait for 4x as long as it would randomly take to reach our node
        //assuming everyone is waiting the same amount of time as this node
        let limit = self.table.len() as u64 * GOSSIP_SLEEP_MILLIS * 4;
        //let limit = std::cmp::max(limit, GOSSIP_MIN_PURGE_MILLIS);

        let dead_ids: Vec<PublicKey> = self.alive
            .iter()
            .filter_map(|(&k, v)| {
                if k != self.me && (now - v) > limit {
                    info!("purge {:x} {}", make_debug_id(&k), now - v);
                    Some(k)
                } else {
                    trace!(
                        "purge skipped {:x} {} {}",
                        make_debug_id(&k),
                        now - v,
                        limit
                    );
                    None
                }
            })
            .collect();

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
        obj: &Arc<RwLock<Self>>,
        blobs: &Vec<SharedBlob>,
        receive_index: &mut u64,
    ) -> Result<()> {
        let me: ReplicatedData = {
            let robj = obj.read().expect("'obj' read lock in crdt::index_blobs");
            debug!(
                "{:x}: broadcast table {}",
                robj.debug_id(),
                robj.table.len()
            );
            robj.table[&robj.me].clone()
        };

        // enumerate all the blobs, those are the indices
        for (i, b) in blobs.iter().enumerate() {
            // only leader should be broadcasting
            let mut blob = b.write().expect("'blob' write lock in crdt::index_blobs");
            blob.set_id(me.id).expect("set_id in pub fn broadcast");
            blob.set_index(*receive_index + i as u64)
                .expect("set_index in pub fn broadcast");
        }

        Ok(())
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn broadcast(
        obj: &Arc<RwLock<Self>>,
        window: &Window,
        s: &UdpSocket,
        transmit_index: &mut u64,
        received_index: u64,
    ) -> Result<()> {
        let (me, table): (ReplicatedData, Vec<ReplicatedData>) = {
            // copy to avoid locking during IO
            let robj = obj.read().expect("'obj' read lock in pub fn broadcast");
            trace!("broadcast table {}", robj.table.len());
            let cloned_table: Vec<ReplicatedData> = robj.table.values().cloned().collect();
            (robj.table[&robj.me].clone(), cloned_table)
        };
        let daddr = "0.0.0.0:0".parse().unwrap();
        let nodes: Vec<&ReplicatedData> = table
            .iter()
            .filter(|v| {
                if me.id == v.id {
                    //filter myself
                    false
                } else if v.replicate_addr == daddr {
                    //filter nodes that are not listening
                    false
                } else {
                    trace!("broadcast node {}", v.replicate_addr);
                    true
                }
            })
            .collect();
        if nodes.len() < 1 {
            warn!("crdt too small");
            Err(CrdtError::TooSmall)?;
        }
        trace!("nodes table {}", nodes.len());

        // enumerate all the blobs in the window, those are the indices
        // transmit them to nodes, starting from a different node
        let mut orders = Vec::new();
        let window_l = window.write().unwrap();
        for i in *transmit_index..received_index {
            let is = i as usize;
            let k = is % window_l.len();
            assert!(window_l[k].is_some());

            orders.push((window_l[k].clone(), nodes[is % nodes.len()]));
        }

        trace!("orders table {}", orders.len());
        let errs: Vec<_> = orders
            .into_iter()
            .map(|(b, v)| {
                // only leader should be broadcasting
                assert!(me.current_leader_id != v.id);
                let bl = b.unwrap();
                let blob = bl.read().expect("blob read lock in streamer::broadcast");
                //TODO profile this, may need multiple sockets for par_iter
                trace!(
                    "broadcast idx: {} sz: {} to {} coding: {}",
                    blob.get_index().unwrap(),
                    blob.meta.size,
                    v.replicate_addr,
                    blob.is_coding()
                );
                assert!(blob.meta.size < BLOB_SIZE);
                let e = s.send_to(&blob.data[..blob.meta.size], &v.replicate_addr);
                trace!("done broadcast {} to {}", blob.meta.size, v.replicate_addr);
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
        let (me, table): (ReplicatedData, Vec<ReplicatedData>) = {
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
                } else if me.current_leader_id == v.id {
                    trace!("skip retransmit to leader {:?}", v.id);
                    false
                } else if v.replicate_addr == daddr {
                    trace!("skip nodes that are not listening {:?}", v.id);
                    false
                } else {
                    true
                }
            })
            .collect();
        let errs: Vec<_> = orders
            .par_iter()
            .map(|v| {
                info!(
                    "{:x}: retransmit blob {} to {:x}",
                    me.debug_id(),
                    rblob.get_index().unwrap(),
                    v.debug_id(),
                );
                //TODO profile this, may need multiple sockets for par_iter
                assert!(rblob.meta.size < BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.replicate_addr)
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
        let rnd = SystemRandom::new();
        let mut buf = [0u8; 8];
        rnd.fill(&mut buf).expect("rnd.fill in pub fn random");
        let mut rdr = Cursor::new(&buf);
        rdr.read_u64::<LittleEndian>()
            .expect("rdr.read_u64 in fn random")
    }

    // TODO: fill in with real implmentation once staking is implemented
    fn get_stake(_id: PublicKey) -> f64 {
        1.0
    }

    fn get_updates_since(&self, v: u64) -> (PublicKey, u64, Vec<ReplicatedData>) {
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
            .filter(|r| r.id != self.me && r.repair_addr != daddr)
            .collect();
        if valid.is_empty() {
            Err(CrdtError::TooSmall)?;
        }
        let n = (Self::random() as usize) % valid.len();
        let addr = valid[n].gossip_addr.clone();
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
                "crdt too small for gossip {:?} {}",
                &self.me[..4],
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
            v.gossip_addr
        );

        Ok((v.gossip_addr, req))
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
        let cur = self.table.values().filter(|x| x.current_leader_id != def);
        for v in cur {
            let cnt = table.entry(&v.current_leader_id).or_insert(0);
            *cnt += 1;
            trace!("leader {:x} {}", make_debug_id(&v.current_leader_id), *cnt);
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
            if self.my_data().current_leader_id != leader_id {
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
        data: &[ReplicatedData],
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
                obj.write().unwrap().purge(timestamp());
                if exit.load(Ordering::Relaxed) {
                    return;
                }
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
        me: &ReplicatedData,
        from: &ReplicatedData,
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
                if me.current_leader_id == me.id
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
                    outblob.meta.set_addr(&from.repair_addr);
                    outblob.set_id(sender_id).expect("blob set_id");
                }

                return Some(out);
            }
        } else {
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
                let addr = from_rd.gossip_addr;
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
                trace!(
                    "{:x}:received RequestWindowIndex {:x} {} ",
                    me.debug_id(),
                    from.debug_id(),
                    ix,
                );
                assert_ne!(from.repair_addr, me.repair_addr);
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
                if e.is_err() {
                    info!(
                        "run_listen timeout, table size: {}",
                        obj.read().unwrap().table.len()
                    );
                }
                if exit.load(Ordering::Relaxed) {
                    return;
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
    pub data: ReplicatedData,
    pub sockets: Sockets,
}

impl TestNode {
    pub fn new() -> TestNode {
        let transaction = UdpSocket::bind("0.0.0.0:0").unwrap();
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let replicate = UdpSocket::bind("0.0.0.0:0").unwrap();
        let requests = UdpSocket::bind("0.0.0.0:0").unwrap();
        let repair = UdpSocket::bind("0.0.0.0:0").unwrap();
        let gossip_send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let respond = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        let pubkey = KeyPair::new().pubkey();
        let data = ReplicatedData::new(
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
    pub fn new_with_bind_addr(data: ReplicatedData, bind_addr: SocketAddr) -> TestNode {
        let mut local_gossip_addr = bind_addr.clone();
        local_gossip_addr.set_port(data.gossip_addr.port());

        let mut local_replicate_addr = bind_addr.clone();
        local_replicate_addr.set_port(data.replicate_addr.port());

        let mut local_requests_addr = bind_addr.clone();
        local_requests_addr.set_port(data.requests_addr.port());

        let mut local_transactions_addr = bind_addr.clone();
        local_transactions_addr.set_port(data.transactions_addr.port());

        let mut local_repair_addr = bind_addr.clone();
        local_repair_addr.set_port(data.repair_addr.port());

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
        parse_port_or_addr, Crdt, CrdtError, ReplicatedData, GOSSIP_SLEEP_MILLIS, MIN_TABLE_SIZE,
    };
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
        let mut d = ReplicatedData::new(
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
    fn sorted(ls: &Vec<ReplicatedData>) -> Vec<ReplicatedData> {
        let mut copy: Vec<_> = ls.iter().cloned().collect();
        copy.sort_by(|x, y| x.id.cmp(&y.id));
        copy
    }
    #[test]
    fn replicated_data_new_leader() {
        let d1 = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_eq!(d1.gossip_addr, "127.0.0.1:1235".parse().unwrap());
        assert_eq!(d1.replicate_addr, "127.0.0.1:1236".parse().unwrap());
        assert_eq!(d1.requests_addr, "127.0.0.1:1237".parse().unwrap());
        assert_eq!(d1.transactions_addr, "127.0.0.1:1234".parse().unwrap());
        assert_eq!(d1.repair_addr, "127.0.0.1:1238".parse().unwrap());
    }
    #[test]
    fn update_test() {
        let d1 = ReplicatedData::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let d2 = ReplicatedData::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        let d3 = ReplicatedData::new(
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
        let d4 = ReplicatedData::new_entry_point("127.0.0.4:1234".parse().unwrap());
        crdt.insert(&d4);
        let (_key, _ix, ups) = crdt.get_updates_since(0);
        assert_eq!(sorted(&ups), sorted(&vec![d2.clone(), d1, d3]));
    }
    #[test]
    fn window_index_request() {
        let me = ReplicatedData::new(
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
        let nxt = ReplicatedData::new(
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
        let nxt = ReplicatedData::new(
            KeyPair::new().pubkey(),
            "127.0.0.2:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        crdt.insert(&nxt);
        let rv = crdt.window_index_request(0).unwrap();
        assert_eq!(nxt.gossip_addr, "127.0.0.2:1234".parse().unwrap());
        assert_eq!(rv.0, "127.0.0.2:1234".parse().unwrap());

        let nxt = ReplicatedData::new(
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
        let me = ReplicatedData::new(
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
        let nxt1 = ReplicatedData::new(
            KeyPair::new().pubkey(),
            "127.0.0.2:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );

        crdt.insert(&nxt1);

        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt1.gossip_addr);

        let nxt2 = ReplicatedData::new_entry_point("127.0.0.3:1234".parse().unwrap());
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
                if i.read().unwrap().meta.addr() == nxt1.gossip_addr {
                    one = true;
                } else if i.read().unwrap().meta.addr() == nxt2.gossip_addr {
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
        let me = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let mut crdt = Crdt::new(me.clone());
        let nxt = ReplicatedData::new_leader(&"127.0.0.2:1234".parse().unwrap());
        assert_ne!(me.id, nxt.id);
        crdt.insert(&nxt);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.gossip_addr);
        let now = crdt.alive[&nxt.id];
        let len = crdt.table.len() as u64;
        crdt.purge(now);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.gossip_addr);

        crdt.purge(now + len * GOSSIP_SLEEP_MILLIS * 4);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.gossip_addr);

        crdt.purge(now + len * GOSSIP_SLEEP_MILLIS * 4 + 1);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.gossip_addr);

        let nxt2 = ReplicatedData::new_leader(&"127.0.0.2:1234".parse().unwrap());
        assert_ne!(me.id, nxt2.id);
        assert_ne!(nxt.id, nxt2.id);
        crdt.insert(&nxt2);
        while now == crdt.alive[&nxt2.id] {
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
            crdt.insert(&nxt2);
        }
        let len = crdt.table.len() as u64;
        assert!((MIN_TABLE_SIZE as u64) < len);
        crdt.purge(now + len * GOSSIP_SLEEP_MILLIS * 4);
        assert_eq!(len as usize, crdt.table.len());
        crdt.purge(now + len * GOSSIP_SLEEP_MILLIS * 4 + 1);
        let rv = crdt.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.gossip_addr);
        assert_eq!(2, crdt.table.len());
    }

    /// test window requests respond with the right blob, and do not overrun
    #[test]
    fn run_window_request() {
        let window = default_window();
        let me = ReplicatedData::new(
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

        let mut me = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());
        me.current_leader_id = me.id;

        let mock_peer = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());

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
        let me = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let leader0 = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let leader1 = ReplicatedData::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let mut crdt = Crdt::new(me.clone());
        assert_eq!(crdt.top_leader(), None);
        crdt.set_leader(leader0.id);
        assert_eq!(crdt.top_leader().unwrap(), leader0.id);
        //add a bunch of nodes with a new leader
        for _ in 0..10 {
            let mut dum = ReplicatedData::new_entry_point("127.0.0.1:1234".parse().unwrap());
            dum.id = KeyPair::new().pubkey();
            dum.current_leader_id = leader1.id;
            crdt.insert(&dum);
        }
        assert_eq!(crdt.top_leader().unwrap(), leader1.id);
        crdt.update_leader();
        assert_eq!(crdt.my_data().current_leader_id, leader0.id);
        crdt.insert(&leader1);
        crdt.update_leader();
        assert_eq!(crdt.my_data().current_leader_id, leader1.id);
    }
}
