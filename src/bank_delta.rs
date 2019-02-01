use crate::accounts::{Accounts, ErrorCounters, InstructionAccounts, InstructionLoaders};
use crate::bank::{BankError, BankSubscriptions, Result};
use crate::counter::Counter;
use crate::last_id_queue::LastIdQueue;
use crate::rpc_pubsub::RpcSubscriptions;
use crate::status_cache::StatusCache;
use log::Level;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::timing::duration_as_us;
use solana_sdk::transaction::Transaction;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

type BankStatusCache = StatusCache<BankError>;

#[derive(Default)]
pub struct BankDelta {
    /// accounts database
    pub accounts: Accounts,
    /// entries
    last_id_queue: RwLock<LastIdQueue>,
    /// status cache
    status_cache: RwLock<BankStatusCache>,
    frozen: AtomicBool,
    fork_id: AtomicUsize,
}

impl std::fmt::Debug for BankDelta {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BankDelta {{ fork_id: {} }}", self.fork_id())
    }
}

impl BankDelta {
    // last_id id is used by the status_cache to filter duplicate signatures
    pub fn new(fork_id: u64, last_id: &Hash) -> Self {
        BankDelta {
            accounts: Accounts::default(),
            last_id_queue: RwLock::new(LastIdQueue::default()),
            status_cache: RwLock::new(StatusCache::new(last_id)),
            frozen: AtomicBool::new(false),
            fork_id: AtomicUsize::new(fork_id as usize),
        }
    }
    /// Create an Bank using a deposit.
    pub fn new_from_accounts(fork: u64, accounts: &[(Pubkey, Account)], last_id: &Hash) -> Self {
        let bank_state = BankDelta::new(fork, last_id);
        for (to, account) in accounts {
            bank_state.accounts.store_slow(false, &to, &account);
        }
        bank_state
    }
    pub fn store_slow(&self, purge: bool, pubkey: &Pubkey, account: &Account) {
        assert!(!self.frozen());
        self.accounts.store_slow(purge, pubkey, account)
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        assert!(!self.frozen());
        self.status_cache.write().unwrap().clear();
    }

    /// Return the last entry ID registered.
    pub fn last_id(&self) -> Hash {
        self.last_id_queue
            .read()
            .unwrap()
            .last_id
            .expect("no last_id has been set")
    }

    pub fn transaction_count(&self) -> u64 {
        self.accounts.transaction_count()
    }
    pub fn freeze(&self) {
        info!(
            "delta {} frozen at {}",
            self.fork_id.load(Ordering::Relaxed),
            self.last_id_queue.read().unwrap().tick_height
        );

        self.frozen.store(true, Ordering::Relaxed);
    }
    pub fn frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved finality
    pub fn get_confirmation_timestamp(
        &self,
        ticks_and_stakes: &mut [(u64, u64)],
        supermajority_stake: u64,
    ) -> Option<u64> {
        let last_id_queue = self.last_id_queue.read().unwrap();
        last_id_queue.get_confirmation_timestamp(ticks_and_stakes, supermajority_stake)
    }
    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        self.status_cache
            .read()
            .unwrap()
            .get_signature_status(signature)
    }
    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.status_cache.read().unwrap().has_signature(signature)
    }

    pub fn tick_height(&self) -> u64 {
        self.last_id_queue.read().unwrap().tick_height
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_tick(&self, last_id: &Hash) {
        assert!(!self.frozen());
        let mut last_id_queue = self.last_id_queue.write().unwrap();
        inc_new_counter_info!("bank-register_tick-registered", 1);
        last_id_queue.register_tick(last_id)
    }
    pub fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        self.accounts.lock_accounts(txs)
    }
    pub fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        self.accounts.unlock_accounts(txs, results)
    }
    pub fn check_age(
        &self,
        txs: &[Transaction],
        lock_results: &[Result<()>],
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let last_ids = self.last_id_queue.read().unwrap();
        txs.iter()
            .zip(lock_results.iter())
            .map(|(tx, lock_res)| {
                if lock_res.is_ok() && !last_ids.check_entry_id_age(tx.last_id, max_age) {
                    error_counters.reserve_last_id += 1;
                    Err(BankError::LastIdNotFound)
                } else {
                    lock_res.clone()
                }
            })
            .collect()
    }
    pub fn check_signatures(
        &self,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let status_cache = self.status_cache.read().unwrap();
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|(tx, lock_res)| {
                if lock_res.is_ok() && status_cache.has_signature(&tx.signatures[0]) {
                    error_counters.duplicate_signature += 1;
                    Err(BankError::DuplicateSignature)
                } else {
                    lock_res
                }
            })
            .collect()
    }

    pub fn first_err(results: &[Result<()>]) -> Result<()> {
        for r in results {
            r.clone()?;
        }
        Ok(())
    }

    pub fn commit_transactions(
        &self,
        subscritpions: &Option<Arc<RpcSubscriptions>>,
        txs: &[Transaction],
        loaded_accounts: &[Result<(InstructionAccounts, InstructionLoaders)>],
        executed: &[Result<()>],
    ) {
        assert!(!self.frozen());
        let now = Instant::now();
        self.accounts
            .store_accounts(true, txs, executed, loaded_accounts);

        // Check account subscriptions and send notifications
        if let Some(subs) = subscritpions {
            Self::send_account_notifications(subs, txs, executed, loaded_accounts);
        }

        // once committed there is no way to unroll
        let write_elapsed = now.elapsed();
        debug!(
            "store: {}us txs_len={}",
            duration_as_us(&write_elapsed),
            txs.len(),
        );
        self.update_transaction_statuses(txs, &executed);
        if let Some(subs) = subscritpions {
            Self::update_subscriptions(subs, txs, &executed);
        }
    }
    fn send_account_notifications(
        subscriptions: &RpcSubscriptions,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let tx = &txs[i];
            let accs = raccs.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(accs.0.iter()) {
                subscriptions.check_account(&key, account);
            }
        }
    }
    fn update_subscriptions(
        subscriptions: &RpcSubscriptions,
        txs: &[Transaction],
        res: &[Result<()>],
    ) {
        for (i, tx) in txs.iter().enumerate() {
            subscriptions.check_signature(&tx.signatures[0], &res[i]);
        }
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        assert!(!self.frozen());
        let mut status_cache = self.status_cache.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            match &res[i] {
                Ok(_) => status_cache.add(&tx.signatures[0]),
                Err(BankError::LastIdNotFound) => (),
                Err(BankError::DuplicateSignature) => (),
                Err(BankError::AccountNotFound) => (),
                Err(e) => {
                    status_cache.add(&tx.signatures[0]);
                    status_cache.save_failure_status(&tx.signatures[0], e.clone());
                }
            }
        }
    }

    pub fn hash_internal_state(&self) -> Hash {
        self.accounts.hash_internal_state()
    }
    pub fn set_genesis_last_id(&self, last_id: &Hash) {
        assert!(!self.frozen());
        self.last_id_queue.write().unwrap().genesis_last_id(last_id)
    }

    pub fn fork_id(&self) -> u64 {
        self.fork_id.load(Ordering::Relaxed) as u64
    }
    /// create a new fork for the bank state
    pub fn fork(&self, fork_id: u64, last_id: &Hash) -> Self {
        Self {
            accounts: Accounts::default(),
            last_id_queue: RwLock::new(self.last_id_queue.read().unwrap().fork()),
            status_cache: RwLock::new(StatusCache::new(last_id)),
            frozen: AtomicBool::new(false),
            fork_id: AtomicUsize::new(fork_id as usize),
        }
    }
    /// consume the delta into the root state
    /// self becomes the new root and its fork_id is updated
    pub fn merge_into_root(&self, other: Self) {
        assert!(self.frozen());
        assert!(other.frozen());
        let (accounts, last_id_queue, status_cache, fork_id) = {
            (
                other.accounts,
                other.last_id_queue,
                other.status_cache,
                other.fork_id,
            )
        };
        self.accounts.merge_into_root(accounts);
        self.last_id_queue
            .write()
            .unwrap()
            .merge_into_root(last_id_queue.into_inner().unwrap());
        self.status_cache
            .write()
            .unwrap()
            .merge_into_root(status_cache.into_inner().unwrap());
        self.fork_id
            .store(fork_id.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn last_ids(&self) -> &RwLock<LastIdQueue> {
        &self.last_id_queue
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bank::BankError;

    #[test]
    fn test_first_err() {
        assert_eq!(BankDelta::first_err(&[Ok(())]), Ok(()));
        assert_eq!(
            BankDelta::first_err(&[Ok(()), Err(BankError::DuplicateSignature)]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            BankDelta::first_err(&[
                Ok(()),
                Err(BankError::DuplicateSignature),
                Err(BankError::AccountInUse)
            ]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            BankDelta::first_err(&[
                Ok(()),
                Err(BankError::AccountInUse),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
        assert_eq!(
            BankDelta::first_err(&[
                Err(BankError::AccountInUse),
                Ok(()),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
    }
}
