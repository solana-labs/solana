use crate::bank::Bank;
use solana_sdk::transaction::{Result, Transaction};

// Represents the results of trying to lock a set of accounts
pub struct TransactionBatch<'a, 'b> {
    lock_results: Vec<Result<()>>,
    bank: &'a Bank,
    transactions: &'b [Transaction],
    iteration_order: Option<Vec<usize>>,
    pub(crate) needs_unlock: bool,
}

impl<'a, 'b> TransactionBatch<'a, 'b> {
    pub fn new(
        lock_results: Vec<Result<()>>,
        bank: &'a Bank,
        transactions: &'b [Transaction],
        iteration_order: Option<Vec<usize>>,
    ) -> Self {
        assert_eq!(lock_results.len(), transactions.len());
        if let Some(iteration_order) = &iteration_order {
            assert_eq!(transactions.len(), iteration_order.len());
        }
        Self {
            lock_results,
            bank,
            transactions,
            iteration_order,
            needs_unlock: true,
        }
    }

    pub fn lock_results(&self) -> &Vec<Result<()>> {
        &self.lock_results
    }

    pub fn transactions(&self) -> &[Transaction] {
        self.transactions
    }

    pub fn iteration_order(&self) -> Option<&[usize]> {
        self.iteration_order.as_ref().map(|v| v.as_slice())
    }
    pub fn bank(&self) -> &Bank {
        self.bank
    }
}

// Unlock all locked accounts in destructor.
impl<'a, 'b> Drop for TransactionBatch<'a, 'b> {
    fn drop(&mut self) {
        self.bank.unlock_accounts(self)
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
    fn test_transaction_batch() {
        let (bank, txs) = setup();

        // Test getting locked accounts
        let batch = bank.prepare_batch(&txs, None);

        // Grab locks
        assert!(batch.lock_results().iter().all(|x| x.is_ok()));

        // Trying to grab locks again should fail
        let batch2 = bank.prepare_batch(&txs, None);
        assert!(batch2.lock_results().iter().all(|x| x.is_err()));

        // Drop the first set of locks
        drop(batch);

        // Now grabbing locks should work again
        let batch2 = bank.prepare_batch(&txs, None);
        assert!(batch2.lock_results().iter().all(|x| x.is_ok()));
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
