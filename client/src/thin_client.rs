//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use crate::rpc_request::RpcClient;
use bincode::serialize_into;
use log::*;
use solana_metrics;
use solana_metrics::influxdb;
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
use std::time::{Duration, Instant};

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
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
            transactions_addr,
            transactions_socket,
            RpcClient::new_socket(rpc_addr),
        )
    }

    pub fn new_socket_with_timeout(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
        timeout: Duration,
    ) -> Self {
        let rpc_client = RpcClient::new_socket_with_timeout(rpc_addr, timeout);
        Self::new_from_client(transactions_addr, transactions_socket, rpc_client)
    }

    fn new_from_client(
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
        rpc_client: RpcClient,
    ) -> Self {
        Self {
            rpc_client,
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
        &self,
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

    pub fn get_account_data(&self, pubkey: &Pubkey) -> io::Result<Vec<u8>> {
        self.rpc_client.get_account_data(pubkey)
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.rpc_client.get_balance(pubkey)
    }

    pub fn transaction_count(&self) -> u64 {
        self.rpc_client.transaction_count()
    }

    pub fn try_get_recent_blockhash(&self, num_retries: u64) -> Option<Hash> {
        self.rpc_client.try_get_recent_blockhash(num_retries)
    }

    pub fn get_recent_blockhash(&self) -> Hash {
        self.rpc_client.get_recent_blockhash()
    }

    pub fn get_next_blockhash(&self, previous_blockhash: &Hash) -> Hash {
        self.rpc_client.get_next_blockhash(previous_blockhash)
    }

    pub fn poll_balance_with_timeout(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<u64> {
        self.rpc_client
            .poll_balance_with_timeout(pubkey, polling_frequency, timeout)
    }

    pub fn poll_get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.rpc_client.poll_get_balance(pubkey)
    }

    pub fn poll_for_signature(&self, signature: &Signature) -> io::Result<()> {
        self.rpc_client.poll_for_signature(signature)
    }

    pub fn check_signature(&self, signature: &Signature) -> bool {
        let now = Instant::now();
        let result = self.rpc_client.check_signature(signature);

        solana_metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("check_signature".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                )
                .to_owned(),
        );
        result
    }

    pub fn fullnode_exit(&self) -> io::Result<bool> {
        self.rpc_client.fullnode_exit()
    }
}

impl Drop for ThinClient {
    fn drop(&mut self) {
        solana_metrics::flush();
    }
}

pub fn create_client((rpc, tpu): (SocketAddr, SocketAddr), range: (u16, u16)) -> ThinClient {
    let (_, transactions_socket) = solana_netutil::bind_in_range(range).unwrap();
    ThinClient::new(rpc, tpu, transactions_socket)
}

pub fn create_client_with_timeout(
    (rpc, tpu): (SocketAddr, SocketAddr),
    range: (u16, u16),
    timeout: Duration,
) -> ThinClient {
    let (_, transactions_socket) = solana_netutil::bind_in_range(range).unwrap();
    ThinClient::new_socket_with_timeout(rpc, tpu, transactions_socket, timeout)
}

pub fn retry_get_balance(
    client: &ThinClient,
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
