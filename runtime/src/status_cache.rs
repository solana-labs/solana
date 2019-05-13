use log::*;
use rand::{thread_rng, Rng};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};

const MAX_CACHE_ENTRIES: usize = solana_sdk::timing::MAX_HASH_AGE_IN_SECONDS;
const CACHED_SIGNATURE_SIZE: usize = 20;

// Store forks in a single chunk of memory to avoid another lookup.
pub type ForkId = u64;
pub type ForkStatus<T> = Vec<(ForkId, T)>;
type SignatureSlice = [u8; CACHED_SIGNATURE_SIZE];
type SignatureMap<T> = HashMap<SignatureSlice, ForkStatus<T>>;
type StatusMap<T> = HashMap<Hash, (ForkId, usize, SignatureMap<T>)>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StatusCache<T: Serialize + Clone> {
    /// all signatures seen during a hash period
    #[serde(serialize_with = "serialize_statusmap")]
    cache: Vec<StatusMap<T>>,
    roots: HashSet<ForkId>,
}

fn serialize_statusmap<S, T: Serialize>(x: &[StatusMap<T>], s: S) -> Result<S::Ok, S::Error>
where
    T: serde::Serialize + Clone,
    S: serde::Serializer,
{
    let cache0: StatusMap<T> = HashMap::new();
    let mut seq = s.serialize_seq(Some(x.len()))?;
    seq.serialize_element(&cache0)?;
    seq.serialize_element(&x[1])?;
    seq.end()
}

impl<T: Serialize + Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self {
            cache: vec![HashMap::default(); 2],
            roots: HashSet::default(),
        }
    }
}

impl<T: Serialize + Clone> StatusCache<T> {
    /// Check if the signature from a transaction is in any of the forks in the ancestors set.
    pub fn get_signature_status(
        &self,
        sig: &Signature,
        transaction_blockhash: &Hash,
        ancestors: &HashMap<ForkId, usize>,
    ) -> Option<(ForkId, T)> {
        for cache in self.cache.iter() {
            let map = cache.get(transaction_blockhash);
            if map.is_none() {
                continue;
            }
            let (_, index, sigmap) = map.unwrap();
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
        }
        None
    }

    /// TODO: wallets should send the Transactions recent blockhash as well
    pub fn get_signature_status_slow(
        &self,
        sig: &Signature,
        ancestors: &HashMap<ForkId, usize>,
    ) -> Option<(usize, T)> {
        trace!("get_signature_status_slow");
        let mut keys = vec![];
        for cache in self.cache.iter() {
            let mut val: Vec<_> = cache.iter().map(|(k, _)| *k).collect();
            keys.append(&mut val);
        }
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
    pub fn add_root(&mut self, fork: ForkId) {
        self.roots.insert(fork);
        if self.roots.len() > MAX_CACHE_ENTRIES {
            if let Some(min) = self.roots.iter().min().cloned() {
                self.roots.remove(&min);
                for cache in self.cache.iter_mut() {
                    cache.retain(|_, (fork, _, _)| *fork > min);
                }
            }
        }
    }

    /// Insert a new signature for a specific fork.
    pub fn insert(&mut self, transaction_blockhash: &Hash, sig: &Signature, fork: ForkId, res: T) {
        let sig_index: usize;
        if let Some(sig_map) = self.cache[0].get(transaction_blockhash) {
            sig_index = sig_map.1;
        } else {
            sig_index =
                thread_rng().gen_range(0, std::mem::size_of::<Hash>() - CACHED_SIGNATURE_SIZE);
        }
        let sig_map = self.cache[1].entry(*transaction_blockhash).or_insert((
            fork,
            sig_index,
            HashMap::new(),
        ));
        sig_map.0 = std::cmp::max(fork, sig_map.0);
        let index = sig_map.1;
        let mut sig_slice = [0u8; CACHED_SIGNATURE_SIZE];
        sig_slice.clone_from_slice(&sig.as_ref()[index..index + CACHED_SIGNATURE_SIZE]);
        let sig_forks = sig_map.2.entry(sig_slice).or_insert_with(|| vec![]);
        sig_forks.push((fork, res));
    }

    fn insert_entry(
        &mut self,
        transaction_blockhash: &Hash,
        sig_slice: &[u8; CACHED_SIGNATURE_SIZE],
        status: Vec<(ForkId, T)>,
        index: usize,
    ) {
        let fork = status
            .iter()
            .fold(0, |acc, (f, _)| if acc > *f { acc } else { *f });
        let sig_map =
            self.cache[0]
                .entry(*transaction_blockhash)
                .or_insert((fork, index, HashMap::new()));
        sig_map.0 = std::cmp::max(fork, sig_map.0);
        let sig_forks = sig_map.2.entry(*sig_slice).or_insert_with(|| vec![]);
        sig_forks.extend(status);
    }

    /// Clear for testing
    pub fn clear_signatures(&mut self) {
        for cache in self.cache.iter_mut() {
            for v in cache.values_mut() {
                v.2 = HashMap::new();
            }
        }
    }

    pub fn append(&mut self, status_cache: &StatusCache<T>) {
        for (hash, sigmap) in status_cache.cache[1].iter() {
            for (signature, fork_status) in sigmap.2.iter() {
                self.insert_entry(hash, signature, fork_status.clone(), sigmap.1);
            }
        }

        self.roots = self.roots.union(&status_cache.roots).cloned().collect();
    }

    pub fn merge_caches(&mut self) {
        let mut cache = HashMap::new();
        std::mem::swap(&mut cache, &mut self.cache[1]);
        for (hash, sigmap) in cache.iter() {
            for (signature, fork_status) in sigmap.2.iter() {
                self.insert_entry(hash, signature, fork_status.clone(), sigmap.1);
            }
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

    #[test]
    fn test_signatures_slice() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        status_cache.clear_signatures();
        status_cache.insert(&blockhash, &sig, 0, ());
        let (_, index, sig_map) = status_cache.cache[1].get(&blockhash).unwrap();
        let mut sig_slice = [0u8; CACHED_SIGNATURE_SIZE];
        sig_slice.clone_from_slice(&sig.as_ref()[*index..*index + CACHED_SIGNATURE_SIZE]);
        assert!(sig_map.get(&sig_slice).is_some());
    }

    #[test]
    fn test_statuscache_append() {
        let sig = Signature::default();
        let mut status_cache0 = BankStatusCache::default();
        let blockhash0 = hash(Hash::new(&vec![0; 32]).as_ref());
        status_cache0.add_root(0);
        status_cache0.insert(&blockhash0, &sig, 0, ());

        let sig = Signature::default();
        let mut status_cache1 = BankStatusCache::default();
        let blockhash1 = hash(Hash::new(&vec![1; 32]).as_ref());
        status_cache1.insert(&blockhash0, &sig, 1, ());
        status_cache1.add_root(1);
        status_cache1.insert(&blockhash1, &sig, 1, ());

        status_cache0.append(&status_cache1);
        let roots: HashSet<_> = [0, 1].iter().cloned().collect();
        assert_eq!(status_cache0.roots, roots);
        let ancestors = vec![(0, 1), (1, 1)].into_iter().collect();
        assert!(status_cache0
            .get_signature_status(&sig, &blockhash0, &ancestors)
            .is_some());
        assert!(status_cache0
            .get_signature_status(&sig, &blockhash1, &ancestors)
            .is_some());
    }

    fn test_serialize(sc: &mut BankStatusCache, blockhash: Vec<Hash>, sig: &Signature) {
        let len = serialized_size(&sc).unwrap();
        let mut buf = vec![0u8; len as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        let cache0 = sc.cache[0].clone();
        serialize_into(&mut writer, sc).unwrap();
        for hash in blockhash.iter() {
            if let Some(map0) = sc.cache[0].get(hash) {
                if let Some(map1) = sc.cache[1].get(hash) {
                    assert_eq!(map0.1, map1.1);
                }
            }
        }
        sc.merge_caches();
        let len = writer.position() as usize;

        let mut reader = Cursor::new(&mut buf[..len]);
        let mut status_cache: BankStatusCache = deserialize_from(&mut reader).unwrap();
        status_cache.cache[0] = cache0;
        status_cache.merge_caches();
        assert!(status_cache.cache[0].len() > 0);
        assert!(status_cache.cache[1].is_empty());
        let ancestors = vec![(0, 1), (1, 1)].into_iter().collect();
        assert_eq!(*sc, status_cache);
        for hash in blockhash.iter() {
            assert!(status_cache
                .get_signature_status(&sig, &hash, &ancestors)
                .is_some());
        }
    }

    #[test]
    fn test_statuscache_serialize() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash0 = hash(Hash::new(&vec![0; 32]).as_ref());
        status_cache.add_root(0);
        status_cache.clear_signatures();
        status_cache.insert(&blockhash0, &sig, 0, ());
        test_serialize(&mut status_cache, vec![blockhash0], &sig);

        status_cache.insert(&blockhash0, &sig, 1, ());
        test_serialize(&mut status_cache, vec![blockhash0], &sig);

        let blockhash1 = hash(Hash::new(&vec![1; 32]).as_ref());
        status_cache.insert(&blockhash1, &sig, 1, ());
        test_serialize(&mut status_cache, vec![blockhash0, blockhash1], &sig);

        let blockhash2 = hash(Hash::new(&vec![2; 32]).as_ref());
        let ancestors = vec![(0, 1), (1, 1)].into_iter().collect();
        assert!(status_cache
            .get_signature_status(&sig, &blockhash2, &ancestors)
            .is_none());
    }
}
