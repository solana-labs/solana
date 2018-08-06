//! The `drone` module provides an object for launching a Solana Drone,
//! which is the custodian of any remaining tokens in a mint.
//! The Solana Drone builds and send airdrop transactions,
//! checking requests against a request cap for a given time time_slice
//! and (to come) an IP rate limit.

use influx_db_client as influxdb;
use metrics;
use signature::Signature;
use signature::{KeyPair, PublicKey};
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;
use thin_client::ThinClient;
use transaction::Transaction;

pub const TIME_SLICE: u64 = 60;
pub const REQUEST_CAP: u64 = 1_000_000;
pub const DRONE_PORT: u16 = 9900;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum DroneRequest {
    GetAirdrop {
        airdrop_request_amount: u64,
        client_public_key: PublicKey,
    },
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

    pub fn check_request_limit(&mut self, request_amount: u64) -> bool {
        (self.request_current + request_amount) <= self.request_cap
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

    pub fn send_airdrop(&mut self, req: DroneRequest) -> Result<Signature, io::Error> {
        let request_amount: u64;
        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(
            self.requests_addr,
            requests_socket,
            self.transactions_addr,
            transactions_socket,
        );
        let last_id = client.get_last_id();

        let tx = match req {
            DroneRequest::GetAirdrop {
                airdrop_request_amount,
                client_public_key,
            } => {
                info!(
                    "Requesting airdrop of {} to {:?}",
                    airdrop_request_amount, client_public_key
                );
                request_amount = airdrop_request_amount;
                Transaction::new(
                    &self.mint_keypair,
                    client_public_key,
                    airdrop_request_amount as i64,
                    last_id,
                )
            }
        };
        if self.check_request_limit(request_amount) {
            self.request_current += request_amount;
            metrics::submit(
                influxdb::Point::new("drone")
                    .add_tag("op", influxdb::Value::String("airdrop".to_string()))
                    .add_field(
                        "request_amount",
                        influxdb::Value::Integer(request_amount as i64),
                    )
                    .add_field(
                        "request_current",
                        influxdb::Value::Integer(self.request_current as i64),
                    )
                    .to_owned(),
            );
            client.transfer_signed(&tx)
        } else {
            Err(Error::new(ErrorKind::Other, "token limit reached"))
        }
    }
}

impl Drop for Drone {
    fn drop(&mut self) {
        metrics::flush();
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::{get_ip_addr, TestNode};
    use drone::{Drone, DroneRequest, REQUEST_CAP, TIME_SLICE};
    use fullnode::FullNode;
    use logger;
    use mint::Mint;
    use service::Service;
    use signature::{KeyPair, KeyPairUtil};
    use std::fs::remove_dir_all;
    use std::net::{SocketAddr, UdpSocket};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use thin_client::ThinClient;

    #[test]
    fn test_check_request_limit() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let transactions_addr = "0.0.0.0:0".parse().unwrap();
        let requests_addr = "0.0.0.0:0".parse().unwrap();
        let mut drone = Drone::new(
            keypair,
            addr,
            transactions_addr,
            requests_addr,
            None,
            Some(3),
        );
        assert!(drone.check_request_limit(1));
        drone.request_current = 3;
        assert!(!drone.check_request_limit(1));
    }

    #[test]
    fn test_clear_request_count() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let transactions_addr = "0.0.0.0:0".parse().unwrap();
        let requests_addr = "0.0.0.0:0".parse().unwrap();
        let mut drone = Drone::new(keypair, addr, transactions_addr, requests_addr, None, None);
        drone.request_current = drone.request_current + 256;
        assert_eq!(drone.request_current, 256);
        drone.clear_request_count();
        assert_eq!(drone.request_current, 0);
    }

    #[test]
    fn test_add_ip_to_cache() {
        let keypair = KeyPair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let transactions_addr = "0.0.0.0:0".parse().unwrap();
        let requests_addr = "0.0.0.0:0".parse().unwrap();
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
        let transactions_addr = "0.0.0.0:0".parse().unwrap();
        let requests_addr = "0.0.0.0:0".parse().unwrap();
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
        let transactions_addr = "0.0.0.0:0".parse().unwrap();
        let requests_addr = "0.0.0.0:0".parse().unwrap();
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

    fn tmp_ledger_path(name: &str) -> String {
        let keypair = KeyPair::new();

        format!("/tmp/tmp-ledger-{}-{}", name, keypair.pubkey())
    }

    #[test]
    #[ignore]
    fn test_send_airdrop() {
        const SMALL_BATCH: i64 = 50;
        const TPS_BATCH: i64 = 5_000_000;

        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());

        let alice = Mint::new(10_000_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let carlos_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let leader_data = leader.data.clone();
        let ledger_path = tmp_ledger_path("send_airdrop");

        let server = FullNode::new_leader(
            leader_keypair,
            bank,
            0,
            None,
            Some(Duration::from_millis(30)),
            leader,
            exit.clone(),
            &ledger_path,
            false,
        );
        //TODO: this seems unstable
        sleep(Duration::from_millis(900));

        let mut addr: SocketAddr = "0.0.0.0:9900".parse().expect("bind to drone socket");
        addr.set_ip(get_ip_addr().expect("drone get_ip_addr"));
        let mut drone = Drone::new(
            alice.keypair(),
            addr,
            leader_data.contact_info.tpu,
            leader_data.contact_info.rpu,
            None,
            Some(150_000),
        );

        let requests_socket = UdpSocket::bind("0.0.0.0:0").expect("drone bind to requests socket");
        let transactions_socket =
            UdpSocket::bind("0.0.0.0:0").expect("drone bind to transactions socket");

        let mut client = ThinClient::new(
            leader_data.contact_info.rpu,
            requests_socket,
            leader_data.contact_info.tpu,
            transactions_socket,
        );

        let bob_req = DroneRequest::GetAirdrop {
            airdrop_request_amount: 50,
            client_public_key: bob_pubkey,
        };
        let bob_sig = drone.send_airdrop(bob_req).unwrap();
        assert!(client.poll_for_signature(&bob_sig).is_ok());

        let carlos_req = DroneRequest::GetAirdrop {
            airdrop_request_amount: 5_000_000,
            client_public_key: carlos_pubkey,
        };
        let carlos_sig = drone.send_airdrop(carlos_req).unwrap();
        assert!(client.poll_for_signature(&carlos_sig).is_ok());

        let bob_balance = client.get_balance(&bob_pubkey);
        info!("Small request balance: {:?}", bob_balance);
        assert_eq!(bob_balance.unwrap(), SMALL_BATCH);

        let carlos_balance = client.get_balance(&carlos_pubkey);
        info!("TPS request balance: {:?}", carlos_balance);
        assert_eq!(carlos_balance.unwrap(), TPS_BATCH);

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
}
