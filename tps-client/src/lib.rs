use {
    log::debug,
    solana_rpc_client_api::{client_error::Error as ClientError, config::RpcBlockConfig},
    solana_sdk::{
        account::Account,
        clock::DEFAULT_MS_PER_SLOT,
        commitment_config::CommitmentConfig,
        epoch_info::EpochInfo,
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::Signature,
        slot_history::Slot,
        transaction::{Result, Transaction},
        transport::TransportError,
    },
    solana_tpu_client::tpu_client::TpuSenderError,
    solana_transaction_status::UiConfirmedBlock,
    std::{
        thread::sleep,
        time::{Duration, Instant},
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum TpsClientError {
    #[error("Airdrop failure")]
    AirdropFailure,
    #[error("IO error: {0:?}")]
    IoError(#[from] std::io::Error),
    #[error("Client error: {0:?}")]
    ClientError(#[from] ClientError),
    #[error("TpuClient error: {0:?}")]
    TpuSenderError(#[from] TpuSenderError),
    #[error("Transport error: {0:?}")]
    TransportError(#[from] TransportError),
    #[error("Custom error: {0}")]
    Custom(String),
}

pub type TpsClientResult<T> = std::result::Result<T, TpsClientError>;

pub trait TpsClient {
    /// Send a signed transaction without confirmation
    fn send_transaction(&self, transaction: Transaction) -> TpsClientResult<Signature>;

    /// Send a batch of signed transactions without confirmation.
    fn send_batch(&self, transactions: Vec<Transaction>) -> TpsClientResult<()>;

    /// Get latest blockhash
    fn get_latest_blockhash(&self) -> TpsClientResult<Hash>;

    /// Get latest blockhash and its last valid block height, using explicit commitment
    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TpsClientResult<(Hash, u64)>;

    fn get_new_latest_blockhash(&self, blockhash: &Hash) -> TpsClientResult<Hash> {
        let start = Instant::now();
        while start.elapsed().as_secs() < 5 {
            if let Ok(new_blockhash) = self.get_latest_blockhash() {
                if new_blockhash != *blockhash {
                    return Ok(new_blockhash);
                }
            }
            debug!("Got same blockhash ({:?}), will retry...", blockhash);

            // Retry ~twice during a slot
            sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT / 2));
        }
        Err(TpsClientError::Custom("Timeout".to_string()))
    }

    fn get_signature_status(&self, signature: &Signature) -> TpsClientResult<Option<Result<()>>>;

    /// Get transaction count
    fn get_transaction_count(&self) -> TpsClientResult<u64>;

    /// Get transaction count, using explicit commitment
    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TpsClientResult<u64>;

    /// Get epoch info
    fn get_epoch_info(&self) -> TpsClientResult<EpochInfo>;

    /// Get account balance
    fn get_balance(&self, pubkey: &Pubkey) -> TpsClientResult<u64>;

    /// Get account balance, using explicit commitment
    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TpsClientResult<u64>;

    /// Calculate the fee for a `Message`
    fn get_fee_for_message(&self, message: &Message) -> TpsClientResult<u64>;

    /// Get the rent-exempt minimum for an account
    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> TpsClientResult<u64>;

    /// Return the address of client
    fn addr(&self) -> String;

    /// Request, submit, and confirm an airdrop transaction
    fn request_airdrop_with_blockhash(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
        recent_blockhash: &Hash,
    ) -> TpsClientResult<Signature>;

    /// Returns all information associated with the account of the provided pubkey
    fn get_account(&self, pubkey: &Pubkey) -> TpsClientResult<Account>;

    /// Returns all information associated with the account of the provided pubkey, using explicit commitment
    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> TpsClientResult<Account>;

    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> TpsClientResult<Vec<Option<Account>>>;

    fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> TpsClientResult<Slot>;

    fn get_blocks_with_commitment(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
        commitment_config: CommitmentConfig,
    ) -> TpsClientResult<Vec<Slot>>;

    fn get_block_with_config(
        &self,
        slot: Slot,
        rpc_block_config: RpcBlockConfig,
    ) -> TpsClientResult<UiConfirmedBlock>;
}

mod bank_client;
mod rpc_client;
mod tpu_client;
pub mod utils;
