use solana_sdk::hash::Hash;
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::signature::{generate_keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::system_program;
use solana_sdk::system_transaction;
use solana_sdk::transaction::Transaction;

pub fn test_tx() -> Transaction {
    let keypair1 = generate_keypair();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    system_transaction::transfer(&keypair1, &pubkey1, 42, zero)
}

pub fn test_multisig_tx() -> Transaction {
    let keypair0 = generate_keypair();
    let keypair1 = generate_keypair();
    let keypairs = vec![&keypair0, &keypair1];
    let lamports = 5;
    let blockhash = Hash::default();

    let transfer_instruction = SystemInstruction::Transfer { lamports };

    let program_ids = vec![system_program::id(), solana_budget_program::id()];

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
