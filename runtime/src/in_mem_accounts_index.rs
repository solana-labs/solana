use crate::accounts_index::{AccountMapEntry, IsCached};
use solana_sdk::pubkey::Pubkey;
use std::collections::{
    hash_map::{Entry, Iter, Keys},
    HashMap,
};
use std::fmt::Debug;

type K = Pubkey;

// one instance of this represents one bin of the accounts index.
#[derive(Debug, Default)]
pub struct InMemAccountsIndex<V: IsCached> {
    // backing store
    map: HashMap<Pubkey, AccountMapEntry<V>>,
}

impl<V: IsCached> InMemAccountsIndex<V> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn entry(&mut self, pubkey: Pubkey) -> Entry<K, AccountMapEntry<V>> {
        self.map.entry(pubkey)
    }

    pub fn iter(&self) -> Iter<K, AccountMapEntry<V>> {
        self.map.iter()
    }

    pub fn keys(&self) -> Keys<K, AccountMapEntry<V>> {
        self.map.keys()
    }

    pub fn get(&self, key: &K) -> Option<&AccountMapEntry<V>> {
        self.map.get(key)
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
