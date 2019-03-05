use solana_runtime::bank::{Bank, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_sdk::system_transaction::SystemTransaction;

struct SystemBank<'a> {
    bank: &'a Bank,
}

impl<'a> SystemBank<'a> {
    fn new(bank: &'a Bank) -> Self {
        bank.add_native_program("solana_system_program", &system_program::id());
        Self { bank }
    }
    fn create_account(&self, from_keypair: &Keypair, to: Pubkey, lamports: u64) -> Result<()> {
        let blockhash = self.bank.last_blockhash();
        let tx = SystemTransaction::new_account(from_keypair, to, lamports, blockhash, 0);
        self.bank.process_transaction(&tx)
    }
    fn move_lamports(&self, from_keypair: &Keypair, to: Pubkey, lamports: u64) -> Result<()> {
        let blockhash = self.bank.last_blockhash();
        let tx = SystemTransaction::new_move(from_keypair, to, lamports, blockhash, 0);
        self.bank.process_transaction(&tx)
    }
}

#[test]
fn test_create_cannot_overwrite_used_account() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let system_bank = SystemBank::new(&bank);

    // create_account on uninitialized account should work
    let system_account = Keypair::new().pubkey();
    system_bank
        .create_account(&from_keypair, system_account, 100)
        .unwrap();
    assert_eq!(system_bank.bank.get_balance(&system_account), 100);

    // Create an account assigned to another program
    let other_account = Keypair::new().pubkey();
    let program_id = Pubkey::new(&[9; 32]);
    let tx = SystemTransaction::new_program_account(
        &from_keypair,
        other_account,
        system_bank.bank.last_blockhash(),
        1,
        0,
        program_id,
        0,
    );
    system_bank.bank.process_transaction(&tx).unwrap();
    assert_eq!(system_bank.bank.get_balance(&other_account), 1);
    assert_eq!(
        system_bank.bank.get_account(&other_account).unwrap().owner,
        program_id
    );

    // create_account on an initialized account should fail
    assert!(system_bank
        .create_account(&from_keypair, other_account, 100)
        .is_err());
    assert_eq!(system_bank.bank.get_balance(&other_account), 1);
    assert_eq!(
        system_bank.bank.get_account(&other_account).unwrap().owner,
        program_id
    );
}
#[test]
fn test_move_can_fund_used_account() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let system_bank = SystemBank::new(&bank);

    // Create an account assigned to another program
    let other_account = Keypair::new().pubkey();
    let program_id = Pubkey::new(&[9; 32]);
    let tx = SystemTransaction::new_program_account(
        &from_keypair,
        other_account,
        system_bank.bank.last_blockhash(),
        1,
        0,
        program_id,
        0,
    );
    system_bank.bank.process_transaction(&tx).unwrap();
    assert_eq!(system_bank.bank.get_balance(&other_account), 1);
    assert_eq!(
        system_bank.bank.get_account(&other_account).unwrap().owner,
        program_id
    );

    system_bank
        .move_lamports(&from_keypair, other_account, 100)
        .unwrap();
    assert_eq!(system_bank.bank.get_balance(&other_account), 101);
    assert_eq!(
        system_bank.bank.get_account(&other_account).unwrap().owner,
        program_id
    );
}
