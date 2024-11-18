use {
    crate::fixed_concurrent_map::FixedConcurrentMap,
    ahash::random_state::RandomState as AHashRandomState,
    dashmap::{mapref::entry::Entry, DashMap, DashSet},
    rand::{thread_rng, Rng},
    serde::Serialize,
    smallvec::SmallVec,
    solana_accounts_db::ancestors::Ancestors,
    solana_sdk::{
        clock::{Slot, MAX_RECENT_BLOCKHASHES},
        hash::Hash,
    },
    std::{
        mem::MaybeUninit,
        ptr,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    },
};

pub const MAX_CACHE_ENTRIES: usize = MAX_RECENT_BLOCKHASHES;
const CACHED_KEY_SIZE: usize = 20;
const CONCURRENT_MAP_SLOTS: usize = (MAX_CACHE_ENTRIES * 4).next_power_of_two();
const DASHMAP_SHARDS: usize = (MAX_CACHE_ENTRIES * 4).next_power_of_two();

// Store forks in a single chunk of memory to avoid another lookup.
pub type ForkStatus<T> = SmallVec<[(Slot, T); 2]>;
type KeySlice = [u8; CACHED_KEY_SIZE];
type KeyMap<T> = DashMap<KeySlice, ForkStatus<T>, AHashRandomState>;
// Map of Hash and status
pub type Status<T> = Arc<DashMap<Hash, (usize, Vec<(KeySlice, T)>), AHashRandomState>>;
// A Map of hash + the highest fork it's been observed on along with
// the key offset and a Map of the key slice + Fork status for that key
type KeyStatusMap<T> = FixedConcurrentMap<Hash, (AtomicU64, usize, KeyMap<T>)>;

// A map of keys recorded in each fork; used to serialize for snapshots easily.
// Doesn't store a `SlotDelta` in it because the bool `root` is usually set much later
type SlotDeltaMap<T> = FixedConcurrentMap<Slot, Status<T>>;

// The statuses added during a slot, can be used to build on top of a status cache or to
// construct a new one. Usually derived from a status cache's `SlotDeltaMap`
pub type SlotDelta<T> = (Slot, bool, Status<T>);

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug)]
pub struct StatusCache<T: Serialize + Clone> {
    // map[blockhash][tx_key] => [(fork1_slot, tx_result), (fork2_slot, tx_result), ...]
    // used to check if a tx_key was seen on a fork and for rpc to retrieve the tx_result
    cache: KeyStatusMap<T>,
    // set of rooted slots
    roots: DashSet<Slot, AHashRandomState>,
    // map[slot][blockhash] => [(tx_key, tx_result), ...] used to serialize for snapshots
    slot_deltas: SlotDeltaMap<T>,
}

impl<T: Serialize + Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self {
            cache: KeyStatusMap::new(CONCURRENT_MAP_SLOTS),
            // 0 is always a root
            roots: DashSet::from_iter([0].into_iter()),
            slot_deltas: SlotDeltaMap::new(CONCURRENT_MAP_SLOTS),
        }
    }
}

impl<T: Serialize + Clone> StatusCache<T> {
    /// Clear all entries for a slot.
    ///
    /// This is used when a slot is purged from the bank, see
    /// ReplayStage::purge_unconfirmed_duplicate_slot(). When this is called, it's guaranteed that
    /// there are no threads inserting new entries for this slot, so there are no races.
    pub fn clear_slot_entries(&self, slot: Slot) {
        let slot_deltas = self.slot_deltas.remove(&slot);
        if let Some(slot_deltas) = slot_deltas {
            for item in slot_deltas.iter() {
                let blockhash = item.key();
                let (_, key_list) = item.value();
                // Any blockhash that exists in self.slot_deltas must also exist
                // in self.cache, because in self.purge_roots(), when an entry
                // (b, (max_slot, _, _)) is removed from self.cache, this implies
                // all entries in self.slot_deltas < max_slot are also removed
                if let Some(guard) = self.cache.get(blockhash) {
                    let (_, _, all_hash_maps) = &*guard;

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
                        drop(guard);
                        self.cache.remove(blockhash);
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
        self.do_get_status(&*map, &key, ancestors)
    }

    fn do_get_status<K: AsRef<[u8]>>(
        &self,
        map: &(
            AtomicU64,
            usize,
            DashMap<[u8; 20], SmallVec<[(u64, T); 2]>, AHashRandomState>,
        ),
        key: &K,
        ancestors: &Ancestors,
    ) -> Option<(u64, T)> {
        let (_, index, keymap) = map;
        let max_key_index = key.as_ref().len().saturating_sub(CACHED_KEY_SIZE + 1);
        let index = (*index).min(max_key_index);
        let key_slice: &[u8; CACHED_KEY_SIZE] =
            arrayref::array_ref![key.as_ref(), index, CACHED_KEY_SIZE];
        if let Some(stored_forks) = keymap.get(key_slice) {
            let res = stored_forks
                .iter()
                .find(|(f, _)| ancestors.contains_key(f) || self.roots.contains(f))
                .cloned();
            if res.is_some() {
                return res;
            }
        }
        None
    }

    /// Search for a key with any blockhash.
    ///
    /// Prefer get_status for performance reasons, it doesn't need to search all blockhashes.
    pub fn get_status_any_blockhash<K: AsRef<[u8]>>(
        &self,
        key: K,
        ancestors: &Ancestors,
    ) -> Option<(Slot, T)> {
        for item in self.cache.iter() {
            let (_blockhash, map) = &*item;
            let status = self.do_get_status(map, &key, ancestors);
            if status.is_some() {
                return status;
            }
        }
        None
    }

    /// Add a known root fork.
    ///
    /// Roots are always valid ancestors. After MAX_CACHE_ENTRIES, roots are removed, and any old
    /// keys are cleared.
    pub fn add_root(&self, fork: Slot) {
        self.roots.insert(fork);
        self.purge_roots();
    }

    /// Get all the roots.
    pub fn roots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.roots.iter().map(|x| *x)
    }

    /// Insert a new key for a specific slot.
    pub fn insert<K: AsRef<[u8]>>(&self, transaction_blockhash: &Hash, key: K, slot: Slot, res: T) {
        let max_key_index = key.as_ref().len().saturating_sub(CACHED_KEY_SIZE + 1);
        let mut key_slice = MaybeUninit::<[u8; CACHED_KEY_SIZE]>::uninit();

        // Get the cache entry for this blockhash.
        let key_index = {
            let (max_slot, key_index, hash_map) = &*self
                .cache
                .get_or_insert_with(*transaction_blockhash, || {
                    let key_index = thread_rng().gen_range(0..max_key_index + 1);
                    (
                        AtomicU64::new(slot),
                        key_index,
                        DashMap::with_hasher_and_shard_amount(
                            AHashRandomState::default(),
                            DASHMAP_SHARDS,
                        ),
                    )
                })
                .unwrap();

            // Update the max slot observed to contain txs using this blockhash.
            max_slot.fetch_max(slot, Ordering::Relaxed);

            // Grab the key slice.
            let key_index = (*key_index).min(max_key_index);
            unsafe {
                ptr::copy_nonoverlapping(
                    key.as_ref()[key_index..key_index + CACHED_KEY_SIZE].as_ptr(),
                    key_slice.as_mut_ptr() as *mut u8,
                    CACHED_KEY_SIZE,
                )
            }

            // Insert the slot and tx result into the cache entry associated with
            // this blockhash and keyslice.
            let mut forks = hash_map
                .entry(unsafe { key_slice.assume_init() })
                .or_default();
            forks.push((slot, res.clone()));

            key_index
        };

        self.add_to_slot_delta(
            transaction_blockhash,
            slot,
            key_index,
            unsafe { key_slice.assume_init() },
            res,
        );
    }

    fn purge_roots(&self) {
        if self.roots.len() > MAX_CACHE_ENTRIES {
            if let Some(min) = self.roots().min() {
                self.roots.remove(&min);
                self.cache
                    .retain(|_, (max_slot, _, _)| max_slot.load(Ordering::Relaxed) > min);
                self.slot_deltas.retain(|slot, _| *slot > min);
            }
        }
    }

    #[cfg(feature = "dev-context-only-utils")]
    pub fn clear(&self) {
        self.cache.clear();
        self.slot_deltas.clear();
    }

    /// Get the statuses for all the root slots.
    ///
    /// This is never called concurrently with add_root(), and for a slot to be a root there must be
    /// no new entries for that slot, so there are no races.
    ///
    /// See ReplayStage::handle_new_root() => BankForks::set_root() =>
    /// BankForks::do_set_root_return_metrics() => root_slot_deltas()
    pub fn root_slot_deltas(&self) -> Vec<SlotDelta<T>> {
        self.roots()
            .map(|root| {
                (
                    root,
                    true, // <-- is_root
                    self.slot_deltas
                        .get(&root)
                        .map(|x| x.clone())
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Pupulate the cache with the slot deltas from a snapshot.
    ///
    /// Really badly named method. See load_bank_forks() => ... =>
    /// rebuild_bank_from_snapshot() => [load slot deltas from snapshot] => append()
    pub fn append(&self, slot_deltas: &[SlotDelta<T>]) {
        for (slot, is_root, statuses) in slot_deltas {
            statuses.iter().for_each(|item| {
                let tx_hash = item.key();
                let (key_index, statuses) = item.value();
                for (key_slice, res) in statuses.iter() {
                    self.insert_with_slice(tx_hash, *slot, *key_index, *key_slice, res.clone())
                }
            });
            if *is_root {
                self.add_root(*slot);
            }
        }
    }

    fn insert_with_slice(
        &self,
        transaction_blockhash: &Hash,
        slot: Slot,
        key_index: usize,
        key_slice: [u8; CACHED_KEY_SIZE],
        res: T,
    ) {
        {
            let (max_slot, _, hash_map) = &*self
                .cache
                .get_or_insert_with(*transaction_blockhash, || {
                    (
                        AtomicU64::new(slot),
                        key_index,
                        DashMap::with_hasher_and_shard_amount(
                            AHashRandomState::default(),
                            DASHMAP_SHARDS,
                        ),
                    )
                })
                .unwrap();
            max_slot.fetch_max(slot, Ordering::Relaxed);

            let mut forks = hash_map.entry(key_slice).or_default();
            forks.push((slot, res.clone()));
        }

        self.add_to_slot_delta(transaction_blockhash, slot, key_index, key_slice, res);
    }

    // Add this key slice to the list of key slices for this slot and blockhash
    // combo.
    fn add_to_slot_delta(
        &self,
        transaction_blockhash: &Hash,
        slot: Slot,
        key_index: usize,
        key_slice: [u8; CACHED_KEY_SIZE],
        res: T,
    ) {
        let fork_entry = self
            .slot_deltas
            .get_or_insert_with(slot, || {
                Arc::new(DashMap::with_hasher_and_shard_amount(
                    AHashRandomState::default(),
                    DASHMAP_SHARDS,
                ))
            })
            .unwrap();

        let (_key_index, hash_entry) = &mut *fork_entry
            .entry(*transaction_blockhash)
            .or_insert_with(|| (key_index, Vec::new()));
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

    impl<T: Serialize + Clone> StatusCache<T> {
        fn from_slot_deltas(slot_deltas: &[SlotDelta<T>]) -> Self {
            let cache = Self::default();
            cache.append(slot_deltas);
            cache
        }
    }

    impl<T: Serialize + Clone + PartialEq> PartialEq for StatusCache<T> {
        fn eq(&self, other: &Self) -> bool {
            use std::collections::HashSet;

            let roots = self.roots.iter().map(|x| *x).collect::<HashSet<_>>();
            let other_roots = other.roots.iter().map(|x| *x).collect::<HashSet<_>>();
            roots == other_roots
                && self.cache.iter().all(|item| {
                    let (hash, (max_slot, key_index, hash_map)) = &*item;
                    if let Some(item) = other.cache.get(hash) {
                        let (other_max_slot, other_key_index, other_hash_map) = &*item;
                        if max_slot.load(Ordering::Relaxed)
                            == other_max_slot.load(Ordering::Relaxed)
                            && key_index == other_key_index
                        {
                            return hash_map.iter().all(|item| {
                                let slice = item.key();
                                let fork_map = item.value();
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
        let status_cache = BankStatusCache::default();
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
        let status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = Ancestors::default();
        status_cache.insert(&blockhash, sig, 1, ());
        assert_eq!(status_cache.get_status(sig, &blockhash, &ancestors), None);
        assert_eq!(status_cache.get_status_any_blockhash(sig, &ancestors), None);
    }

    #[test]
    fn test_find_sig_with_root_ancestor_fork() {
        let sig = Signature::default();
        let status_cache = BankStatusCache::default();
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
        let status_cache = BankStatusCache::default();
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
        let status_cache = BankStatusCache::default();
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
        let status_cache = BankStatusCache::default();
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
        let status_cache = BankStatusCache::default();
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
        let status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear();
        status_cache.insert(&blockhash, sig, 0, ());
        let (_, index, sig_map) = &*status_cache.cache.get(&blockhash).unwrap();
        let sig_slice: &[u8; CACHED_KEY_SIZE] =
            arrayref::array_ref![sig.as_ref(), *index, CACHED_KEY_SIZE];
        assert!(sig_map.get(sig_slice).is_some());
    }

    #[test]
    fn test_slot_deltas() {
        let sig = Signature::default();
        let status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear();
        status_cache.insert(&blockhash, sig, 0, ());
        assert!(status_cache.roots().collect::<Vec<_>>().contains(&0));
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
        let status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let blockhash2 = hash(blockhash.as_ref());
        status_cache.insert(&blockhash, sig, 0, ());
        status_cache.insert(&blockhash, sig, 1, ());
        status_cache.insert(&blockhash2, sig, 1, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
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
        let status_cache = BankStatusCache::default();
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
        assert!(!status_cache.slot_deltas.get(&0).is_some());
        assert!(status_cache.slot_deltas.get(&1).is_some());

        // Clear slot 1 related data
        status_cache.clear_slot_entries(1);
        assert!(status_cache.slot_deltas.get(&0).is_none());
        assert!(status_cache.slot_deltas.get(&1).is_none());
        assert!(status_cache
            .get_status(sig, &blockhash, &ancestors1)
            .is_none());
        assert!(status_cache
            .get_status(sig, &blockhash2, &ancestors1)
            .is_none());
    }

    // Status cache uses a random key offset for each blockhash. Ensure that shorter
    // keys can still be used if the offset if greater than the key length.
    #[test]
    fn test_different_sized_keys() {
        let status_cache = BankStatusCache::default();
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
