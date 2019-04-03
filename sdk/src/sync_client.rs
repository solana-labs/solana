//! Defines a trait for blocking (synchronous) communication with a Solana server.
//! Implementations are expected to create tranasctions, sign them, and send
//! them with multiple retries, updating blockhashes and resigning as-needed.

use crate::instruction::Instruction;
use crate::message::Message;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, Signature};
use crate::transaction::TransactionError;

pub trait SyncClient {
    /// Create a transaction from the given message, and send it to the
    /// server, retrying as-needed.
    fn send_message(
        &self,
        keypairs: &[&Keypair],
        message: Message,
    ) -> Result<Signature, TransactionError>;

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, retrying as-needed.
    fn send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> Result<Signature, TransactionError>;

    /// Transfer lamports from `keypair` to `pubkey`, retrying until the
    /// transfer completes or produces and error.
    fn transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> Result<Signature, TransactionError>;

    /// Get an account or None if not found.
    fn get_account_data(&self, pubkey: &Pubkey) -> Option<Vec<u8>>;

    /// Get account balance or 0 if not found.
    fn get_balance(&self, pubkey: &Pubkey) -> u64;
}
