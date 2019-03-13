use solana_runtime::bank::Bank;
use solana_runtime::bank::BankError;
use solana_runtime::loader_utils::load_program;
use solana_runtime::runtime::InstructionError;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::native_loader;
use solana_sdk::native_program::ProgramError;
use solana_sdk::transaction::Transaction;

#[test]
fn test_program_native_failure() {
    let (genesis_block, mint_keypair) = GenesisBlock::new(50);
    let bank = Bank::new(&genesis_block);

    let program = "failure".as_bytes().to_vec();
    let program_id = load_program(&bank, &mint_keypair, &native_loader::id(), program);

    // Call user program
    let tx = Transaction::new(
        &mint_keypair,
        &[],
        &program_id,
        &1u8,
        bank.last_blockhash(),
        0,
    );
    assert_eq!(
        bank.process_transaction(&tx),
        Err(BankError::InstructionError(
            0,
            InstructionError::ProgramError(ProgramError::GenericError)
        ))
    );
}
