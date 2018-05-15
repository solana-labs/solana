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
use packet::SharedBlob;
use rayon::prelude::*;
use result::{Error, Result};
use ring::rand::{SecureRandom, SystemRandom};
use signature::{PublicKey, Signature};
use std::collections::HashMap;
use std::io::Cursor;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::Duration;

/// Structure to be replicated by the network
#[derive(Serialize, Deserialize, Clone)]
pub struct ReplicatedData {
    pub id: PublicKey,
    sig: Signature,
    /// should always be increasing
    version: u64,
    /// address to connect to for gossip
    pub gossip_addr: SocketAddr,
    /// address to connect to for replication
    pub replicate_addr: SocketAddr,
    /// address to connect to when this node is leader
    pub serve_addr: SocketAddr,
    /// current leader identity
    current_leader_id: PublicKey,
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
        serve_addr: SocketAddr,
    ) -> ReplicatedData {
        ReplicatedData {
            id,
            sig: Signature::default(),
            version: 0,
            gossip_addr,
            replicate_addr,
            serve_addr,
            current_leader_id: PublicKey::default(),
            last_verified_hash: Hash::default(),
            last_verified_count: 0,
        }
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
#[derive(Serialize, Deserialize)]
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
            trace!("me: {:?}", self.me[0]);
            trace!("v.id: {:?}", v.id[0]);
            trace!("insert! {}", v.version);
            self.update_index += 1;
            let _ = self.table.insert(v.id.clone(), v.clone());
            let _ = self.local.insert(v.id, self.update_index);
        } else {
            trace!(
                "INSERT FAILED new.version: {} me.version: {}",
                v.version,
                self.table[&v.id].version
            );
        }
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing any io, such as the `send_to`
    pub fn broadcast(
        obj: &Arc<RwLock<Self>>,
        blobs: &Vec<SharedBlob>,
        s: &UdpSocket,
        transmit_index: &mut u64,
    ) -> Result<()> {
        let (me, table): (ReplicatedData, Vec<ReplicatedData>) = {
            // copy to avoid locking during IO
            let robj = obj.read().expect("'obj' read lock in pub fn broadcast");
            info!("broadcast table {}", robj.table.len());
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
                    info!("broadcast node {}", v.replicate_addr);
                    true
                }
            })
            .collect();
        if nodes.len() < 1 {
            return Err(Error::CrdtTooSmall);
        }

        info!("nodes table {}", nodes.len());
        info!("blobs table {}", blobs.len());
        // enumerate all the blobs, those are the indices
        // transmit them to nodes, starting from a different node
        let orders: Vec<_> = blobs
            .iter()
            .enumerate()
            .zip(
                nodes
                    .iter()
                    .cycle()
                    .skip((*transmit_index as usize) % nodes.len()),
            )
            .collect();
        info!("orders table {}", orders.len());
        let errs: Vec<_> = orders
            .into_iter()
            .map(|((i, b), v)| {
                // only leader should be broadcasting
                assert!(me.current_leader_id != v.id);
                let mut blob = b.write().expect("'b' write lock in pub fn broadcast");
                blob.set_id(me.id).expect("set_id in pub fn broadcast");
                blob.set_index(*transmit_index + i as u64)
                    .expect("set_index in pub fn broadcast");
                //TODO profile this, may need multiple sockets for par_iter
                info!("broadcast {} to {}", blob.meta.size, v.replicate_addr);
                let e = s.send_to(&blob.data[..blob.meta.size], &v.replicate_addr);
                info!("done broadcast {} to {}", blob.meta.size, v.replicate_addr);
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
                info!(
                    "retransmit blob {} to {}",
                    rblob.get_index().unwrap(),
                    v.replicate_addr
                );
                //TODO profile this, may need multiple sockets for par_iter
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
        if self.table.len() <= 1 {
            return Err(Error::CrdtTooSmall);
        }
        let mut n = (Self::random() as usize) % self.table.len();
        while self.table.values().nth(n).unwrap().id == self.me {
            n = (Self::random() as usize) % self.table.len();
        }
        let addr = self.table.values().nth(n).unwrap().gossip_addr.clone();
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
            return Err(Error::CrdtTooSmall);
        }
        let n = (Self::random() as usize) % options.len();
        let v = options[n].clone();
        let remote_update_index = *self.remote.get(&v.id).unwrap_or(&0);
        let req = Protocol::RequestUpdates(remote_update_index, self.table[&self.me].clone());
        Ok((v.gossip_addr, req))
    }

    /// At random pick a node and try to get updated changes from them
    fn run_gossip(obj: &Arc<RwLock<Self>>) -> Result<()> {
        //TODO we need to keep track of stakes and weight the selection by stake size
        //TODO cache sockets

        // Lock the object only to do this operation and not for any longer
        // especially not when doing the `sock.send_to`
        let (remote_gossip_addr, req) = obj.read()
            .expect("'obj' read lock in fn run_gossip")
            .gossip_request()?;
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        // TODO this will get chatty, so we need to first ask for number of updates since
        // then only ask for specific data that we dont have
        let r = serialize(&req)?;
        sock.send_to(&r, remote_gossip_addr)?;
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
    pub fn gossip(obj: Arc<RwLock<Self>>, exit: Arc<AtomicBool>) -> JoinHandle<()> {
        spawn(move || loop {
            let _ = Self::run_gossip(&obj);
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
    }
    fn run_window_request(
        window: &Arc<RwLock<Vec<Option<SharedBlob>>>>,
        sock: &UdpSocket,
        from: &ReplicatedData,
        ix: u64,
    ) -> Result<()> {
        let pos = (ix as usize) % window.read().unwrap().len();
        let mut outblob = vec![];
        if let &Some(ref blob) = &window.read().unwrap()[pos] {
            let rblob = blob.read().unwrap();
            let blob_ix = rblob.get_index().expect("run_window_request get_index");
            if blob_ix == ix {
                // copy to avoid doing IO inside the lock
                outblob.extend(&rblob.data[..rblob.meta.size]);
            }
        } else {
            assert!(window.read().unwrap()[pos].is_none());
            info!("failed RequestWindowIndex {} {}", ix, from.replicate_addr);
        }
        if outblob.len() > 0 {
            info!(
                "responding RequestWindowIndex {} {}",
                ix, from.replicate_addr
            );
            sock.send_to(&outblob, from.replicate_addr)?;
        }
        Ok(())
    }
    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        window: &Arc<RwLock<Vec<Option<SharedBlob>>>>,
        sock: &UdpSocket,
    ) -> Result<()> {
        //TODO cache connections
        let mut buf = vec![0u8; 1024 * 64];
        let (amt, src) = sock.recv_from(&mut buf)?;
        trace!("got request from {}", src);
        buf.resize(amt, 0);
        let r = deserialize(&buf)?;
        match r {
            // TODO sigverify these
            Protocol::RequestUpdates(v, reqdata) => {
                trace!("RequestUpdates {}", v);
                let addr = reqdata.gossip_addr;
                // only lock for this call, dont lock during IO `sock.send_to` or `sock.recv_from`
                let (from, ups, data) = obj.read()
                    .expect("'obj' read lock in RequestUpdates")
                    .get_updates_since(v);
                trace!("get updates since response {} {}", v, data.len());
                let rsp = serialize(&Protocol::ReceiveUpdates(from, ups, data))?;
                trace!("send_to {}", addr);
                //TODO verify reqdata belongs to sender
                obj.write()
                    .expect("'obj' write lock in RequestUpdates")
                    .insert(&reqdata);
                sock.send_to(&rsp, addr)
                    .expect("'sock.send_to' in RequestUpdates");
                trace!("send_to done!");
            }
            Protocol::ReceiveUpdates(from, ups, data) => {
                trace!("ReceivedUpdates");
                obj.write()
                    .expect("'obj' write lock in ReceiveUpdates")
                    .apply_updates(from, ups, &data);
            }
            Protocol::RequestWindowIndex(from, ix) => {
                //TODO verify from is signed
                obj.write().unwrap().insert(&from);
                let me = obj.read().unwrap().my_data().clone();
                info!(
                    "received RequestWindowIndex {} {} myaddr {}",
                    ix, from.replicate_addr, me.replicate_addr
                );
                assert_ne!(from.replicate_addr, me.replicate_addr);
                let _ = Self::run_window_request(window, sock, &from, ix);
            }
        }
        Ok(())
    }
    pub fn listen(
        obj: Arc<RwLock<Self>>,
        window: Arc<RwLock<Vec<Option<SharedBlob>>>>,
        sock: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        sock.set_read_timeout(Some(Duration::new(2, 0)))
            .expect("'sock.set_read_timeout' in crdt.rs");
        spawn(move || loop {
            let _ = Self::run_listen(&obj, &window, &sock);
            if exit.load(Ordering::Relaxed) {
                return;
            }
        })
    }
}

#[cfg(test)]
mod test {
    use crdt::{Crdt, ReplicatedData};
    use logger;
    use packet::Blob;
    use rayon::iter::*;
    use signature::KeyPair;
    use signature::KeyPairUtil;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread::{sleep, JoinHandle};
    use std::time::Duration;

    fn test_node() -> (Crdt, UdpSocket, UdpSocket, UdpSocket) {
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let replicate = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve = UdpSocket::bind("0.0.0.0:0").unwrap();
        let pubkey = KeyPair::new().pubkey();
        let d = ReplicatedData::new(
            pubkey,
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            serve.local_addr().unwrap(),
        );
        let crdt = Crdt::new(d);
        trace!(
            "id: {} gossip: {} replicate: {} serve: {}",
            crdt.my_data().id[0],
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            serve.local_addr().unwrap(),
        );
        (crdt, gossip, replicate, serve)
    }

    /// Test that the network converges.
    /// Run until every node in the network has a full ReplicatedData set.
    /// Check that nodes stop sending updates after all the ReplicatedData has been shared.
    /// tests that actually use this function are below
    fn run_gossip_topo<F>(topo: F)
    where
        F: Fn(&Vec<(Arc<RwLock<Crdt>>, JoinHandle<()>)>) -> (),
    {
        let num: usize = 5;
        let exit = Arc::new(AtomicBool::new(false));
        let listen: Vec<_> = (0..num)
            .map(|_| {
                let (crdt, gossip, _, _) = test_node();
                let c = Arc::new(RwLock::new(crdt));
                let w = Arc::new(RwLock::new(vec![]));
                let l = Crdt::listen(c.clone(), w, gossip, exit.clone());
                (c, l)
            })
            .collect();
        topo(&listen);
        let gossip: Vec<_> = listen
            .iter()
            .map(|&(ref c, _)| Crdt::gossip(c.clone(), exit.clone()))
            .collect();
        let mut done = true;
        for _ in 0..(num * 32) {
            done = true;
            for &(ref c, _) in listen.iter() {
                trace!(
                    "done updates {} {}",
                    c.read().unwrap().table.len(),
                    c.read().unwrap().update_index
                );
                //make sure the number of updates doesn't grow unbounded
                assert!(c.read().unwrap().update_index <= num as u64);
                //make sure we got all the updates
                if c.read().unwrap().table.len() != num {
                    done = false;
                }
            }
            if done == true {
                break;
            }
            sleep(Duration::new(1, 0));
        }
        exit.store(true, Ordering::Relaxed);
        for j in gossip {
            j.join().unwrap();
        }
        for (c, j) in listen.into_iter() {
            j.join().unwrap();
            // make it clear what failed
            // protocol is to chatty, updates should stop after everyone receives `num`
            assert!(c.read().unwrap().update_index <= num as u64);
            // protocol is not chatty enough, everyone should get `num` entries
            assert_eq!(c.read().unwrap().table.len(), num);
        }
        assert!(done);
    }
    /// ring a -> b -> c -> d -> e -> a
    #[test]
    #[ignore]
    fn gossip_ring_test() {
        run_gossip_topo(|listen| {
            let num = listen.len();
            for n in 0..num {
                let y = n % listen.len();
                let x = (n + 1) % listen.len();
                let mut xv = listen[x].0.write().unwrap();
                let yv = listen[y].0.read().unwrap();
                let mut d = yv.table[&yv.me].clone();
                d.version = 0;
                xv.insert(&d);
            }
        });
    }

    /// star (b,c,d,e) -> a
    #[test]
    #[ignore]
    fn gossip_star_test() {
        run_gossip_topo(|listen| {
            let num = listen.len();
            for n in 0..(num - 1) {
                let x = 0;
                let y = (n + 1) % listen.len();
                let mut xv = listen[x].0.write().unwrap();
                let yv = listen[y].0.read().unwrap();
                let mut d = yv.table[&yv.me].clone();
                d.version = 0;
                xv.insert(&d);
            }
        });
    }

    /// Test that insert drops messages that are older
    #[test]
    fn insert_test() {
        let mut d = ReplicatedData::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
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
    #[ignore]
    pub fn test_crdt_retransmit() {
        logger::setup();
        trace!("c1:");
        let (mut c1, s1, r1, e1) = test_node();
        trace!("c2:");
        let (mut c2, s2, r2, _) = test_node();
        trace!("c3:");
        let (mut c3, s3, r3, _) = test_node();
        let c1_id = c1.my_data().id;
        c1.set_leader(c1_id);

        c2.insert(&c1.my_data());
        c3.insert(&c1.my_data());

        c2.set_leader(c1.my_data().id);
        c3.set_leader(c1.my_data().id);

        let exit = Arc::new(AtomicBool::new(false));

        // Create listen threads
        let win1 = Arc::new(RwLock::new(vec![]));
        let a1 = Arc::new(RwLock::new(c1));
        let t1 = Crdt::listen(a1.clone(), win1, s1, exit.clone());

        let a2 = Arc::new(RwLock::new(c2));
        let win2 = Arc::new(RwLock::new(vec![]));
        let t2 = Crdt::listen(a2.clone(), win2, s2, exit.clone());

        let a3 = Arc::new(RwLock::new(c3));
        let win3 = Arc::new(RwLock::new(vec![]));
        let t3 = Crdt::listen(a3.clone(), win3, s3, exit.clone());

        // Create gossip threads
        let t1_gossip = Crdt::gossip(a1.clone(), exit.clone());
        let t2_gossip = Crdt::gossip(a2.clone(), exit.clone());
        let t3_gossip = Crdt::gossip(a3.clone(), exit.clone());

        //wait to converge
        trace!("waitng to converge:");
        let mut done = false;
        for _ in 0..30 {
            done = a1.read().unwrap().table.len() == 3 && a2.read().unwrap().table.len() == 3
                && a3.read().unwrap().table.len() == 3;
            if done {
                break;
            }
            sleep(Duration::new(1, 0));
        }
        assert!(done);
        let mut b = Blob::default();
        b.meta.size = 10;
        Crdt::retransmit(&a1, &Arc::new(RwLock::new(b)), &e1).unwrap();
        let res: Vec<_> = [r1, r2, r3]
            .into_par_iter()
            .map(|s| {
                let mut b = Blob::default();
                s.set_read_timeout(Some(Duration::new(1, 0))).unwrap();
                let res = s.recv_from(&mut b.data);
                res.is_err() //true if failed to receive the retransmit packet
            })
            .collect();
        //true if failed receive the retransmit packet, r2, and r3 should succeed
        //r1 was the sender, so it should fail to receive the packet
        assert_eq!(res, [true, false, false]);
        exit.store(true, Ordering::Relaxed);
        let threads = vec![t1, t2, t3, t1_gossip, t2_gossip, t3_gossip];
        for t in threads.into_iter() {
            t.join().unwrap();
        }
    }
}
