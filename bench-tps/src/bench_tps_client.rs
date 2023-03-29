use {
    solana_rpc_client_api::client_error::Error as ClientError,
    solana_sdk::{
        account::Account, commitment_config::CommitmentConfig, epoch_info::EpochInfo, hash::Hash,
        message::Message, pubkey::Pubkey, signature::Signature, transaction::Transaction,
        transport::TransportError,
    },
    solana_tpu_client::tpu_client::TpuSenderError,
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum BenchTpsError {
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

pub(crate) type Result<T> = std::result::Result<T, BenchTpsError>;

pub trait BenchTpsClient {
    /// Send a signed transaction without confirmation
    fn send_transaction(&self, transaction: Transaction) -> Result<Signature>;

    /// Send a batch of signed transactions without confirmation.
    fn send_batch(&self, transactions: Vec<Transaction>) -> Result<()>;

    /// Get latest blockhash
    fn get_latest_blockhash(&self) -> Result<Hash>;

    /// Get latest blockhash and its last valid block height, using explicit commitment
    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<(Hash, u64)>;

    /// Get transaction count
    fn get_transaction_count(&self) -> Result<u64>;

    /// Get transaction count, using explicit commitment
    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<u64>;

    /// Get epoch info
    fn get_epoch_info(&self) -> Result<EpochInfo>;

    /// Get account balance
    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64>;

    /// Get account balance, using explicit commitment
    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<u64>;

    /// Calculate the fee for a `Message`
    fn get_fee_for_message(&self, message: &Message) -> Result<u64>;

    /// Get the rent-exempt minimum for an account
    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64>;

    /// Return the address of client
    fn addr(&self) -> String;

    /// Request, submit, and confirm an airdrop transaction
    fn request_airdrop_with_blockhash(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
        recent_blockhash: &Hash,
    ) -> Result<Signature>;

    /// Returns all information associated with the account of the provided pubkey
    fn get_account(&self, pubkey: &Pubkey) -> Result<Account>;

    /// Returns all information associated with the account of the provided pubkey, using explicit commitment
    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<Account>;

    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>>;
}

mod bank_client;
mod rpc_client;
mod thin_client;
mod tpu_client;
