//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use {
    crate::{connection_cache::ConnectionCache, tpu_connection::TpuConnection},
    log::*,
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
    solana_thin_client::thin_client::temporary_pub::*,
    std::{
        io,
        net::SocketAddr,
        sync::Arc,
        time::{Duration, Instant},
    },
};

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    rpc_clients: Vec<RpcClient>,
    tpu_addrs: Vec<SocketAddr>,
    optimizer: ClientOptimizer,
    connection_cache: Arc<ConnectionCache>,
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
        Self::new_from_client(RpcClient::new_socket(rpc_addr), tpu_addr, connection_cache)
    }

    pub fn new_socket_with_timeout(
        rpc_addr: SocketAddr,
        tpu_addr: SocketAddr,
        timeout: Duration,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        let rpc_client = RpcClient::new_socket_with_timeout(rpc_addr, timeout);
        Self::new_from_client(rpc_client, tpu_addr, connection_cache)
    }

    fn new_from_client(
        rpc_client: RpcClient,
        tpu_addr: SocketAddr,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        Self {
            rpc_clients: vec![rpc_client],
            tpu_addrs: vec![tpu_addr],
            optimizer: ClientOptimizer::new(0),
            connection_cache,
        }
    }

    pub fn new_from_addrs(
        rpc_addrs: Vec<SocketAddr>,
        tpu_addrs: Vec<SocketAddr>,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        assert!(!rpc_addrs.is_empty());
        assert_eq!(rpc_addrs.len(), tpu_addrs.len());

        let rpc_clients: Vec<_> = rpc_addrs.into_iter().map(RpcClient::new_socket).collect();
        let optimizer = ClientOptimizer::new(rpc_clients.len());
        Self {
            rpc_clients,
            tpu_addrs,
            optimizer,
            connection_cache,
        }
    }

    fn tpu_addr(&self) -> &SocketAddr {
        &self.tpu_addrs[self.optimizer.best()]
    }

    pub fn rpc_client(&self) -> &RpcClient {
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

    pub fn send_and_confirm_transaction<T: Signers>(
        &self,
        keypairs: &T,
        transaction: &mut Transaction,
        tries: usize,
        pending_confirmations: usize,
    ) -> TransportResult<Signature> {
        for x in 0..tries {
            let now = Instant::now();
            let mut num_confirmed = 0;
            let mut wait_time = MAX_PROCESSING_AGE;
            // resend the same transaction until the transaction has no chance of succeeding
            let wire_transaction =
                bincode::serialize(&transaction).expect("transaction serialization failed");
            while now.elapsed().as_secs() < wait_time as u64 {
                if num_confirmed == 0 {
                    let conn = self.connection_cache.get_connection(self.tpu_addr());
                    // Send the transaction if there has been no confirmation (e.g. the first time)
                    #[allow(clippy::needless_borrow)]
                    conn.send_wire_transaction(&wire_transaction)?;
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
            let blockhash = self.get_latest_blockhash()?;
            transaction.sign(keypairs, blockhash);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("retry_transfer failed in {tries} retries"),
        )
        .into())
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

    pub fn get_program_accounts_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcProgramAccountsConfig,
    ) -> TransportResult<Vec<(Pubkey, Account)>> {
        self.rpc_client()
            .get_program_accounts_with_config(pubkey, config)
            .map_err(|e| e.into())
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
    fn send_and_confirm_message<T: Signers>(
        &self,
        keypairs: &T,
        message: Message,
    ) -> TransportResult<Signature> {
        let blockhash = self.get_latest_blockhash()?;
        let mut transaction = Transaction::new(keypairs, message, blockhash);
        let signature = self.send_and_confirm_transaction(keypairs, &mut transaction, 5, 0)?;
        Ok(signature)
    }

    fn send_and_confirm_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> TransportResult<Signature> {
        let message = Message::new(&[instruction], Some(&keypair.pubkey()));
        self.send_and_confirm_message(&[keypair], message)
    }

    fn transfer_and_confirm(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> TransportResult<Signature> {
        let transfer_instruction =
            system_instruction::transfer(&keypair.pubkey(), pubkey, lamports);
        self.send_and_confirm_instruction(keypair, transfer_instruction)
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

    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> TransportResult<u64> {
        self.rpc_client()
            .get_minimum_balance_for_rent_exemption(data_len)
            .map_err(|e| e.into())
    }

    fn get_recent_blockhash(&self) -> TransportResult<(Hash, FeeCalculator)> {
        #[allow(deprecated)]
        let (blockhash, fee_calculator, _last_valid_slot) =
            self.get_recent_blockhash_with_commitment(CommitmentConfig::default())?;
        Ok((blockhash, fee_calculator))
    }

    fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<(Hash, FeeCalculator, Slot)> {
        let index = self.optimizer.experiment();
        let now = Instant::now();
        #[allow(deprecated)]
        let recent_blockhash =
            self.rpc_clients[index].get_recent_blockhash_with_commitment(commitment_config);
        match recent_blockhash {
            Ok(Response { value, .. }) => {
                self.optimizer.report(index, duration_as_ms(&now.elapsed()));
                Ok((value.0, value.1, value.2))
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
        #[allow(deprecated)]
        self.rpc_client()
            .get_fee_calculator_for_blockhash(blockhash)
            .map_err(|e| e.into())
    }

    fn get_fee_rate_governor(&self) -> TransportResult<FeeRateGovernor> {
        #[allow(deprecated)]
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
            .get_signature_status(signature)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("send_transaction failed with error {err:?}"),
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
            .get_signature_status_with_commitment(signature, commitment_config)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("send_transaction failed with error {err:?}"),
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
                    format!("send_transaction failed with error {err:?}"),
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
        #[allow(deprecated)]
        self.rpc_client()
            .get_new_blockhash(blockhash)
            .map_err(|e| e.into())
    }

    fn get_latest_blockhash(&self) -> TransportResult<Hash> {
        let (blockhash, _) =
            self.get_latest_blockhash_with_commitment(CommitmentConfig::default())?;
        Ok(blockhash)
    }

    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<(Hash, u64)> {
        let index = self.optimizer.experiment();
        let now = Instant::now();
        match self.rpc_clients[index].get_latest_blockhash_with_commitment(commitment_config) {
            Ok((blockhash, last_valid_block_height)) => {
                self.optimizer.report(index, duration_as_ms(&now.elapsed()));
                Ok((blockhash, last_valid_block_height))
            }
            Err(e) => {
                self.optimizer.report(index, std::u64::MAX);
                Err(e.into())
            }
        }
    }

    fn is_blockhash_valid(
        &self,
        blockhash: &Hash,
        commitment_config: CommitmentConfig,
    ) -> TransportResult<bool> {
        self.rpc_client()
            .is_blockhash_valid(blockhash, commitment_config)
            .map_err(|e| e.into())
    }

    fn get_fee_for_message(&self, message: &Message) -> TransportResult<u64> {
        self.rpc_client()
            .get_fee_for_message(message)
            .map_err(|e| e.into())
    }
}

impl AsyncClient for ThinClient {
    fn async_send_versioned_transaction(
        &self,
        transaction: VersionedTransaction,
    ) -> TransportResult<Signature> {
        let conn = self.connection_cache.get_connection(self.tpu_addr());
        conn.serialize_and_send_transaction(&transaction)?;
        Ok(transaction.signatures[0])
    }

    fn async_send_versioned_transaction_batch(
        &self,
        batch: Vec<VersionedTransaction>,
    ) -> TransportResult<()> {
        let conn = self.connection_cache.get_connection(self.tpu_addr());
        conn.par_serialize_and_send_transaction_batch(&batch[..])?;
        Ok(())
    }
}
