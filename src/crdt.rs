//! The `crdt` module defines a data structure that is shared by all the nodes in the network over
//! a gossip control plane.  The goal is to share small bits of of-chain information and detect and
//! repair partitions.
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Data {
    key: PublicKey,
    sig: Signature,
    /// should always be increasing
    version: u64,
    /// address to connect to for gossip
    gossip: SocketAddr,
    /// address to connect to for replication
    replicate: SocketAddr,
    /// address to connect to when this node is leader
    lead: SocketAddr,
    /// current leader identity
    who_leads: PublicKey,
    /// last verified hash that was submitted to the leader
    last_verified_hash: Hash,
    /// last verified count, always increasing
    last_verified_count: u64,
}
impl Data {
    pub fn new(key: PublicKey, gossip: SocketAddr) -> Data {
        let daddr = "0.0.0.0:0".parse().unwrap();
        Data {
            key,
            sig: Signature::default(),
            version: 0,
            gossip,
            replicate: daddr,
            lead: daddr,
            who_leads: PublicKey::default(),
            last_verified_hash: Hash::default(),
            last_verified_count: 0,
        }
    }
    fn verify_sig(&self) -> bool {
        //TODO implement this
        true
    }
}

/// `Crdt` structure keeps a table of `Data` structs
/// # Properties
/// * `table` - map of public key's to versioned and signed Data structs
/// * `local` - map of public key's to self.table was updated
/// * `remote` - map of public key's to the last update index we saw from the key
/// * `update_index` - my update index
/// # Remarks
/// This implements two services, `gossip` and `listen`.
/// * `gossip` - asynchronously ask nodes to send updates
/// * `listen` - listen for requests and responses
/// No attempt to keep track of timeouts or dropped requests is made
pub struct Crdt {
    table: HashMap<PublicKey, Data>,
    /// value of my update index when entry in table was updated
    local: HashMap<PublicKey, u64>,
    /// the value of the remote update index that i have last seen
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
    /// from key, form's last update index, data
    ReceiveUpdates(PublicKey, u64, Vec<Data>),
}

impl Crdt {
    pub fn new(me: Data) -> Crdt {
        assert_eq!(me.version, 0);
        let mut g = Crdt {
            table: HashMap::new(),
            local: HashMap::new(),
            remote: HashMap::new(),
            me: me.key,
            update_index: 1,
            timeout: Duration::new(0, 100_000),
        };
        g.local.insert(me.key, g.update_index);
        g.table.insert(me.key, me);
        g
    }
    pub fn insert(&mut self, v: &Data) {
        if self.table.get(&v.key).is_none() || (v.version > self.table[&v.key].version) {
            trace!("insert! {}", v.version);
            self.update_index += 1;
            let _ = self.table.insert(v.key, v.clone());
            let _ = self.local.insert(v.key, self.update_index);
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
    fn get_updates_since(&self, v: u64) -> (PublicKey, u64, Vec<Data>) {
        trace!("get updates since {}", v);
        let data = self.table
            .values()
            .filter(|x| self.local[&x.key] > v)
            .cloned()
            .collect();
        let key = self.me;
        let ups = self.update_index;
        (key, ups, data)
    }
    /// At random pick a node and try to get updated changes from them
    fn gossip_loop(obj: &Arc<RwLock<Self>>) -> Result<()> {
        // TODO we need to keep track of stakes and weight the selection by stake size
        //TODO cache connections
        let (v, mut addr, ups) = {
            let o = obj.read().unwrap();
            let n = (Self::random() as usize) % o.table.len();
            let v = o.table.values().nth(n).unwrap().clone();
            let ups = *o.remote.get(&v.key).unwrap_or(&0);
            let mut addr = o.table[&o.me].gossip;
            (v, addr, ups)
        };
        let myaddr = addr;
        addr.set_port(0);
        let sock = UdpSocket::bind(addr)?;
        // TODO this will get chatty, so we need to first ask for number of updates since
        // then only ask for specific data that we dont have
        let r = serialize(&Protocol::RequestUpdates(ups, myaddr))?;
        sock.send_to(&r, v.gossip)?;
        Ok(())
    }

    fn got_updates(&mut self, from: PublicKey, ups: u64, data: Vec<Data>) {
        trace!("got updates {}", data.len());
        // TODO we need to punish/spam resist here
        // sig verify the whole update and slash anyone who sends a bad update
        for v in data {
            if !v.verify_sig() {
                continue;
            }
            // TODO probably an error or attack
            if v.key == self.me {
                continue;
            }
            // TODO check that last_verified types are always increasing
            self.insert(&v);
        }
        *self.remote.entry(from).or_insert(ups) = ups;
    }
    /// randomly pick a node and ask them for updates asynchronously
    pub fn gossip(obj: Arc<RwLock<Self>>, exit: Arc<AtomicBool>) -> JoinHandle<()> {
        spawn(move || loop {
            let _ = Self::gossip_loop(&obj);
            if exit.load(Ordering::Relaxed) {
                return;
            }
            //TODO this should be a tuned parameter
            sleep(obj.read().unwrap().timeout);
        })
    }
    /// Process messages from the network
    fn listen_loop(obj: &Arc<RwLock<Self>>, sock: &UdpSocket) -> Result<()> {
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
                let (from, ups, data) = obj.read().unwrap().get_updates_since(v);
                trace!("get updates since response {} {}", v, data.len());
                let rsp = serialize(&Protocol::ReceiveUpdates(from, ups, data))?;
                trace!("send_to {}", addr);
                sock.send_to(&rsp, addr).unwrap();
                trace!("send_to done!");
            }
            Protocol::ReceiveUpdates(from, ups, data) => {
                trace!("ReceivedUpdates");
                obj.write().unwrap().got_updates(from, ups, data);
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
            let _ = Self::listen_loop(&obj, &sock);
            if exit.load(Ordering::Relaxed) {
                return;
            }
        })
    }
}

#[cfg(test)]
mod test {
    use crdt::{Crdt, Data};
    use signature::KeyPair;
    use signature::KeyPairUtil;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    /// Test that the network converges.
    /// Create a ring a -> b -> c -> d -> e -> a of size num.
    /// Run until every node in the network has a full data set.
    /// Check that nodes stop sending updates after all the data has been shared.
    #[test]
    fn gossip_test() {
        let num: usize = 5;
        let exit = Arc::new(AtomicBool::new(false));
        let listen: Vec<_> = (0..num)
            .map(|_| {
                let listener = UdpSocket::bind("0.0.0.0:0").unwrap();
                let pubkey = KeyPair::new().pubkey();
                let d = Data::new(pubkey, listener.local_addr().unwrap());
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
        for _ in 0..(num * 8) {
            done = true;
            for &(ref c, _) in listen.iter() {
                trace!("done updates {} {}", c.read().unwrap().table.len(), num);
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
            // make it clear what failed
            // protocol is to chatty, updates should stop after everyone receives `num`
            assert!(c.read().unwrap().update_index <= num as u64);
            // protocol is not chatty enough, everyone should get `num` entries
            assert_eq!(c.read().unwrap().table.len(), num);
            j.join().unwrap();
        }
        assert!(done);
    }
    /// Test that insert drops messages that are older
    #[test]
    fn insert_test() {
        let mut d = Data::new(KeyPair::new().pubkey(), "127.0.0.1:1234".parse().unwrap());
        assert_eq!(d.version, 0);
        let mut crdt = Crdt::new(d.clone());
        assert_eq!(crdt.table[&d.key].version, 0);
        d.version = 2;
        crdt.insert(&d);
        assert_eq!(crdt.table[&d.key].version, 2);
        d.version = 1;
        crdt.insert(&d);
        assert_eq!(crdt.table[&d.key].version, 2);
    }

}
