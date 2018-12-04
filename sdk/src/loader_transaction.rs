//! The `loader_transaction` module provides functionality for loading and calling a program

use hash::Hash;
use loader_instruction::LoaderInstruction;
use pubkey::Pubkey;
use signature::Keypair;
use transaction::Transaction;

pub trait LoaderTransaction {
    fn loader_write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bytes: Vec<u8>,
        last_id: Hash,
        fee: u64,
    ) -> Self;

    fn loader_finalize(from_keypair: &Keypair, loader: Pubkey, last_id: Hash, fee: u64) -> Self;
}

impl LoaderTransaction for Transaction {
    fn loader_write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bytes: Vec<u8>,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        let instruction = LoaderInstruction::Write { offset, bytes };
        Transaction::new(from_keypair, &[], loader, &instruction, last_id, fee)
    }

    fn loader_finalize(from_keypair: &Keypair, loader: Pubkey, last_id: Hash, fee: u64) -> Self {
        let instruction = LoaderInstruction::Finalize;
        Transaction::new(from_keypair, &[], loader, &instruction, last_id, fee)
    }
}
