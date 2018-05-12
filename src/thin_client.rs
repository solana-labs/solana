//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use futures::future::{ok, FutureResult};
use hash::Hash;
use request_stage::{Request, Response, Subscription};
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
    num_events: u64,
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
            num_events: 0,
            balances: HashMap::new(),
        };
        client.init();
        client
    }

    pub fn init(&self) {
        let subscriptions = vec![Subscription::EntryInfo];
        let req = Request::Subscribe { subscriptions };
        let data = serialize(&req).expect("serialize Subscribe in thin_client");
        trace!("subscribing to {}", self.addr);
        let _res = self.requests_socket.send_to(&data, &self.addr);
    }

    pub fn recv_response(&self) -> io::Result<Response> {
        let mut buf = vec![0u8; 1024];
        info!("start recv_from");
        self.requests_socket.recv_from(&mut buf)?;
        info!("end recv_from");
        let resp = deserialize(&buf).expect("deserialize balance in thin_client");
        Ok(resp)
    }

    pub fn process_response(&mut self, resp: Response) {
        match resp {
            Response::Balance { key, val } => {
                info!("Response balance {:?} {:?}", key, val);
                self.balances.insert(key, val);
            }
            Response::EntryInfo(entry_info) => {
                trace!("Response entry_info {:?}", entry_info.id);
                self.last_id = Some(entry_info.id);
                self.num_events += entry_info.num_events;
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
        info!("get_balance");
        let req = Request::GetBalance { key: *pubkey };
        let data = serialize(&req).expect("serialize GetBalance in pub fn get_balance");
        self.requests_socket
            .send_to(&data, &self.addr)
            .expect("buffer error in pub fn get_balance");
        let mut done = false;
        while !done {
            let resp = self.recv_response()?;
            info!("recv_response {:?}", resp);
            if let &Response::Balance { ref key, .. } = &resp {
                done = key == pubkey;
            }
            self.process_response(resp);
        }
        self.balances[pubkey].ok_or(io::Error::new(io::ErrorKind::Other, "nokey"))
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_last_id(&mut self) -> FutureResult<Hash, ()> {
        self.transaction_count();
        ok(self.last_id.unwrap_or(Hash::default()))
    }

    /// Return the number of transactions the server processed since creating
    /// this client instance.
    pub fn transaction_count(&mut self) -> u64 {
        // Wait for at least one EntryInfo.
        let mut done = false;
        while !done {
            let resp = self.recv_response()
                .expect("recv_response in pub fn transaction_count");
            if let &Response::EntryInfo(_) = &resp {
                done = true;
            }
            self.process_response(resp);
        }

        // Then take the rest.
        self.requests_socket
            .set_nonblocking(true)
            .expect("set_nonblocking in pub fn transaction_count");
        loop {
            match self.recv_response() {
                Err(_) => break,
                Ok(resp) => self.process_response(resp),
            }
        }
        self.requests_socket
            .set_nonblocking(false)
            .expect("set_nonblocking in pub fn transaction_count");
        self.num_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accountant::Accountant;
    use accounting_stage::AccountingStage;
    use crdt::{Crdt, ReplicatedData};
    use futures::Future;
    use logger;
    use mint::Mint;
    use plan::Plan;
    use rpu::Rpu;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;
    use std::time::Instant;
    use tvu::{self, Tvu};

    #[test]
    fn test_thin_client() {
        logger::setup();
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve = UdpSocket::bind("0.0.0.0:0").unwrap();
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
        let accountant = Accountant::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let accounting_stage = AccountingStage::new(accountant, &alice.last_id(), Some(30));
        let rpu = Arc::new(Rpu::new(accounting_stage));
        let threads = rpu.serve(d, serve, gossip, exit.clone(), sink()).unwrap();
        sleep(Duration::from_millis(300));

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
        for t in threads {
            t.join().unwrap();
        }
    }

    #[test]
    fn test_bad_sig() {
        let (leader_data, leader_gossip, _, leader_serve, _leader_events) = tvu::test_node();
        let alice = Mint::new(10_000);
        let accountant = Accountant::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let accounting_stage = AccountingStage::new(accountant, &alice.last_id(), Some(30));
        let rpu = Arc::new(Rpu::new(accounting_stage));
        let serve_addr = leader_serve.local_addr().unwrap();
        let threads = rpu.serve(
            leader_data,
            leader_serve,
            leader_gossip,
            exit.clone(),
            sink(),
        ).unwrap();
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
        for t in threads {
            t.join().unwrap();
        }
    }

    fn test_node() -> (ReplicatedData, UdpSocket, UdpSocket, UdpSocket, UdpSocket) {
        let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve = UdpSocket::bind("0.0.0.0:0").unwrap();
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

    #[test]
    #[ignore]
    fn test_multi_node() {
        logger::setup();
        info!("test_multi_node");
        let leader = test_node();
        let replicant = test_node();
        let alice = Mint::new(10_000);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let leader_acc = {
            let accountant = Accountant::new(&alice);
            let accounting_stage = AccountingStage::new(accountant, &alice.last_id(), Some(30));
            Arc::new(Rpu::new(accounting_stage))
        };

        let replicant_acc = {
            let accountant = Accountant::new(&alice);
            let accounting_stage = AccountingStage::new(accountant, &alice.last_id(), Some(30));
            Arc::new(Tvu::new(accounting_stage))
        };

        let leader_threads = leader_acc
            .serve(leader.0.clone(), leader.2, leader.1, exit.clone(), sink())
            .unwrap();
        let replicant_threads = Tvu::serve(
            &replicant_acc,
            replicant.0.clone(),
            replicant.1,
            replicant.2,
            replicant.3,
            leader.0.clone(),
            exit.clone(),
        ).unwrap();

        //lets spy on the network
        let (mut spy, spy_gossip, _, _, _) = test_node();
        let daddr = "0.0.0.0:0".parse().unwrap();
        spy.replicate_addr = daddr;
        spy.serve_addr = daddr;
        let mut spy_crdt = Crdt::new(spy);
        spy_crdt.insert(leader.0.clone());
        spy_crdt.set_leader(leader.0.id);

        let spy_ref = Arc::new(RwLock::new(spy_crdt));
        let t_spy_listen = Crdt::listen(spy_ref.clone(), spy_gossip, exit.clone());
        let t_spy_gossip = Crdt::gossip(spy_ref.clone(), exit.clone());
        //wait for the network to converge
        for _ in 0..20 {
            let ix = spy_ref.read().unwrap().update_index;
            info!("my update index is {}", ix);
            let len = spy_ref.read().unwrap().remote.values().len();
            let mut done = false;
            info!("remote len {}", len);
            if len > 1 && ix > 2 {
                done = true;
                //check if everyones remote index is greater or equal to ours
                let vs: Vec<u64> = spy_ref.read().unwrap().remote.values().cloned().collect();
                for t in vs.into_iter() {
                    info!("remote update index is {} vs {}", t, ix);
                    if t < 3 {
                        done = false;
                    }
                }
            }
            if done == true {
                info!("converged!");
                break;
            }
            sleep(Duration::new(1, 0));
        }

        //verify leader can do transfer
        let leader_balance = {
            let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            requests_socket
                .set_read_timeout(Some(Duration::new(1, 0)))
                .unwrap();
            let events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let mut client = ThinClient::new(leader.0.serve_addr, requests_socket, events_socket);
            info!("getting leader last_id");
            let last_id = client.get_last_id().wait().unwrap();
            info!("executing leader transer");
            let _sig = client
                .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
                .unwrap();
            info!("getting leader balance");
            client.get_balance(&bob_pubkey).unwrap()
        };
        assert_eq!(leader_balance, 500);
        //verify replicant has the same balance
        let mut replicant_balance = 0;
        for _ in 0..10 {
            let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            requests_socket
                .set_read_timeout(Some(Duration::new(1, 0)))
                .unwrap();
            let events_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let mut client =
                ThinClient::new(replicant.0.serve_addr, requests_socket, events_socket);
            info!("getting replicant balance");
            if let Ok(bal) = client.get_balance(&bob_pubkey) {
                replicant_balance = bal;
            }
            info!("replicant balance {}", replicant_balance);
            if replicant_balance == leader_balance {
                break;
            }
            sleep(Duration::new(1, 0));
        }
        assert_eq!(replicant_balance, leader_balance);

        exit.store(true, Ordering::Relaxed);
        for t in leader_threads {
            t.join().unwrap();
        }
        for t in replicant_threads {
            t.join().unwrap();
        }
        for t in vec![t_spy_listen, t_spy_gossip] {
            t.join().unwrap();
        }
    }
}
