use crate::bank::Bank;
use solana_sdk::transaction::{Result, Transaction};

// Represents the results of trying to lock a set of accounts
pub struct LockedAccountsResults<'a, 'b> {
    locked_accounts_results: Vec<Result<()>>,
    bank: &'a Bank,
    transactions: &'b [Transaction],
    pub(crate) needs_unlock: bool,
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
            needs_unlock: true,
        }
    }

    pub fn locked_accounts_results(&self) -> &Vec<Result<()>> {
        &self.locked_accounts_results
    }

    pub fn transactions(&self) -> &[Transaction] {
        self.transactions
    }
}

// Unlock all locked accounts in destructor.
impl<'a, 'b> Drop for LockedAccountsResults<'a, 'b> {
    fn drop(&mut self) {
        if self.needs_unlock {
            self.bank.unlock_accounts(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block_with_leader, GenesisBlockInfo};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;

    #[test]
    fn test_account_locks() {
        let (bank, txs) = setup();

        // Test getting locked accounts
        let lock_results = bank.lock_accounts(&txs, None);

        // Grab locks
        assert!(lock_results
            .locked_accounts_results()
            .iter()
            .all(|x| x.is_ok()));

        // Trying to grab locks again should fail
        let lock_results2 = bank.lock_accounts(&txs, None);
        assert!(lock_results2
            .locked_accounts_results()
            .iter()
            .all(|x| x.is_err()));

        // Drop the first set of locks
        drop(lock_results);

        // Now grabbing locks should work again
        let lock_results2 = bank.lock_accounts(&txs, None);
        assert!(lock_results2
            .locked_accounts_results()
            .iter()
            .all(|x| x.is_ok()));
    }

    fn setup() -> (Bank, Vec<Transaction>) {
        let dummy_leader_pubkey = Pubkey::new_rand();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block_with_leader(500, &dummy_leader_pubkey, 100);
        let bank = Bank::new(&genesis_block);

        let pubkey = Pubkey::new_rand();
        let keypair2 = Keypair::new();
        let pubkey2 = Pubkey::new_rand();

        let txs = vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&keypair2, &pubkey2, 1, genesis_block.hash()),
        ];

        (bank, txs)
    }
}
