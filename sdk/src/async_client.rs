//! Defines a trait for non-blocking (asynchronous) communication with a Solana server.
//! Implementations are expected to create tranasctions, sign them, and send
//! them but without waiting to see if the server accepted it.

use crate::instruction::Instruction;
use crate::message::Message;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, Signature};
use crate::transaction::Transaction;
use std::io;

pub trait AsyncClient {
    /// Send a signed transaction, but don't wait to see if the server accepted it.
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature>;

    /// Create a transaction from the given message, and send it to the
    /// server, but don't wait for to see if the server accepted it.
    fn async_send_message(&self, keypairs: &[&Keypair], message: Message) -> io::Result<Signature>;

    /// Create a transaction from a single instruction that only requires
    /// a single signer. Then send it to the server, but don't wait for a reply.
    fn async_send_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction,
    ) -> io::Result<Signature>;

    /// Attempt to transfer lamports from `keypair` to `pubkey`, but don't wait to confirm.
    fn async_transfer(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey,
    ) -> io::Result<Signature>;
}
