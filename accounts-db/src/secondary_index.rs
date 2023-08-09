use {
    dashmap::{mapref::entry::Entry::Occupied, DashMap},
    log::*,
    solana_sdk::{pubkey::Pubkey, timing::AtomicInterval},
    std::{
        collections::HashSet,
        fmt::Debug,
        sync::{
            atomic::{AtomicU64, Ordering},
            RwLock,
        },
    },
};

pub const MAX_NUM_LARGEST_INDEX_KEYS_RETURNED: usize = 20;
pub const NUM_LARGEST_INDEX_KEYS_CACHED: usize = 200;

// The only cases where an inner key should map to a different outer key is
// if the key had different account data for the indexed key across different
// slots. As this is rare, it should be ok to use a Vec here over a HashSet, even
// though we are running some key existence checks.
pub type SecondaryReverseIndexEntry = RwLock<Vec<Pubkey>>;

pub trait SecondaryIndexEntry: Debug {
    fn insert_if_not_exists(&self, key: &Pubkey, inner_keys_count: &AtomicU64);
    // Removes a value from the set. Returns whether the value was present in the set.
    fn remove_inner_key(&self, key: &Pubkey) -> bool;
    fn is_empty(&self) -> bool;
    fn keys(&self) -> Vec<Pubkey>;
    fn len(&self) -> usize;
}

#[derive(Debug, Default)]
pub struct SecondaryIndexStats {
    last_report: AtomicInterval,
    num_inner_keys: AtomicU64,
}

#[derive(Debug, Default)]
pub struct DashMapSecondaryIndexEntry {
    account_keys: DashMap<Pubkey, ()>,
}

impl SecondaryIndexEntry for DashMapSecondaryIndexEntry {
    fn insert_if_not_exists(&self, key: &Pubkey, inner_keys_count: &AtomicU64) {
        if self.account_keys.get(key).is_none() {
            self.account_keys.entry(*key).or_insert_with(|| {
                inner_keys_count.fetch_add(1, Ordering::Relaxed);
            });
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
    fn insert_if_not_exists(&self, key: &Pubkey, inner_keys_count: &AtomicU64) {
        let exists = self.account_keys.read().unwrap().contains(key);
        if !exists {
            let mut w_account_keys = self.account_keys.write().unwrap();
            w_account_keys.insert(*key);
            inner_keys_count.fetch_add(1, Ordering::Relaxed);
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
struct HierarchicalOrderedMap<K, V>
where
    K: Default + PartialEq + Ord + Clone,
    V: Default + PartialEq + Ord + Clone,
{
    capacity: usize,
    map: Vec<(K, V)>,
}

impl<K, V> HierarchicalOrderedMap<K, V>
where
    K: Default + PartialEq + Ord + Clone,
    V: Default + PartialEq + Ord + Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            map: Vec::new(),
        }
    }
    fn get_map(&self) -> &Vec<(K, V)> {
        &self.map
    }
    fn sort_slice_by_value(&mut self, slice_key: &K) {
        // Obtain a slice of mutable references to all elements with the same key
        for sub_slice in self.map.split_mut(|(k, _)| k != slice_key) {
            // Sort them
            if !sub_slice.is_empty() {
                sub_slice.sort_unstable_by_key(|(_, v)| v.clone());
            }
        }
    }
    fn update_map(&mut self, key: &K, value: &V) {
        // Check if the value already exists.
        let existing_value_position = self.map.iter().position(|(_, y)| y == value);
        // Remove it if it does.
        // Note: Removal maintains sorted order, updating would require a re-sort.
        // Thus, since we have to search to find the new position anyways,
        // just throw it away and re-insert as if its a new element.
        if let Some(position) = existing_value_position {
            self.map.remove(position);
        }
        // If its a new value...
        else {
            // Check if the list is full, and if the key is less than the smallest element, if so exit early.
            if self.map.len() >= self.capacity && self.map[0].0 > *key {
                return;
            }
        };
        // Find where the new entry goes and insert it.
        // Also report if there are more elements in the list with the same key => they need sorting.
        let (key_position, needs_sort) =
            match self.map.binary_search_by_key(key, |(k, _)| k.clone()) {
                Ok(found_position) => (found_position, true),
                Err(woudbe_position) => (woudbe_position, false),
            };
        self.map.insert(key_position, (key.clone(), value.clone()));
        // If there were indeed more elements with the same key sort them by value
        if needs_sort {
            self.sort_slice_by_value(key);
        }
        // Prune list if too big
        while self.map.len() > self.capacity {
            self.map.remove(0);
        }
    }
}

#[derive(Debug)]
pub struct SecondaryIndexLargestKeys(RwLock<HierarchicalOrderedMap<usize, Pubkey>>);
impl Default for SecondaryIndexLargestKeys {
    fn default() -> Self {
        let container = HierarchicalOrderedMap::<usize, Pubkey>::new(NUM_LARGEST_INDEX_KEYS_CACHED);
        SecondaryIndexLargestKeys(RwLock::new(container))
    }
}
impl SecondaryIndexLargestKeys {
    pub fn get_largest_keys(&self, max_entries: usize) -> Vec<(usize, Pubkey)> {
        // Obtain the shared resource.
        let largest_key_list = self.0.read().unwrap();
        // Collect elements into a vector.
        let num_entries = std::cmp::min(MAX_NUM_LARGEST_INDEX_KEYS_RETURNED, max_entries);
        largest_key_list
            .get_map()
            .iter()
            .rev()
            .take(num_entries)
            .copied()
            .collect::<Vec<(usize, Pubkey)>>()
    }
    pub fn update(&self, key_size: &usize, pubkey: &Pubkey) {
        // Obtain the shared resource.
        let mut largest_key_list = self.0.write().unwrap();
        // Update the list
        largest_key_list.update_map(key_size, pubkey);
    }
}

#[derive(Debug, Default)]
pub struct SecondaryIndex<SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send> {
    metrics_name: &'static str,
    // Map from index keys to index values
    pub index: DashMap<Pubkey, SecondaryIndexEntryType>,
    pub reverse_index: DashMap<Pubkey, SecondaryReverseIndexEntry>,
    pub key_size_index: SecondaryIndexLargestKeys,
    stats: SecondaryIndexStats,
}

impl<SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send>
    SecondaryIndex<SecondaryIndexEntryType>
{
    pub fn new(metrics_name: &'static str) -> Self {
        Self {
            metrics_name,
            ..Self::default()
        }
    }

    pub fn insert(&self, key: &Pubkey, inner_key: &Pubkey) {
        {
            let pubkeys_map = self
                .index
                .get(key)
                .unwrap_or_else(|| self.index.entry(*key).or_default().downgrade());

            let key_size_cache = pubkeys_map.len();
            pubkeys_map.insert_if_not_exists(inner_key, &self.stats.num_inner_keys);
            if key_size_cache != pubkeys_map.len() {
                self.key_size_index.update(&pubkeys_map.len(), key);
            }
        }

        {
            let outer_keys = self.reverse_index.get(inner_key).unwrap_or_else(|| {
                self.reverse_index
                    .entry(*inner_key)
                    .or_insert(RwLock::new(Vec::with_capacity(1)))
                    .downgrade()
            });

            let should_insert = !outer_keys.read().unwrap().contains(key);
            if should_insert {
                let mut w_outer_keys = outer_keys.write().unwrap();
                if !w_outer_keys.contains(key) {
                    w_outer_keys.push(*key);
                }
            }
        }

        if self.stats.last_report.should_update(1000) {
            datapoint_info!(
                self.metrics_name,
                ("num_secondary_keys", self.index.len() as i64, i64),
                (
                    "num_inner_keys",
                    self.stats.num_inner_keys.load(Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "num_reverse_index_keys",
                    self.reverse_index.len() as i64,
                    i64
                ),
            );
        }
    }

    // Only safe to call from `remove_by_inner_key()` due to asserts
    fn remove_index_entries(&self, outer_key: &Pubkey, removed_inner_key: &Pubkey) {
        let is_outer_key_empty = {
            let inner_key_map = self
                .index
                .get_mut(outer_key)
                .expect("If we're removing a key, then it must have an entry in the map");
            // If we deleted a pubkey from the reverse_index, then the corresponding entry
            // better exist in this index as well or the two indexes are out of sync!
            assert!(inner_key_map.value().remove_inner_key(removed_inner_key));
            self.key_size_index.update(&inner_key_map.len(), outer_key);
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
        for outer_key in &removed_outer_keys {
            self.remove_index_entries(outer_key, inner_key);
        }

        // Safe to `fetch_sub()` here because a dead key cannot be removed more than once,
        // and the `num_inner_keys` must have been incremented by exactly removed_outer_keys.len()
        // in previous unique insertions of `inner_key` into `self.index` for each key
        // in `removed_outer_keys`
        self.stats
            .num_inner_keys
            .fetch_sub(removed_outer_keys.len() as u64, Ordering::Relaxed);
    }

    pub fn get(&self, key: &Pubkey) -> Vec<Pubkey> {
        if let Some(inner_keys_map) = self.index.get(key) {
            inner_keys_map.keys()
        } else {
            vec![]
        }
    }

    /// log top 20 (owner, # accounts) in descending order of # accounts
    pub fn log_contents(&self) {
        let mut entries = self
            .index
            .iter()
            .map(|entry| (entry.value().len(), *entry.key()))
            .collect::<Vec<_>>();
        entries.sort_unstable();
        entries
            .iter()
            .rev()
            .take(20)
            .for_each(|(v, k)| info!("owner: {}, accounts: {}", k, v));
    }
}
