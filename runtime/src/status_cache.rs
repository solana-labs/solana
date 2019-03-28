use hashbrown::{HashMap, HashSet};
use log::*;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;

const MAX_CACHE_ENTRIES: usize = solana_sdk::timing::MAX_HASH_AGE_IN_SECONDS;

// Store forks in a single chunk of memory to avoid another lookup.
pub type ForkId = u64;
pub type ForkStatus<T> = Vec<(ForkId, T)>;
type SignatureMap<T> = HashMap<Signature, ForkStatus<T>>;
type StatusMap<T> = HashMap<Hash, (ForkId, SignatureMap<T>)>;

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
        self.cache
            .get(transaction_blockhash)
            .and_then(|(_, sigmap)| sigmap.get(sig))
            .and_then(|stored_forks| {
                stored_forks
                    .iter()
                    .filter(|(f, _)| ancestors.get(f).is_some() || self.roots.get(f).is_some())
                    .nth(0)
            })
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

    pub fn register_hash(&mut self, blockhash: Hash, fork: ForkId) {
        let hash = self.cache.entry(blockhash).or_insert((fork, HashMap::new()));
        hash.0 = std::cmp::max(fork, hash.0); 
    }

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
            .get_mut(transaction_blockhash)
            .expect("new map should be created with register_hash");
        let sig_forks = sig_map.1.entry(*sig).or_insert(vec![]);
        sig_forks.push((fork, res));
    }

    /// Clear for testing
    pub fn clear(&mut self) {
        self.cache.clear();
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
        status_cache.register_hash(blockhash, 0);
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
        status_cache.register_hash(blockhash, 0);
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
        status_cache.register_hash(blockhash, 0);
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
        status_cache.register_hash(blockhash, 0);
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.add_root(0);
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &ancestors),
            Some((ancestors.len(), ()))
        );
    }
 
    #[test]
    fn test_register_picks_latest() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = vec![(0, 0)].into_iter().collect();
        status_cache.register_hash(blockhash, 0);
        status_cache.register_hash(blockhash, 1);
        status_cache.insert(&blockhash, &sig, 0, ());
        for i in 0..(MAX_CACHE_ENTRIES + 1) {
            status_cache.add_root(i as u64);
        }
        assert!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors).is_some()
        );
    }
 
    #[test]
    fn test_root_expires() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.register_hash(blockhash, 0);
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
    fn test_clear() {
        let sig = Signature::default();
        let mut status_cache = BankStatusCache::default();
        let blockhash = hash(Hash::default().as_ref());
        let ancestors = HashMap::new();
        status_cache.register_hash(blockhash, 0);
        status_cache.insert(&blockhash, &sig, 0, ());
        status_cache.add_root(0);
        status_cache.clear();
        assert_eq!(
            status_cache.get_signature_status(&sig, &blockhash, &ancestors),
            None
        );
        assert_eq!(
            status_cache.get_signature_status_slow(&sig, &ancestors),
            None
        );
    }
}
