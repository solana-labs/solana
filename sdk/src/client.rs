//! Defines traits for blocking (synchronous) and non-blocking (asynchronous)
//! communication with a Solana server as well a a trait that encompasses both.
//!
//! //! Synchronous implementations are expected to create transactions, sign them, and send
//! them with multiple retries, updating blockhashes and resigning as-needed.
//!
//! Asynchronous implementations are expected to create transactions, sign them, and send
//! them but without waiting to see if the server accepted it.

#![cfg(feature = "full")]

use crate::{
    account::Account,
    clock::Slot,
    commitment_config::CommitmentConfig,
    epoch_info::EpochInfo,
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    hash::Hash,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
    signers::Signers,
    system_instruction,
    transaction::{self, Transaction, VersionedTransaction},
    transport::Result,
};

pub trait Client: SyncClient + AsyncClient {
    fn tpu_addr(&self) -> String;
}

pub trait SyncClient {
    /// Create a transaction from the given message, and send it to the
    /// server, retrying as-needed.
    fn send_and_confirm_message<T: Signers + ?Sized>(
        &self,
        keypairs: &T,
        message: Message,
    ) -> Result<Signature>;

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, retrying as-needed.
    fn send_and_confirm_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> Result<Signature>;

    /// Transfer lamports from `keypair` to `pubkey`, retrying until the
    /// transfer completes or produces and error.
    fn transfer_and_confirm(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> Result<Signature>;

    /// Get an account or None if not found.
    fn get_account_data(&self, pubkey: &Pubkey) -> Result<Option<Vec<u8>>>;

    /// Get an account or None if not found.
    fn get_account(&self, pubkey: &Pubkey) -> Result<Option<Account>>;

    /// Get an account or None if not found. Uses explicit commitment configuration.
    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<Option<Account>>;

    /// Get account balance or 0 if not found.
    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64>;

    /// Get account balance or 0 if not found. Uses explicit commitment configuration.
    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<u64>;

    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64>;

    /// Get recent blockhash
    #[deprecated(since = "1.9.0", note = "Please use `get_latest_blockhash` instead")]
    fn get_recent_blockhash(&self) -> Result<(Hash, FeeCalculator)>;

    /// Get recent blockhash. Uses explicit commitment configuration.
    #[deprecated(
        since = "1.9.0",
        note = "Please use `get_latest_blockhash_with_commitment` and `get_latest_blockhash_with_commitment` instead"
    )]
    fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<(Hash, FeeCalculator, Slot)>;

    /// Get `Some(FeeCalculator)` associated with `blockhash` if it is still in
    /// the BlockhashQueue`, otherwise `None`
    #[deprecated(
        since = "1.9.0",
        note = "Please use `get_fee_for_message` or `is_blockhash_valid` instead"
    )]
    fn get_fee_calculator_for_blockhash(&self, blockhash: &Hash) -> Result<Option<FeeCalculator>>;

    /// Get recent fee rate governor
    #[deprecated(
        since = "1.9.0",
        note = "Please do not use, will no longer be available in the future"
    )]
    fn get_fee_rate_governor(&self) -> Result<FeeRateGovernor>;

    /// Get signature status.
    fn get_signature_status(
        &self,
        signature: &Signature,
    ) -> Result<Option<transaction::Result<()>>>;

    /// Get signature status. Uses explicit commitment configuration.
    fn get_signature_status_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> Result<Option<transaction::Result<()>>>;

    /// Get last known slot
    fn get_slot(&self) -> Result<Slot>;

    /// Get last known slot. Uses explicit commitment configuration.
    fn get_slot_with_commitment(&self, commitment_config: CommitmentConfig) -> Result<u64>;

    /// Get transaction count
    fn get_transaction_count(&self) -> Result<u64>;

    /// Get transaction count. Uses explicit commitment configuration.
    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<u64>;

    fn get_epoch_info(&self) -> Result<EpochInfo>;

    /// Poll until the signature has been confirmed by at least `min_confirmed_blocks`
    fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> Result<usize>;

    /// Poll to confirm a transaction.
    fn poll_for_signature(&self, signature: &Signature) -> Result<()>;

    #[deprecated(
        since = "1.9.0",
        note = "Please do not use, will no longer be available in the future"
    )]
    fn get_new_blockhash(&self, blockhash: &Hash) -> Result<(Hash, FeeCalculator)>;

    /// Get last known blockhash
    fn get_latest_blockhash(&self) -> Result<Hash>;

    /// Get latest blockhash with last valid block height. Uses explicit commitment configuration.
    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<(Hash, u64)>;

    /// Check if the blockhash is valid
    fn is_blockhash_valid(&self, blockhash: &Hash, commitment: CommitmentConfig) -> Result<bool>;

    /// Calculate the fee for a `Message`
    fn get_fee_for_message(&self, message: &Message) -> Result<u64>;
}

pub trait AsyncClient {
    /// Send a signed transaction, but don't wait to see if the server accepted it.
    fn async_send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        self.async_send_versioned_transaction(transaction.into())
    }

    /// Send a batch of signed transactions without confirmation.
    fn async_send_batch(&self, transactions: Vec<Transaction>) -> Result<()> {
        let transactions = transactions.into_iter().map(Into::into).collect();
        self.async_send_versioned_transaction_batch(transactions)
    }

    /// Send a signed versioned transaction, but don't wait to see if the server accepted it.
    fn async_send_versioned_transaction(
        &self,
        transaction: VersionedTransaction,
    ) -> Result<Signature>;

    /// Send a batch of signed versioned transactions without confirmation.
    fn async_send_versioned_transaction_batch(
        &self,
        transactions: Vec<VersionedTransaction>,
    ) -> Result<()> {
        for t in transactions {
            self.async_send_versioned_transaction(t)?;
        }
        Ok(())
    }

    /// Create a transaction from the given message, and send it to the
    /// server, but don't wait for to see if the server accepted it.
    fn async_send_message<T: Signers + ?Sized>(
        &self,
        keypairs: &T,
        message: Message,
        recent_blockhash: Hash,
    ) -> Result<Signature> {
        let transaction = Transaction::new(keypairs, message, recent_blockhash);
        self.async_send_transaction(transaction)
    }

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, but don't wait for a reply.
    fn async_send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
        recent_blockhash: Hash,
    ) -> Result<Signature> {
        let message = Message::new(&[instruction], Some(&keypair.pubkey()));
        self.async_send_message(&[keypair], message, recent_blockhash)
    }

    /// Attempt to transfer lamports from `keypair` to `pubkey`, but don't wait to confirm.
    fn async_transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
        recent_blockhash: Hash,
    ) -> Result<Signature> {
        let transfer_instruction =
            system_instruction::transfer(&keypair.pubkey(), pubkey, lamports);
        self.async_send_instruction(keypair, transfer_instruction, recent_blockhash)
    }
}
