//! Defines a trait for blocking (synchronous) communication with a Solana server.
//! Implementations are expected to create tranasctions, sign them, and send
//! them with multiple retries, updating blockhashes and resigning as-needed.

use crate::instruction::Instruction;
use crate::message::Message;
use crate::pubkey::Pubkey;
use crate::signature::Keypair;
use crate::transaction::TransactionError;

pub trait SyncClient {
    /// Create a transaction from the given message, and send it to the
    /// server, retrying as-needed.
    fn send_message(&self, keypairs: &[&Keypair], message: Message)
        -> Result<(), TransactionError>;

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, retrying as-needed.
    fn send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> Result<(), TransactionError>;

    /// Transfer lamports from `keypair` to `pubkey`, retrying until the
    /// transfer completes or produces and error.
    fn transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> Result<(), TransactionError>;
}
