//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use crate::rpc_client::RpcClient;
use bincode::{serialize_into, serialized_size};
use log::*;
use solana_sdk::hash::Hash;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::transaction::Transaction;
use std::error;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

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
        let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
        let mut wr = std::io::Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &transaction)
            .expect("serialize Transaction in pub fn transfer_signed");
        assert!(buf.len() < PACKET_DATA_SIZE);
        self.transactions_socket
            .send_to(&buf[..], &self.transactions_addr)?;
        Ok(transaction.signatures[0])
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer_until_confirmed(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
        min_confirmed_blocks: usize,
    ) -> io::Result<Signature> {
        for x in 0..tries {
            transaction.sign(&[keypair], self.get_recent_blockhash()?);
            let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
            let mut wr = std::io::Cursor::new(&mut buf[..]);
            serialize_into(&mut wr, &transaction)
                .expect("serialize Transaction in pub fn transfer_signed");
            self.transactions_socket
                .send_to(&buf[..], &self.transactions_addr)?;
            if self
                .poll_for_signature_confirmation(&transaction.signatures[0], min_confirmed_blocks)
                .is_ok()
            {
                return Ok(transaction.signatures[0]);
            }
            info!("{} tries failed transfer to {}", x, self.transactions_addr);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "retry_transfer failed",
        ))
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
    ) -> io::Result<Signature> {
        for x in 0..tries {
            transaction.sign(&[keypair], self.get_recent_blockhash()?);
            let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
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

    pub fn get_account_data(&self, pubkey: &Pubkey) -> io::Result<Vec<u8>> {
        self.rpc_client.get_account_data(pubkey)
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.rpc_client.get_balance(pubkey)
    }

    pub fn get_transaction_count(&self) -> Result<u64, Box<dyn error::Error>> {
        self.rpc_client.get_transaction_count()
    }

    pub fn get_recent_blockhash(&self) -> io::Result<Hash> {
        self.rpc_client.get_recent_blockhash()
    }

    pub fn get_new_blockhash(&self, blockhash: &Hash) -> io::Result<Hash> {
        self.rpc_client.get_new_blockhash(blockhash)
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

    pub fn wait_for_balance(&self, pubkey: &Pubkey, expected_balance: Option<u64>) -> Option<u64> {
        self.rpc_client.wait_for_balance(pubkey, expected_balance)
    }

    pub fn poll_for_signature(&self, signature: &Signature) -> io::Result<()> {
        self.rpc_client.poll_for_signature(signature)
    }
    /// Poll the server until the signature has been confirmed by at least `min_confirmed_blocks`
    pub fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> io::Result<()> {
        self.rpc_client
            .poll_for_signature_confirmation(signature, min_confirmed_blocks)
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn check_signature(&self, signature: &Signature) -> bool {
        self.rpc_client.check_signature(signature)
    }

    pub fn fullnode_exit(&self) -> io::Result<bool> {
        self.rpc_client.fullnode_exit()
    }
    pub fn get_num_blocks_since_signature_confirmation(
        &mut self,
        sig: &Signature,
    ) -> io::Result<usize> {
        self.rpc_client
            .get_num_blocks_since_signature_confirmation(sig)
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
