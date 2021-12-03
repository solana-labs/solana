use {
    solana_sdk::{
        hash::Hash,
        instruction::CompiledInstruction,
        signature::{Keypair, Signer},
        stake,
        system_instruction::SystemInstruction,
        system_program, system_transaction,
        transaction::Transaction,
    },
    solana_vote_program::vote_transaction,
};

pub fn test_tx() -> Transaction {
    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    system_transaction::transfer(&keypair1, &pubkey1, 42, zero)
}

pub fn test_multisig_tx() -> Transaction {
    let keypair0 = Keypair::new();
    let keypair1 = Keypair::new();
    let keypairs = vec![&keypair0, &keypair1];
    let lamports = 5;
    let blockhash = Hash::default();

    let transfer_instruction = SystemInstruction::Transfer { lamports };

    let program_ids = vec![system_program::id(), stake::program::id()];

    let instructions = vec![CompiledInstruction::new(
        0,
        &transfer_instruction,
        vec![0, 1],
    )];

    Transaction::new_with_compiled_instructions(
        &keypairs,
        &[],
        blockhash,
        program_ids,
        instructions,
    )
}

pub fn vote_tx() -> Transaction {
    let keypair = Keypair::new();
    vote_transaction::new_vote_transaction(
        vec![2],
        Hash::default(),
        Hash::default(),
        &keypair,
        &keypair,
        &keypair,
        None,
    )
}
