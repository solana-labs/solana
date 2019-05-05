use crate::accounts::AccountLockType;
use crate::bank::Bank;
use solana_sdk::transaction::{Result, Transaction};
use std::borrow::Borrow;

// Represents the results of trying to lock a set of accounts
pub struct LockedAccountsResults<'a, 'b, I: Borrow<Transaction>> {
    locked_accounts_results: Vec<Result<()>>,
    bank: &'a Bank,
    transactions: &'b [I],
    lock_type: AccountLockType,
    pub(crate) needs_unlock: bool,
}

impl<'a, 'b, I: Borrow<Transaction>> LockedAccountsResults<'a, 'b, I> {
    pub fn new(
        locked_accounts_results: Vec<Result<()>>,
        bank: &'a Bank,
        transactions: &'b [I],
        lock_type: AccountLockType,
    ) -> Self {
        Self {
            locked_accounts_results,
            bank,
            transactions,
            needs_unlock: true,
            lock_type,
        }
    }

    pub fn lock_type(&self) -> AccountLockType {
        self.lock_type.clone()
    }

    pub fn locked_accounts_results(&self) -> &Vec<Result<()>> {
        &self.locked_accounts_results
    }

    pub fn transactions(&self) -> &[I] {
        self.transactions
    }
}

// Unlock all locked accounts in destructor.
impl<'a, 'b, I: Borrow<Transaction>> Drop for LockedAccountsResults<'a, 'b, I> {
    fn drop(&mut self) {
        if self.needs_unlock {
            self.bank.unlock_accounts(self)
        }
    }
}
