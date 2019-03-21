//! The `loader_transaction` module provides functionality for loading and calling a program

use crate::hash::Hash;
use crate::loader_instruction::LoaderInstruction;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::transaction::Transaction;

pub struct LoaderTransaction {}

impl LoaderTransaction {
    pub fn new_write(
        from_keypair: &Keypair,
        loader: &Pubkey,
        offset: u32,
        bytes: Vec<u8>,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let write_instruction =
            LoaderInstruction::new_write(&from_keypair.pubkey(), loader, offset, bytes);
        let instructions = vec![write_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, fee)
    }

    pub fn new_finalize(
        from_keypair: &Keypair,
        loader: &Pubkey,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let finalize_instruction = LoaderInstruction::new_finalize(&from_keypair.pubkey(), loader);
        let instructions = vec![finalize_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, fee)
    }
}
