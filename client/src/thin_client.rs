//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use crate::{rpc_client::RpcClient, rpc_response::Response};
use bincode::{serialize_into, serialized_size};
use log::*;
use solana_sdk::{
    account::Account,
    client::{AsyncClient, Client, SyncClient},
    clock::MAX_PROCESSING_AGE,
    commitment_config::CommitmentConfig,
    epoch_info::EpochInfo,
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    hash::Hash,
    instruction::Instruction,
    message::Message,
    packet::PACKET_DATA_SIZE,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    signers::Signers,
    system_instruction,
    timing::duration_as_ms,
    transaction::{self, Transaction},
    transport::Result as TransportResult,
};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        RwLock,
    },
    time::{Duration, Instant},
};

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
    tpu_addrs: Vec<SocketAddr>,
    rpc_clients: Vec<RpcClient>,
    optimizer: ClientOptimizer,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with the Rpc at `rpc_addr` using TCP
    /// and the Tpu at `tpu_addr` over `transactions_socket` using UDP.
    pub fn new(rpc_addr: SocketAddr, tpu_addr: SocketAddr, transactions_socket: UdpSocket) -> Self {
        Self::new_from_client(
            tpu_addr,
            transactions_socket,
            RpcClient::new_socket(rpc_addr),
        )
    }

    pub fn new_socket_with_timeout(
        rpc_addr: SocketAddr,
        tpu_addr: SocketAddr,
        transactions_socket: UdpSocket,
        timeout: Duration,
    ) -> Self {
        let rpc_client = RpcClient::new_socket_with_timeout(rpc_addr, timeout);
        Self::new_from_client(tpu_addr, transactions_socket, rpc_client)
    }

    fn new_from_client(
        tpu_addr: SocketAddr,
        transactions_socket: UdpSocket,
        rpc_client: RpcClient,
    ) -> Self {
        Self {
            transactions_socket,
            tpu_addrs: vec![tpu_addr],
            rpc_clients: vec![rpc_client],
            optimizer: ClientOptimizer::new(0),
        }
    }

    pub fn new_from_addrs(
        rpc_addrs: Vec<SocketAddr>,
        tpu_addrs: Vec<SocketAddr>,
        transactions_socket: UdpSocket,
    ) -> Self {
        assert!(!rpc_addrs.is_empty());
        assert_eq!(rpc_addrs.len(), tpu_addrs.len());

        let rpc_clients: Vec<_> = rpc_addrs.into_iter().map(RpcClient::new_socket).collect();
        let optimizer = ClientOptimizer::new(rpc_clients.len());
        Self {
            tpu_addrs,
            transactions_socket,
            rpc_clients,
            optimizer,
        }
    }

    fn tpu_addr(&self) -> &SocketAddr {
        &self.tpu_addrs[self.optimizer.best()]
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
    ) -> TransportResult<Signature> {
        self.send_and_confirm_transaction(&[keypair], transaction, tries, min_confirmed_blocks)
    }

    /// Retry sending a signed Transaction with one signing Keypair to the server for processing.
    pub fn retry_transfer(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
    ) -> TransportResult<Signature> {
        self.send_and_confirm_transaction(&[keypair], transaction, tries, 0)
    }

    /// Retry sending a signed Transaction to the server for processing
    pub fn send_and_confirm_transaction<T: Signers>(
        &self,
        keypairs: &T,
        transaction: &mut Transaction,
        tries: usize,
        pending_confirmations: usize,
    ) -> TransportResult<Signature> {
        for x in 0..tries {
            let now = Instant::now();
            let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
            let mut wr = std::io::Cursor::new(&mut buf[..]);
            let mut num_confirmed = 0;
            let mut wait_time = MAX_PROCESSING_AGE;
            serialize_into(&mut wr, &transaction)
                .expect("serialize Transaction in pub fn transfer_signed");
            // resend the same transaction until the transaction has no chance of succeeding
            while now.elapsed().as_secs() < wait_time as u64 {
                if num_confirmed == 0 {
                    // Send the transaction if there has been no confirmation (e.g. the first time)
                    self.transactions_socket
                        .send_to(&buf[..], &self.tpu_addr())?;
                }

                if let Ok(confirmed_blocks) = self.poll_for_signature_confirmation(
                    &transaction.signatures[0],
                    pending_confirmations,
                ) {
                    num_confirmed = confirmed_blocks;
                    if confirmed_blocks >= pending_confirmations {
                        return Ok(transaction.signatures[0]);
                    }
                    // Since network has seen the transaction, wait longer to receive
                    // all pending confirmations. Resending the transaction could result into
                    // extra transaction fees
                    wait_time = wait_time.max(
                        MAX_PROCESSING_AGE * pending_confirmations.saturating_sub(num_confirmed),
                    );
                }
            }
            info!("{} tries failed transfer to {}", x, self.tpu_addr());
            let (blockhash, _fee_calculator) = self.get_recent_blockhash()?;
            transaction.sign(keypairs, blockhash);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("retry_transfer failed in {} retries", tries),
        )
        .into())
    }

    pub fn poll_balance_with_timeout_and_commitment(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.rpc_client()
            .poll_balance_with_timeout_and_commitment(
                pubkey,
                polling_frequency,
                timeout,
                commitment_config,
            )
            .map_err(|e| e.into())
    }

    pub fn poll_balance_with_timeout(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> TransportResult<u64> {
        self.poll_balance_with_timeout_and_commitment(
            pubkey,
            polling_frequency,
            timeout,
            CommitmentConfig::default(),
        )
    }

    pub fn poll_get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64> {
        self.poll_get_balance_with_commitment(pubkey, CommitmentConfig::default())
    }

    pub fn poll_get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.rpc_client()
            .poll_get_balance_with_commitment(pubkey, commitment_config)
            .map_err(|e| e.into())
    }

    pub fn wait_for_balance(&self, pubkey: &Pubkey, expected_balance: Option<u64>) -> Option<u64> {
        self.rpc_client().wait_for_balance_with_commitment(
            pubkey,
            expected_balance,
            CommitmentConfig::default(),
        )
    }

    pub fn wait_for_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        expected_balance: Option<u64>,
        commitment_config: CommitmentConfig,
    ) -> Option<u64> {
        self.rpc_client().wait_for_balance_with_commitment(
            pubkey,
            expected_balance,
            commitment_config,
        )
    }

    pub fn poll_for_signature_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<()> {
        self.rpc_client()
            .poll_for_signature_with_commitment(signature, commitment_config)
            .map_err(|e| e.into())
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn check_signature(&self, signature: &Signature) -> bool {
        self.rpc_client().check_signature(signature)
    }

    pub fn validator_exit(&self) -> TransportResult<bool> {
        self.rpc_client().validator_exit().map_err(|e| e.into())
    }

    pub fn get_num_blocks_since_signature_confirmation(
        &mut self,
        sig: &Signature,
    ) -> TransportResult<usize> {
        self.rpc_client()
            .get_num_blocks_since_signature_confirmation(sig)
            .map_err(|e| e.into())
    }
}

impl Client for ThinClient {
    fn tpu_addr(&self) -> String {
        self.tpu_addr().to_string()
    }
}

impl SyncClient for ThinClient {
    fn send_message<T: Signers>(
        &self,
        keypairs: &T,
        message: Message,
    ) -> TransportResult<Signature> {
        let (blockhash, _fee_calculator) = self.get_recent_blockhash()?;
        let mut transaction = Transaction::new(keypairs, message, blockhash);
        let signature = self.send_and_confirm_transaction(keypairs, &mut transaction, 5, 0)?;
        Ok(signature)
    }

    fn send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> TransportResult<Signature> {
        let message = Message::new(&[instruction]);
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

    fn get_account(&self, pubkey: &Pubkey) -> TransportResult<Option<Account>> {
        let account = self.rpc_client().get_account(pubkey);
        match account {
            Ok(value) => Ok(Some(value)),
            Err(_) => Ok(None),
        }
    }

    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<Option<Account>> {
        self.rpc_client()
            .get_account_with_commitment(pubkey, commitment_config)
            .map_err(|e| e.into())
            .map(|r| r.value)
    }

    fn get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64> {
        self.rpc_client().get_balance(pubkey).map_err(|e| e.into())
    }

    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.rpc_client()
            .get_balance_with_commitment(pubkey, commitment_config)
            .map_err(|e| e.into())
            .map(|r| r.value)
    }

    fn get_recent_blockhash(&self) -> TransportResult<(Hash, FeeCalculator)> {
        self.get_recent_blockhash_with_commitment(CommitmentConfig::default())
    }

    fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<(Hash, FeeCalculator)> {
        let index = self.optimizer.experiment();
        let now = Instant::now();
        let recent_blockhash =
            self.rpc_clients[index].get_recent_blockhash_with_commitment(commitment_config);
        match recent_blockhash {
            Ok(Response { value, .. }) => {
                self.optimizer.report(index, duration_as_ms(&now.elapsed()));
                Ok((value.0, value.1))
            }
            Err(e) => {
                self.optimizer.report(index, std::u64::MAX);
                Err(e.into())
            }
        }
    }

    fn get_fee_calculator_for_blockhash(
        &self,
        blockhash: &Hash,
    ) -> TransportResult<Option<FeeCalculator>> {
        self.rpc_client()
            .get_fee_calculator_for_blockhash(blockhash)
            .map_err(|e| e.into())
    }

    fn get_fee_rate_governor(&self) -> TransportResult<FeeRateGovernor> {
        self.rpc_client()
            .get_fee_rate_governor()
            .map_err(|e| e.into())
            .map(|r| r.value)
    }

    fn get_signature_status(
        &self,
        signature: &Signature,
    ) -> TransportResult<Option<transaction::Result<()>>> {
        let status = self
            .rpc_client()
            .get_signature_status(&signature)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("send_transaction failed with error {:?}", err),
                )
            })?;
        Ok(status)
    }

    fn get_signature_status_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<Option<transaction::Result<()>>> {
        let status = self
            .rpc_client()
            .get_signature_status_with_commitment(&signature, commitment_config)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("send_transaction failed with error {:?}", err),
                )
            })?;
        Ok(status)
    }

    fn get_slot(&self) -> TransportResult<u64> {
        self.get_slot_with_commitment(CommitmentConfig::default())
    }

    fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        let slot = self
            .rpc_client()
            .get_slot_with_commitment(commitment_config)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("send_transaction failed with error {:?}", err),
                )
            })?;
        Ok(slot)
    }

    fn get_epoch_info(&self) -> TransportResult<EpochInfo> {
        self.rpc_client().get_epoch_info().map_err(|e| e.into())
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
                Err(e.into())
            }
        }
    }

    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        let index = self.optimizer.experiment();
        let now = Instant::now();
        match self
            .rpc_client()
            .get_transaction_count_with_commitment(commitment_config)
        {
            Ok(transaction_count) => {
                self.optimizer.report(index, duration_as_ms(&now.elapsed()));
                Ok(transaction_count)
            }
            Err(e) => {
                self.optimizer.report(index, std::u64::MAX);
                Err(e.into())
            }
        }
    }

    /// Poll the server until the signature has been confirmed by at least `min_confirmed_blocks`
    fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> TransportResult<usize> {
        self.rpc_client()
            .poll_for_signature_confirmation(signature, min_confirmed_blocks)
            .map_err(|e| e.into())
    }

    fn poll_for_signature(&self, signature: &Signature) -> TransportResult<()> {
        self.rpc_client()
            .poll_for_signature(signature)
            .map_err(|e| e.into())
    }

    fn get_new_blockhash(&self, blockhash: &Hash) -> TransportResult<(Hash, FeeCalculator)> {
        self.rpc_client()
            .get_new_blockhash(blockhash)
            .map_err(|e| e.into())
    }
}

impl AsyncClient for ThinClient {
    fn async_send_transaction(&self, transaction: Transaction) -> TransportResult<Signature> {
        let mut buf = vec![0; serialized_size(&transaction).unwrap() as usize];
        let mut wr = std::io::Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &transaction)
            .expect("serialize Transaction in pub fn transfer_signed");
        assert!(buf.len() < PACKET_DATA_SIZE);
        self.transactions_socket
            .send_to(&buf[..], &self.tpu_addr())?;
        Ok(transaction.signatures[0])
    }
    fn async_send_message<T: Signers>(
        &self,
        keypairs: &T,
        message: Message,
        recent_blockhash: Hash,
    ) -> TransportResult<Signature> {
        let transaction = Transaction::new(keypairs, message, recent_blockhash);
        self.async_send_transaction(transaction)
    }
    fn async_send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
        recent_blockhash: Hash,
    ) -> TransportResult<Signature> {
        let message = Message::new(&[instruction]);
        self.async_send_message(&[keypair], message, recent_blockhash)
    }
    fn async_transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
        recent_blockhash: Hash,
    ) -> TransportResult<Signature> {
        let transfer_instruction =
            system_instruction::transfer(&keypair.pubkey(), pubkey, lamports);
        self.async_send_instruction(keypair, transfer_instruction, recent_blockhash)
    }
}

pub fn create_client((rpc, tpu): (SocketAddr, SocketAddr), range: (u16, u16)) -> ThinClient {
    let (_, transactions_socket) =
        solana_net_utils::bind_in_range(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), range).unwrap();
    ThinClient::new(rpc, tpu, transactions_socket)
}

pub fn create_client_with_timeout(
    (rpc, tpu): (SocketAddr, SocketAddr),
    range: (u16, u16),
    timeout: Duration,
) -> ThinClient {
    let (_, transactions_socket) =
        solana_net_utils::bind_in_range(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), range).unwrap();
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
