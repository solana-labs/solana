use {
    crate::bank::Bank,
    solana_sdk::transaction::{Result, SanitizedTransaction},
    std::borrow::Cow,
};

// Represents the results of trying to lock a set of accounts
pub struct TransactionBatch<'a, 'b> {
    lock_results: Vec<Result<()>>,
    bank: &'a Bank,
    sanitized_txs: Cow<'b, [SanitizedTransaction]>,
    needs_unlock: bool,
}

impl<'a, 'b> TransactionBatch<'a, 'b> {
    pub fn new(
        lock_results: Vec<Result<()>>,
        bank: &'a Bank,
        sanitized_txs: Cow<'b, [SanitizedTransaction]>,
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

    pub fn bank(&self) -> &Bank {
        self.bank
    }

    pub fn set_needs_unlock(&mut self, needs_unlock: bool) {
        self.needs_unlock = needs_unlock;
    }

    pub fn needs_unlock(&self) -> bool {
        self.needs_unlock
    }

    /// Release unnecessary locks before execution.
    pub fn release_locks_early(&mut self, results: impl Iterator<Item = Result<()>>) {
        // If we don't need to unlock, then we don't need to release locks early
        if !self.needs_unlock() {
            return;
        }

        let Self {
            ref mut lock_results,
            bank,
            ref sanitized_txs,
            ..
        } = self;

        let tx_and_is_locked = results
            .zip(sanitized_txs.iter())
            .zip(lock_results.iter_mut())
            .filter(|(_, lock_result)| lock_result.is_ok())
            .filter(|((result, _), _)| result.is_err())
            .map(|((result, tx), lock_result)| {
                *lock_result = result; // modify `lock_result` so it won't be unlocked later on drop
                (tx, true)
            });

        bank.accounts().unlock_accounts(tx_and_is_locked);
    }
}

// Unlock all locked accounts in destructor.
impl<'a, 'b> Drop for TransactionBatch<'a, 'b> {
    fn drop(&mut self) {
        if self.needs_unlock() {
            self.bank.accounts().unlock_accounts(
                self.sanitized_transactions()
                    .iter()
                    .zip(self.lock_results().iter().map(|r| r.is_ok())),
            )
        }

        self.needs_unlock = false;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
        solana_sdk::{signature::Keypair, system_transaction},
    };

    #[test]
    fn test_transaction_batch() {
        let (bank, txs) = setup();

        // Test getting locked accounts
        let batch = bank.prepare_sanitized_batch(&txs);

        // Grab locks
        assert!(batch.lock_results().iter().all(|x| x.is_ok()));

        // Trying to grab locks again should fail
        let batch2 = bank.prepare_sanitized_batch(&txs);
        assert!(batch2.lock_results().iter().all(|x| x.is_err()));

        // Drop the first set of locks
        drop(batch);

        // Now grabbing locks should work again
        let batch2 = bank.prepare_sanitized_batch(&txs);
        assert!(batch2.lock_results().iter().all(|x| x.is_ok()));
    }

    #[test]
    fn test_simulation_batch() {
        let (bank, txs) = setup();

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

    fn setup() -> (Bank, Vec<SanitizedTransaction>) {
        let dummy_leader_pubkey = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(500, &dummy_leader_pubkey, 100);
        let bank = Bank::new_for_tests(&genesis_config);

        let pubkey = solana_sdk::pubkey::new_rand();
        let keypair2 = Keypair::new();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let txs = vec![
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_config.hash(),
            )),
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &keypair2,
                &pubkey2,
                1,
                genesis_config.hash(),
            )),
        ];

        (bank, txs)
    }
}
