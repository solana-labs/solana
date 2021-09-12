use crate::accounts_index::{AccountMapEntry, IsCached};
use solana_sdk::pubkey::Pubkey;
use std::collections::{
    hash_map::{Entry, Keys},
    HashMap,
};
use std::fmt::Debug;

type K = Pubkey;

// one instance of this represents one bin of the accounts index.
#[derive(Debug, Default)]
pub struct InMemAccountsIndex<T: IsCached> {
    // backing store
    map: HashMap<Pubkey, AccountMapEntry<T>>,
}

impl<T: IsCached> InMemAccountsIndex<T> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn entry(&mut self, pubkey: Pubkey) -> Entry<K, AccountMapEntry<T>> {
        self.map.entry(pubkey)
    }

    pub fn items(&self) -> Vec<(K, AccountMapEntry<T>)> {
        self.map.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    pub fn keys(&self) -> Keys<K, AccountMapEntry<T>> {
        self.map.keys()
    }

    pub fn get(&self, key: &K) -> Option<AccountMapEntry<T>> {
        self.map.get(key).cloned()
    }

    pub fn remove(&mut self, key: &K) {
        self.map.remove(key);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
