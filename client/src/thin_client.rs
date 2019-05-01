//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use crate::rpc_client::RpcClient;
use bincode::{serialize_into, serialized_size};
use log::*;
use solana_sdk::client::{AsyncClient, Client, SyncClient};
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::Message;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_instruction;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::transaction::{self, Transaction};
use solana_sdk::transport::Result as TransportResult;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

struct ClientOptimizer {
    cur_index: AtomicUsize,
    experiment_index: AtomicUsize,
    experiment_done: AtomicBool,
    times: RwLock<Vec<u64>>,
    num_clients: usize,
}

fn min_index(array: &[u64]) -> (u64, usize) {
    let mut min_time = std::u64::MAX;
    let mut min_index = 0;
    for (i, time) in array.iter().enumerate() {
        if *time < min_time {
            min_time = *time;
            min_index = i;
        }
    }
    (min_time, min_index)
}

impl ClientOptimizer {
    fn new(num_clients: usize) -> Self {
        Self {
            cur_index: AtomicUsize::new(0),
            experiment_index: AtomicUsize::new(0),
            experiment_done: AtomicBool::new(false),
            times: RwLock::new(vec![std::u64::MAX; num_clients]),
            num_clients,
        }
    }

    fn experiment(&self) -> usize {
        if self.experiment_index.load(Ordering::Relaxed) < self.num_clients {
            let old = self.experiment_index.fetch_add(1, Ordering::Relaxed);
            if old < self.num_clients {
                old
            } else {
                self.best()
            }
        } else {
            self.best()
        }
    }

    fn report(&self, index: usize, time_ms: u64) {
        if self.num_clients > 1
            && (!self.experiment_done.load(Ordering::Relaxed) || time_ms == std::u64::MAX)
        {
            trace!(
                "report {} with {} exp: {}",
                index,
                time_ms,
                self.experiment_index.load(Ordering::Relaxed)
            );

            self.times.write().unwrap()[index] = time_ms;

            if index == (self.num_clients - 1) || time_ms == std::u64::MAX {
                let times = self.times.read().unwrap();
                let (min_time, min_index) = min_index(&times);
                trace!(
                    "done experimenting min: {} time: {} times: {:?}",
                    min_index,
                    min_time,
                    times
                );

                // Only 1 thread should grab the num_clients-1 index, so this should be ok.
                self.cur_index.store(min_index, Ordering::Relaxed);
                self.experiment_done.store(true, Ordering::Relaxed);
            }
        }
    }

    fn best(&self) -> usize {
        self.cur_index.load(Ordering::Relaxed)
    }
}

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    transactions_socket: UdpSocket,
    transactions_addrs: Vec<SocketAddr>,
    rpc_clients: Vec<RpcClient>,
    optimizer: ClientOptimizer,
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
            transactions_socket,
            transactions_addrs: vec![transactions_addr],
            rpc_clients: vec![rpc_client],
            optimizer: ClientOptimizer::new(0),
        }
    }

    pub fn new_from_addrs(
        transactions_addrs: Vec<SocketAddr>,
        transactions_socket: UdpSocket,
        rpc_sockets: Vec<SocketAddr>,
    ) -> Self {
        assert!(!transactions_addrs.is_empty());
        assert!(!rpc_sockets.is_empty());
        assert_eq!(rpc_sockets.len(), transactions_addrs.len());
        let rpc_len = rpc_sockets.len();
        let rpc_clients: Vec<_> = rpc_sockets.into_iter().map(RpcClient::new_socket).collect();
        Self {
            transactions_addrs,
            transactions_socket,
            rpc_clients,
            optimizer: ClientOptimizer::new(rpc_len),
        }
    }

    fn transactions_addr(&self) -> &SocketAddr {
        &self.transactions_addrs[self.optimizer.best()]
    }

    fn rpc_client(&self) -> &RpcClient {
        &self.rpc_clients[self.optimizer.best()]
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer_until_confirmed(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
        min_confirmed_blocks: usize,
    ) -> io::Result<Signature> {
        self.send_and_confirm_transaction(&[keypair], transaction, tries, min_confirmed_blocks)
    }

    /// Retry sending a signed Transaction with one signing Keypair to the server for processing.
    pub fn retry_transfer(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
    ) -> io::Result<Signature> {
        self.send_and_confirm_transaction(&[keypair], transaction, tries, 0)
    }

    /// Retry sending a signed Transaction to the server for processing
    pub fn send_and_confirm_transaction(
        &self,
        keypairs: &[&Keypair],
        transaction: &mut Transaction,
        tries: usize,
        min_confirmed_blocks: usize,
    ) -> io::Result<Signature> {
        for x in 0..tries {
            let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
            let mut wr = std::io::Cursor::new(&mut buf[..]);
            serialize_into(&mut wr, &transaction)
                .expect("serialize Transaction in pub fn transfer_signed");
            self.transactions_socket
                .send_to(&buf[..], &self.transactions_addr())?;
            if self
                .poll_for_signature_confirmation(&transaction.signatures[0], min_confirmed_blocks)
                .is_ok()
            {
                return Ok(transaction.signatures[0]);
            }
            info!(
                "{} tries failed transfer to {}",
                x,
                self.transactions_addr()
            );
            let (blockhash, _fee_calculator) = self.rpc_client().get_recent_blockhash()?;
            transaction.sign(keypairs, blockhash);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("retry_transfer failed in {} retries", tries),
        ))
    }

    pub fn poll_balance_with_timeout(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<u64> {
        self.rpc_client()
            .poll_balance_with_timeout(pubkey, polling_frequency, timeout)
    }

    pub fn poll_get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.rpc_client().poll_get_balance(pubkey)
    }

    pub fn wait_for_balance(&self, pubkey: &Pubkey, expected_balance: Option<u64>) -> Option<u64> {
        self.rpc_client().wait_for_balance(pubkey, expected_balance)
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn check_signature(&self, signature: &Signature) -> bool {
        self.rpc_client().check_signature(signature)
    }

    pub fn fullnode_exit(&self) -> io::Result<bool> {
        self.rpc_client().fullnode_exit()
    }

    pub fn get_num_blocks_since_signature_confirmation(
        &mut self,
        sig: &Signature,
    ) -> io::Result<usize> {
        self.rpc_client()
            .get_num_blocks_since_signature_confirmation(sig)
    }
}

impl Client for ThinClient {
    fn transactions_addr(&self) -> String {
        self.transactions_addr().to_string()
    }
}

impl SyncClient for ThinClient {
    fn send_message(&self, keypairs: &[&Keypair], message: Message) -> TransportResult<Signature> {
        let (blockhash, _fee_calculator) = self.get_recent_blockhash()?;
        let mut transaction = Transaction::new(&keypairs, message, blockhash);
        let signature = self.send_and_confirm_transaction(keypairs, &mut transaction, 5, 0)?;
        Ok(signature)
    }

    fn send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> TransportResult<Signature> {
        let message = Message::new(vec![instruction]);
        self.send_message(&[keypair], message)
    }

    fn transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> TransportResult<Signature> {
        let transfer_instruction =
            system_instruction::transfer(&keypair.pubkey(), pubkey, lamports);
        self.send_instruction(keypair, transfer_instruction)
    }

    fn get_account_data(&self, pubkey: &Pubkey) -> TransportResult<Option<Vec<u8>>> {
        Ok(self.rpc_client().get_account_data(pubkey).ok())
    }

    fn get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64> {
        let balance = self.rpc_client().get_balance(pubkey)?;
        Ok(balance)
    }

    fn get_signature_status(
        &self,
        signature: &Signature,
    ) -> TransportResult<Option<transaction::Result<()>>> {
        let status = self
            .rpc_client()
            .get_signature_status(&signature.to_string())
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("send_transaction failed with error {:?}", err),
                )
            })?;
        Ok(status)
    }

    fn get_recent_blockhash(&self) -> TransportResult<(Hash, FeeCalculator)> {
        let index = self.optimizer.experiment();
        let now = Instant::now();
        let recent_blockhash = self.rpc_clients[index].get_recent_blockhash();
        match recent_blockhash {
            Ok(recent_blockhash) => {
                self.optimizer.report(index, duration_as_ms(&now.elapsed()));
                Ok(recent_blockhash)
            }
            Err(e) => {
                self.optimizer.report(index, std::u64::MAX);
                Err(e)?
            }
        }
    }

    fn get_transaction_count(&self) -> TransportResult<u64> {
        let index = self.optimizer.experiment();
        let now = Instant::now();
        match self.rpc_client().get_transaction_count() {
            Ok(transaction_count) => {
                self.optimizer.report(index, duration_as_ms(&now.elapsed()));
                Ok(transaction_count)
            }
            Err(e) => {
                self.optimizer.report(index, std::u64::MAX);
                Err(e)?
            }
        }
    }

    /// Poll the server until the signature has been confirmed by at least `min_confirmed_blocks`
    fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> TransportResult<()> {
        Ok(self
            .rpc_client()
            .poll_for_signature_confirmation(signature, min_confirmed_blocks)?)
    }

    fn poll_for_signature(&self, signature: &Signature) -> TransportResult<()> {
        Ok(self.rpc_client().poll_for_signature(signature)?)
    }

    fn get_new_blockhash(&self, blockhash: &Hash) -> TransportResult<(Hash, FeeCalculator)> {
        let new_blockhash = self.rpc_client().get_new_blockhash(blockhash)?;
        Ok(new_blockhash)
    }
}

impl AsyncClient for ThinClient {
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature> {
        let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
        let mut wr = std::io::Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &transaction)
            .expect("serialize Transaction in pub fn transfer_signed");
        assert!(buf.len() < PACKET_DATA_SIZE);
        self.transactions_socket
            .send_to(&buf[..], &self.transactions_addr())?;
        Ok(transaction.signatures[0])
    }
    fn async_send_message(
        &self,
        keypairs: &[&Keypair],
        message: Message,
        recent_blockhash: Hash,
    ) -> io::Result<Signature> {
        let transaction = Transaction::new(&keypairs, message, recent_blockhash);
        self.async_send_transaction(transaction)
    }
    fn async_send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
        recent_blockhash: Hash,
    ) -> io::Result<Signature> {
        let message = Message::new(vec![instruction]);
        self.async_send_message(&[keypair], message, recent_blockhash)
    }
    fn async_transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
        recent_blockhash: Hash,
    ) -> io::Result<Signature> {
        let transfer_instruction =
            system_instruction::transfer(&keypair.pubkey(), pubkey, lamports);
        self.async_send_instruction(keypair, transfer_instruction, recent_blockhash)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;

    #[test]
    fn test_client_optimizer() {
        solana_logger::setup();

        const NUM_CLIENTS: usize = 5;
        let optimizer = ClientOptimizer::new(NUM_CLIENTS);
        (0..NUM_CLIENTS).into_par_iter().for_each(|_| {
            let index = optimizer.experiment();
            optimizer.report(index, (NUM_CLIENTS - index) as u64);
        });

        let index = optimizer.experiment();
        optimizer.report(index, 50);
        assert_eq!(optimizer.best(), NUM_CLIENTS - 1);

        optimizer.report(optimizer.best(), std::u64::MAX);
        assert_eq!(optimizer.best(), NUM_CLIENTS - 2);
    }
}
