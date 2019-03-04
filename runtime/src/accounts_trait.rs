use crate::bank::{BankError, Result};
use crate::runtime::has_duplicates;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::debug;
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::{Mutex, RwLock};

pub type InstructionAccounts = Vec<Account>;
pub type InstructionLoaders = Vec<Vec<(Pubkey, Account)>>;
pub type Fork = u64;

#[derive(Debug, Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub account_loaded_twice: usize,
    pub last_id_not_found: usize,
    pub last_id_too_old: usize,
    pub reserve_last_id: usize,
    pub insufficient_funds: usize,
    pub duplicate_signature: usize,
    pub call_chain_too_deep: usize,
    pub missing_signature_for_fee: usize,
}

pub trait AccountsStore {
    fn new(fork: Fork) -> Self;
    fn new_from_parent(fork: Fork, parent: Fork) -> Self;
    fn load_slow(fork: Fork, pubkey: &Pubkey) -> Option<Account>;
    fn load_slow_no_parent(fork: Fork, pubkey: &Pubkey) -> Option<Account>;
    fn store_slow(fork: Fork, purge: bool, pubkey: &Pubkey, account: &Account);
    fn hash_internal_state(fork: Fork) -> Option<Hash>;
    fn lock_accounts(fork: Fork, txs: &[Transaction]) -> Vec<Result<()>>;
    fn unlock_accounts(fork: Fork, txs: &[Transaction], results: &[Result<()>]);
    fn has_accounts(fork: Fork) -> bool;
    fn load_accounts(fork: Fork, txs: &[Transaction], results: Vec<Result<()>>, error_counters: &mut ErrorCounters) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>>;
    fn store_accounts(fork: Fork, purge: bool, txs: &[Transaction], res: &[Result<()>], loaded: &[Result<(InstructionAccounts, InstructionLoaders)>]);
    fn increment_transaction_count(&self, fork: Fork, tx_count: usize);
    fn transaction_count(&self, fork: Fork) -> u64;
    fn squash(&self, fork: Fork);
    fn get_vote_accounts(&self) -> Vec<Account>;
}
