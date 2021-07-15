use crate::bank::Bank;
use solana_sdk::{
    sanitized_transaction::SanitizedTransaction,
    transaction::{Result, Transaction},
};
use std::borrow::Cow;
use std::ops::Deref;

// Represents the results of trying to lock a set of accounts
pub struct TransactionBatch<'a, 'b> {
    lock_results: Vec<Result<()>>,
    bank: &'a Bank,
    sanitized_txs: Cow<'b, [SanitizedTransaction<'b>]>,
    pub(crate) needs_unlock: bool,
}

impl<'a, 'b> TransactionBatch<'a, 'b> {
    pub fn new(
        lock_results: Vec<Result<()>>,
        bank: &'a Bank,
        sanitized_txs: Cow<'b, [SanitizedTransaction<'b>]>,
    ) -> Self {
        assert_eq!(lock_results.len(), sanitized_txs.len());
        Self {
            lock_results,
            bank,
            sanitized_txs,
            needs_unlock: true,
        }
    }

    pub fn lock_results(&self) -> &Vec<Result<()>> {
        &self.lock_results
    }

    pub fn sanitized_transactions(&self) -> &[SanitizedTransaction] {
        &self.sanitized_txs
    }

    pub fn transactions_iter(&self) -> impl Iterator<Item = &Transaction> {
        self.sanitized_txs.iter().map(Deref::deref)
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
    use crate::genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo};
    use solana_sdk::{signature::Keypair, system_transaction};
    use std::convert::TryFrom;

    #[test]
    fn test_transaction_batch() {
        let (bank, txs) = setup();

        // Test getting locked accounts
        let batch = bank.prepare_batch(txs.iter()).unwrap();

        // Grab locks
        assert!(batch.lock_results().iter().all(|x| x.is_ok()));

        // Trying to grab locks again should fail
        let batch2 = bank.prepare_batch(txs.iter()).unwrap();
        assert!(batch2.lock_results().iter().all(|x| x.is_err()));

        // Drop the first set of locks
        drop(batch);

        // Now grabbing locks should work again
        let batch2 = bank.prepare_batch(txs.iter()).unwrap();
        assert!(batch2.lock_results().iter().all(|x| x.is_ok()));
    }

    #[test]
    fn test_simulation_batch() {
        let (bank, txs) = setup();
        let txs = txs
            .into_iter()
            .map(SanitizedTransaction::try_from)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        // Prepare batch without locks
        let batch = bank.prepare_simulation_batch(txs[0].clone());
        assert!(batch.lock_results().iter().all(|x| x.is_ok()));

        // Grab locks
        let batch2 = bank.prepare_sanitized_batch(&txs);
        assert!(batch2.lock_results().iter().all(|x| x.is_ok()));

        // Prepare another batch without locks
        let batch3 = bank.prepare_simulation_batch(txs[0].clone());
        assert!(batch3.lock_results().iter().all(|x| x.is_ok()));
    }

    fn setup() -> (Bank, Vec<Transaction>) {
        let dummy_leader_pubkey = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(500, &dummy_leader_pubkey, 100);
        let bank = Bank::new(&genesis_config);

        let pubkey = solana_sdk::pubkey::new_rand();
        let keypair2 = Keypair::new();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let txs = vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash()),
            system_transaction::transfer(&keypair2, &pubkey2, 1, genesis_config.hash()),
        ];

        (bank, txs)
    }
}
