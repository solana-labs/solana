use solana_runtime::bank::Bank;
use solana_runtime::loader_utils::load_program;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::native_loader;
use solana_sdk::transaction::{InstructionError, Transaction, TransactionError};

#[test]
fn test_program_native_failure() {
    let (genesis_block, mint_keypair) = GenesisBlock::new(50);
    let bank = Bank::new(&genesis_block);

    let program = "failure".as_bytes().to_vec();
    let program_id = load_program(&bank, &mint_keypair, &native_loader::id(), program);

    // Call user program
    let tx = Transaction::new_signed(
        &mint_keypair,
        &[],
        &program_id,
        &1u8,
        bank.last_blockhash(),
        0,
    );
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::GenericError
        ))
    );
}
