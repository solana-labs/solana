//! The `drone` module provides an object for launching a Solana Drone,
//! which is the custodian of any remaining tokens in a mint.
//! The Solana Drone builds and send airdrop transactions,
//! checking requests against a request cap for a given time time_slice
//! and (to come) an IP rate limit.

use bincode::{deserialize, serialize};
use bytes::Bytes;
use influx_db_client as influxdb;
use metrics;
use signature::{Keypair, Signature};
use solana_sdk::pubkey::Pubkey;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use system_transaction::SystemTransaction;
use thin_client::{poll_gossip_for_leader, ThinClient};
use tokio;
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio_codec::{BytesCodec, Decoder};
use transaction::Transaction;

pub const TIME_SLICE: u64 = 60;
pub const REQUEST_CAP: u64 = 500_000_000;
pub const DRONE_PORT: u16 = 9900;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum DroneRequest {
    GetAirdrop {
        airdrop_request_amount: u64,
        client_pubkey: Pubkey,
    },
}

pub struct Drone {
    mint_keypair: Keypair,
    ip_cache: Vec<IpAddr>,
    _airdrop_addr: SocketAddr,
    network_addr: SocketAddr,
    pub time_slice: Duration,
    request_cap: u64,
    pub request_current: u64,
}

impl Drone {
    pub fn new(
        mint_keypair: Keypair,
        _airdrop_addr: SocketAddr,
        network_addr: SocketAddr,
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
            network_addr,
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

        let leader = poll_gossip_for_leader(self.network_addr, Some(10))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let mut client = ThinClient::new(
            leader.contact_info.rpu,
            requests_socket,
            leader.contact_info.tpu,
            transactions_socket,
        );
        let last_id = client.get_last_id();

        let tx = match req {
            DroneRequest::GetAirdrop {
                airdrop_request_amount,
                client_pubkey,
            } => {
                info!(
                    "Requesting airdrop of {} to {:?}",
                    airdrop_request_amount, client_pubkey
                );
                request_amount = airdrop_request_amount;
                Transaction::system_new(
                    &self.mint_keypair,
                    client_pubkey,
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
                    ).add_field(
                        "request_current",
                        influxdb::Value::Integer(self.request_current as i64),
                    ).to_owned(),
            );
            client.retry_transfer_signed(&tx, 10)
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

pub fn run_local_drone(mint_keypair: Keypair, network: SocketAddr, sender: Sender<SocketAddr>) {
    thread::spawn(move || {
        let drone_addr = socketaddr!(0, 0);
        let drone = Arc::new(Mutex::new(Drone::new(
            mint_keypair,
            drone_addr,
            network,
            None,
            None,
        )));
        let socket = TcpListener::bind(&drone_addr).unwrap();
        sender.send(socket.local_addr().unwrap()).unwrap();
        info!("Drone started. Listening on: {}", drone_addr);
        let done = socket
            .incoming()
            .map_err(|e| debug!("failed to accept socket; error = {:?}", e))
            .for_each(move |socket| {
                let drone2 = drone.clone();
                let framed = BytesCodec::new().framed(socket);
                let (writer, reader) = framed.split();

                let processor = reader.and_then(move |bytes| {
                    let req: DroneRequest = deserialize(&bytes).or_else(|err| {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("deserialize packet in drone: {:?}", err),
                        ))
                    })?;

                    info!("Airdrop requested...");
                    let res1 = drone2.lock().unwrap().send_airdrop(req);
                    match res1 {
                        Ok(_) => info!("Airdrop sent!"),
                        Err(_) => info!("Request limit reached for this time slice"),
                    }
                    let response = res1?;
                    info!("Airdrop tx signature: {:?}", response);
                    let response_vec = serialize(&response).or_else(|err| {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("serialize signature in drone: {:?}", err),
                        ))
                    })?;
                    let response_bytes = Bytes::from(response_vec.clone());
                    Ok(response_bytes)
                });
                let server = writer
                    .send_all(processor.or_else(|err| {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Drone response: {:?}", err),
                        ))
                    })).then(|_| Ok(()));
                tokio::spawn(server)
            });
        tokio::run(done);
    });
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use cluster_info::Node;
    use drone::{Drone, DroneRequest, REQUEST_CAP, TIME_SLICE};
    use fullnode::Fullnode;
    use leader_scheduler::LeaderScheduler;
    use ledger::get_tmp_ledger_path;
    use logger;
    use mint::Mint;
    use netutil::get_ip_addr;
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::net::{SocketAddr, UdpSocket};
    use std::time::Duration;
    use thin_client::ThinClient;

    #[test]
    fn test_check_request_limit() {
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let network_addr = "0.0.0.0:0".parse().unwrap();
        let mut drone = Drone::new(keypair, addr, network_addr, None, Some(3));
        assert!(drone.check_request_limit(1));
        drone.request_current = 3;
        assert!(!drone.check_request_limit(1));
    }

    #[test]
    fn test_clear_request_count() {
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let network_addr = "0.0.0.0:0".parse().unwrap();
        let mut drone = Drone::new(keypair, addr, network_addr, None, None);
        drone.request_current = drone.request_current + 256;
        assert_eq!(drone.request_current, 256);
        drone.clear_request_count();
        assert_eq!(drone.request_current, 0);
    }

    #[test]
    fn test_add_ip_to_cache() {
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let network_addr = "0.0.0.0:0".parse().unwrap();
        let mut drone = Drone::new(keypair, addr, network_addr, None, None);
        let ip = "127.0.0.1".parse().expect("create IpAddr from string");
        assert_eq!(drone.ip_cache.len(), 0);
        drone.add_ip_to_cache(ip);
        assert_eq!(drone.ip_cache.len(), 1);
        assert!(drone.ip_cache.contains(&ip));
    }

    #[test]
    fn test_clear_ip_cache() {
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let network_addr = "0.0.0.0:0".parse().unwrap();
        let mut drone = Drone::new(keypair, addr, network_addr, None, None);
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
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let network_addr = "0.0.0.0:0".parse().unwrap();
        let time_slice: Option<u64> = None;
        let request_cap: Option<u64> = None;
        let drone = Drone::new(keypair, addr, network_addr, time_slice, request_cap);
        assert_eq!(drone.time_slice, Duration::new(TIME_SLICE, 0));
        assert_eq!(drone.request_cap, REQUEST_CAP);
    }

    #[test]
    #[ignore]
    fn test_send_airdrop() {
        const SMALL_BATCH: i64 = 50;
        const TPS_BATCH: i64 = 5_000_000;

        logger::setup();
        let leader_keypair = Keypair::new();
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let alice = Mint::new(10_000_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let carlos_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let ledger_path = get_tmp_ledger_path("send_airdrop");

        let server = Fullnode::new_with_bank(
            leader_keypair,
            bank,
            0,
            0,
            &[],
            leader,
            None,
            &ledger_path,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_data.id),
            Some(0),
        );

        let mut addr: SocketAddr = "0.0.0.0:9900".parse().expect("bind to drone socket");
        addr.set_ip(get_ip_addr().expect("drone get_ip_addr"));
        let mut drone = Drone::new(
            alice.keypair(),
            addr,
            leader_data.contact_info.ncp,
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
            client_pubkey: bob_pubkey,
        };
        let bob_sig = drone.send_airdrop(bob_req).unwrap();
        assert!(client.poll_for_signature(&bob_sig).is_ok());

        // restart the leader, drone should find the new one at the same gossip port
        server.close().unwrap();

        let leader_keypair = Keypair::new();
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();
        let server = Fullnode::new(
            leader,
            &ledger_path,
            leader_keypair,
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_data.id),
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

        let carlos_req = DroneRequest::GetAirdrop {
            airdrop_request_amount: 5_000_000,
            client_pubkey: carlos_pubkey,
        };

        // using existing drone, new thin client
        let carlos_sig = drone.send_airdrop(carlos_req).unwrap();
        assert!(client.poll_for_signature(&carlos_sig).is_ok());

        let bob_balance = client.get_balance(&bob_pubkey);
        info!("Small request balance: {:?}", bob_balance);
        assert_eq!(bob_balance.unwrap(), SMALL_BATCH);

        let carlos_balance = client.get_balance(&carlos_pubkey);
        info!("TPS request balance: {:?}", carlos_balance);
        assert_eq!(carlos_balance.unwrap(), TPS_BATCH);

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
}
