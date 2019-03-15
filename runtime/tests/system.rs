use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::native_program::ProgramError;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::system_program;
use solana_sdk::transaction::{Instruction, InstructionError, Transaction, TransactionError};

#[test]
fn test_system_unsigned_transaction() {
    let (genesis_block, from_keypair) = GenesisBlock::new(100);
    let bank = Bank::new(&genesis_block);
    let from_pubkey = from_keypair.pubkey();
    let alice_client = BankClient::new(&bank, from_keypair);

    let to_keypair = Keypair::new();
    let to_pubkey = to_keypair.pubkey();
    let mallory_client = BankClient::new(&bank, to_keypair);

    // Fund to account to bypass AccountNotFound error
    let ix = SystemInstruction::new_move(&from_pubkey, &to_pubkey, 50);
    let mut tx = Transaction::new(vec![ix]);
    alice_client.process_transaction(&mut tx).unwrap();

    // Erroneously sign transaction with recipient account key
    // No signature case is tested by bank `test_zero_signatures()`
    let ix = Instruction::new(
        system_program::id(),
        &SystemInstruction::Move { lamports: 10 },
        vec![(from_pubkey, false), (to_pubkey, true)],
    );
    let mut tx = Transaction::new(vec![ix]);
    assert_eq!(
        mallory_client.process_transaction(&mut tx),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::ProgramError(ProgramError::MissingRequiredSignature)
        ))
    );
    assert_eq!(bank.get_balance(&from_pubkey), 50);
    assert_eq!(bank.get_balance(&to_pubkey), 50);
}
