//! The `dynamic_transaction` module provides functionality for loading and calling a program

use hash::Hash;
use signature::{Keypair, KeypairUtil};
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use transaction::Transaction;

pub trait LoaderTransaction {
    fn write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bytes: Vec<u8>,
        last_id: Hash,
        fee: u64,
    ) -> Self;

    fn finalize(from_keypair: &Keypair, loader: Pubkey, last_id: Hash, fee: u64) -> Self;
}

impl LoaderTransaction for Transaction {
    fn write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bytes: Vec<u8>,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        trace!(
            "LoaderTransaction::Write() program {:?} offset {} length {}",
            from_keypair.pubkey(),
            offset,
            bytes.len()
        );
        let instruction = LoaderInstruction::Write { offset, bytes };
        Transaction::new(from_keypair, &[], loader, &instruction, last_id, fee)
    }

    fn finalize(from_keypair: &Keypair, loader: Pubkey, last_id: Hash, fee: u64) -> Self {
        trace!(
            "LoaderTransaction::Finalize() program {:?}",
            from_keypair.pubkey(),
        );
        let instruction = LoaderInstruction::Finalize;
        Transaction::new(from_keypair, &[], loader, &instruction, last_id, fee)
    }
}
