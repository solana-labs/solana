use crate::bank::{Bank, Result};
use solana_sdk::transaction::Transaction;

// Represents the results of trying to lock a set of accounts
pub struct LockedAccountsResults<'a, 'b> {
    locked_accounts_results: Vec<Result<()>>,
    bank: &'a Bank,
    transactions: &'b [Transaction],
}

impl<'a, 'b> LockedAccountsResults<'a, 'b> {
    pub fn new(
        locked_accounts_results: Vec<Result<()>>,
        bank: &'a Bank,
        transactions: &'b [Transaction],
    ) -> Self {
        Self {
            locked_accounts_results,
            bank,
            transactions,
        }
    }

    pub fn locked_accounts_results(&self) -> &Vec<Result<()>> {
        &self.locked_accounts_results
    }
}

// Unlock all locked accounts in destructor.
impl<'a, 'b> Drop for LockedAccountsResults<'a, 'b> {
    fn drop(&mut self) {
        self.bank
            .unlock_accounts(self.transactions, &self.locked_accounts_results[..])
    }
}
