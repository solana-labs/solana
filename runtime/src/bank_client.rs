use crate::bank::Bank;
use solana_sdk::signature::Keypair;
use solana_sdk::transaction::{Transaction, TransactionError};

pub struct BankClient<'a> {
    bank: &'a Bank,
    keypair: Keypair,
}

impl<'a> BankClient<'a> {
    pub fn new(bank: &'a Bank, keypair: Keypair) -> Self {
        Self { bank, keypair }
    }

    pub fn process_transaction(&self, tx: &mut Transaction) -> Result<(), TransactionError> {
        tx.sign(&[&self.keypair], self.bank.last_blockhash());
        self.bank.process_transaction(tx)
    }
}
