use dashmap::{mapref::entry::Entry::Occupied, DashMap};
use log::*;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::{
    collections::{hash_map, HashMap, HashSet},
    fmt::Debug,
    sync::{Arc, RwLock},
};

pub type SecondaryReverseIndexEntry = RwLock<HashMap<Slot, Pubkey>>;

pub trait SecondaryIndexEntry: Debug {
    fn get_or_create(&self, key: &Pubkey, f: &dyn Fn(&RwLock<HashSet<Slot>>));
    fn get<T>(&self, key: &Pubkey, f: &dyn Fn(Option<&RwLock<HashSet<Slot>>>) -> T) -> T;
    fn remove_key_if_empty(&self, key: &Pubkey);
    fn is_empty(&self) -> bool;
    fn keys(&self) -> Vec<Pubkey>;
    fn len(&self) -> usize;
}

#[derive(Debug, Default)]
pub struct DashMapSecondaryIndexEntry {
    pubkey_to_slot_set: DashMap<Pubkey, RwLock<HashSet<Slot>>>,
}

impl SecondaryIndexEntry for DashMapSecondaryIndexEntry {
    fn get_or_create(&self, key: &Pubkey, f: &dyn Fn(&RwLock<HashSet<Slot>>)) {
        let slot_set = self.pubkey_to_slot_set.get(key).unwrap_or_else(|| {
            self.pubkey_to_slot_set
                .entry(*key)
                .or_insert(RwLock::new(HashSet::new()))
                .downgrade()
        });

        f(&slot_set)
    }

    fn get<T>(&self, key: &Pubkey, f: &dyn Fn(Option<&RwLock<HashSet<Slot>>>) -> T) -> T {
        let slot_set = self.pubkey_to_slot_set.get(key);

        f(slot_set.as_ref().map(|entry_ref| entry_ref.value()))
    }

    fn remove_key_if_empty(&self, key: &Pubkey) {
        if let Occupied(key_entry) = self.pubkey_to_slot_set.entry(*key) {
            // Delete the `key` if the slot set is empty
            let slot_set = key_entry.get();

            // Write lock on `key_entry` above through the `entry`
            // means nobody else has access to this lock at this time,
            // so this check for empty -> remove() is atomic
            if slot_set.read().unwrap().is_empty() {
                key_entry.remove();
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.pubkey_to_slot_set.is_empty()
    }

    fn keys(&self) -> Vec<Pubkey> {
        self.pubkey_to_slot_set
            .iter()
            .map(|entry_ref| *entry_ref.key())
            .collect()
    }

    fn len(&self) -> usize {
        self.pubkey_to_slot_set.len()
    }
}

#[derive(Debug, Default)]
pub struct RwLockSecondaryIndexEntry {
    pubkey_to_slot_set: RwLock<HashMap<Pubkey, Arc<RwLock<HashSet<Slot>>>>>,
}

impl SecondaryIndexEntry for RwLockSecondaryIndexEntry {
    fn get_or_create(&self, key: &Pubkey, f: &dyn Fn(&RwLock<HashSet<Slot>>)) {
        let slot_set = self.pubkey_to_slot_set.read().unwrap().get(key).cloned();

        let slot_set = {
            if let Some(slot_set) = slot_set {
                slot_set
            } else {
                self.pubkey_to_slot_set
                    .write()
                    .unwrap()
                    .entry(*key)
                    .or_insert_with(|| Arc::new(RwLock::new(HashSet::new())))
                    .clone()
            }
        };

        f(&slot_set)
    }

    fn get<T>(&self, key: &Pubkey, f: &dyn Fn(Option<&RwLock<HashSet<Slot>>>) -> T) -> T {
        let slot_set = self.pubkey_to_slot_set.read().unwrap().get(key).cloned();
        f(slot_set.as_deref())
    }

    fn remove_key_if_empty(&self, key: &Pubkey) {
        if let hash_map::Entry::Occupied(key_entry) =
            self.pubkey_to_slot_set.write().unwrap().entry(*key)
        {
            // Delete the `key` if the slot set is empty
            let slot_set = key_entry.get();

            // Write lock on `key_entry` above through the `entry`
            // means nobody else has access to this lock at this time,
            // so this check for empty -> remove() is atomic
            if slot_set.read().unwrap().is_empty() {
                key_entry.remove();
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.pubkey_to_slot_set.read().unwrap().is_empty()
    }

    fn keys(&self) -> Vec<Pubkey> {
        self.pubkey_to_slot_set
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    fn len(&self) -> usize {
        self.pubkey_to_slot_set.read().unwrap().len()
    }
}

#[derive(Debug, Default)]
pub struct SecondaryIndex<SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send> {
    // Map from index keys to index values
    pub index: DashMap<Pubkey, SecondaryIndexEntryType>,
    // Map from index values back to index keys, used for cleanup.
    // Alternative is to store Option<Pubkey> in each AccountInfo in the
    // AccountsIndex if something is an SPL account with a mint, but then
    // every AccountInfo would have to allocate `Option<Pubkey>`
    pub reverse_index: DashMap<Pubkey, SecondaryReverseIndexEntry>,
}

impl<SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send>
    SecondaryIndex<SecondaryIndexEntryType>
{
    pub fn insert(&self, key: &Pubkey, inner_key: &Pubkey, slot: Slot) {
        {
            let pubkeys_map = self.index.get(key).unwrap_or_else(|| {
                self.index
                    .entry(*key)
                    .or_insert(SecondaryIndexEntryType::default())
                    .downgrade()
            });

            pubkeys_map.get_or_create(inner_key, &|slots_set: &RwLock<HashSet<Slot>>| {
                let contains_key = slots_set.read().unwrap().contains(&slot);
                if !contains_key {
                    slots_set.write().unwrap().insert(slot);
                }
            });
        }

        let prev_key = {
            let slots_map = self.reverse_index.get(inner_key).unwrap_or_else(|| {
                self.reverse_index
                    .entry(*inner_key)
                    .or_insert(RwLock::new(HashMap::new()))
                    .downgrade()
            });
            let should_insert = {
                // Most of the time, key should already exist and match
                // the one in the update
                if let Some(existing_key) = slots_map.read().unwrap().get(&slot) {
                    existing_key != key
                } else {
                    // If there is no key yet, then insert
                    true
                }
            };
            if should_insert {
                slots_map.write().unwrap().insert(slot, *key)
            } else {
                None
            }
        };

        if let Some(prev_key) = prev_key {
            // If the inner key was moved to a different primary key, remove
            // the previous index entry.

            // Check is necessary because anoher thread's writes could feasibly be
            // interleaved between  `should_insert = { ... slots_map.get(...) ... }` and
            // `prev_key = { ... slots_map.insert(...) ... }`
            // Currently this isn't possible due to current AccountsIndex's (pubkey, slot)-per-thread
            // exclusive-locking, but check is here for future-proofing a more relaxed implementation
            if prev_key != *key {
                self.remove_index_entries(&prev_key, inner_key, &[slot]);
            }
        }
    }

    pub fn remove_index_entries(&self, key: &Pubkey, inner_key: &Pubkey, slots: &[Slot]) {
        let is_key_empty = if let Some(inner_key_map) = self.index.get(&key) {
            // Delete the slot from the slot set
            let is_inner_key_empty =
                inner_key_map.get(&inner_key, &|slot_set: Option<&RwLock<HashSet<Slot>>>| {
                    if let Some(slot_set) = slot_set {
                        let mut w_slot_set = slot_set.write().unwrap();
                        for slot in slots.iter() {
                            let is_present = w_slot_set.remove(slot);
                            if !is_present {
                                warn!("Reverse index is missing previous entry for key {}, inner_key: {}, slot: {}",
                                key, inner_key, slot);
                            }
                        }
                        w_slot_set.is_empty()
                    } else {
                        false
                    }
                });

            // Check if `key` is empty
            if is_inner_key_empty {
                // Write lock on `inner_key_entry` above through the `entry`
                // means nobody else has access to this lock at this time,
                // so this check for empty -> remove() is atomic
                inner_key_map.remove_key_if_empty(inner_key);
                inner_key_map.is_empty()
            } else {
                false
            }
        } else {
            false
        };

        // Delete the `key` if the set of inner keys is empty
        if is_key_empty {
            // Other threads may have interleaved writes to this `key`,
            // so double-check again for its emptiness
            if let Occupied(key_entry) = self.index.entry(*key) {
                if key_entry.get().is_empty() {
                    key_entry.remove();
                }
            }
        }
    }

    // Specifying `slots_to_remove` == Some will only remove keys for those specific slots
    // found for the `inner_key` in the reverse index. Otherwise, passing `None`
    // will  remove all keys that are found for the `inner_key` in the reverse index.

    // Note passing `None` is dangerous unless you're sure there's no other competing threads
    // writing updates to the index for this Pubkey at the same time!
    pub fn remove_by_inner_key(&self, inner_key: &Pubkey, slots_to_remove: Option<&HashSet<Slot>>) {
        // Save off which keys in `self.index` had slots removed so we can remove them
        // after we purge the reverse index
        let mut key_to_removed_slots: HashMap<Pubkey, Vec<Slot>> = HashMap::new();

        // Check if the entry for `inner_key` in the reverse index is empty
        // and can be removed
        let needs_remove = {
            if let Some(slots_to_remove) = slots_to_remove {
                self.reverse_index
                    .get(inner_key)
                    .map(|slots_map| {
                        // Ideally we use a concurrent map here as well to prevent clean
                        // from blocking writes, but memory usage of DashMap is high
                        let mut w_slots_map = slots_map.value().write().unwrap();
                        for slot in slots_to_remove.iter() {
                            if let Some(removed_key) = w_slots_map.remove(slot) {
                                key_to_removed_slots
                                    .entry(removed_key)
                                    .or_default()
                                    .push(*slot);
                            }
                        }
                        w_slots_map.is_empty()
                    })
                    .unwrap_or(false)
            } else {
                if let Some((_, removed_slot_map)) = self.reverse_index.remove(inner_key) {
                    for (slot, removed_key) in removed_slot_map.into_inner().unwrap().into_iter() {
                        key_to_removed_slots
                            .entry(removed_key)
                            .or_default()
                            .push(slot);
                    }
                }
                // We just removed the key, no need to remove it again
                false
            }
        };

        if needs_remove {
            // Other threads may have interleaved writes to this `inner_key`, between
            // releasing the `self.reverse_index.get(inner_key)` lock and now,
            // so double-check again for emptiness
            if let Occupied(slot_map) = self.reverse_index.entry(*inner_key) {
                if slot_map.get().read().unwrap().is_empty() {
                    slot_map.remove();
                }
            }
        }

        // Remove this value from those keys
        for (key, slots) in key_to_removed_slots {
            self.remove_index_entries(&key, inner_key, &slots);
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
