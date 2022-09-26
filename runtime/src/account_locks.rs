//! Limits account access using locks to prevent race conditions in writing to accounts.

use {
    log::*,
    solana_sdk::{
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, TransactionAccountLocks, TransactionError},
    },
    std::collections::{hash_map::Entry, HashMap, HashSet},
};

type Result<T> = std::result::Result<T, TransactionError>;

#[derive(Debug, Default, AbiExample)]
pub(crate) struct AccountLocks {
    write_locks: HashSet<Pubkey>,
    readonly_locks: HashMap<Pubkey, u64>,
}

impl AccountLocks {
    pub(crate) fn lock_transactions_accounts<'a>(
        &mut self,
        transaction_locks: impl Iterator<Item = Result<TransactionAccountLocks<'a>>>,
    ) -> Vec<Result<()>> {
        transaction_locks
            .map(|transaction_locks| match transaction_locks {
                Ok(transaction_locks) => {
                    self.lock_accounts(&transaction_locks.writable, &transaction_locks.readonly)
                }
                Err(err) => Err(err),
            })
            .collect()
    }

    pub(crate) fn unlock_transactions_accounts<'a>(
        &mut self,
        sanitized_transactions: impl Iterator<Item = &'a SanitizedTransaction>,
        results: impl Iterator<Item = &'a Result<()>>,
    ) {
        sanitized_transactions
            .zip(results)
            .for_each(|(transaction, result)| {
                if result.is_ok() {
                    let transaction_locks = transaction.get_account_locks_unchecked();
                    self.unlock_accounts(&transaction_locks.writable, &transaction_locks.readonly);
                }
            });
    }

    fn can_lock_accounts(
        &mut self,
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
    ) -> Result<()> {
        for k in writable_keys {
            if self.is_locked_write(k) || self.is_locked_readonly(k) {
                debug!("Writable account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }
        for k in readonly_keys {
            if self.is_locked_write(k) {
                debug!("Read-only account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }

        Ok(())
    }

    fn lock_accounts(
        &mut self,
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
    ) -> Result<()> {
        self.can_lock_accounts(writable_keys, readonly_keys)?;

        for k in writable_keys {
            self.lock_writable(k);
        }

        for k in readonly_keys {
            if !self.lock_readonly(k) {
                self.insert_new_readonly(k);
            }
        }

        Ok(())
    }

    fn unlock_accounts(&mut self, writable_keys: &[&Pubkey], readonly_keys: &[&Pubkey]) {
        for k in writable_keys {
            self.unlock_write(k);
        }
        for k in readonly_keys {
            self.unlock_readonly(k);
        }
    }

    fn is_locked_readonly(&self, key: &Pubkey) -> bool {
        self.readonly_locks
            .get(key)
            .map_or(false, |count| *count > 0)
    }

    fn is_locked_write(&self, key: &Pubkey) -> bool {
        self.write_locks.contains(key)
    }

    fn insert_new_readonly(&mut self, key: &Pubkey) {
        assert!(self.readonly_locks.insert(*key, 1).is_none());
    }

    fn lock_readonly(&mut self, key: &Pubkey) -> bool {
        self.readonly_locks.get_mut(key).map_or(false, |count| {
            *count += 1;
            true
        })
    }

    fn lock_writable(&mut self, key: &Pubkey) {
        assert!(self.write_locks.insert(*key));
    }

    fn unlock_readonly(&mut self, key: &Pubkey) {
        if let Entry::Occupied(mut occupied_entry) = self.readonly_locks.entry(*key) {
            let count = occupied_entry.get_mut();
            *count -= 1;
            if *count == 0 {
                occupied_entry.remove_entry();
            }
        }
    }

    fn unlock_write(&mut self, key: &Pubkey) {
        self.write_locks.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::account_locks::AccountLocks,
        solana_sdk::{
            hash::Hash,
            instruction::CompiledInstruction,
            message::Message,
            native_loader,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            signers::Signers,
            transaction::{
                SanitizedTransaction, Transaction, TransactionError, MAX_TX_ACCOUNT_LOCKS,
            },
        },
        std::{
            sync::{
                atomic::{AtomicBool, AtomicU64, Ordering},
                Arc, Mutex,
            },
            thread,
        },
    };

    impl AccountLocks {
        fn get_readonly_count(&self, key: &Pubkey) -> Option<u64> {
            self.readonly_locks.get(key).cloned()
        }
    }

    fn new_sanitized_tx<T: Signers>(
        from_keypairs: &T,
        message: Message,
        recent_blockhash: Hash,
    ) -> SanitizedTransaction {
        SanitizedTransaction::from_transaction_for_tests(Transaction::new(
            from_keypairs,
            message,
            recent_blockhash,
        ))
    }

    #[test]
    fn test_accounts_locks() {
        let mut account_locks = AccountLocks::default();

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx = new_sanitized_tx(&[&keypair0], message, Hash::default());
        let txs0 = [tx];
        let transaction_locks = txs0
            .iter()
            .map(|tx| tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS));
        let results0 = account_locks.lock_transactions_accounts(transaction_locks);

        assert!(results0[0].is_ok());
        assert!(account_locks.is_locked_write(&keypair0.pubkey()));
        assert_eq!(
            account_locks.get_readonly_count(&keypair1.pubkey()),
            Some(1)
        );

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair2.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx0 = new_sanitized_tx(&[&keypair2], message, Hash::default());
        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair3.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx1 = new_sanitized_tx(&[&keypair1], message, Hash::default());
        let txs1 = vec![tx0, tx1];
        let transaction_locks = txs1
            .iter()
            .map(|tx| tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS));
        let results1 = account_locks.lock_transactions_accounts(transaction_locks);

        assert!(results1[0].is_ok()); // Read-only account (keypair1) can be referenced multiple times
        assert!(results1[1].is_err()); // Read-only account (keypair1) cannot also be locked as writable
        assert_eq!(
            account_locks.get_readonly_count(&keypair1.pubkey()),
            Some(2)
        );

        account_locks.unlock_transactions_accounts(txs0.iter(), results0.iter());
        account_locks.unlock_transactions_accounts(txs1.iter(), results1.iter());
        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair3.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx = new_sanitized_tx(&[&keypair1], message, Hash::default());
        let txs2 = [tx];
        let transaction_locks = txs2
            .iter()
            .map(|tx| tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS));
        let results2 = account_locks.lock_transactions_accounts(transaction_locks);
        assert!(results2[0].is_ok()); // Now keypair1 account can be locked as writable

        // Check that read-only lock with zero references is deleted
        assert_eq!(account_locks.get_readonly_count(&keypair1.pubkey()), None);
    }

    #[test]
    fn test_accounts_locks_multithreaded() {
        let account_locks = Arc::new(Mutex::new(AccountLocks::default()));

        let counter = Arc::new(AtomicU64::new(0));
        let exit = Arc::new(AtomicBool::new(false));

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let readonly_message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let readonly_tx = new_sanitized_tx(&[&keypair0], readonly_message, Hash::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let writable_message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair2.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let writable_tx = new_sanitized_tx(&[&keypair1], writable_message, Hash::default());

        let counter_clone = counter.clone();
        let account_locks_clone = account_locks.clone();
        let exit_clone = exit.clone();
        thread::spawn(move || {
            let counter_clone = counter_clone.clone();
            let exit_clone = exit_clone.clone();
            loop {
                let txs = vec![writable_tx.clone()];
                let transaction_locks = txs
                    .iter()
                    .map(|tx| tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS));
                let results = account_locks_clone
                    .lock()
                    .unwrap()
                    .lock_transactions_accounts(transaction_locks);
                for result in results.iter() {
                    if result.is_ok() {
                        counter_clone.clone().fetch_add(1, Ordering::SeqCst);
                    }
                }
                account_locks_clone
                    .lock()
                    .unwrap()
                    .unlock_transactions_accounts(txs.iter(), results.iter());
                if exit_clone.clone().load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        let counter_clone = counter;
        for _ in 0..5 {
            let txs = vec![readonly_tx.clone()];
            let transaction_locks = txs
                .iter()
                .map(|tx| tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS));
            let results = account_locks
                .lock()
                .unwrap()
                .lock_transactions_accounts(transaction_locks);
            if results[0].is_ok() {
                let counter_value = counter_clone.clone().load(Ordering::SeqCst);
                thread::sleep(std::time::Duration::from_millis(50));
                assert_eq!(counter_value, counter_clone.clone().load(Ordering::SeqCst));
            }
            account_locks
                .lock()
                .unwrap()
                .unlock_transactions_accounts(txs.iter(), results.iter());
            thread::sleep(std::time::Duration::from_millis(50));
        }
        exit.store(true, Ordering::Relaxed);
    }

    #[test]
    fn test_demote_program_write_locks() {
        let mut account_locks = AccountLocks::default();

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            0, // All accounts marked as writable
            vec![keypair0.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx = new_sanitized_tx(&[&keypair0], message, Hash::default());
        let txs = [tx];
        let transaction_locks = txs
            .iter()
            .map(|tx| tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS));
        let results0 = account_locks.lock_transactions_accounts(transaction_locks);

        assert!(results0[0].is_ok());
        // Instruction program-id account demoted to readonly
        assert_eq!(
            account_locks.get_readonly_count(&native_loader::id()),
            Some(1)
        );
        // Non-program accounts remain writable
        assert!(account_locks.is_locked_write(&keypair0.pubkey()));
        assert!(account_locks.is_locked_write(&keypair1.pubkey()));
    }

    #[test]
    fn test_accounts_locks_with_results() {
        let mut account_locks = AccountLocks::default();

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair0.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx0 = new_sanitized_tx(&[&keypair1], message, Hash::default());
        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair2.pubkey(), keypair0.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx1 = new_sanitized_tx(&[&keypair2], message, Hash::default());
        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair3.pubkey(), keypair0.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx2 = new_sanitized_tx(&[&keypair3], message, Hash::default());
        let txs = vec![tx0, tx1, tx2];

        let qos_results = vec![
            Ok(()),
            Err(TransactionError::WouldExceedMaxBlockCostLimit),
            Ok(()),
        ];

        let transaction_locks =
            txs.iter()
                .zip(qos_results.iter())
                .map(|(tx, qos_results)| match qos_results {
                    Ok(_) => tx.get_account_locks(MAX_TX_ACCOUNT_LOCKS),
                    Err(err) => Err(err.clone()),
                });

        let results = account_locks.lock_transactions_accounts(transaction_locks);

        assert!(results[0].is_ok()); // Read-only account (keypair0) can be referenced multiple times
        assert!(results[1].is_err()); // is not locked due to !qos_results[1].is_ok()
        assert!(results[2].is_ok()); // Read-only account (keypair0) can be referenced multiple times

        // verify that keypair0 read-only lock twice (for tx0 and tx2)
        assert_eq!(
            account_locks.get_readonly_count(&keypair0.pubkey()),
            Some(2)
        );
        assert!(!account_locks.is_locked_write(&keypair2.pubkey()));

        account_locks.unlock_transactions_accounts(txs.iter(), results.iter());

        // check all locks to be removed
        assert!(account_locks.readonly_locks.is_empty());
        assert!(account_locks.write_locks.is_empty());
    }
}
