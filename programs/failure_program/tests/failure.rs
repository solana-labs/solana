use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_runtime::loader_utils::run_program;
use solana_sdk::genesis_block::create_genesis_block;
use solana_sdk::instruction::AccountMeta;
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
    let bank_client = BankClient::new(bank);

    // Call user program
    let account_metas = vec![AccountMeta::new(alice_keypair.pubkey(), true)];
    assert_eq!(
        run_program(
            &bank_client,
            &alice_keypair,
            &program_id,
            account_metas,
            &1u8,
        )
        .unwrap_err()
        .unwrap(),
        TransactionError::InstructionError(0, InstructionError::GenericError)
    );
}
