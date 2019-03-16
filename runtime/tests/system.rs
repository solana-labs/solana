use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::native_program::ProgramError;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::system_program;
use solana_sdk::transaction::{Instruction, InstructionError, TransactionError};

#[test]
fn test_system_unsigned_transaction() {
    let (genesis_block, mint_keypair) = GenesisBlock::new(100);
    let bank = Bank::new(&genesis_block);

    let alice_client = BankClient::new(&bank, mint_keypair);
    let alice_pubkey = alice_client.pubkey();

    let mallory_client = BankClient::new(&bank, Keypair::new());
    let mallory_pubkey = mallory_client.pubkey();

    // Fund to account to bypass AccountNotFound error
    alice_client.transfer(50, &mallory_pubkey).unwrap();

    // Erroneously sign transaction with recipient account key
    // No signature case is tested by bank `test_zero_signatures()`
    let malicious_script = vec![Instruction::new(
        system_program::id(),
        &SystemInstruction::Move { lamports: 10 },
        vec![(alice_pubkey, false), (mallory_pubkey, true)],
    )];
    assert_eq!(
        mallory_client.process_script(malicious_script),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::ProgramError(ProgramError::MissingRequiredSignature)
        ))
    );
    assert_eq!(bank.get_balance(&alice_pubkey), 50);
    assert_eq!(bank.get_balance(&mallory_pubkey), 50);
}
