//! The `dynamic_transaction` module provides functionality for loading and calling a program

use bincode::serialize;
use solana_program_interface::loader_instruction::LoaderInstruction;
// use dynamic_program::DynamicProgram;
use hash::Hash;
use signature::{Keypair, KeypairUtil};
use solana_program_interface::pubkey::Pubkey;
use transaction::Transaction;

pub trait LoaderTransaction {
    fn write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bits: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn finalize(from_keypair: &Keypair, loader: Pubkey, last_id: Hash, fee: i64) -> Self;
}

impl LoaderTransaction for Transaction {
    fn write(
        from_keypair: &Keypair,
        loader: Pubkey,
        offset: u32,
        bits: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        trace!(
            "LoaderTransaction::Write() program {:?} offset {} length {}",
            from_keypair.pubkey(),
            offset,
            bits.len()
        );
        let inst = LoaderInstruction::Write { offset, bits };
        let userdata = serialize(&inst).unwrap();
        Transaction::new(from_keypair, &[], loader, userdata, last_id, fee)
    }

    fn finalize(from_keypair: &Keypair, loader: Pubkey, last_id: Hash, fee: i64) -> Self {
        trace!(
            "LoaderTransaction::Finalize() program {:?}",
            from_keypair.pubkey(),
        );
        let inst = LoaderInstruction::Finalize;
        let userdata = serialize(&inst).unwrap();
        Transaction::new(from_keypair, &[], loader, userdata, last_id, fee)
    }
}
