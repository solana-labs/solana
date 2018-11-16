//! The `drone` module provides an object for launching a Solana Drone,
//! which is the custodian of any remaining tokens in a mint.
//! The Solana Drone builds and send airdrop transactions,
//! checking requests against a request cap for a given time time_slice
//! and (to come) an IP rate limit.

use bincode::{deserialize, serialize};
use byteorder::{ByteOrder, LittleEndian};
use bytes::Bytes;
use solana_metrics;
use solana_metrics::influxdb;
use solana_sdk::hash::Hash;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::system_instruction::{SystemInstruction, SYSTEM_PROGRAM_ID};
use solana_sdk::transaction::Transaction;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio;
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio_codec::{BytesCodec, Decoder};

#[macro_export]
macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: SocketAddr = $str.parse().unwrap();
        a
    }};
}

pub const TIME_SLICE: u64 = 60;
pub const REQUEST_CAP: u64 = 500_000_000;
pub const DRONE_PORT: u16 = 9900;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum DroneRequest {
    GetAirdrop {
        tokens: u64,
        to: Pubkey,
        last_id: Hash,
    },
}

pub struct Drone {
    mint_keypair: Keypair,
    ip_cache: Vec<IpAddr>,
    pub time_slice: Duration,
    request_cap: u64,
    pub request_current: u64,
}

impl Drone {
    pub fn new(
        mint_keypair: Keypair,
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

    pub fn build_airdrop_transaction(
        &mut self,
        req: DroneRequest,
    ) -> Result<Transaction, io::Error> {
        trace!("build_airdrop_transaction: {:?}", req);
        match req {
            DroneRequest::GetAirdrop {
                tokens,
                to,
                last_id,
            } => {
                if self.check_request_limit(tokens) {
                    self.request_current += tokens;
                    solana_metrics::submit(
                        influxdb::Point::new("drone")
                            .add_tag("op", influxdb::Value::String("airdrop".to_string()))
                            .add_field("request_amount", influxdb::Value::Integer(tokens as i64))
                            .add_field(
                                "request_current",
                                influxdb::Value::Integer(self.request_current as i64),
                            ).to_owned(),
                    );

                    info!("Requesting airdrop of {} to {:?}", tokens, to);

                    let create_instruction = SystemInstruction::CreateAccount {
                        tokens,
                        space: 0,
                        program_id: Pubkey::default(),
                    };
                    let mut transaction = Transaction::new(
                        &self.mint_keypair,
                        &[to],
                        Pubkey::new(&SYSTEM_PROGRAM_ID),
                        &create_instruction,
                        last_id,
                        0, /*fee*/
                    );

                    transaction.sign(&[&self.mint_keypair], last_id);
                    Ok(transaction)
                } else {
                    Err(Error::new(ErrorKind::Other, "token limit reached"))
                }
            }
        }
    }
}

impl Drop for Drone {
    fn drop(&mut self) {
        solana_metrics::flush();
    }
}

pub fn request_airdrop_transaction(
    drone_addr: &SocketAddr,
    id: &Pubkey,
    tokens: u64,
    last_id: Hash,
) -> Result<Transaction, Error> {
    // TODO: make this async tokio client
    let mut stream = TcpStream::connect_timeout(drone_addr, Duration::new(3, 0))?;
    stream.set_read_timeout(Some(Duration::new(10, 0)))?;
    let req = DroneRequest::GetAirdrop {
        tokens,
        last_id,
        to: *id,
    };
    let req = serialize(&req).expect("serialize drone request");
    stream.write_all(&req)?;

    // Read length of transaction
    let mut buffer = [0; 2];
    stream.read_exact(&mut buffer).or_else(|err| {
        info!(
            "request_airdrop_transaction: buffer length read_exact error: {:?}",
            err
        );
        Err(Error::new(ErrorKind::Other, "Airdrop failed"))
    })?;
    let transaction_length = LittleEndian::read_u16(&buffer) as usize;
    if transaction_length >= PACKET_DATA_SIZE {
        Err(Error::new(
            ErrorKind::Other,
            format!(
                "request_airdrop_transaction: invalid transaction_length from drone: {}",
                transaction_length
            ),
        ))?;
    }

    // Read the transaction
    let mut buffer = Vec::new();
    buffer.resize(transaction_length, 0);
    stream.read_exact(&mut buffer).or_else(|err| {
        info!(
            "request_airdrop_transaction: buffer read_exact error: {:?}",
            err
        );
        Err(Error::new(ErrorKind::Other, "Airdrop failed"))
    })?;

    let transaction: Transaction = deserialize(&buffer).or_else(|err| {
        Err(Error::new(
            ErrorKind::Other,
            format!("request_airdrop_transaction deserialize failure: {:?}", err),
        ))
    })?;
    Ok(transaction)
}

pub fn run_local_drone(mint_keypair: Keypair, sender: Sender<SocketAddr>) {
    thread::spawn(move || {
        let drone_addr = socketaddr!(0, 0);
        let drone = Arc::new(Mutex::new(Drone::new(mint_keypair, None, None)));
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
                    let res = drone2.lock().unwrap().build_airdrop_transaction(req);
                    match res {
                        Ok(_) => info!("Airdrop sent!"),
                        Err(_) => info!("Request limit reached for this time slice"),
                    }
                    let response = res?;

                    info!("Airdrop tx signature: {:?}", response);
                    let response_vec = serialize(&response).or_else(|err| {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("serialize signature in drone: {:?}", err),
                        ))
                    })?;

                    let mut response_vec_with_length = vec![0; 2];
                    LittleEndian::write_u16(
                        &mut response_vec_with_length,
                        response_vec.len() as u16,
                    );
                    info!(
                        "Airdrop response_vec_with_length: {:?}",
                        response_vec_with_length
                    );
                    response_vec_with_length.extend_from_slice(&response_vec);

                    let response_bytes = Bytes::from(response_vec_with_length.clone());
                    info!("Airdrop response_bytes: {:?}", response_bytes);
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
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::net::{SocketAddr, UdpSocket};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use thin_client::ThinClient;

    #[test]
    fn test_check_request_limit() {
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut drone = Drone::new(keypair, None, Some(3));
        assert!(drone.check_request_limit(1));
        drone.request_current = 3;
        assert!(!drone.check_request_limit(1));
    }

    #[test]
    fn test_clear_request_count() {
        let keypair = Keypair::new();
        let mut addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
        addr.set_ip(get_ip_addr().unwrap());
        let mut drone = Drone::new(keypair, None, None);
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
        let mut drone = Drone::new(keypair, None, None);
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
        let mut drone = Drone::new(keypair, None, None);
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
        let time_slice: Option<u64> = None;
        let request_cap: Option<u64> = None;
        let drone = Drone::new(keypair, time_slice, request_cap);
        assert_eq!(drone.time_slice, Duration::new(TIME_SLICE, 0));
        assert_eq!(drone.request_cap, REQUEST_CAP);
    }

    #[test]
    #[ignore]
    fn test_send_airdrop() {
        const SMALL_BATCH: u64 = 50;
        const TPS_BATCH: u64 = 5_000_000;

        logger::setup();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let alice = Mint::new(10_000_000);
        let mut bank = Bank::new(&alice);
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader.info.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let bob_pubkey = Keypair::new().pubkey();
        let carlos_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let ledger_path = get_tmp_ledger_path("send_airdrop");

        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );

        let mut addr: SocketAddr = "0.0.0.0:9900".parse().expect("bind to drone socket");
        addr.set_ip(get_ip_addr().expect("drone get_ip_addr"));
        let mut drone = Drone::new(alice.keypair(), None, Some(150_000));

        let transactions_socket =
            UdpSocket::bind("0.0.0.0:0").expect("drone bind to transactions socket");

        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);

        let bob_req = DroneRequest::GetAirdrop {
            tokens: 50,
            to: bob_pubkey,
            last_id,
        };
        let bob_tx = drone.build_airdrop_transaction(bob_req).unwrap();
        let bob_sig = client.transfer_signed(&bob_tx).unwrap();
        assert!(client.poll_for_signature(&bob_sig).is_ok());

        // restart the leader, drone should find the new one at the same gossip port
        server.close().unwrap();

        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();
        let server = Fullnode::new(
            leader,
            &ledger_path,
            leader_keypair,
            Arc::new(Keypair::new()),
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_data.id),
            None,
        );

        let transactions_socket =
            UdpSocket::bind("0.0.0.0:0").expect("drone bind to transactions socket");

        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);

        let carlos_req = DroneRequest::GetAirdrop {
            tokens: 5_000_000,
            to: carlos_pubkey,
            last_id,
        };

        // using existing drone, new thin client
        let carlos_tx = drone.build_airdrop_transaction(carlos_req).unwrap();
        let carlos_sig = client.transfer_signed(&carlos_tx).unwrap();
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
