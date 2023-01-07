use {
    crate::ancestors::Ancestors,
    log::*,
    rand::{thread_rng, Rng},
    serde::Serialize,
    solana_sdk::{
        clock::{Slot, MAX_RECENT_BLOCKHASHES},
        hash::Hash,
    },
    std::{
        collections::{hash_map::Entry, HashMap, HashSet},
        sync::{Arc, Mutex},
    },
};

pub const MAX_CACHE_ENTRIES: usize = MAX_RECENT_BLOCKHASHES;
const CACHED_KEY_SIZE: usize = 20;

// Store forks in a single chunk of memory to avoid another lookup.
pub type ForkStatus<T> = Vec<(Slot, T)>;
type KeySlice = [u8; CACHED_KEY_SIZE];
type KeyMap<T> = HashMap<KeySlice, ForkStatus<T>>;
// Map of Hash and status
pub type Status<T> = Arc<Mutex<HashMap<Hash, (usize, Vec<(KeySlice, T)>)>>>;
// A Map of hash + the highest fork it's been observed on along with
// the key offset and a Map of the key slice + Fork status for that key
type KeyStatusMap<T> = HashMap<Hash, (Slot, usize, KeyMap<T>)>;

// A map of keys recorded in each fork; used to serialize for snapshots easily.
// Doesn't store a `SlotDelta` in it because the bool `root` is usually set much later
type SlotDeltaMap<T> = HashMap<Slot, Status<T>>;

// The statuses added during a slot, can be used to build on top of a status cache or to
// construct a new one. Usually derived from a status cache's `SlotDeltaMap`
pub type SlotDelta<T> = (Slot, bool, Status<T>);

#[derive(Clone, Debug, AbiExample)]
pub struct StatusCache<T: Serialize + Clone> {
    cache: KeyStatusMap<T>,
    roots: HashSet<Slot>,
    /// all keys seen during a fork/slot
    slot_deltas: SlotDeltaMap<T>,
}

impl<T: Serialize + Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self {
            cache: HashMap::default(),
            // 0 is always a root
            roots: HashSet::from([0]),
            slot_deltas: HashMap::default(),
        }
    }
}

impl<T: Serialize + Clone + PartialEq> PartialEq for StatusCache<T> {
    fn eq(&self, other: &Self) -> bool {
        self.roots == other.roots
            && self
                .cache
                .iter()
                .all(|(hash, (slot, key_index, hash_map))| {
                    if let Some((other_slot, other_key_index, other_hash_map)) =
                        other.cache.get(hash)
                    {
                        if slot == other_slot && key_index == other_key_index {
                            return hash_map.iter().all(|(slice, fork_map)| {
                                if let Some(other_fork_map) = other_hash_map.get(slice) {
                                    // all this work just to compare the highest forks in the fork map
                                    // per entry
                                    return fork_map.last() == other_fork_map.last();
                                }
                                false
                            });
                        }
                    }
                    false
                })
    }
}

impl<T: Serialize + Clone> StatusCache<T> {
    pub fn clear_slot_entries(&mut self, slot: Slot) {
        let slot_deltas = self.slot_deltas.remove(&slot);
        if let Some(slot_deltas) = slot_deltas {
            let slot_deltas = slot_deltas.lock().unwrap();
            for (blockhash, (_, key_list)) in slot_deltas.iter() {
                // Any blockhash that exists in self.slot_deltas must also exist
                // in self.cache, because in self.purge_roots(), when an entry
                // (b, (max_slot, _, _)) is removed from self.cache, this implies
                // all entries in self.slot_deltas < max_slot are also removed
                if let Entry::Occupied(mut o_blockhash_entries) = self.cache.entry(*blockhash) {
                    let (_, _, all_hash_maps) = o_blockhash_entries.get_mut();

                    for (key_slice, _) in key_list {
                        if let Entry::Occupied(mut o_key_list) = all_hash_maps.entry(*key_slice) {
                            let key_list = o_key_list.get_mut();
                            key_list.retain(|(updated_slot, _)| *updated_slot != slot);
                            if key_list.is_empty() {
                                o_key_list.remove_entry();
                            }
                        } else {
                            panic!(
                                "Map for key must exist if key exists in self.slot_deltas, slot: {slot}"
                            )
                        }
                    }

                    if all_hash_maps.is_empty() {
                        o_blockhash_entries.remove_entry();
                    }
                } else {
                    panic!("Blockhash must exist if it exists in self.slot_deltas, slot: {slot}")
                }
            }
        }
    }

    /// Check if the key is in any of the forks in the ancestors set and
    /// with a certain blockhash.
    pub fn get_status<K: AsRef<[u8]>>(
        &self,
        key: K,
        transaction_blockhash: &Hash,
        ancestors: &Ancestors,
    ) -> Option<(Slot, T)> {
        let map = self.cache.get(transaction_blockhash)?;
        let (_, index, keymap) = map;
        let max_key_index = key.as_ref().len().saturating_sub(CACHED_KEY_SIZE + 1);
        let index = (*index).min(max_key_index);
        let key_slice: &[u8; CACHED_KEY_SIZE] =
            arrayref::array_ref![key.as_ref(), index, CACHED_KEY_SIZE];
        if let Some(stored_forks) = keymap.get(key_slice) {
            let res = stored_forks
                .iter()
                .find(|(f, _)| ancestors.contains_key(f) || self.roots.get(f).is_some())
                .cloned();
            if res.is_some() {
                return res;
            }
        }
        None
    }

    /// Search for a key with any blockhash
    /// Prefer get_status for performance reasons, it doesn't need
    /// to search all blockhashes.
    pub fn get_status_any_blockhash<K: AsRef<[u8]>>(
        &self,
        key: K,
        ancestors: &Ancestors,
    ) -> Option<(Slot, T)> {
        let keys: Vec<_> = self.cache.keys().copied().collect();

        for blockhash in keys.iter() {
            trace!("get_status_any_blockhash: trying {}", blockhash);
            let status = self.get_status(&key, blockhash, ancestors);
            if status.is_some() {
                return status;
            }
        }
        None
    }

    /// Add a known root fork.  Roots are always valid ancestors.
    /// After MAX_CACHE_ENTRIES, roots are removed, and any old keys are cleared.
    pub fn add_root(&mut self, fork: Slot) {
        self.roots.insert(fork);
        self.purge_roots();
    }

    pub fn roots(&self) -> &HashSet<Slot> {
        &self.roots
    }

    /// Insert a new key for a specific slot.
    pub fn insert<K: AsRef<[u8]>>(
        &mut self,
        transaction_blockhash: &Hash,
        key: K,
        slot: Slot,
        res: T,
    ) {
        let max_key_index = key.as_ref().len().saturating_sub(CACHED_KEY_SIZE + 1);
        let hash_map = self.cache.entry(*transaction_blockhash).or_insert_with(|| {
            let key_index = thread_rng().gen_range(0, max_key_index + 1);
            (slot, key_index, HashMap::new())
        });

        hash_map.0 = std::cmp::max(slot, hash_map.0);
        let key_index = hash_map.1.min(max_key_index);
        let mut key_slice = [0u8; CACHED_KEY_SIZE];
        key_slice.clone_from_slice(&key.as_ref()[key_index..key_index + CACHED_KEY_SIZE]);
        self.insert_with_slice(transaction_blockhash, slot, key_index, key_slice, res);
    }

    pub fn purge_roots(&mut self) {
        if self.roots.len() > MAX_CACHE_ENTRIES {
            if let Some(min) = self.roots.iter().min().cloned() {
                self.roots.remove(&min);
                self.cache.retain(|_, (fork, _, _)| *fork > min);
                self.slot_deltas.retain(|slot, _| *slot > min);
            }
        }
    }

    /// Clear for testing
    pub fn clear(&mut self) {
        for v in self.cache.values_mut() {
            v.2 = HashMap::new();
        }

        self.slot_deltas
            .iter_mut()
            .for_each(|(_, status)| status.lock().unwrap().clear());
    }

    /// Get the statuses for all the root slots
    pub fn root_slot_deltas(&self) -> Vec<SlotDelta<T>> {
        self.roots()
            .iter()
            .map(|root| {
                (
                    *root,
                    true, // <-- is_root
                    self.slot_deltas.get(root).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    // replay deltas into a status_cache allows "appending" data
    pub fn append(&mut self, slot_deltas: &[SlotDelta<T>]) {
        for (slot, is_root, statuses) in slot_deltas {
            statuses
                .lock()
                .unwrap()
                .iter()
                .for_each(|(tx_hash, (key_index, statuses))| {
                    for (key_slice, res) in statuses.iter() {
                        self.insert_with_slice(tx_hash, *slot, *key_index, *key_slice, res.clone())
                    }
                });
            if *is_root {
                self.add_root(*slot);
            }
        }
    }

    pub fn from_slot_deltas(slot_deltas: &[SlotDelta<T>]) -> Self {
        // play all deltas back into the status cache
        let mut me = Self::default();
        me.append(slot_deltas);
        me
    }

    fn insert_with_slice(
        &mut self,
        transaction_blockhash: &Hash,
        slot: Slot,
        key_index: usize,
        key_slice: [u8; CACHED_KEY_SIZE],
        res: T,
    ) {
        let hash_map =
            self.cache
                .entry(*transaction_blockhash)
                .or_insert((slot, key_index, HashMap::new()));
        hash_map.0 = std::cmp::max(slot, hash_map.0);

        let forks = hash_map.2.entry(key_slice).or_insert_with(Vec::new);
        forks.push((slot, res.clone()));
        let slot_deltas = self.slot_deltas.entry(slot).or_default();
        let mut fork_entry = slot_deltas.lock().unwrap();
        let (_, hash_entry) = fork_entry
            .entry(*transaction_blockhash)
            .or_insert((key_index, vec![]));
        hash_entry.push((key_slice, res))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{hash::hash, signature::Signature},
    };

    type BankStatusCache = StatusCache<()>;

    #[test]
    fn test_empty_has_no_sigs() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let status_cache = BankStatusCache::default();
        assert_eq!(
            status_cache.get_status(sig, &blockhash, &Ancestors::default()),
            None
        );
        assert_eq!(
            status_cache.get_status_any_blockhash(sig, &Ancestors::default()),
            None
        );
    }

    #[test]
    fn test_find_sig_with_ancestor_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = vec![(0, 1)].into_iter().collect();
        status_cache.insert(&blockhash, sig, 0, ());
        assert_eq!(
            status_cache.get_status(sig, &blockhash, &ancestors),
            Some((0, ()))
        );
        assert_eq!(
            status_cache.get_status_any_blockhash(sig, &ancestors),
            Some((0, ()))
        );
    }

    #[test]
    fn test_find_sig_without_ancestor_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = Ancestors::default();
        status_cache.insert(&blockhash, sig, 1, ());
        assert_eq!(status_cache.get_status(sig, &blockhash, &ancestors), None);
        assert_eq!(status_cache.get_status_any_blockhash(sig, &ancestors), None);
    }

    #[test]
    fn test_find_sig_with_root_ancestor_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = Ancestors::default();
        status_cache.insert(&blockhash, sig, 0, ());
        status_cache.add_root(0);
        assert_eq!(
            status_cache.get_status(sig, &blockhash, &ancestors),
            Some((0, ()))
        );
    }

    #[test]
    fn test_insert_picks_latest_blockhash_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = vec![(0, 0)].into_iter().collect();
        status_cache.insert(&blockhash, sig, 0, ());
        status_cache.insert(&blockhash, sig, 1, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors)
            .is_some());
    }

    #[test]
    fn test_root_expires() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = Ancestors::default();
        status_cache.insert(&blockhash, sig, 0, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        assert_eq!(status_cache.get_status(sig, &blockhash, &ancestors), None);
    }

    #[test]
    fn test_clear_signatures_sigs_are_gone() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = Ancestors::default();
        status_cache.insert(&blockhash, sig, 0, ());
        status_cache.add_root(0);
        status_cache.clear();
        assert_eq!(status_cache.get_status(sig, &blockhash, &ancestors), None);
    }

    #[test]
    fn test_clear_signatures_insert_works() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = Ancestors::default();
        status_cache.add_root(0);
        status_cache.clear();
        status_cache.insert(&blockhash, sig, 0, ());
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors)
            .is_some());
    }

    #[test]
    fn test_signatures_slice() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear();
        status_cache.insert(&blockhash, sig, 0, ());
        let (_, index, sig_map) = status_cache.cache.get(&blockhash).unwrap();
        let sig_slice: &[u8; CACHED_KEY_SIZE] =
            arrayref::array_ref![sig.as_ref(), *index, CACHED_KEY_SIZE];
        assert!(sig_map.get(sig_slice).is_some());
    }

    #[test]
    fn test_slot_deltas() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear();
        status_cache.insert(&blockhash, sig, 0, ());
        assert!(status_cache.roots().contains(&0));
        let slot_deltas = status_cache.root_slot_deltas();
        let cache = StatusCache::from_slot_deltas(&slot_deltas);
        assert_eq!(cache, status_cache);
        let slot_deltas = cache.root_slot_deltas();
        let cache = StatusCache::from_slot_deltas(&slot_deltas);
        assert_eq!(cache, status_cache);
    }

    #[test]
    fn test_roots_deltas() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let blockhash2 = hash(blockhash.as_ref());
        status_cache.insert(&blockhash, sig, 0, ());
        status_cache.insert(&blockhash, sig, 1, ());
        status_cache.insert(&blockhash2, sig, 1, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        assert_eq!(status_cache.slot_deltas.len(), 1);
        assert!(status_cache.slot_deltas.get(&1).is_some());
        let slot_deltas = status_cache.root_slot_deltas();
        let cache = StatusCache::from_slot_deltas(&slot_deltas);
        assert_eq!(cache, status_cache);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_age_sanity() {
        assert!(MAX_CACHE_ENTRIES <= MAX_RECENT_BLOCKHASHES);
    }

    #[test]
    fn test_clear_slot_signatures() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let blockhash2 = hash(blockhash.as_ref());
        status_cache.insert(&blockhash, sig, 0, ());
        status_cache.insert(&blockhash, sig, 1, ());
        status_cache.insert(&blockhash2, sig, 1, ());

        let mut ancestors0 = Ancestors::default();
        ancestors0.insert(0, 0);
        let mut ancestors1 = Ancestors::default();
        ancestors1.insert(1, 0);

        // Clear slot 0 related data
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors0)
            .is_some());
        status_cache.clear_slot_entries(0);
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors0)
            .is_none());
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors1)
            .is_some());
        assert!(status_cache
            .get_status(sig, &blockhash2, &ancestors1)
            .is_some());

        // Check that the slot delta for slot 0 is gone, but slot 1 still
        // exists
        assert!(status_cache.slot_deltas.get(&0).is_none());
        assert!(status_cache.slot_deltas.get(&1).is_some());

        // Clear slot 1 related data
        status_cache.clear_slot_entries(1);
        assert!(status_cache.slot_deltas.is_empty());
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors1)
            .is_none());
        assert!(status_cache
            .get_status(sig, &blockhash2, &ancestors1)
            .is_none());
        assert!(status_cache.cache.is_empty());
    }

    // Status cache uses a random key offset for each blockhash. Ensure that shorter
    // keys can still be used if the offset if greater than the key length.
    #[test]
    fn test_different_sized_keys() {
        let mut status_cache = BankStatusCache::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let blockhash = Hash::default();
        for _ in 0..100 {
            let blockhash = hash(blockhash.as_ref());
            let sig_key = Signature::default();
            let hash_key = Hash::new_unique();
            status_cache.insert(&blockhash, sig_key, 0, ());
            status_cache.insert(&blockhash, hash_key, 0, ());
            assert!(status_cache
                .get_status(sig_key, &blockhash, &ancestors)
                .is_some());
            assert!(status_cache
                .get_status(hash_key, &blockhash, &ancestors)
                .is_some());
        }
    }
}
