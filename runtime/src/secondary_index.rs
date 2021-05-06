use dashmap::{mapref::entry::Entry::Occupied, DashMap};
use solana_sdk::pubkey::Pubkey;
use std::{collections::HashSet, fmt::Debug, sync::RwLock};

// The only cases where an inner key should map to a different outer key is
// if the key had different account data for the indexed key across different
// slots. As this is rare, it should be ok to use a Vec here over a HashSet, even
// though we are running some key existence checks.
pub type SecondaryReverseIndexEntry = RwLock<Vec<Pubkey>>;

pub trait SecondaryIndexEntry: Debug {
    fn insert_if_not_exists(&self, key: &Pubkey);
    // Removes a value from the set. Returns whether the value was present in the set.
    fn remove_inner_key(&self, key: &Pubkey) -> bool;
    fn is_empty(&self) -> bool;
    fn keys(&self) -> Vec<Pubkey>;
    fn len(&self) -> usize;
}

#[derive(Debug, Default)]
pub struct DashMapSecondaryIndexEntry {
    account_keys: DashMap<Pubkey, ()>,
}

impl SecondaryIndexEntry for DashMapSecondaryIndexEntry {
    fn insert_if_not_exists(&self, key: &Pubkey) {
        if self.account_keys.get(key).is_none() {
            self.account_keys.entry(*key).or_default();
        }
    }

    fn remove_inner_key(&self, key: &Pubkey) -> bool {
        self.account_keys.remove(key).is_some()
    }

    fn is_empty(&self) -> bool {
        self.account_keys.is_empty()
    }

    fn keys(&self) -> Vec<Pubkey> {
        self.account_keys
            .iter()
            .map(|entry_ref| *entry_ref.key())
            .collect()
    }

    fn len(&self) -> usize {
        self.account_keys.len()
    }
}

#[derive(Debug, Default)]
pub struct RwLockSecondaryIndexEntry {
    account_keys: RwLock<HashSet<Pubkey>>,
}

impl SecondaryIndexEntry for RwLockSecondaryIndexEntry {
    fn insert_if_not_exists(&self, key: &Pubkey) {
        let exists = self.account_keys.read().unwrap().contains(key);
        if !exists {
            self.account_keys.write().unwrap().insert(*key);
        };
    }

    fn remove_inner_key(&self, key: &Pubkey) -> bool {
        self.account_keys.write().unwrap().remove(key)
    }

    fn is_empty(&self) -> bool {
        self.account_keys.read().unwrap().is_empty()
    }

    fn keys(&self) -> Vec<Pubkey> {
        self.account_keys.read().unwrap().iter().cloned().collect()
    }

    fn len(&self) -> usize {
        self.account_keys.read().unwrap().len()
    }
}

#[derive(Debug, Default)]
pub struct SecondaryIndex<SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send> {
    // Map from index keys to index values
    pub index: DashMap<Pubkey, SecondaryIndexEntryType>,
    pub reverse_index: DashMap<Pubkey, SecondaryReverseIndexEntry>,
}

impl<SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send>
    SecondaryIndex<SecondaryIndexEntryType>
{
    pub fn insert(&self, key: &Pubkey, inner_key: &Pubkey) {
        {
            let pubkeys_map = self.index.get(key).unwrap_or_else(|| {
                self.index
                    .entry(*key)
                    .or_insert(SecondaryIndexEntryType::default())
                    .downgrade()
            });

            pubkeys_map.insert_if_not_exists(inner_key);
        }

        let outer_keys = self.reverse_index.get(inner_key).unwrap_or_else(|| {
            self.reverse_index
                .entry(*inner_key)
                .or_insert(RwLock::new(Vec::with_capacity(1)))
                .downgrade()
        });

        let should_insert = !outer_keys.read().unwrap().contains(&key);
        if should_insert {
            let mut w_outer_keys = outer_keys.write().unwrap();
            if !w_outer_keys.contains(&key) {
                w_outer_keys.push(*key);
            }
        }
    }

    // Only safe to call from `remove_by_inner_key()` due to asserts
    fn remove_index_entries(&self, outer_key: &Pubkey, removed_inner_key: &Pubkey) {
        let is_outer_key_empty = {
            let inner_key_map = self
                .index
                .get_mut(&outer_key)
                .expect("If we're removing a key, then it must have an entry in the map");
            // If we deleted a pubkey from the reverse_index, then the corresponding entry
            // better exist in this index as well or the two indexes are out of sync!
            assert!(inner_key_map.value().remove_inner_key(&removed_inner_key));
            inner_key_map.is_empty()
        };

        // Delete the `key` if the set of inner keys is empty
        if is_outer_key_empty {
            // Other threads may have interleaved writes to this `key`,
            // so double-check again for its emptiness
            if let Occupied(key_entry) = self.index.entry(*outer_key) {
                if key_entry.get().is_empty() {
                    key_entry.remove();
                }
            }
        }
    }

    pub fn remove_by_inner_key(&self, inner_key: &Pubkey) {
        // Save off which keys in `self.index` had slots removed so we can remove them
        // after we purge the reverse index
        let mut removed_outer_keys: HashSet<Pubkey> = HashSet::new();

        // Check if the entry for `inner_key` in the reverse index is empty
        // and can be removed
        if let Some((_, outer_keys_set)) = self.reverse_index.remove(inner_key) {
            for removed_outer_key in outer_keys_set.into_inner().unwrap().into_iter() {
                removed_outer_keys.insert(removed_outer_key);
            }
        }

        // Remove this value from those keys
        for outer_key in removed_outer_keys {
            self.remove_index_entries(&outer_key, inner_key);
        }
    }

    pub fn get(&self, key: &Pubkey) -> Vec<Pubkey> {
        if let Some(inner_keys_map) = self.index.get(key) {
            inner_keys_map.keys()
        } else {
            vec![]
        }
    }
}
