//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use futures::future::{ok, FutureResult};
use hash::Hash;
use request::{Request, Response};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use transaction::Transaction;

pub struct ThinClient {
    pub addr: SocketAddr,
    pub requests_socket: UdpSocket,
    pub events_socket: UdpSocket,
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<PublicKey, Option<i64>>,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with Rpu
    /// over `requests_socket` and `events_socket`. To receive responses, the caller must bind `socket`
    /// to a public address before invoking ThinClient methods.
    pub fn new(addr: SocketAddr, requests_socket: UdpSocket, events_socket: UdpSocket) -> Self {
        let client = ThinClient {
            addr: addr,
            requests_socket,
            events_socket,
            last_id: None,
            transaction_count: 0,
            balances: HashMap::new(),
        };
        client
    }

    pub fn recv_response(&self) -> io::Result<Response> {
        let mut buf = vec![0u8; 1024];
        trace!("start recv_from");
        self.requests_socket.recv_from(&mut buf)?;
        trace!("end recv_from");
        let resp = deserialize(&buf).expect("deserialize balance in thin_client");
        Ok(resp)
    }

    pub fn process_response(&mut self, resp: Response) {
        match resp {
            Response::Balance { key, val } => {
                trace!("Response balance {:?} {:?}", key, val);
                self.balances.insert(key, val);
            }
            Response::LastId { id } => {
                info!("Response last_id {:?}", id);
                self.last_id = Some(id);
            }
            Response::TransactionCount { transaction_count } => {
                info!("Response transaction count {:?}", transaction_count);
                self.transaction_count = transaction_count;
            }
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tr: Transaction) -> io::Result<usize> {
        let req = Request::Transaction(tr);
        let data = serialize(&req).expect("serialize Transaction in pub fn transfer_signed");
        self.requests_socket.send_to(&data, &self.addr)
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let tr = Transaction::new(keypair, to, n, *last_id);
        let sig = tr.sig;
        self.transfer_signed(tr).map(|_| sig)
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        trace!("get_balance");
        let req = Request::GetBalance { key: *pubkey };
        let data = serialize(&req).expect("serialize GetBalance in pub fn get_balance");
        self.requests_socket
            .send_to(&data, &self.addr)
            .expect("buffer error in pub fn get_balance");
        let mut done = false;
        while !done {
            let resp = self.recv_response()?;
            trace!("recv_response {:?}", resp);
            if let &Response::Balance { ref key, .. } = &resp {
                done = key == pubkey;
            }
            self.process_response(resp);
        }
        self.balances[pubkey].ok_or(io::Error::new(io::ErrorKind::Other, "nokey"))
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will hang.
    pub fn transaction_count(&mut self) -> u64 {
        info!("transaction_count");
        let req = Request::GetTransactionCount;
        let data =
            serialize(&req).expect("serialize GetTransactionCount in pub fn transaction_count");
        self.requests_socket
            .send_to(&data, &self.addr)
            .expect("buffer error in pub fn transaction_count");
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("transaction count dropped");
            info!("recv_response {:?}", resp);
            if let &Response::TransactionCount { .. } = &resp {
                done = true;
            }
            self.process_response(resp);
        }
        self.transaction_count
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_last_id(&mut self) -> FutureResult<Hash, ()> {
        info!("get_last_id");
        let req = Request::GetLastId;
        let data = serialize(&req).expect("serialize GetLastId in pub fn get_last_id");
        self.requests_socket
            .send_to(&data, &self.addr)
            .expect("buffer error in pub fn get_last_id");
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("get_last_id response");
            info!("recv_response {:?}", resp);
            if let &Response::LastId { .. } = &resp {
                done = true;
            }
            self.process_response(resp);
        }
        ok(self.last_id.expect("some last_id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use crdt::{Crdt, ReplicatedData};
    use futures::Future;
    use logger;
    use mint::Mint;
    use plan::Plan;
    use server::Server;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread::JoinHandle;
    use std::thread::sleep;
    use std::time::Duration;
    use std::time::Instant;
    use streamer::default_window;
    use tvu::{self, Tvu};

    #[test]
    #[ignore]
    fn test_thin_client() {
        logger::setup();
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve = UdpSocket::bind("0.0.0.0:0").unwrap();
        serve.set_read_timeout(Some(Duration::new(1, 0))).unwrap();
        let _events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let addr = serve.local_addr().unwrap();
        let pubkey = KeyPair::new().pubkey();
        let d = ReplicatedData::new(
            pubkey,
            gossip.local_addr().unwrap(),
            "0.0.0.0:0".parse().unwrap(),
            serve.local_addr().unwrap(),
        );

        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let mut local = serve.local_addr().unwrap();
        local.set_port(0);
        let broadcast_socket = UdpSocket::bind(local).unwrap();
        let respond_socket = UdpSocket::bind(local.clone()).unwrap();

        let server = Server::new(
            bank,
            alice.last_id(),
            Some(Duration::from_millis(30)),
            d,
            serve,
            broadcast_socket,
            respond_socket,
            gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(900));

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(addr, requests_socket, events_socket);
        let last_id = client.get_last_id().wait().unwrap();
        let _sig = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        let mut balance;
        let now = Instant::now();
        loop {
            balance = client.get_balance(&bob_pubkey);
            if balance.is_ok() {
                break;
            }
            if now.elapsed().as_secs() > 0 {
                break;
            }
        }
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }

    #[test]
    #[ignore]
    fn test_bad_sig() {
        let (leader_data, leader_gossip, _, leader_serve, _leader_events) = tvu::test_node();
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let serve_addr = leader_serve.local_addr().unwrap();

        let mut local = leader_serve.local_addr().unwrap();
        local.set_port(0);
        let broadcast_socket = UdpSocket::bind(local).unwrap();
        let respond_socket = UdpSocket::bind(local.clone()).unwrap();

        let server = Server::new(
            bank,
            alice.last_id(),
            Some(Duration::from_millis(30)),
            leader_data,
            leader_serve,
            broadcast_socket,
            respond_socket,
            leader_gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(300));

        let requests_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        requests_socket
            .set_read_timeout(Some(Duration::new(5, 0)))
            .unwrap();
        let events_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut client = ThinClient::new(serve_addr, requests_socket, events_socket);
        let last_id = client.get_last_id().wait().unwrap();

        trace!("doing stuff");

        let tr = Transaction::new(&alice.keypair(), bob_pubkey, 500, last_id);

        let _sig = client.transfer_signed(tr).unwrap();

        let last_id = client.get_last_id().wait().unwrap();

        let mut tr2 = Transaction::new(&alice.keypair(), bob_pubkey, 501, last_id);
        tr2.data.tokens = 502;
        tr2.data.plan = Plan::new_payment(502, bob_pubkey);
        let _sig = client.transfer_signed(tr2).unwrap();

        assert_eq!(client.get_balance(&bob_pubkey).unwrap(), 500);
        trace!("exiting");
        exit.store(true, Ordering::Relaxed);
        trace!("joining threads");
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }

    fn test_node() -> (ReplicatedData, UdpSocket, UdpSocket, UdpSocket, UdpSocket) {
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve = UdpSocket::bind("0.0.0.0:0").unwrap();
        serve.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        let events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let replicate = UdpSocket::bind("0.0.0.0:0").unwrap();
        let pubkey = KeyPair::new().pubkey();
        let leader = ReplicatedData::new(
            pubkey,
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            serve.local_addr().unwrap(),
        );
        (leader, gossip, serve, replicate, events_socket)
    }

    fn replicant(
        leader: &ReplicatedData,
        exit: Arc<AtomicBool>,
        alice: &Mint,
        threads: &mut Vec<JoinHandle<()>>,
    ) {
        let replicant = test_node();
        let replicant_bank = {
            let bank = Bank::new(&alice);
            Arc::new(Tvu::new(bank, alice.last_id(), None))
        };
        let mut ts = Tvu::serve(
            &replicant_bank,
            replicant.0.clone(),
            replicant.1,
            replicant.2,
            replicant.3,
            leader.clone(),
            exit.clone(),
        ).unwrap();
        threads.append(&mut ts);
    }

    fn converge(
        leader: &ReplicatedData,
        exit: Arc<AtomicBool>,
        num_nodes: usize,
        threads: &mut Vec<JoinHandle<()>>,
    ) -> Vec<SocketAddr> {
        //lets spy on the network
        let (mut spy, spy_gossip, _, _, _) = test_node();
        let daddr = "0.0.0.0:0".parse().unwrap();
        let me = spy.id.clone();
        spy.replicate_addr = daddr;
        spy.serve_addr = daddr;
        let mut spy_crdt = Crdt::new(spy);
        spy_crdt.insert(&leader);
        spy_crdt.set_leader(leader.id);

        let spy_ref = Arc::new(RwLock::new(spy_crdt));
        let spy_window = default_window();
        let t_spy_listen = Crdt::listen(spy_ref.clone(), spy_window, spy_gossip, exit.clone());
        let t_spy_gossip = Crdt::gossip(spy_ref.clone(), exit.clone());
        //wait for the network to converge
        for _ in 0..30 {
            let len = spy_ref.read().unwrap().table.values().len();
            let mut min = num_nodes as u64;
            for u in spy_ref.read().unwrap().remote.values() {
                if min > *u {
                    min = *u;
                }
            }
            info!("length {} {}", len, min);
            if num_nodes == len && min >= (num_nodes as u64) {
                warn!("converged! {} {}", len, min);
                break;
            }
            sleep(Duration::new(1, 0));
        }
        threads.push(t_spy_listen);
        threads.push(t_spy_gossip);
        let v: Vec<SocketAddr> = spy_ref
            .read()
            .unwrap()
            .table
            .values()
            .into_iter()
            .filter(|x| x.id != me)
            .map(|x| x.serve_addr)
            .collect();
        v.clone()
    }
    #[test]
    #[ignore]
    fn test_multi_node() {
        logger::setup();
        const N: usize = 5;
        trace!("test_multi_accountant_stub");
        let leader = test_node();
        let alice = Mint::new(10_000);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let leader_bank = Bank::new(&alice);

        let mut local = leader.2.local_addr().unwrap();
        local.set_port(0);
        let broadcast_socket = UdpSocket::bind(local).unwrap();
        let respond_socket = UdpSocket::bind(local.clone()).unwrap();

        let server = Server::new(
            leader_bank,
            alice.last_id(),
            None,
            leader.0.clone(),
            leader.2,
            broadcast_socket,
            respond_socket,
            leader.1,
            exit.clone(),
            sink(),
        );

        let mut threads = server.thread_hdls;
        for _ in 0..N {
            replicant(&leader.0, exit.clone(), &alice, &mut threads);
        }
        let addrs = converge(&leader.0, exit.clone(), N + 2, &mut threads);
        //contains the leader addr as well
        assert_eq!(addrs.len(), N + 1);
        //verify leader can do transfer
        let leader_balance = {
            let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            requests_socket
                .set_read_timeout(Some(Duration::new(1, 0)))
                .unwrap();
            let events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let mut client = ThinClient::new(leader.0.serve_addr, requests_socket, events_socket);
            trace!("getting leader last_id");
            let last_id = client.get_last_id().wait().unwrap();
            info!("executing leader transer");
            let _sig = client
                .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
                .unwrap();
            trace!("getting leader balance");
            client.get_balance(&bob_pubkey).unwrap()
        };
        assert_eq!(leader_balance, 500);
        //verify replicant has the same balance
        let mut success = 0usize;
        for serve_addr in addrs.iter() {
            let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            requests_socket
                .set_read_timeout(Some(Duration::new(1, 0)))
                .unwrap();
            let events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let mut client = ThinClient::new(*serve_addr, requests_socket, events_socket);
            for i in 0..10 {
                trace!("getting replicant balance {} {}/10", *serve_addr, i);
                if let Ok(bal) = client.get_balance(&bob_pubkey) {
                    trace!("replicant balance {}", bal);
                    if bal == leader_balance {
                        success += 1;
                        break;
                    }
                }
                sleep(Duration::new(1, 0));
            }
        }
        assert_eq!(success, addrs.len());
        exit.store(true, Ordering::Relaxed);
        for t in threads {
            t.join().unwrap();
        }
    }
}
