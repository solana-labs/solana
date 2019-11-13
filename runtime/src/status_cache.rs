use log::*;
use rand::{thread_rng, Rng};
use serde::Serialize;
use solana_sdk::clock::{Slot, MAX_RECENT_BLOCKHASHES};
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub const MAX_CACHE_ENTRIES: usize = MAX_RECENT_BLOCKHASHES;
const CACHED_SIGNATURE_SIZE: usize = 20;

// Store forks in a single chunk of memory to avoid another lookup.
pub type ForkStatus<T> = Vec<(Slot, T)>;
type SignatureSlice = [u8; CACHED_SIGNATURE_SIZE];
type SignatureMap<T> = HashMap<SignatureSlice, ForkStatus<T>>;
// Map of Hash and signature status
pub type SignatureStatus<T> = Arc<Mutex<HashMap<Hash, (usize, Vec<(SignatureSlice, T)>)>>>;
// A Map of hash + the highest fork it's been observed on along with
// the signature offset and a Map of the signature slice + Fork status for that signature
type StatusMap<T> = HashMap<Hash, (Slot, usize, SignatureMap<T>)>;

// A map of signatures recorded in each fork; used to serialize for snapshots easily.
// Doesn't store a `SlotDelta` in it because the bool `root` is usually set much later
type SlotDeltaMap<T> = HashMap<Slot, SignatureStatus<T>>;

// The signature statuses added during a slot, can be used to build on top of a status cache or to
// construct a new one. Usually derived from a status cache's `SlotDeltaMap`
pub type SlotDelta<T> = (Slot, bool, SignatureStatus<T>);

#[derive(Clone, Debug)]
pub struct StatusCache<T: Serialize + Clone> {
    cache: StatusMap<T>,
    roots: HashSet<Slot>,
    /// all signatures seen during a fork/slot
    slot_deltas: SlotDeltaMap<T>,
}

impl<T: Serialize + Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self {
            cache: HashMap::default(),
            // 0 is always a root
            roots: [0].iter().cloned().collect(),
            slot_deltas: HashMap::default(),
        }
    }
}

impl<T: Serialize + Clone + PartialEq> PartialEq for StatusCache<T> {
    fn eq(&self, other: &Self) -> bool {
        self.roots == other.roots
            && self.cache.iter().all(|(hash, (slot, sig_index, sig_map))| {
                if let Some((other_slot, other_sig_index, other_sig_map)) = other.cache.get(hash) {
                    if slot == other_slot && sig_index == other_sig_index {
                        return sig_map.iter().all(|(slice, fork_map)| {
                            if let Some(other_fork_map) = other_sig_map.get(slice) {
                                // all this work just to compare the highest forks in the fork map
                                // per signature
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
    /// Check if the signature from a transaction is in any of the forks in the ancestors set.
    pub fn get_signature_status(
        &self,
        sig: &Signature,
        transaction_blockhash: &Hash,
        ancestors: &HashMap<Slot, usize>,
    ) -> Option<(Slot, T)> {
        let map = self.cache.get(transaction_blockhash)?;
        let (_, index, sigmap) = map;
        let mut sig_slice = [0u8; CACHED_SIGNATURE_SIZE];
        sig_slice.clone_from_slice(&sig.as_ref()[*index..*index + CACHED_SIGNATURE_SIZE]);
        if let Some(stored_forks) = sigmap.get(&sig_slice) {
            let res = stored_forks
                .iter()
                .filter(|(f, _)| ancestors.get(f).is_some() || self.roots.get(f).is_some())
                .nth(0)
                .cloned();
            if res.is_some() {
                return res;
            }
        }
        None
    }

    pub fn get_signature_status_slow(
        &self,
        sig: &Signature,
        ancestors: &HashMap<Slot, usize>,
    ) -> Option<(usize, T)> {
        trace!("get_signature_status_slow");
        let mut keys = vec![];
        let mut val: Vec<_> = self.cache.iter().map(|(k, _)| *k).collect();
        keys.append(&mut val);

        for blockhash in keys.iter() {
            trace!("get_signature_status_slow: trying {}", blockhash);
            if let Some((forkid, res)) = self.get_signature_status(sig, blockhash, ancestors) {
                trace!("get_signature_status_slow: got {}", forkid);
                return ancestors
                    .get(&forkid)
                    .map(|id| (*id, res.clone()))
                    .or_else(|| Some((ancestors.len(), res)));
            }
        }
        None
    }

    /// Add a known root fork.  Roots are always valid ancestors.
    /// After MAX_CACHE_ENTRIES, roots are removed, and any old signatures are cleared.
    pub fn add_root(&mut self, fork: Slot) {
        self.roots.insert(fork);
        self.purge_roots();
    }

    pub fn roots(&self) -> &HashSet<u64> {
        &self.roots
    }

    /// Insert a new signature for a specific slot.
    pub fn insert(&mut self, transaction_blockhash: &Hash, sig: &Signature, slot: Slot, res: T) {
        let sig_index: usize;
        if let Some(sig_map) = self.cache.get(transaction_blockhash) {
            sig_index = sig_map.1;
        } else {
            sig_index =
                thread_rng().gen_range(0, std::mem::size_of::<Hash>() - CACHED_SIGNATURE_SIZE);
        }

        let sig_map =
            self.cache
                .entry(*transaction_blockhash)
                .or_insert((slot, sig_index, HashMap::new()));
        sig_map.0 = std::cmp::max(slot, sig_map.0);
        let index = sig_map.1;
        let mut sig_slice = [0u8; CACHED_SIGNATURE_SIZE];
        sig_slice.clone_from_slice(&sig.as_ref()[index..index + CACHED_SIGNATURE_SIZE]);
        self.insert_with_slice(transaction_blockhash, slot, sig_index, sig_slice, res);
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
    pub fn clear_signatures(&mut self) {
        for v in self.cache.values_mut() {
            v.2 = HashMap::new();
        }

        self.slot_deltas
            .iter_mut()
            .for_each(|(_, status)| status.lock().unwrap().clear());
    }

    // returns the signature statuses for each slot in the slots provided
    pub fn slot_deltas(&self, slots: &[Slot]) -> Vec<SlotDelta<T>> {
        let empty = Arc::new(Mutex::new(HashMap::new()));
        slots
            .iter()
            .map(|slot| {
                (
                    *slot,
                    self.roots.contains(slot),
                    self.slot_deltas.get(slot).unwrap_or_else(|| &empty).clone(),
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
                .for_each(|(tx_hash, (sig_index, statuses))| {
                    for (sig_slice, res) in statuses.iter() {
                        self.insert_with_slice(&tx_hash, *slot, *sig_index, *sig_slice, res.clone())
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
        sig_index: usize,
        sig_slice: [u8; CACHED_SIGNATURE_SIZE],
        res: T,
    ) {
        let sig_map =
            self.cache
                .entry(*transaction_blockhash)
                .or_insert((slot, sig_index, HashMap::new()));
        sig_map.0 = std::cmp::max(slot, sig_map.0);

        let sig_forks = sig_map.2.entry(sig_slice).or_insert_with(|| vec![]);
        sig_forks.push((slot, res.clone()));
        let slot_deltas = self.slot_deltas.entry(slot).or_default();
        let mut fork_entry = slot_deltas.lock().unwrap();
        let (_, hash_entry) = fork_entry
            .entry(*transaction_blockhash)
            .or_insert((sig_index, vec![]));
        hash_entry.push((sig_slice, res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::hash;

    type BankStatusCache = StatusCache<()>;

    #[test]
    fn test_empty_has_no_sigs() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let status_cache = BankStatusCache::default();
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &HashMap::new()),
            None
        );
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &HashMap::new()),
            None
        );
    }

    #[test]
    fn test_find_sig_with_ancestor_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = vec![(0, 1)].into_iter().collect();
        status_cache.insert(&blockhash, &sig, 0, ());
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors),
            Some((0, ()))
        );
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &ancestors),
            Some((1, ()))
        );
    }

    #[test]
    fn test_find_sig_without_ancestor_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.insert(&blockhash, &sig, 1, ());
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors),
            None
        );
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &ancestors),
            None
        );
    }

    #[test]
    fn test_find_sig_with_root_ancestor_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.add_root(0);
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors),
            Some((0, ()))
        );
    }

    #[test]
    fn test_find_sig_with_root_ancestor_fork_max_len() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = vec![(2, 2)].into_iter().collect();
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.add_root(0);
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &ancestors),
            Some((ancestors.len(), ()))
        );
    }

    #[test]
    fn test_insert_picks_latest_blockhash_fork() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = vec![(0, 0)].into_iter().collect();
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.insert(&blockhash, &sig, 1, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        assert!(status_cache
            .get_signature_status(&sig, &blockhash, &ancestors)
            .is_some());
    }

    #[test]
    fn test_root_expires() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.insert(&blockhash, &sig, 0, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors),
            None
        );
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &ancestors),
            None
        );
    }

    #[test]
    fn test_clear_signatures_sigs_are_gone() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.add_root(0);
        status_cache.clear_signatures();
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors),
            None
        );
    }

    #[test]
    fn test_clear_signatures_insert_works() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.add_root(0);
        status_cache.clear_signatures();
        status_cache.insert(&blockhash, &sig, 0, ());
        assert!(status_cache
            .get_signature_status(&sig, &blockhash, &ancestors)
            .is_some());
    }

    #[test]
    fn test_signatures_slice() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear_signatures();
        status_cache.insert(&blockhash, &sig, 0, ());
        let (_, index, sig_map) = status_cache.cache.get(&blockhash).unwrap();
        let mut sig_slice = [0u8; CACHED_SIGNATURE_SIZE];
        sig_slice.clone_from_slice(&sig.as_ref()[*index..*index + CACHED_SIGNATURE_SIZE]);
        assert!(sig_map.get(&sig_slice).is_some());
    }

    #[test]
    fn test_slot_deltas() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear_signatures();
        status_cache.insert(&blockhash, &sig, 0, ());
        let slot_deltas = status_cache.slot_deltas(&[0]);
        let cache = StatusCache::from_slot_deltas(&slot_deltas);
        assert_eq!(cache, status_cache);
        let slot_deltas = cache.slot_deltas(&[0]);
        let cache = StatusCache::from_slot_deltas(&slot_deltas);
        assert_eq!(cache, status_cache);
    }

    #[test]
    fn test_roots_deltas() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let blockhash2 = hash(blockhash.as_ref());
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.insert(&blockhash, &sig, 1, ());
        status_cache.insert(&blockhash2, &sig, 1, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        let slots: Vec<_> = (0_u64..MAX_CACHE_ENTRIES as u64 + 1).collect();
        let slot_deltas = status_cache.slot_deltas(&slots);
        let cache = StatusCache::from_slot_deltas(&slot_deltas);
        assert_eq!(cache, status_cache);
    }

    #[test]
    fn test_age_sanity() {
        assert!(MAX_CACHE_ENTRIES <= MAX_RECENT_BLOCKHASHES);
    }
}
