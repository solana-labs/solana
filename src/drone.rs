//! The `drone` module provides an object for launching a Solana Drone,
//! which is the custodian of any remaining tokens in a mint.
//! The Solana Drone builds and send airdrop transactions,
//! checking requests against a request cap for a given time time_slice
//! and (to come) an IP rate limit.

use signature::{KeyPair, PublicKey};
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;
use thin_client::ThinClient;
use transaction::Transaction;

pub const TIME_SLICE: u64 = 60;
pub const REQUEST_CAP: u64 = 500;
pub const SMALL_BATCH: i64 = 50;
pub const TPS_BATCH: i64 = 5_000_000;

#[derive(Serialize, Deserialize, Debug)]
pub enum DroneRequest {
    GetAirdrop {
        request_type: DroneRequestType,
        client_public_key: PublicKey,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum DroneRequestType {
    SmallBatch,
    TPSBatch,
}

pub struct Drone {
    mint_keypair: KeyPair,
    ip_cache: Vec<IpAddr>,
    _airdrop_addr: SocketAddr,
    transactions_addr: SocketAddr,
    requests_addr: SocketAddr,
    pub time_slice: Duration,
    request_cap: u64,
    pub request_current: u64,
}

impl Drone {
    pub fn new(
        mint_keypair: KeyPair,
        _airdrop_addr: SocketAddr,
        transactions_addr: SocketAddr,
        requests_addr: SocketAddr,
        time_input: Option<u64>,
        request_cap_input: Option<u64>,
    ) -> Drone {
        let time_slice = match time_input {
            Some(time) => Duration::new(time, 0),
            None => Duration::new(TIME_SLICE, 0),
        };
        let request_cap = match request_cap_input {
            Some(cap) => cap,
            None => REQUEST_CAP,
        };
        Drone {
            mint_keypair,
            ip_cache: Vec::new(),
            _airdrop_addr,
            transactions_addr,
            requests_addr,
            time_slice,
            request_cap,
            request_current: 0,
        }
    }

    pub fn check_request_count(&mut self) -> bool {
        if self.request_current <= self.request_cap {
            true
        } else {
            false
        }
    }

    pub fn clear_request_count(&mut self) {
        self.request_current = 0;
    }

    pub fn add_ip_to_cache(&mut self, ip: IpAddr) {
        self.ip_cache.push(ip);
    }

    pub fn clear_ip_cache(&mut self) {
        self.ip_cache.clear();
    }

    pub fn check_rate_limit(&mut self, ip: IpAddr) -> Result<IpAddr, IpAddr> {
        // [WIP] This is placeholder code for a proper rate limiter.
        // Right now it will only allow one total drone request per IP
        if self.ip_cache.contains(&ip) {
            // Add proper error handling here
            Err(ip)
        } else {
            self.add_ip_to_cache(ip);
            Ok(ip)
        }
    }

    pub fn send_airdrop(&mut self, req: DroneRequest) -> Result<usize, io::Error> {
        self.request_current += 1;

        if self.check_request_count() {
            let tx: Transaction;
            let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let mut client = ThinClient::new(
                self.requests_addr,
                requests_socket,
                self.transactions_addr,
                transactions_socket,
            );
            let last_id = client.get_last_id();

            match req {
                DroneRequest::GetAirdrop {
                    request_type,
                    client_public_key,
                } => match request_type {
                    DroneRequestType::SmallBatch => {
                        tx = Transaction::new(
                            &self.mint_keypair,
                            client_public_key,
                            SMALL_BATCH,
                            last_id,
                        );
                    }
                    DroneRequestType::TPSBatch => {
                        tx = Transaction::new(
                            &self.mint_keypair,
                            client_public_key,
                            TPS_BATCH,
                            last_id,
                        );
                    }
                },
            }
            client.transfer_signed(tx)
        } else {
            Err(Error::new(ErrorKind::Other, "request limit reached"))
        }
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::{get_ip_addr, TestNode};
    use drone::{Drone, DroneRequest, DroneRequestType, REQUEST_CAP, SMALL_BATCH, TIME_SLICE,
                TPS_BATCH};
    use logger;
    use mint::Mint;
    use server::Server;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::net::{SocketAddr, UdpSocket};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::sleep;
    use std::time::Duration;
    use thin_client::ThinClient;

    #[test]
    fn test_check_request_count() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut transactions_addr = addr.clone();
        transactions_addr.set_port(8000);
        let mut requests_addr = addr.clone();
        requests_addr.set_port(8003);
        let mut drone = Drone::new(
            keypair,
            addr,
            transactions_addr,
            requests_addr,
            None,
            Some(3),
        );
        assert!(drone.check_request_count());
        drone.request_current = 4;
        assert!(!drone.check_request_count());
    }

    #[test]
    fn test_clear_request_count() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut transactions_addr = addr.clone();
        transactions_addr.set_port(8000);
        let mut requests_addr = addr.clone();
        requests_addr.set_port(8003);
        let mut drone = Drone::new(keypair, addr, transactions_addr, requests_addr, None, None);
        drone.request_current = drone.request_current + 256;
        assert!(drone.request_current == 256);
        drone.clear_request_count();
        assert!(drone.request_current == 0);
    }

    #[test]
    fn test_add_ip_to_cache() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut transactions_addr = addr.clone();
        transactions_addr.set_port(8000);
        let mut requests_addr = addr.clone();
        requests_addr.set_port(8003);
        let mut drone = Drone::new(keypair, addr, transactions_addr, requests_addr, None, None);
        let ip = "127.0.0.1".parse().expect("create IpAddr from string");
        assert_eq!(drone.ip_cache.len(), 0);
        drone.add_ip_to_cache(ip);
        assert_eq!(drone.ip_cache.len(), 1);
        assert!(drone.ip_cache.contains(&ip));
    }

    #[test]
    fn test_clear_ip_cache() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut transactions_addr = addr.clone();
        transactions_addr.set_port(8000);
        let mut requests_addr = addr.clone();
        requests_addr.set_port(8003);
        let mut drone = Drone::new(keypair, addr, transactions_addr, requests_addr, None, None);
        let ip = "127.0.0.1".parse().expect("create IpAddr from string");
        assert_eq!(drone.ip_cache.len(), 0);
        drone.add_ip_to_cache(ip);
        assert_eq!(drone.ip_cache.len(), 1);
        drone.clear_ip_cache();
        assert_eq!(drone.ip_cache.len(), 0);
        assert!(drone.ip_cache.is_empty());
    }

    #[test]
    fn test_drone_default_init() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut transactions_addr = addr.clone();
        transactions_addr.set_port(8000);
        let mut requests_addr = addr.clone();
        requests_addr.set_port(8003);
        let time_slice: Option<u64> = None;
        let request_cap: Option<u64> = None;
        let drone = Drone::new(
            keypair,
            addr,
            transactions_addr,
            requests_addr,
            time_slice,
            request_cap,
        );
        assert_eq!(drone.time_slice, Duration::new(TIME_SLICE, 0));
        assert_eq!(drone.request_cap, REQUEST_CAP);
    }

    #[test]
    fn test_send_airdrop() {
        logger::setup();
        let leader = TestNode::new();

        let alice = Mint::new(10_000_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let carlos_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let server = Server::new_leader(
            bank,
            Some(Duration::from_millis(30)),
            leader.data.clone(),
            leader.sockets.requests,
            leader.sockets.transaction,
            leader.sockets.broadcast,
            leader.sockets.respond,
            leader.sockets.gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(900));

        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut drone = Drone::new(
            alice.keypair(),
            addr,
            leader.data.transactions_addr,
            leader.data.requests_addr,
            None,
            None,
        );

        let bob_req = DroneRequest::GetAirdrop {
            request_type: DroneRequestType::SmallBatch,
            client_public_key: bob_pubkey,
        };
        let bob_result = drone.send_airdrop(bob_req).expect("send airdrop test");
        assert!(bob_result > 0);

        let carlos_req = DroneRequest::GetAirdrop {
            request_type: DroneRequestType::TPSBatch,
            client_public_key: carlos_pubkey,
        };
        let carlos_result = drone.send_airdrop(carlos_req).expect("send airdrop test");
        assert!(carlos_result > 0);

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(
            leader.data.requests_addr,
            requests_socket,
            leader.data.transactions_addr,
            transactions_socket,
        );

        let bob_balance = client.poll_get_balance(&bob_pubkey);
        info!("Small batch balance: {:?}", bob_balance);
        assert_eq!(bob_balance.unwrap(), SMALL_BATCH);

        let carlos_balance = client.poll_get_balance(&carlos_pubkey);
        info!("TPS batch balance: {:?}", carlos_balance);
        assert_eq!(carlos_balance.unwrap(), TPS_BATCH);

        exit.store(true, Ordering::Relaxed);
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }
}
