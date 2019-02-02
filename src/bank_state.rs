use crate::accounts::{Accounts, ErrorCounters, InstructionAccounts, InstructionLoaders};
use crate::bank::{BankError, Result};
use crate::bank_checkpoint::BankCheckpoint;
use crate::counter::Counter;
use crate::entry::Entry;
use crate::last_id_queue::MAX_ENTRY_IDS;
use crate::poh_recorder::{PohRecorder, PohRecorderError};
use crate::result::Error;
use crate::rpc_pubsub::RpcSubscriptions;
use crate::runtime::{self, RuntimeError};
use log::Level;
use rayon::prelude::*;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::timing::duration_as_us;
use solana_sdk::transaction::Transaction;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

pub struct BankState {
    pub checkpoints: Vec<Arc<BankCheckpoint>>,
}

impl BankState {
    pub fn head(&self) -> &Arc<BankCheckpoint> {
        self.checkpoints
            .first()
            .expect("at least 1 checkpoint needs to be available for the state")
    }
    fn load_accounts(
        &self,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let accounts: Vec<&Accounts> = self.checkpoints.iter().map(|c| &c.accounts).collect();
        Accounts::load_accounts(&accounts, txs, results, error_counters)
    }
    pub fn hash_internal_state(&self) -> Hash {
        self.head().hash_internal_state()
    }
    pub fn transaction_count(&self) -> u64 {
        self.head().transaction_count()
    }

    pub fn register_tick(&self, last_id: &Hash) {
        self.head().register_tick(last_id)
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        self.head().get_signature_status(signature)
    }

    /// Each program would need to be able to introspect its own state
    /// this is hard-coded to the Budget language
    pub fn get_balance_slow(&self, pubkey: &Pubkey) -> u64 {
        self.load_slow(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }
    pub fn read_balance(account: &Account) -> u64 {
        // TODO: Re-instate budget_program special case?
        /*
        if budget_program::check_id(&account.owner) {
           return budget_program::get_balance(account);
        }
        */
        account.tokens
    }

    pub fn get_account_slow(&self, pubkey: &Pubkey) -> Option<Account> {
        self.load_slow(pubkey)
    }

    pub fn load_slow(&self, pubkey: &Pubkey) -> Option<Account> {
        let accounts: Vec<&Accounts> = self.checkpoints.iter().map(|c| &c.accounts).collect();
        Accounts::load_slow(&accounts, pubkey)
    }

    pub fn tick_height(&self) -> u64 {
        self.head().tick_height()
    }

    pub fn last_id(&self) -> Hash {
        self.head().last_id()
    }

    #[allow(clippy::type_complexity)]
    fn load_and_execute_transactions(
        &self,
        txs: &[Transaction],
        lock_results: &[Result<()>],
        max_age: usize,
    ) -> (
        Vec<Result<(InstructionAccounts, InstructionLoaders)>>,
        Vec<Result<()>>,
    ) {
        let head = &self.checkpoints[0];
        debug!("processing transactions: {}", txs.len());
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let age_results = head.check_age(txs, lock_results, max_age, &mut error_counters);
        let sig_results = head.check_signatures(txs, age_results, &mut error_counters);
        let mut loaded_accounts = self.load_accounts(txs, sig_results, &mut error_counters);
        let tick_height = head.tick_height();

        let load_elapsed = now.elapsed();
        let now = Instant::now();
        let executed: Vec<Result<()>> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(accs, tx)| match accs {
                Err(e) => Err(e.clone()),
                Ok((ref mut accounts, ref mut loaders)) => {
                    runtime::execute_transaction(tx, loaders, accounts, tick_height).map_err(
                        |RuntimeError::ProgramError(index, err)| {
                            BankError::ProgramError(index, err)
                        },
                    )
                }
            })
            .collect();

        let execution_elapsed = now.elapsed();

        debug!(
            "load: {}us execute: {}us txs_len={}",
            duration_as_us(&load_elapsed),
            duration_as_us(&execution_elapsed),
            txs.len(),
        );
        let mut tx_count = 0;
        let mut err_count = 0;
        for (r, tx) in executed.iter().zip(txs.iter()) {
            if r.is_ok() {
                tx_count += 1;
            } else {
                if err_count == 0 {
                    info!("tx error: {:?} {:?}", r, tx);
                }
                err_count += 1;
            }
        }
        if err_count > 0 {
            info!("{} errors of {} txs", err_count, err_count + tx_count);
            inc_new_counter_info!(
                "bank-process_transactions-account_not_found",
                error_counters.account_not_found
            );
            inc_new_counter_info!("bank-process_transactions-error_count", err_count);
        }

        head.accounts.increment_transaction_count(tx_count);

        inc_new_counter_info!("bank-process_transactions-txs", tx_count);
        if 0 != error_counters.last_id_not_found {
            inc_new_counter_info!(
                "bank-process_transactions-error-last_id_not_found",
                error_counters.last_id_not_found
            );
        }
        if 0 != error_counters.reserve_last_id {
            inc_new_counter_info!(
                "bank-process_transactions-error-reserve_last_id",
                error_counters.reserve_last_id
            );
        }
        if 0 != error_counters.duplicate_signature {
            inc_new_counter_info!(
                "bank-process_transactions-error-duplicate_signature",
                error_counters.duplicate_signature
            );
        }
        if 0 != error_counters.insufficient_funds {
            inc_new_counter_info!(
                "bank-process_transactions-error-insufficient_funds",
                error_counters.insufficient_funds
            );
        }
        (loaded_accounts, executed)
    }

    /// Process a batch of transactions.
    #[must_use]
    pub fn load_execute_record_notify_commit(
        &self,
        txs: &[Transaction],
        recorder: Option<&PohRecorder>,
        subs: &Option<Arc<RpcSubscriptions>>,
        lock_results: &[Result<()>],
        max_age: usize,
    ) -> Result<Vec<Result<()>>> {
        let head = &self.checkpoints[0];
        let (loaded_accounts, executed) =
            self.load_and_execute_transactions(txs, lock_results, max_age);
        if let Some(poh) = recorder {
            Self::record_transactions(txs, &executed, poh)?;
        }
        head.commit_transactions(subs, txs, &loaded_accounts, &executed);
        Ok(executed)
    }

    #[must_use]
    pub fn load_execute_record_commit(
        &self,
        txs: &[Transaction],
        recorder: Option<&PohRecorder>,
        lock_results: &[Result<()>],
        max_age: usize,
    ) -> Result<Vec<Result<()>>> {
        self.load_execute_record_notify_commit(txs, recorder, &None, lock_results, max_age)
    }
    pub fn process_and_record_transactions(
        &self,
        subs: &Option<Arc<RpcSubscriptions>>,
        txs: &[Transaction],
        poh: Option<&PohRecorder>,
    ) -> Result<Vec<Result<()>>> {
        let head = &self.checkpoints[0];
        let now = Instant::now();
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let lock_results = head.lock_accounts(txs);
        let lock_time = now.elapsed();

        let now = Instant::now();
        // Use a shorter maximum age when adding transactions into the pipeline.  This will reduce
        // the likelihood of any single thread getting starved and processing old ids.
        // TODO: Banking stage threads should be prioritized to complete faster then this queue
        // expires.
        let (loaded_accounts, results) =
            self.load_and_execute_transactions(txs, &lock_results, MAX_ENTRY_IDS as usize / 2);
        let load_execute_time = now.elapsed();

        let record_time = {
            let now = Instant::now();
            if let Some(recorder) = poh {
                Self::record_transactions(txs, &results, recorder)?;
            }
            now.elapsed()
        };

        let commit_time = {
            let now = Instant::now();
            head.commit_transactions(subs, txs, &loaded_accounts, &results);
            now.elapsed()
        };

        let now = Instant::now();
        // Once the accounts are new transactions can enter the pipeline to process them
        head.unlock_accounts(&txs, &lock_results);
        let unlock_time = now.elapsed();
        debug!(
            "lock: {}us load_execute: {}us record: {}us commit: {}us unlock: {}us txs_len: {}",
            duration_as_us(&lock_time),
            duration_as_us(&load_execute_time),
            duration_as_us(&record_time),
            duration_as_us(&commit_time),
            duration_as_us(&unlock_time),
            txs.len(),
        );
        Ok(results)
    }
    pub fn record_transactions(
        txs: &[Transaction],
        results: &[Result<()>],
        poh: &PohRecorder,
    ) -> Result<()> {
        let processed_transactions: Vec<_> = results
            .iter()
            .zip(txs.iter())
            .filter_map(|(r, x)| match r {
                Ok(_) => Some(x.clone()),
                Err(BankError::ProgramError(index, err)) => {
                    info!("program error {:?}, {:?}", index, err);
                    Some(x.clone())
                }
                Err(ref e) => {
                    debug!("process transaction failed {:?}", e);
                    None
                }
            })
            .collect();
        debug!("processed: {} ", processed_transactions.len());
        // unlock all the accounts with errors which are filtered by the above `filter_map`
        if !processed_transactions.is_empty() {
            let hash = Transaction::hash(&processed_transactions);
            // record and unlock will unlock all the successfull transactions
            poh.record(hash, processed_transactions).map_err(|e| {
                warn!("record failure: {:?}", e);
                match e {
                    Error::PohRecorderError(PohRecorderError::MaxHeightReached) => {
                        BankError::MaxHeightReached
                    }
                    _ => BankError::RecordFailure,
                }
            })?;
        }
        Ok(())
    }
    fn ignore_program_errors(results: Vec<Result<()>>) -> Vec<Result<()>> {
        results
            .into_iter()
            .map(|result| match result {
                // Entries that result in a ProgramError are still valid and are written in the
                // ledger so map them to an ok return value
                Err(BankError::ProgramError(index, err)) => {
                    info!("program error {:?}, {:?}", index, err);
                    inc_new_counter_info!("bank-ignore_program_err", 1);
                    Ok(())
                }
                _ => result,
            })
            .collect()
    }

    fn par_execute_entries(&self, entries: &[(&Entry, Vec<Result<()>>)]) -> Result<()> {
        let head = &self.checkpoints[0];
        inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
        let results: Vec<Result<()>> = entries
            .into_par_iter()
            .map(|(e, lock_results)| {
                let old_results = self
                    .load_execute_record_commit(&e.transactions, None, lock_results, MAX_ENTRY_IDS)
                    .expect("no record failures");
                let results = Self::ignore_program_errors(old_results);
                head.unlock_accounts(&e.transactions, &results);
                BankCheckpoint::first_err(&results)
            })
            .collect();
        BankCheckpoint::first_err(&results)
    }

    /// process entries in parallel
    /// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
    /// 2. Process the locked group in parallel
    /// 3. Register the `Tick` if it's available, goto 1
    pub fn process_entries(&self, entries: &[Entry]) -> Result<()> {
        let head = &self.checkpoints[0];
        // accumulator for entries that can be processed in parallel
        let mut mt_group = vec![];
        for entry in entries {
            if entry.is_tick() {
                // if its a tick, execute the group and register the tick
                self.par_execute_entries(&mt_group)?;
                head.register_tick(&entry.id);
                mt_group = vec![];
                continue;
            }
            // try to lock the accounts
            let lock_results = head.lock_accounts(&entry.transactions);
            // if any of the locks error out
            // execute the current group
            if BankCheckpoint::first_err(&lock_results).is_err() {
                self.par_execute_entries(&mt_group)?;
                mt_group = vec![];
                //reset the lock and push the entry
                head.unlock_accounts(&entry.transactions, &lock_results);
                let lock_results = head.lock_accounts(&entry.transactions);
                mt_group.push((entry, lock_results));
            } else {
                // push the entry to the mt_group
                mt_group.push((entry, lock_results));
            }
        }
        self.par_execute_entries(&mt_group)?;
        Ok(())
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::native_program::ProgramError;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::system_program;
    use solana_sdk::system_transaction::SystemTransaction;

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `last_id` is the last Entry ID observed by the client.
    pub fn transfer(
        bank: &BankState,
        n: u64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
    ) -> Result<Signature> {
        let tx = SystemTransaction::new_move(keypair, to, n, last_id, 0);
        let signature = tx.signatures[0];
        let e = bank
            .process_and_record_transactions(&None, &[tx], None)
            .expect("no recorder");
        match &e[0] {
            Ok(_) => Ok(signature),
            Err(e) => Err(e.clone()),
        }
    }

    fn new_state(mint: &Keypair, tokens: u64, last_id: &Hash) -> BankState {
        let accounts = [(mint.pubkey(), Account::new(tokens, 0, Pubkey::default()))];
        let bank = Arc::new(BankCheckpoint::new_from_accounts(0, &accounts, &last_id));
        BankState {
            checkpoints: vec![bank],
        }
    }

    fn add_system_program(checkpoint: &BankCheckpoint) {
        let system_program_account = Account {
            tokens: 1,
            owner: system_program::id(),
            userdata: b"solana_system_program".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };
        checkpoint.store_slow(false, &system_program::id(), &system_program_account);
    }

    #[test]
    fn test_interleaving_locks() {
        let last_id = Hash::default();
        let mint = Keypair::new();
        let alice = Keypair::new();
        let bob = Keypair::new();
        let bank = new_state(&mint, 3, &last_id);
        bank.head().register_tick(&last_id);
        add_system_program(bank.head());

        let tx1 = SystemTransaction::new_move(&mint, alice.pubkey(), 1, last_id, 0);
        let pay_alice = vec![tx1];

        let locked_alice = bank.head().lock_accounts(&pay_alice);
        assert!(locked_alice[0].is_ok());
        let results_alice = bank
            .load_execute_record_commit(&pay_alice, None, &locked_alice, MAX_ENTRY_IDS)
            .unwrap();
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            transfer(&bank, 1, &mint, bob.pubkey(), last_id),
            Err(BankError::AccountInUse)
        );
        // the second time should fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            transfer(&bank, 1, &mint, bob.pubkey(), last_id),
            Err(BankError::AccountInUse)
        );

        bank.head().unlock_accounts(&pay_alice, &locked_alice);

        assert_matches!(transfer(&bank, 2, &mint, bob.pubkey(), last_id), Ok(_));
    }
    #[test]
    fn test_bank_ignore_program_errors() {
        let expected_results = vec![Ok(()), Ok(())];
        let results = vec![Ok(()), Ok(())];
        let updated_results = BankState::ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        let results = vec![
            Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens,
            )),
            Ok(()),
        ];
        let updated_results = BankState::ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        // Other BankErrors should not be ignored
        let results = vec![Err(BankError::AccountNotFound), Ok(())];
        let updated_results = BankState::ignore_program_errors(results);
        assert_ne!(updated_results, expected_results);
    }
}
