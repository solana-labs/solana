//! The `faucet` module provides an object for launching a Solana Faucet,
//! which is the custodian of any remaining lamports in a mint.
//! The Solana Faucet builds and send airdrop transactions,
//! checking requests against a request cap for a given time time_slice
//! and (to come) an IP rate limit.

use bincode::{deserialize, serialize, serialized_size};
use byteorder::{ByteOrder, LittleEndian};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_metrics::datapoint_info;
use solana_sdk::{
    hash::Hash,
    message::Message,
    packet::PACKET_DATA_SIZE,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use std::{
    io::{self, Error, ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    sync::{mpsc::Sender, Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream as TokioTcpStream},
    runtime::Runtime,
};

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
pub const REQUEST_CAP: u64 = solana_sdk::native_token::LAMPORTS_PER_SOL * 10_000_000;
pub const FAUCET_PORT: u16 = 9900;
pub const FAUCET_PORT_STR: &str = "9900";

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum FaucetRequest {
    GetAirdrop {
        lamports: u64,
        to: Pubkey,
        blockhash: Hash,
    },
}

impl Default for FaucetRequest {
    fn default() -> Self {
        Self::GetAirdrop {
            lamports: u64::default(),
            to: Pubkey::default(),
            blockhash: Hash::default(),
        }
    }
}

pub struct Faucet {
    faucet_keypair: Keypair,
    ip_cache: Vec<IpAddr>,
    pub time_slice: Duration,
    per_time_cap: u64,
    per_request_cap: Option<u64>,
    pub request_current: u64,
}

impl Faucet {
    pub fn new(
        faucet_keypair: Keypair,
        time_input: Option<u64>,
        per_time_cap: Option<u64>,
        per_request_cap: Option<u64>,
    ) -> Faucet {
        let time_slice = Duration::new(time_input.unwrap_or(TIME_SLICE), 0);
        let per_time_cap = per_time_cap.unwrap_or(REQUEST_CAP);
        Faucet {
            faucet_keypair,
            ip_cache: Vec::new(),
            time_slice,
            per_time_cap,
            per_request_cap,
            request_current: 0,
        }
    }

    pub fn check_time_request_limit(&mut self, request_amount: u64) -> bool {
        self.request_current
            .checked_add(request_amount)
            .map(|s| s <= self.per_time_cap)
            .unwrap_or(false)
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

    pub fn build_airdrop_transaction(
        &mut self,
        req: FaucetRequest,
    ) -> Result<Transaction, io::Error> {
        trace!("build_airdrop_transaction: {:?}", req);
        match req {
            FaucetRequest::GetAirdrop {
                lamports,
                to,
                blockhash,
            } => {
                if let Some(cap) = self.per_request_cap {
                    if lamports > cap {
                        return Err(Error::new(
                            ErrorKind::Other,
                            format!("request too large; req: {} cap: {}", lamports, cap),
                        ));
                    }
                }
                if self.check_time_request_limit(lamports) {
                    self.request_current = self.request_current.saturating_add(lamports);
                    datapoint_info!(
                        "faucet-airdrop",
                        ("request_amount", lamports, i64),
                        ("request_current", self.request_current, i64)
                    );
                    info!("Requesting airdrop of {} to {:?}", lamports, to);

                    let mint_pubkey = self.faucet_keypair.pubkey();
                    let create_instruction =
                        system_instruction::transfer(&mint_pubkey, &to, lamports);
                    let message = Message::new(&[create_instruction], Some(&mint_pubkey));
                    Ok(Transaction::new(
                        &[&self.faucet_keypair],
                        message,
                        blockhash,
                    ))
                } else {
                    Err(Error::new(
                        ErrorKind::Other,
                        format!(
                            "token limit reached; req: {} current: {} cap: {}",
                            lamports, self.request_current, self.per_time_cap
                        ),
                    ))
                }
            }
        }
    }
    pub fn process_faucet_request(&mut self, bytes: &[u8]) -> Result<Vec<u8>, io::Error> {
        let req: FaucetRequest = deserialize(bytes).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("deserialize packet in faucet: {:?}", err),
            )
        })?;

        info!("Airdrop transaction requested...{:?}", req);
        let res = self.build_airdrop_transaction(req);
        match res {
            Ok(tx) => {
                let response_vec = bincode::serialize(&tx).map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("deserialize packet in faucet: {:?}", err),
                    )
                })?;

                let mut response_vec_with_length = vec![0; 2];
                LittleEndian::write_u16(&mut response_vec_with_length, response_vec.len() as u16);
                response_vec_with_length.extend_from_slice(&response_vec);

                info!("Airdrop transaction granted");
                Ok(response_vec_with_length)
            }
            Err(err) => {
                warn!("Airdrop transaction failed: {:?}", err);
                Err(err)
            }
        }
    }
}

impl Drop for Faucet {
    fn drop(&mut self) {
        solana_metrics::flush();
    }
}

pub fn request_airdrop_transaction(
    faucet_addr: &SocketAddr,
    id: &Pubkey,
    lamports: u64,
    blockhash: Hash,
) -> Result<Transaction, Error> {
    info!(
        "request_airdrop_transaction: faucet_addr={} id={} lamports={} blockhash={}",
        faucet_addr, id, lamports, blockhash
    );

    let mut stream = TcpStream::connect_timeout(faucet_addr, Duration::new(3, 0))?;
    stream.set_read_timeout(Some(Duration::new(10, 0)))?;
    let req = FaucetRequest::GetAirdrop {
        lamports,
        blockhash,
        to: *id,
    };
    let req = serialize(&req).expect("serialize faucet request");
    stream.write_all(&req)?;

    // Read length of transaction
    let mut buffer = [0; 2];
    stream.read_exact(&mut buffer).map_err(|err| {
        info!(
            "request_airdrop_transaction: buffer length read_exact error: {:?}",
            err
        );
        Error::new(ErrorKind::Other, "Airdrop failed")
    })?;
    let transaction_length = LittleEndian::read_u16(&buffer) as usize;
    if transaction_length > PACKET_DATA_SIZE || transaction_length == 0 {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "request_airdrop_transaction: invalid transaction_length from faucet: {}",
                transaction_length
            ),
        ));
    }

    // Read the transaction
    let mut buffer = Vec::new();
    buffer.resize(transaction_length, 0);
    stream.read_exact(&mut buffer).map_err(|err| {
        info!(
            "request_airdrop_transaction: buffer read_exact error: {:?}",
            err
        );
        Error::new(ErrorKind::Other, "Airdrop failed")
    })?;

    let transaction: Transaction = deserialize(&buffer).map_err(|err| {
        Error::new(
            ErrorKind::Other,
            format!("request_airdrop_transaction deserialize failure: {:?}", err),
        )
    })?;
    Ok(transaction)
}

pub fn run_local_faucet_with_port(
    faucet_keypair: Keypair,
    sender: Sender<Result<SocketAddr, String>>,
    per_time_cap: Option<u64>,
    port: u16, // 0 => auto assign
) {
    thread::spawn(move || {
        let faucet_addr = socketaddr!(0, port);
        let faucet = Arc::new(Mutex::new(Faucet::new(
            faucet_keypair,
            None,
            per_time_cap,
            None,
        )));
        let runtime = Runtime::new().unwrap();
        runtime.block_on(run_faucet(faucet, faucet_addr, Some(sender)));
    });
}

// For integration tests. Listens on random open port and reports port to Sender.
pub fn run_local_faucet(faucet_keypair: Keypair, per_time_cap: Option<u64>) -> SocketAddr {
    let (sender, receiver) = std::sync::mpsc::channel();
    run_local_faucet_with_port(faucet_keypair, sender, per_time_cap, 0);
    receiver
        .recv()
        .expect("run_local_faucet")
        .expect("faucet_addr")
}

pub async fn run_faucet(
    faucet: Arc<Mutex<Faucet>>,
    faucet_addr: SocketAddr,
    sender: Option<Sender<Result<SocketAddr, String>>>,
) {
    let listener = TcpListener::bind(&faucet_addr).await;
    if let Some(sender) = sender {
        sender.send(
            listener.as_ref().map(|listener| listener.local_addr().unwrap())
                .map_err(|err| {
                    format!(
                        "Unable to bind faucet to {:?}, check the address is not already in use: {}",
                        faucet_addr, err
                    )
                })
            )
            .unwrap();
    }

    let listener = match listener {
        Err(err) => {
            error!("Faucet failed to start: {}", err);
            return;
        }
        Ok(listener) => listener,
    };
    info!("Faucet started. Listening on: {}", faucet_addr);
    info!(
        "Faucet account address: {}",
        faucet.lock().unwrap().faucet_keypair.pubkey()
    );

    loop {
        let _faucet = faucet.clone();
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(async move {
                    if let Err(e) = process(stream, _faucet).await {
                        info!("failed to process request; error = {:?}", e);
                    }
                });
            }
            Err(e) => debug!("failed to accept socket; error = {:?}", e),
        }
    }
}

async fn process(
    mut stream: TokioTcpStream,
    faucet: Arc<Mutex<Faucet>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut request = vec![0u8; serialized_size(&FaucetRequest::default()).unwrap() as usize];
    while stream.read_exact(&mut request).await.is_ok() {
        trace!("{:?}", request);

        let response = match faucet.lock().unwrap().process_faucet_request(&request) {
            Ok(response_bytes) => {
                trace!("Airdrop response_bytes: {:?}", response_bytes);
                response_bytes
            }
            Err(e) => {
                info!("Error in request: {:?}", e);
                0u16.to_le_bytes().to_vec()
            }
        };
        stream.write_all(&response).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::system_instruction::SystemInstruction;
    use std::time::Duration;

    #[test]
    fn test_check_time_request_limit() {
        let keypair = Keypair::new();
        let mut faucet = Faucet::new(keypair, None, Some(3), None);
        assert!(faucet.check_time_request_limit(1));
        faucet.request_current = 3;
        assert!(!faucet.check_time_request_limit(1));
        faucet.request_current = 1;
        assert!(!faucet.check_time_request_limit(u64::MAX));
    }

    #[test]
    fn test_clear_request_count() {
        let keypair = Keypair::new();
        let mut faucet = Faucet::new(keypair, None, None, None);
        faucet.request_current += 256;
        assert_eq!(faucet.request_current, 256);
        faucet.clear_request_count();
        assert_eq!(faucet.request_current, 0);
    }

    #[test]
    fn test_add_ip_to_cache() {
        let keypair = Keypair::new();
        let mut faucet = Faucet::new(keypair, None, None, None);
        let ip = "127.0.0.1".parse().expect("create IpAddr from string");
        assert_eq!(faucet.ip_cache.len(), 0);
        faucet.add_ip_to_cache(ip);
        assert_eq!(faucet.ip_cache.len(), 1);
        assert!(faucet.ip_cache.contains(&ip));
    }

    #[test]
    fn test_clear_ip_cache() {
        let keypair = Keypair::new();
        let mut faucet = Faucet::new(keypair, None, None, None);
        let ip = "127.0.0.1".parse().expect("create IpAddr from string");
        assert_eq!(faucet.ip_cache.len(), 0);
        faucet.add_ip_to_cache(ip);
        assert_eq!(faucet.ip_cache.len(), 1);
        faucet.clear_ip_cache();
        assert_eq!(faucet.ip_cache.len(), 0);
        assert!(faucet.ip_cache.is_empty());
    }

    #[test]
    fn test_faucet_default_init() {
        let keypair = Keypair::new();
        let time_slice: Option<u64> = None;
        let request_cap: Option<u64> = None;
        let faucet = Faucet::new(keypair, time_slice, request_cap, Some(100));
        assert_eq!(faucet.time_slice, Duration::new(TIME_SLICE, 0));
        assert_eq!(faucet.per_time_cap, REQUEST_CAP);
        assert_eq!(faucet.per_request_cap, Some(100));
    }

    #[test]
    fn test_faucet_build_airdrop_transaction() {
        let to = solana_sdk::pubkey::new_rand();
        let blockhash = Hash::default();
        let request = FaucetRequest::GetAirdrop {
            lamports: 2,
            to,
            blockhash,
        };

        let mint = Keypair::new();
        let mint_pubkey = mint.pubkey();
        let mut faucet = Faucet::new(mint, None, None, None);

        let tx = faucet.build_airdrop_transaction(request).unwrap();
        let message = tx.message();

        assert_eq!(tx.signatures.len(), 1);
        assert_eq!(
            message.account_keys,
            vec![mint_pubkey, to, Pubkey::default()]
        );
        assert_eq!(message.recent_blockhash, blockhash);

        assert_eq!(message.instructions.len(), 1);
        let instruction: SystemInstruction = deserialize(&message.instructions[0].data).unwrap();
        assert_eq!(instruction, SystemInstruction::Transfer { lamports: 2 });

        // Test per-time request cap
        let mint = Keypair::new();
        faucet = Faucet::new(mint, None, Some(1), None);
        let tx = faucet.build_airdrop_transaction(request);
        assert!(tx.is_err());

        // Test per-request cap
        let mint = Keypair::new();
        faucet = Faucet::new(mint, None, None, Some(1));
        let tx = faucet.build_airdrop_transaction(request);
        assert!(tx.is_err());
    }

    #[test]
    fn test_process_faucet_request() {
        let to = solana_sdk::pubkey::new_rand();
        let blockhash = Hash::new(&to.as_ref());
        let lamports = 50;
        let req = FaucetRequest::GetAirdrop {
            lamports,
            blockhash,
            to,
        };
        let req = serialize(&req).unwrap();

        let keypair = Keypair::new();
        let expected_instruction = system_instruction::transfer(&keypair.pubkey(), &to, lamports);
        let message = Message::new(&[expected_instruction], Some(&keypair.pubkey()));
        let expected_tx = Transaction::new(&[&keypair], message, blockhash);
        let expected_bytes = serialize(&expected_tx).unwrap();
        let mut expected_vec_with_length = vec![0; 2];
        LittleEndian::write_u16(&mut expected_vec_with_length, expected_bytes.len() as u16);
        expected_vec_with_length.extend_from_slice(&expected_bytes);

        let mut faucet = Faucet::new(keypair, None, None, None);
        let response = faucet.process_faucet_request(&req);
        let response_vec = response.unwrap().to_vec();
        assert_eq!(expected_vec_with_length, response_vec);

        let bad_bytes = "bad bytes".as_bytes();
        assert!(faucet.process_faucet_request(&bad_bytes).is_err());
    }
}
