use solana_runtime::bank::{Bank, BankError};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::native_program::ProgramError;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::system_program;
use solana_sdk::transaction_builder::{BuilderInstruction, TransactionBuilder};

struct SystemBank<'a> {
    bank: &'a Bank,
}

impl<'a> SystemBank<'a> {
    fn new(bank: &'a Bank) -> Self {
        bank.add_native_program("solana_system_program", &system_program::id());
        Self { bank }
    }
}
#[test]
fn test_system_unsigned_transaction() {
    let (genesis_block, from_keypair) = GenesisBlock::new(100);
    let bank = Bank::new(&genesis_block);
    let system_bank = SystemBank::new(&bank);

    // Fund to account to bypass AccountNotFound error
    let to_keypair = Keypair::new();
    let blockhash = system_bank.bank.last_blockhash();
    let tx = TransactionBuilder::default()
        .push(SystemInstruction::new_move(
            &from_keypair.pubkey(),
            &to_keypair.pubkey(),
            50,
        ))
        .sign(&[&from_keypair], blockhash);
    system_bank.bank.process_transaction(&tx).unwrap();

    // Erroneously sign transaction with recipient account key
    // No signature case is tested by bank `test_zero_signatures()`
    let blockhash = system_bank.bank.last_blockhash();
    let tx = TransactionBuilder::default()
        .push(BuilderInstruction::new(
            system_program::id(),
            &SystemInstruction::Move { lamports: 10 },
            vec![(from_keypair.pubkey(), false), (to_keypair.pubkey(), true)],
        ))
        .sign(&[&to_keypair], blockhash);
    assert_eq!(
        system_bank.bank.process_transaction(&tx),
        Err(BankError::ProgramError(0, ProgramError::InvalidArgument))
    );
    assert_eq!(system_bank.bank.get_balance(&from_keypair.pubkey()), 50);
    assert_eq!(system_bank.bank.get_balance(&to_keypair.pubkey()), 50);
}
