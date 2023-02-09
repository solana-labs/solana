//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use {
    crate::connection_cache::ConnectionCache,
    log::*,
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_connection_cache::client_connection::ClientConnection,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{config::RpcProgramAccountsConfig, response::Response},
    solana_sdk::{
        account::Account,
        client::{AsyncClient, Client, SyncClient},
        clock::{Slot, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        epoch_info::EpochInfo,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        hash::Hash,
        instruction::Instruction,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
        signers::Signers,
        system_instruction,
        timing::duration_as_ms,
        transaction::{self, Transaction, VersionedTransaction},
        transport::Result as TransportResult,
    },
    solana_thin_client::thin_client::{temporary_pub::*, ThinClient as BackendThinClient},
    std::{
        io,
        net::SocketAddr,
        sync::Arc,
        time::{Duration, Instant},
    },
};

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    thin_client: BackendThinClient,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with the Rpc at `rpc_addr` using TCP
    /// and the Tpu at `tpu_addr` over `transactions_socket` using Quic or UDP
    /// (currently hardcoded to UDP)
    pub fn new(
        rpc_addr: SocketAddr,
        tpu_addr: SocketAddr,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        let connection_cache = Arc::new(connection_cache.into());
        Self {
            thin_client: BackendThinClient::new(rpc_addr, tpu_addr, connection_cache)
        }
    }

    pub fn new_socket_with_timeout(
        rpc_addr: SocketAddr,
        tpu_addr: SocketAddr,
        timeout: Duration,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        let connection_cache = Arc::new(connection_cache.into());
        Self {
            thin_client: BackendThinClient::new_socket_with_timeout(rpc_addr, tpu_addr, timeout, connection_cache)
        }
    }

    pub fn new_from_addrs(
        rpc_addrs: Vec<SocketAddr>,
        tpu_addrs: Vec<SocketAddr>,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        let connection_cache = Arc::new(connection_cache.into());
        Self {
            thin_client: BackendThinClient::new_from_addrs(rpc_addrs, tpu_addrs, connection_cache)
        }
    }

    pub fn rpc_client(&self) -> &RpcClient {
        self.thin_client.rpc_client()
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer_until_confirmed(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
        min_confirmed_blocks: usize,
    ) -> TransportResult<Signature> {
        self.thin_client.retry_transfer_until_confirmed(keypair, transaction, tries, min_confirmed_blocks)
    }

    /// Retry sending a signed Transaction with one signing Keypair to the server for processing.
    pub fn retry_transfer(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
    ) -> TransportResult<Signature> {
        self.thin_client.retry_transfer(keypair, transaction, tries)
    }

    pub fn send_and_confirm_transaction<T: Signers>(
        &self,
        keypairs: &T,
        transaction: &mut Transaction,
        tries: usize,
        pending_confirmations: usize,
    ) -> TransportResult<Signature> {
        self.thin_client.send_and_confirm_transaction(keypairs, transaction, tries, pending_confirmations)
    }

    pub fn poll_get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64> {
        self.thin_client.poll_get_balance(pubkey)
    }

    pub fn poll_get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.thin_client.poll_get_balance_with_commitment(pubkey, commitment_config)
    }

    pub fn wait_for_balance(&self, pubkey: &Pubkey, expected_balance: Option<u64>) -> Option<u64> {
        self.thin_client.wait_for_balance(pubkey, expected_balance)
    }

    pub fn get_program_accounts_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcProgramAccountsConfig,
    ) -> TransportResult<Vec<(Pubkey, Account)>> {
        self.thin_client.get_program_accounts_with_config(pubkey, config)
    }

    pub fn wait_for_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        expected_balance: Option<u64>,
        commitment_config: CommitmentConfig,
    ) -> Option<u64> {
        self.thin_client.wait_for_balance_with_commitment(pubkey, expected_balance, commitment_config)
    }

    pub fn poll_for_signature_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<()> {
        self.thin_client.poll_for_signature_with_commitment(signature, commitment_config)
    }

    pub fn get_num_blocks_since_signature_confirmation(
        &mut self,
        sig: &Signature,
    ) -> TransportResult<usize> {
        self.thin_client.get_num_blocks_since_signature_confirmation(sig)
    }
}

impl Client for ThinClient {
    fn tpu_addr(&self) -> String {
        self.thin_client.tpu_addr()
    }
}

impl SyncClient for ThinClient {
    fn send_and_confirm_message<T: Signers>(
        &self,
        keypairs: &T,
        message: Message,
    ) -> TransportResult<Signature> {
        self.thin_client.send_and_confirm_message(keypairs, message)
    }

    fn send_and_confirm_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> TransportResult<Signature> {
        self.thin_client.send_and_confirm_instruction(keypair, instruction)
    }

    fn transfer_and_confirm(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> TransportResult<Signature> {
        self.thin_client.transfer_and_confirm(lamports, keypair, pubkey)
    }

    fn get_account_data(&self, pubkey: &Pubkey) -> TransportResult<Option<Vec<u8>>> {
        self.thin_client.get_account_data(pubkey)
    }

    fn get_account(&self, pubkey: &Pubkey) -> TransportResult<Option<Account>> {
        self.thin_client.get_account(pubkey)
    }

    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<Option<Account>> {
        self.thin_client.get_account_with_commitment(pubkey, commitment_config)
    }

    fn get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64> {
        self.thin_client.get_balance(pubkey)
    }

    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.thin_client.get_balance_with_commitment(pubkey, commitment_config)
    }

    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> TransportResult<u64> {
        self.thin_client.get_minimum_balance_for_rent_exemption(data_len)
    }

    fn get_recent_blockhash(&self) -> TransportResult<(Hash, FeeCalculator)> {
        #[allow(deprecated)]
        self.thin_client.get_recent_blockhash()
    }

    fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<(Hash, FeeCalculator, Slot)> {
        #[allow(deprecated)]
        self.thin_client.get_recent_blockhash_with_commitment(commitment_config)
    }

    fn get_fee_calculator_for_blockhash(
        &self,
        blockhash: &Hash,
    ) -> TransportResult<Option<FeeCalculator>> {
        #[allow(deprecated)]
        self.thin_client.get_fee_calculator_for_blockhash(blockhash)
    }

    fn get_fee_rate_governor(&self) -> TransportResult<FeeRateGovernor> {
        #[allow(deprecated)]
        self.thin_client.get_fee_rate_governor()
    }

    fn get_signature_status(
        &self,
        signature: &Signature,
    ) -> TransportResult<Option<transaction::Result<()>>> {
        self.thin_client.get_signature_status(signature)
    }

    fn get_signature_status_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<Option<transaction::Result<()>>> {
        self.thin_client.get_signature_status_with_commitment(signature, commitment_config)
    }

    fn get_slot(&self) -> TransportResult<u64> {
        self.thin_client.get_slot()
    }

    fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.thin_client.get_slot_with_commitment(commitment_config)
    }

    fn get_epoch_info(&self) -> TransportResult<EpochInfo> {
        self.thin_client.get_epoch_info()
    }

    fn get_transaction_count(&self) -> TransportResult<u64> {
        self.thin_client.get_transaction_count()
    }

    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<u64> {
        self.thin_client.get_transaction_count_with_commitment(commitment_config)
    }

    /// Poll the server until the signature has been confirmed by at least `min_confirmed_blocks`
    fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> TransportResult<usize> {
        self.thin_client.poll_for_signature_confirmation(signature, min_confirmed_blocks)
    }

    fn poll_for_signature(&self, signature: &Signature) -> TransportResult<()> {
        self.thin_client.poll_for_signature(signature)
    }

    fn get_new_blockhash(&self, blockhash: &Hash) -> TransportResult<(Hash, FeeCalculator)> {
        #[allow(deprecated)]
        self.thin_client.get_new_blockhash(blockhash)
    }

    fn get_latest_blockhash(&self) -> TransportResult<Hash> {
        self.thin_client.get_latest_blockhash()
    }

    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<(Hash, u64)> {
        self.thin_client.get_latest_blockhash_with_commitment(commitment_config)
    }

    fn is_blockhash_valid(
        &self,
        blockhash: &Hash,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<bool> {
        self.thin_client.is_blockhash_valid(blockhash, commitment_config)
    }

    fn get_fee_for_message(&self, message: &Message) -> TransportResult<u64> {
        self.thin_client.get_fee_for_message(message)
    }
}

impl AsyncClient for ThinClient {
    fn async_send_versioned_transaction(
        &self,
        transaction: VersionedTransaction,
    ) -> TransportResult<Signature> {
        self.thin_client.async_send_versioned_transaction(transaction)
    }

    fn async_send_versioned_transaction_batch(
        &self,
        batch: Vec<VersionedTransaction>,
    ) -> TransportResult<()> {
        self.thin_client.async_send_versioned_transaction_batch(batch)
    }
}
