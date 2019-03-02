//! The `loader_transaction` module provides functionality for loading and calling a program

use crate::hash::Hash;
use crate::loader_instruction::LoaderInstruction;
use crate::pubkey::Pubkey;
use crate::signature::Keypair;
use crate::transaction::Transaction;

pub struct LoaderTransaction {}

impl LoaderTransaction {
    pub fn new_write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bytes: Vec<u8>,
        recent_block_hash: Hash,
        fee: u64,
    ) -> Transaction {
        let instruction = LoaderInstruction::Write { offset, bytes };
        Transaction::new(from_keypair, &[], loader, &instruction, recent_block_hash, fee)
    }

    pub fn new_finalize(
        from_keypair: &Keypair,
        loader: Pubkey,
        recent_block_hash: Hash,
        fee: u64,
    ) -> Transaction {
        let instruction = LoaderInstruction::Finalize;
        Transaction::new(from_keypair, &[], loader, &instruction, recent_block_hash, fee)
    }
}
