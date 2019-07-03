//! Defines traits for blocking (synchronous) and non-blocking (asynchronous)
//! communication with a Solana server as well a a trait that encompasses both.
//!
//! //! Synchronous implementations are expected to create transactions, sign them, and send
//! them with multiple retries, updating blockhashes and resigning as-needed.
//!
//! Asynchronous implementations are expected to create transactions, sign them, and send
//! them but without waiting to see if the server accepted it.

use crate::account::Account;
use crate::fee_calculator::FeeCalculator;
use crate::hash::Hash;
use crate::instruction::Instruction;
use crate::message::Message;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, Signature};
use crate::transaction;
use crate::transport::Result;
use std::io;

pub trait Client: SyncClient + AsyncClient {
    fn transactions_addr(&self) -> String;
}

pub trait SyncClient {
    /// Create a transaction from the given message, and send it to the
    /// server, retrying as-needed.
    fn send_message(&self, keypairs: &[&Keypair], message: Message) -> Result<Signature>;

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, retrying as-needed.
    fn send_instruction(&self, keypair: &Keypair, instruction: Instruction) -> Result<Signature>;

    /// Transfer lamports from `keypair` to `pubkey`, retrying until the
    /// transfer completes or produces and error.
    fn transfer(&self, lamports: u64, keypair: &Keypair, pubkey: &Pubkey) -> Result<Signature>;

    /// Get an account or None if not found.
    fn get_account_data(&self, pubkey: &Pubkey) -> Result<Option<Vec<u8>>>;

    /// Get an account or None if not found.
    fn get_account(&self, pubkey: &Pubkey) -> Result<Option<Account>>;

    /// Get account balance or 0 if not found.
    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64>;

    /// Get recent blockhash
    fn get_recent_blockhash(&self) -> Result<(Hash, FeeCalculator)>;

    /// Get signature status.
    fn get_signature_status(
        &self,
        signature: &Signature,
    ) -> Result<Option<transaction::Result<()>>>;

    /// Get last known slot
    fn get_slot(&self) -> Result<u64>;

    /// Get transaction count
    fn get_transaction_count(&self) -> Result<u64>;

    /// Poll until the signature has been confirmed by at least `min_confirmed_blocks`
    fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> Result<usize>;

    /// Poll to confirm a transaction.
    fn poll_for_signature(&self, signature: &Signature) -> Result<()>;

    fn get_new_blockhash(&self, blockhash: &Hash) -> Result<(Hash, FeeCalculator)>;
}

pub trait AsyncClient {
    /// Send a signed transaction, but don't wait to see if the server accepted it.
    fn async_send_transaction(
        &self,
        transaction: transaction::Transaction,
    ) -> io::Result<Signature>;

    /// Create a transaction from the given message, and send it to the
    /// server, but don't wait for to see if the server accepted it.
    fn async_send_message(
        &self,
        keypairs: &[&Keypair],
        message: Message,
        recent_blockhash: Hash,
    ) -> io::Result<Signature>;

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, but don't wait for a reply.
    fn async_send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
        recent_blockhash: Hash,
    ) -> io::Result<Signature>;

    /// Attempt to transfer lamports from `keypair` to `pubkey`, but don't wait to confirm.
    fn async_transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
        recent_blockhash: Hash,
    ) -> io::Result<Signature>;
}
