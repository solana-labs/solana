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
use hash::Hash;
use packet::{to_blob, Blob, BlobRecycler, SharedBlob, BLOB_SIZE};
use pnet::datalink;
use rayon::prelude::*;
use result::{Error, Result};
use ring::rand::{SecureRandom, SystemRandom};
use signature::{KeyPair, KeyPairUtil};
use signature::{PublicKey, Signature};
use std;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::Duration;
use streamer::{BlobReceiver, BlobSender};

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
                return Some(p.ip());
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
    pub update_index: u64,
    pub me: PublicKey,
    timeout: Duration,
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
    ReceiveUpdates(PublicKey, u64, Vec<ReplicatedData>),
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
            me: me.id,
            update_index: 1,
            timeout: Duration::from_millis(100),
        };
        g.local.insert(me.id, g.update_index);
        g.table.insert(me.id, me);
        g
    }
    pub fn my_data(&self) -> &ReplicatedData {
        &self.table[&self.me]
    }
    pub fn leader_data(&self) -> &ReplicatedData {
        &self.table[&self.table[&self.me].current_leader_id]
    }

    pub fn set_leader(&mut self, key: PublicKey) -> () {
        let mut me = self.my_data().clone();
        me.current_leader_id = key;
        me.version += 1;
        self.insert(&me);
    }

    pub fn insert(&mut self, v: &ReplicatedData) {
        // TODO check that last_verified types are always increasing
        if self.table.get(&v.id).is_none() || (v.version > self.table[&v.id].version) {
            //somehow we signed a message for our own identity with a higher version that
            // we have stored ourselves
            trace!(
                "me: {:?} v.id: {:?} version: {}",
                &self.me[..4],
                &v.id[..4],
                v.version
            );
            self.update_index += 1;
            let _ = self.table.insert(v.id.clone(), v.clone());
            let _ = self.local.insert(v.id, self.update_index);
        } else {
            trace!(
                "INSERT FAILED me: {:?} data: {:?} new.version: {} me.version: {}",
                &self.me[..4],
                &v.id[..4],
                v.version,
                self.table[&v.id].version
            );
        }
    }

    pub fn index_blobs(
        obj: &Arc<RwLock<Self>>,
        blobs: &Vec<SharedBlob>,
        transmit_index: &mut u64,
    ) -> Result<()> {
        let me: ReplicatedData = {
            let robj = obj.read().expect("'obj' read lock in crdt::index_blobs");
            info!("broadcast table {}", robj.table.len());
            robj.table[&robj.me].clone()
        };

        // enumerate all the blobs, those are the indices
        let orders: Vec<_> = blobs.iter().enumerate().collect();
        info!("orders table {}", orders.len());
        let _: Vec<_> = orders
            .into_iter()
            .map(|(i, b)| {
                // only leader should be broadcasting
                let mut blob = b.write().expect("'blob' write lock in crdt::index_blobs");
                blob.set_id(me.id).expect("set_id in pub fn broadcast");
                blob.set_index(*transmit_index + i as u64)
                    .expect("set_index in pub fn broadcast");
            })
            .collect();
        info!("set blobs index");

        Ok(())
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn broadcast(
        obj: &Arc<RwLock<Self>>,
        window: &Arc<RwLock<Vec<Option<SharedBlob>>>>,
        s: &UdpSocket,
        transmit_index: &mut u64,
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
            return Err(Error::CrdtTooSmall);
        }
        info!("nodes table {}", nodes.len());

        // enumerate all the blobs in the window, those are the indices
        // transmit them to nodes, starting from a different node
        let mut orders = Vec::new();
        let window_l = window.write().unwrap();
        let mut i = (*transmit_index as usize) % window_l.len();
        loop {
            if window_l[i].is_none() || orders.len() >= window_l.len() {
                break;
            }
            orders.push((window_l[i].clone(), nodes[i % nodes.len()]));
            i += 1;
            i %= window_l.len();
        }

        info!("orders table {}", orders.len());
        let errs: Vec<_> = orders
            .into_iter()
            .map(|(b, v)| {
                // only leader should be broadcasting
                assert!(me.current_leader_id != v.id);
                let bl = b.unwrap();
                let blob = bl.read().expect("blob read lock in streamer::broadcast");
                //TODO profile this, may need multiple sockets for par_iter
                trace!("broadcast {} to {}", blob.meta.size, v.replicate_addr);
                assert!(blob.meta.size < BLOB_SIZE);
                let e = s.send_to(&blob.data[..blob.meta.size], &v.replicate_addr);
                trace!("done broadcast {} to {}", blob.meta.size, v.replicate_addr);
                e
            })
            .collect();
        info!("broadcast results {}", errs.len());
        for e in errs {
            match e {
                Err(e) => {
                    error!("broadcast result {:?}", e);
                    return Err(Error::IO(e));
                }
                _ => (),
            }
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
                trace!(
                    "retransmit blob {} to {}",
                    rblob.get_index().unwrap(),
                    v.replicate_addr
                );
                //TODO profile this, may need multiple sockets for par_iter
                assert!(rblob.meta.size < BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.replicate_addr)
            })
            .collect();
        for e in errs {
            match e {
                Err(e) => {
                    info!("retransmit error {:?}", e);
                    return Err(Error::IO(e));
                }
                _ => (),
            }
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
    fn get_updates_since(&self, v: u64) -> (PublicKey, u64, Vec<ReplicatedData>) {
        //trace!("get updates since {}", v);
        let data = self.table
            .values()
            .filter(|x| self.local[&x.id] > v)
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
            return Err(Error::CrdtTooSmall);
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
        if options.len() < 1 {
            trace!(
                "crdt too small for gossip {:?} {}",
                &self.me[..4],
                self.table.len()
            );
            return Err(Error::CrdtTooSmall);
        }
        let n = (Self::random() as usize) % options.len();
        let v = options[n].clone();
        let remote_update_index = *self.remote.get(&v.id).unwrap_or(&0);
        let req = Protocol::RequestUpdates(remote_update_index, self.table[&self.me].clone());
        trace!(
            "created gossip request from {:?} to {:?} {}",
            &self.me[..4],
            &v.id[..4],
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

    /// Apply updates that we received from the identity `from`
    /// # Arguments
    /// * `from` - identity of the sender of the updates
    /// * `update_index` - the number of updates that `from` has completed and this set of `data` represents
    /// * `data` - the update data
    fn apply_updates(&mut self, from: PublicKey, update_index: u64, data: &[ReplicatedData]) {
        trace!("got updates {}", data.len());
        // TODO we need to punish/spam resist here
        // sig verify the whole update and slash anyone who sends a bad update
        for v in data {
            self.insert(&v);
        }
        *self.remote.entry(from).or_insert(update_index) = update_index;
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
                let _ = Self::run_gossip(&obj, &blob_sender, &blob_recycler);
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                //TODO this should be a tuned parameter
                sleep(
                    obj.read()
                        .expect("'obj' read lock in pub fn gossip")
                        .timeout,
                );
            })
            .unwrap()
    }
    fn run_window_request(
        window: &Arc<RwLock<Vec<Option<SharedBlob>>>>,
        from: &ReplicatedData,
        ix: u64,
        blob_recycler: &BlobRecycler,
    ) -> Option<SharedBlob> {
        let pos = (ix as usize) % window.read().unwrap().len();
        if let Some(blob) = &window.read().unwrap()[pos] {
            let rblob = blob.read().unwrap();
            let blob_ix = rblob.get_index().expect("run_window_request get_index");
            if blob_ix == ix {
                let out = blob_recycler.allocate();
                // copy to avoid doing IO inside the lock
                {
                    let mut outblob = out.write().unwrap();
                    let sz = rblob.meta.size;
                    outblob.meta.size = sz;
                    outblob.data[..sz].copy_from_slice(&rblob.data[..sz]);
                    outblob.meta.set_addr(&from.repair_addr);
                    //TODO, set the sender id to the requester so we dont retransmit
                    //come up with a cleaner solution for this when sender signatures are checked
                    outblob.set_id(from.id).expect("blob set_id");
                }
                return Some(out);
            }
        } else {
            assert!(window.read().unwrap()[pos].is_none());
            info!("failed RequestWindowIndex {} {}", ix, from.repair_addr);
        }
        None
    }

    //TODO we should first coalesce all the requests
    fn handle_blob(
        obj: &Arc<RwLock<Self>>,
        window: &Arc<RwLock<Vec<Option<SharedBlob>>>>,
        blob_recycler: &BlobRecycler,
        blob: &Blob,
    ) -> Option<SharedBlob> {
        match deserialize(&blob.data[..blob.meta.size]) {
            // TODO sigverify these
            Ok(Protocol::RequestUpdates(v, reqdata)) => {
                trace!("RequestUpdates {}", v);
                let addr = reqdata.gossip_addr;
                // only lock for this call, dont lock during IO `sock.send_to` or `sock.recv_from`
                let (from, ups, data) = obj.read()
                    .expect("'obj' read lock in RequestUpdates")
                    .get_updates_since(v);
                trace!("get updates since response {} {}", v, data.len());
                let len = data.len();
                let rsp = Protocol::ReceiveUpdates(from, ups, data);
                obj.write().unwrap().insert(&reqdata);
                if len < 1 {
                    let me = obj.read().unwrap();
                    trace!(
                        "no updates me {:?} ix {} since {}",
                        &me.me[..4],
                        me.update_index,
                        v
                    );
                    None
                } else if let Ok(r) = to_blob(rsp, addr, &blob_recycler) {
                    trace!(
                        "sending updates me {:?} len {} to {:?} {}",
                        &obj.read().unwrap().me[..4],
                        len,
                        &reqdata.id[..4],
                        addr,
                    );
                    Some(r)
                } else {
                    warn!("to_blob failed");
                    None
                }
            }
            Ok(Protocol::ReceiveUpdates(from, ups, data)) => {
                trace!("ReceivedUpdates {:?} {} {}", &from[0..4], ups, data.len());
                obj.write()
                    .expect("'obj' write lock in ReceiveUpdates")
                    .apply_updates(from, ups, &data);
                None
            }
            Ok(Protocol::RequestWindowIndex(from, ix)) => {
                //TODO this doesn't depend on CRDT module, can be moved
                //but we are using the listen thread to service these request
                //TODO verify from is signed
                obj.write().unwrap().insert(&from);
                let me = obj.read().unwrap().my_data().clone();
                trace!(
                    "received RequestWindowIndex {} {} myaddr {}",
                    ix,
                    from.repair_addr,
                    me.repair_addr
                );
                assert_ne!(from.repair_addr, me.repair_addr);
                Self::run_window_request(&window, &from, ix, blob_recycler)
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
        window: &Arc<RwLock<Vec<Option<SharedBlob>>>>,
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
        window: Arc<RwLock<Vec<Option<SharedBlob>>>>,
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
}

pub struct TestNode {
    pub data: ReplicatedData,
    pub sockets: Sockets,
}

impl TestNode {
    pub fn new() -> TestNode {
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let gossip_send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let requests = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transaction = UdpSocket::bind("0.0.0.0:0").unwrap();
        let replicate = UdpSocket::bind("0.0.0.0:0").unwrap();
        let respond = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let repair = UdpSocket::bind("0.0.0.0:0").unwrap();
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crdt::{parse_port_or_addr, Crdt, ReplicatedData};
    use packet::BlobRecycler;
    use result::Error;
    use signature::{KeyPair, KeyPairUtil};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
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
        assert_eq!(sorted(&ups), sorted(&vec![d2.clone(), d1, d3]));
        let mut crdt2 = Crdt::new(d2.clone());
        crdt2.apply_updates(key, ix, &ups);
        assert_eq!(crdt2.table.values().len(), 3);
        assert_eq!(
            sorted(&crdt2.table.values().map(|x| x.clone()).collect()),
            sorted(&crdt.table.values().map(|x| x.clone()).collect())
        );
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
        assert_matches!(rv, Err(Error::CrdtTooSmall));
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
        assert_matches!(rv, Err(Error::CrdtTooSmall));
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
        assert_matches!(rv, Err(Error::CrdtTooSmall));
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

        let nxt2 = ReplicatedData::new(
            KeyPair::new().pubkey(),
            "127.0.0.3:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
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
        let rv = Crdt::run_window_request(&window, &me, 0, &recycler);
        assert!(rv.is_none());
        let out = recycler.allocate();
        out.write().unwrap().meta.size = 200;
        window.write().unwrap()[0] = Some(out);
        let rv = Crdt::run_window_request(&window, &me, 0, &recycler);
        assert!(rv.is_some());
        let v = rv.unwrap();
        //test we copied the blob
        assert_eq!(v.read().unwrap().meta.size, 200);
        let len = window.read().unwrap().len() as u64;
        let rv = Crdt::run_window_request(&window, &me, len, &recycler);
        assert!(rv.is_none());
    }
}
