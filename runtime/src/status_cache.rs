use log::*;
use serde::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};

const MAX_CACHE_ENTRIES: usize = solana_sdk::timing::MAX_HASH_AGE_IN_SECONDS;

// Store forks in a single chunk of memory to avoid another lookup.
pub type ForkId = u64;
pub type ForkStatus<T> = Vec<(ForkId, T)>;
type SignatureMap<T> = HashMap<Signature, ForkStatus<T>>;
type StatusMap<T> = HashMap<Hash, (ForkId, SignatureMap<T>)>;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct StatusCache<T: Clone> {
    /// all signatures seen during a hash period
    cache: StatusMap<T>,
    roots: HashSet<ForkId>,
}

impl<T: Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self {
            cache: HashMap::default(),
            roots: HashSet::default(),
        }
    }
}

impl<T: Clone> StatusCache<T> {
    /// Check if the signature from a transaction is in any of the forks in the ancestors set.
    pub fn get_signature_status(
        &self,
        sig: &Signature,
        transaction_blockhash: &Hash,
        ancestors: &HashMap<ForkId, usize>,
    ) -> Option<(ForkId, T)> {
        let (_, sigmap) = self.cache.get(transaction_blockhash)?;
        let stored_forks = sigmap.get(sig)?;
        stored_forks
            .iter()
            .filter(|(f, _)| ancestors.get(f).is_some() || self.roots.get(f).is_some())
            .nth(0)
            .cloned()
    }

    /// TODO: wallets should send the Transactions recent blockhash as well
    pub fn get_signature_status_slow(
        &self,
        sig: &Signature,
        ancestors: &HashMap<ForkId, usize>,
    ) -> Option<(usize, T)> {
        trace!("get_signature_status_slow");
        for blockhash in self.cache.keys() {
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
    pub fn add_root(&mut self, fork: ForkId) {
        self.roots.insert(fork);
        if self.roots.len() > MAX_CACHE_ENTRIES {
            if let Some(min) = self.roots.iter().min().cloned() {
                self.roots.remove(&min);
                self.cache.retain(|_, (fork, _)| *fork > min);
            }
        }
    }

    /// Insert a new signature for a specific fork.
    pub fn insert(&mut self, transaction_blockhash: &Hash, sig: &Signature, fork: ForkId, res: T) {
        let sig_map = self
            .cache
            .entry(*transaction_blockhash)
            .or_insert((fork, HashMap::new()));
        sig_map.0 = std::cmp::max(fork, sig_map.0);
        let sig_forks = sig_map.1.entry(*sig).or_insert_with(|| vec![]);
        sig_forks.push((fork, res));
    }

    /// Clear for testing
    pub fn clear_signatures(&mut self) {
        for v in self.cache.values_mut() {
            v.1 = HashMap::new();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize_from, serialize_into, serialized_size};
    use solana_sdk::hash::hash;
    use std::io::Cursor;

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
        status_cache.insert(&blockhash, &sig, 0, ());
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

    fn test_serialize(sc: &BankStatusCache) {
        let mut buf = vec![0u8; serialized_size(sc).unwrap() as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        serialize_into(&mut writer, sc).unwrap();

        let mut reader = Cursor::new(&mut buf[..]);
        let deser: BankStatusCache = deserialize_from(&mut reader).unwrap();
        assert_eq!(*sc, deser);
    }

    #[test]
    fn test_statuscache_serialize() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.add_root(0);
        status_cache.clear_signatures();
        status_cache.insert(&blockhash, &sig, 0, ());
        test_serialize(&status_cache);

        let new_blockhash = hash(Hash::default().as_ref());
        status_cache.insert(&new_blockhash, &sig, 1, ());
        test_serialize(&status_cache);
    }
}
