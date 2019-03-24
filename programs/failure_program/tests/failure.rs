use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_runtime::loader_utils::{create_invoke_instruction, load_program};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::instruction::InstructionError;
use solana_sdk::native_loader;
use solana_sdk::transaction::TransactionError;

#[test]
fn test_program_native_failure() {
    let (genesis_block, mint_keypair) = GenesisBlock::new(50);
    let bank = Bank::new(&genesis_block);
    let alice_client = BankClient::new(&bank, mint_keypair);

    let program = "failure".as_bytes().to_vec();
    let program_id = load_program(&bank, &alice_client, &native_loader::id(), program);

    // Call user program
    let instruction = create_invoke_instruction(alice_client.pubkey(), program_id, &1u8);
    assert_eq!(
        alice_client.process_instruction(instruction),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::GenericError
        ))
    );
}
