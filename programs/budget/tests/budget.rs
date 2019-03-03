use solana_budget_api::budget_transaction::BudgetTransaction;
use solana_runtime::bank::{Bank, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};

struct BudgetBank<'a> {
    bank: &'a Bank,
}

impl<'a> BudgetBank<'a> {
    fn new(bank: &'a Bank) -> Self {
        bank.add_native_program("solana_budget_program", &solana_budget_api::id());
        Self { bank }
    }

    fn pay(&self, from_keypair: &Keypair, to_id: Pubkey, lamports: u64) -> Result<()> {
        let blockhash = self.bank.last_blockhash();
        let tx = BudgetTransaction::new_payment(from_keypair, to_id, lamports, blockhash, 0);
        self.bank.process_transaction(&tx)
    }
}

#[test]
fn test_budget_payment_via_bank() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let budget_bank = BudgetBank::new(&bank);
    let to_id = Keypair::new().pubkey();
    budget_bank.pay(&from_keypair, to_id, 100).unwrap();
    assert_eq!(bank.get_balance(&to_id), 100);
}
