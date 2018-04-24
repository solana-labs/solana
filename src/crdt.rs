//! The `crdt` module defines a data structure that is shared by all the nodes in the network over
//! a gossip control plane.  The goal is to share small bits of of-chain information and detect and
//! repair partitions.
//!
//! This CRDT only supports a very limited set of types.  A map of PublicKey -> Versioned Struct.
//! The last version is always picked durring an update.

use bincode::{deserialize, serialize};
use byteorder::{LittleEndian, ReadBytesExt};
use hash::Hash;
use result::Result;
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
    id: PublicKey,
    sig: Signature,
    /// should always be increasing
    version: u64,
    /// address to connect to for gossip
    gossip_addr: SocketAddr,
    /// address to connect to for replication
    replicate_addr: SocketAddr,
    /// address to connect to when this node is leader
    lead_addr: SocketAddr,
    /// current leader identity
    current_leader_id: PublicKey,
    /// last verified hash that was submitted to the leader
    last_verified_hash: Hash,
    /// last verified count, always increasing
    last_verified_count: u64,
}

impl ReplicatedData {
    pub fn new(id: PublicKey, gossip_addr: SocketAddr) -> ReplicatedData {
        let daddr = "0.0.0.0:0".parse().unwrap();
        ReplicatedData {
            id,
            sig: Signature::default(),
            version: 0,
            gossip_addr,
            replicate_addr: daddr,
            lead_addr: daddr,
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
    table: HashMap<PublicKey, ReplicatedData>,
    /// Value of my update index when entry in table was updated.
    /// Nodes will ask for updates since `update_index`, and this node
    /// should respond with all the identities that are greater then the
    /// request's `update_index` in this list
    local: HashMap<PublicKey, u64>,
    /// The value of the remote update index that i have last seen
    /// This Node will ask external nodes for updates since the value in this list
    remote: HashMap<PublicKey, u64>,
    update_index: u64,
    me: PublicKey,
    timeout: Duration,
}
// TODO These messages should be signed, and go through the gpu pipeline for spam filtering
#[derive(Serialize, Deserialize)]
enum Protocol {
    RequestUpdates(u64, SocketAddr),
    //TODO might need a since?
    /// from id, form's last update index, ReplicatedData
    ReceiveUpdates(PublicKey, u64, Vec<ReplicatedData>),
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
            timeout: Duration::new(0, 100_000),
        };
        g.local.insert(me.id, g.update_index);
        g.table.insert(me.id, me);
        g
    }
    pub fn insert(&mut self, v: &ReplicatedData) {
        if self.table.get(&v.id).is_none() || (v.version > self.table[&v.id].version) {
            trace!("insert! {}", v.version);
            self.update_index += 1;
            let _ = self.table.insert(v.id, v.clone());
            let _ = self.local.insert(v.id, self.update_index);
        } else {
            trace!("INSERT FAILED {}", v.version);
        }
    }
    fn random() -> u64 {
        let rnd = SystemRandom::new();
        let mut buf = [0u8; 8];
        rnd.fill(&mut buf).unwrap();
        let mut rdr = Cursor::new(&buf);
        rdr.read_u64::<LittleEndian>().unwrap()
    }
    fn get_updates_since(&self, v: u64) -> (PublicKey, u64, Vec<ReplicatedData>) {
        trace!("get updates since {}", v);
        let data = self.table
            .values()
            .filter(|x| self.local[&x.id] > v)
            .cloned()
            .collect();
        let id = self.me;
        let ups = self.update_index;
        (id, ups, data)
    }

    /// Create a random gossip request
    /// # Returns
    /// (A,B,C)
    /// * A - Remote gossip address
    /// * B - My gossip address
    /// * C - Remote update index to request updates since
    fn gossip_request(&self) -> (SocketAddr, SocketAddr, u64) {
        let n = (Self::random() as usize) % self.table.len();
        trace!("random {:?} {}", &self.me[0..1], n);
        let v = self.table.values().nth(n).unwrap().clone();
        let remote_update_index = *self.remote.get(&v.id).unwrap_or(&0);
        let my_addr = self.table[&self.me].gossip_addr;
        (v.gossip_addr, my_addr, remote_update_index)
    }

    /// At random pick a node and try to get updated changes from them
    fn run_gossip(obj: &Arc<RwLock<Self>>) -> Result<()> {
        //TODO we need to keep track of stakes and weight the selection by stake size
        //TODO cache sockets

        // Lock the object only to do this operation and not for any longer
        // especially not when doing the `sock.send_to`
        let (remote_gossip_addr, my_addr, remote_update_index) =
            obj.read().unwrap().gossip_request();
        let mut req_addr = my_addr;
        req_addr.set_port(0);
        let sock = UdpSocket::bind(req_addr)?;
        // TODO this will get chatty, so we need to first ask for number of updates since
        // then only ask for specific data that we dont have
        let r = serialize(&Protocol::RequestUpdates(remote_update_index, my_addr))?;
        sock.send_to(&r, remote_gossip_addr)?;
        Ok(())
    }

    /// Apply updates that we received from the identity `from`
    /// # Arguments
    /// * `from` - identity of the sender of the updates
    /// * `update_index` - the number of updates that `from` has completed and this set of `data` represents
    /// * `data` - the update data
    fn apply_updates(&mut self, from: PublicKey, update_index: u64, data: Vec<ReplicatedData>) {
        trace!("got updates {}", data.len());
        // TODO we need to punish/spam resist here
        // sig verify the whole update and slash anyone who sends a bad update
        for v in data {
            // TODO probably an error or attack
            if v.id == self.me {
                continue;
            }
            // TODO check that last_verified types are always increasing
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
            sleep(obj.read().unwrap().timeout);
        })
    }

    /// Process messages from the network
    fn run_listen(obj: &Arc<RwLock<Self>>, sock: &UdpSocket) -> Result<()> {
        //TODO cache connections
        let mut buf = vec![0u8; 1024 * 64];
        let (amt, src) = sock.recv_from(&mut buf)?;
        trace!("got request from {}", src);
        buf.resize(amt, 0);
        let r = deserialize(&buf)?;
        match r {
            // TODO sigverify these
            Protocol::RequestUpdates(v, addr) => {
                trace!("RequestUpdates {}", v);
                // only lock for this call, dont lock durring IO `sock.send_to` or `sock.recv_from`
                let (from, ups, data) = obj.read().unwrap().get_updates_since(v);
                trace!("get updates since response {} {}", v, data.len());
                let rsp = serialize(&Protocol::ReceiveUpdates(from, ups, data))?;
                trace!("send_to {}", addr);
                sock.send_to(&rsp, addr).unwrap();
                trace!("send_to done!");
            }
            Protocol::ReceiveUpdates(from, ups, data) => {
                trace!("ReceivedUpdates");
                obj.write().unwrap().apply_updates(from, ups, data);
            }
        }
        Ok(())
    }
    pub fn listen(
        obj: Arc<RwLock<Self>>,
        sock: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        sock.set_read_timeout(Some(Duration::new(2, 0))).unwrap();
        spawn(move || loop {
            let _ = Self::run_listen(&obj, &sock);
            if exit.load(Ordering::Relaxed) {
                return;
            }
        })
    }
}

#[cfg(test)]
mod test {
    use crdt::{Crdt, ReplicatedData};
    use signature::KeyPair;
    use signature::KeyPairUtil;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    /// Test that the network converges.
    /// Create a ring a -> b -> c -> d -> e -> a of size num.
    /// Run until every node in the network has a full ReplicatedData set.
    /// Check that nodes stop sending updates after all the ReplicatedData has been shared.
    #[test]
    fn gossip_test() {
        let num: usize = 5;
        let exit = Arc::new(AtomicBool::new(false));
        let listen: Vec<_> = (0..num)
            .map(|_| {
                let listener = UdpSocket::bind("0.0.0.0:0").unwrap();
                let pubkey = KeyPair::new().pubkey();
                let d = ReplicatedData::new(pubkey, listener.local_addr().unwrap());
                let crdt = Crdt::new(d);
                let c = Arc::new(RwLock::new(crdt));
                let l = Crdt::listen(c.clone(), listener, exit.clone());
                (c, l)
            })
            .collect();
        for n in 0..num {
            let y = n % listen.len();
            let x = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut d = yv.table[&yv.me].clone();
            d.version = 0;
            xv.insert(&d);
        }
        let gossip: Vec<_> = listen
            .iter()
            .map(|&(ref c, _)| Crdt::gossip(c.clone(), exit.clone()))
            .collect();
        let mut done = true;
        for _ in 0..(num * 16) {
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
    /// Test that insert drops messages that are older
    #[test]
    fn insert_test() {
        let mut d = ReplicatedData::new(KeyPair::new().pubkey(), "127.0.0.1:1234".parse().unwrap());
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

}
