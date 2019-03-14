//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use crate::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use bincode::serialize_into;
use bs58;
use log::*;
use serde_json::json;
use solana_metrics;
use solana_metrics::influxdb;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing;
use solana_sdk::transaction::Transaction;
use std;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    rpc_addr: SocketAddr,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    rpc_client: RpcClient,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with the Rpc at `rpc_addr` using TCP
    /// and the Tpu at `transactions_addr` over `transactions_socket` using UDP.
    pub fn new(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
    ) -> Self {
        Self::new_from_client(
            rpc_addr,
            transactions_addr,
            transactions_socket,
            RpcClient::new_from_socket(rpc_addr),
        )
    }

    pub fn new_with_timeout(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
        timeout: Duration,
    ) -> Self {
        let rpc_client = RpcClient::new_with_timeout(rpc_addr, timeout);
        Self::new_from_client(rpc_addr, transactions_addr, transactions_socket, rpc_client)
    }

    fn new_from_client(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
        rpc_client: RpcClient,
    ) -> Self {
        ThinClient {
            rpc_client,
            rpc_addr,
            transactions_addr,
            transactions_socket,
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, transaction: &Transaction) -> io::Result<Signature> {
        let mut buf = vec![0; transaction.serialized_size().unwrap() as usize];
        let mut wr = std::io::Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &transaction)
            .expect("serialize Transaction in pub fn transfer_signed");
        assert!(buf.len() < PACKET_DATA_SIZE);
        self.transactions_socket
            .send_to(&buf[..], &self.transactions_addr)?;
        Ok(transaction.signatures[0])
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer(
        &mut self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
    ) -> io::Result<Signature> {
        for x in 0..tries {
            transaction.sign(&[keypair], self.get_recent_blockhash());
            let mut buf = vec![0; transaction.serialized_size().unwrap() as usize];
            let mut wr = std::io::Cursor::new(&mut buf[..]);
            serialize_into(&mut wr, &transaction)
                .expect("serialize Transaction in pub fn transfer_signed");
            self.transactions_socket
                .send_to(&buf[..], &self.transactions_addr)?;
            if self.poll_for_signature(&transaction.signatures[0]).is_ok() {
                return Ok(transaction.signatures[0]);
            }
            info!("{} tries failed transfer to {}", x, self.transactions_addr);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "retry_transfer failed",
        ))
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        to: &Pubkey,
        blockhash: &Hash,
    ) -> io::Result<Signature> {
        debug!(
            "transfer: lamports={} from={:?} to={:?} blockhash={:?}",
            lamports,
            keypair.pubkey(),
            to,
            blockhash
        );
        let now = Instant::now();
        let transaction = SystemTransaction::new_account(keypair, to, lamports, *blockhash, 0);
        let result = self.transfer_signed(&transaction);
        solana_metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("transfer".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                )
                .to_owned(),
        );
        result
    }

    pub fn get_account_data(&mut self, pubkey: &Pubkey) -> io::Result<Option<Vec<u8>>> {
        let params = json!([format!("{}", pubkey)]);
        let response =
            self.rpc_client
                .make_rpc_request(1, RpcRequest::GetAccountInfo, Some(params));
        match response {
            Ok(account_json) => {
                let account: Account =
                    serde_json::from_value(account_json).expect("deserialize account");
                Ok(Some(account.data))
            }
            Err(error) => {
                debug!("get_account_data failed: {:?}", error);
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "get_account_data failed",
                ))
            }
        }
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &Pubkey) -> io::Result<u64> {
        trace!("get_balance sending request to {}", self.rpc_addr);
        let params = json!([format!("{}", pubkey)]);
        let response =
            self.rpc_client
                .make_rpc_request(1, RpcRequest::GetAccountInfo, Some(params));

        response
            .and_then(|account_json| {
                let account: Account =
                    serde_json::from_value(account_json).expect("deserialize account");
                trace!("Response account {:?} {:?}", pubkey, account);
                trace!("get_balance {:?}", account.lamports);
                Ok(account.lamports)
            })
            .map_err(|error| {
                debug!("Response account {}: None (error: {:?})", pubkey, error);
                io::Error::new(io::ErrorKind::Other, "AccountNotFound")
            })
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will try again 5 times.
    pub fn transaction_count(&mut self) -> u64 {
        debug!("transaction_count");
        for _tries in 0..5 {
            let response =
                self.rpc_client
                    .make_rpc_request(1, RpcRequest::GetTransactionCount, None);

            match response {
                Ok(value) => {
                    debug!("transaction_count response: {:?}", value);
                    let transaction_count = value.as_u64().unwrap();
                    return transaction_count;
                }
                Err(error) => {
                    debug!("transaction_count failed: {:?}", error);
                }
            };
        }
        0
    }

    /// Request the last Entry ID from the server without blocking.
    /// Returns the blockhash Hash or None if there was no response from the server.
    pub fn try_get_recent_blockhash(&mut self, mut num_retries: u64) -> Option<Hash> {
        loop {
            trace!("try_get_recent_blockhash send_to {}", &self.rpc_addr);
            let response =
                self.rpc_client
                    .make_rpc_request(1, RpcRequest::GetRecentBlockhash, None);

            match response {
                Ok(value) => {
                    let blockhash_str = value.as_str().unwrap();
                    let blockhash_vec = bs58::decode(blockhash_str).into_vec().unwrap();
                    return Some(Hash::new(&blockhash_vec));
                }
                Err(error) => {
                    debug!("thin_client get_recent_blockhash error: {:?}", error);
                    num_retries -= 1;
                    if num_retries == 0 {
                        return None;
                    }
                }
            }
        }
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_recent_blockhash(&mut self) -> Hash {
        loop {
            trace!("get_recent_blockhash send_to {}", &self.rpc_addr);
            if let Some(hash) = self.try_get_recent_blockhash(10) {
                return hash;
            }
        }
    }

    /// Request a new last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_next_blockhash(&mut self, previous_blockhash: &Hash) -> Hash {
        self.get_next_blockhash_ext(previous_blockhash, &|| {
            sleep(Duration::from_millis(100));
        })
    }
    pub fn get_next_blockhash_ext(&mut self, previous_blockhash: &Hash, func: &Fn()) -> Hash {
        loop {
            let blockhash = self.get_recent_blockhash();
            if blockhash != *previous_blockhash {
                break blockhash;
            }
            debug!("Got same blockhash ({:?}), will retry...", blockhash);
            func()
        }
    }

    pub fn submit_poll_balance_metrics(elapsed: &Duration) {
        solana_metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("get_balance".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(elapsed) as i64),
                )
                .to_owned(),
        );
    }

    pub fn poll_balance_with_timeout(
        &mut self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<u64> {
        let now = Instant::now();
        loop {
            match self.get_balance(&pubkey) {
                Ok(bal) => {
                    ThinClient::submit_poll_balance_metrics(&now.elapsed());
                    return Ok(bal);
                }
                Err(e) => {
                    sleep(*polling_frequency);
                    if now.elapsed() > *timeout {
                        ThinClient::submit_poll_balance_metrics(&now.elapsed());
                        return Err(e);
                    }
                }
            };
        }
    }

    pub fn poll_get_balance(&mut self, pubkey: &Pubkey) -> io::Result<u64> {
        self.poll_balance_with_timeout(pubkey, &Duration::from_millis(100), &Duration::from_secs(1))
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature(&mut self, signature: &Signature) -> io::Result<()> {
        let now = Instant::now();
        while !self.check_signature(signature) {
            if now.elapsed().as_secs() > 15 {
                // TODO: Return a better error.
                return Err(io::Error::new(io::ErrorKind::Other, "signature not found"));
            }
            sleep(Duration::from_millis(250));
        }
        Ok(())
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn check_signature(&mut self, signature: &Signature) -> bool {
        trace!("check_signature: {:?}", signature);
        let params = json!([format!("{}", signature)]);
        let now = Instant::now();

        loop {
            let response = self.rpc_client.make_rpc_request(
                1,
                RpcRequest::ConfirmTransaction,
                Some(params.clone()),
            );

            match response {
                Ok(confirmation) => {
                    let signature_status = confirmation.as_bool().unwrap();
                    if signature_status {
                        trace!("Response found signature");
                    } else {
                        trace!("Response signature not found");
                    }
                    solana_metrics::submit(
                        influxdb::Point::new("thinclient")
                            .add_tag("op", influxdb::Value::String("check_signature".to_string()))
                            .add_field(
                                "duration_ms",
                                influxdb::Value::Integer(
                                    timing::duration_as_ms(&now.elapsed()) as i64
                                ),
                            )
                            .to_owned(),
                    );
                    return signature_status;
                }
                Err(err) => {
                    debug!("check_signature request failed: {:?}", err);
                }
            };
        }
    }
    pub fn fullnode_exit(&mut self) -> io::Result<bool> {
        trace!("fullnode_exit sending request to {}", self.rpc_addr);
        let response = self
            .rpc_client
            .make_rpc_request(1, RpcRequest::FullnodeExit, None)
            .map_err(|error| {
                debug!("Response from {} fullndoe_exit: {}", self.rpc_addr, error);
                io::Error::new(io::ErrorKind::Other, "FullodeExit request failure")
            })?;
        serde_json::from_value(response).map_err(|error| {
            debug!(
                "ParseError: from {} fullndoe_exit: {}",
                self.rpc_addr, error
            );
            io::Error::new(io::ErrorKind::Other, "FullodeExit parse failure")
        })
    }
}

impl Drop for ThinClient {
    fn drop(&mut self) {
        solana_metrics::flush();
    }
}

pub fn retry_get_balance(
    client: &mut ThinClient,
    bob_pubkey: &Pubkey,
    expected_balance: Option<u64>,
) -> Option<u64> {
    const LAST: usize = 30;
    for run in 0..LAST {
        let balance_result = client.poll_get_balance(bob_pubkey);
        if expected_balance.is_none() {
            return balance_result.ok();
        }
        trace!(
            "retry_get_balance[{}] {:?} {:?}",
            run,
            balance_result,
            expected_balance
        );
        if let (Some(expected_balance), Ok(balance_result)) = (expected_balance, balance_result) {
            if expected_balance == balance_result {
                return Some(balance_result);
            }
        }
    }
    None
}
