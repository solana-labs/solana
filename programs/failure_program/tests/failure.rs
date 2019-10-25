use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_runtime::loader_utils::create_invoke_instruction;
use solana_sdk::client::SyncClient;
use solana_sdk::genesis_block::create_genesis_block;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::transaction::TransactionError;

#[test]
fn test_program_native_failure() {
    let (genesis_block, alice_keypair) = create_genesis_block(50);
    let program_id = Pubkey::new_rand();
    let bank = Bank::new(&genesis_block);
    bank.register_native_instruction_processor("solana_failure_program", &program_id);

    // Call user program
    let instruction = create_invoke_instruction(alice_keypair.pubkey(), program_id, &1u8);
    let bank_client = BankClient::new(bank);
    assert_eq!(
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::GenericError)
    );
}
