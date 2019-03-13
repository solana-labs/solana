use crate::bloom::{Bloom, BloomHashIndex};
use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::VecDeque;
use std::ops::Deref;
#[cfg(test)]
use std::ops::DerefMut;

/// Each cache entry is designed to span ~1 second of signatures
const MAX_CACHE_ENTRIES: usize = solana_sdk::timing::MAX_HASH_AGE_IN_SECONDS;

type FailureMap<T> = HashMap<Signature, T>;

#[derive(Clone)]
pub struct StatusCache<T> {
    /// all signatures seen at this checkpoint
    signatures: Bloom<Signature>,

    /// failures
    failures: FailureMap<T>,

    /// Merges are empty unless this is the root checkpoint which cannot be unrolled
    merges: VecDeque<StatusCache<T>>,
}

impl<T: Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self::new(&Hash::default())
    }
}

impl<T: Clone> StatusCache<T> {
    pub fn new(blockhash: &Hash) -> Self {
        let keys = (0..27).map(|i| blockhash.hash_at_index(i)).collect();
        Self {
            signatures: Bloom::new(38_340_234, keys),
            failures: HashMap::new(),
            merges: VecDeque::new(),
        }
    }
    fn has_signature_merged(&self, sig: &Signature) -> bool {
        for c in &self.merges {
            if c.has_signature(sig) {
                return true;
            }
        }
        false
    }
    /// test if a signature is known
    pub fn has_signature(&self, sig: &Signature) -> bool {
        self.signatures.contains(&sig) || self.has_signature_merged(sig)
    }
    /// add a signature
    pub fn add(&mut self, sig: &Signature) {
        self.signatures.add(&sig)
    }
    /// Save an error status for a signature
    pub fn save_failure_status(&mut self, sig: &Signature, err: T) {
        assert!(self.has_signature(sig), "sig not found");
        self.failures.insert(*sig, err);
    }
    /// Forget all signatures. Useful for benchmarking.
    pub fn clear(&mut self) {
        self.failures.clear();
        self.signatures.clear();
        self.merges = VecDeque::new();
    }
    fn get_signature_status_merged(&self, sig: &Signature) -> Option<Result<(), T>> {
        for c in &self.merges {
            if c.has_signature(sig) {
                return c.get_signature_status(sig);
            }
        }
        None
    }
    pub fn get_signature_status(&self, sig: &Signature) -> Option<Result<(), T>> {
        if let Some(res) = self.failures.get(sig) {
            return Some(Err(res.clone()));
        } else if self.signatures.contains(sig) {
            return Some(Ok(()));
        }
        self.get_signature_status_merged(sig)
    }

    fn squash_parent_is_full(&mut self, parent: &Self) -> bool {
        // flatten and squash the parent and its merges into self.merges,
        //  returns true if self is full

        self.merges.push_back(StatusCache {
            signatures: parent.signatures.clone(),
            failures: parent.failures.clone(),
            merges: VecDeque::new(),
        });
        for merge in &parent.merges {
            self.merges.push_back(StatusCache {
                signatures: merge.signatures.clone(),
                failures: merge.failures.clone(),
                merges: VecDeque::new(),
            });
        }
        self.merges.truncate(MAX_CACHE_ENTRIES);

        self.merges.len() == MAX_CACHE_ENTRIES
    }

    /// copy the parents and parents' merges up to this instance, up to
    ///   MAX_CACHE_ENTRIES deep
    pub fn squash<U>(&mut self, parents: &[U])
    where
        U: Deref<Target = Self>,
    {
        for parent in parents {
            if self.squash_parent_is_full(parent) {
                break;
            }
        }
    }

    /// Crate a new cache, pushing the old cache into the merged queue
    pub fn new_cache(&mut self, blockhash: &Hash) {
        let mut old = Self::new(blockhash);
        std::mem::swap(&mut old.signatures, &mut self.signatures);
        std::mem::swap(&mut old.failures, &mut self.failures);
        assert!(old.merges.is_empty());
        self.merges.push_front(old);
        if self.merges.len() > MAX_CACHE_ENTRIES {
            self.merges.pop_back();
        }
    }
    pub fn get_signature_status_all<U>(
        checkpoints: &[U],
        signature: &Signature,
    ) -> Option<Result<(), T>>
    where
        U: Deref<Target = Self>,
    {
        for c in checkpoints {
            if let Some(status) = c.get_signature_status(signature) {
                return Some(status);
            }
        }
        None
    }
    pub fn has_signature_all<U>(checkpoints: &[U], signature: &Signature) -> bool
    where
        U: Deref<Target = Self>,
    {
        for c in checkpoints {
            if c.has_signature(signature) {
                return true;
            }
        }
        false
    }
    #[cfg(test)]
    pub fn clear_all<U>(checkpoints: &mut [U]) -> bool
    where
        U: DerefMut<Target = Self>,
    {
        for c in checkpoints.iter_mut() {
            c.clear();
        }
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::hash;
    use solana_sdk::transaction::TransactionError;

    type BankStatusCache = StatusCache<TransactionError>;

    #[test]
    fn test_has_signature() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut status_cache = BankStatusCache::new(&blockhash);
        assert_eq!(status_cache.has_signature(&sig), false);
        assert_eq!(status_cache.get_signature_status(&sig), None);
        status_cache.add(&sig);
        assert_eq!(status_cache.has_signature(&sig), true);
        assert_eq!(status_cache.get_signature_status(&sig), Some(Ok(())));
    }

    #[test]
    fn test_has_signature_checkpoint() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = BankStatusCache::new(&blockhash);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        let blockhash = hash(blockhash.as_ref());
        let second = StatusCache::new(&blockhash);
        let checkpoints = [&second, &first];
        assert_eq!(
            BankStatusCache::get_signature_status_all(&checkpoints, &sig),
            Some(Ok(())),
        );
        assert!(StatusCache::has_signature_all(&checkpoints, &sig));
    }

    #[test]
    fn test_new_cache() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = BankStatusCache::new(&blockhash);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        let blockhash = hash(blockhash.as_ref());
        first.new_cache(&blockhash);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        assert!(first.has_signature(&sig));
        first.clear();
        assert_eq!(first.get_signature_status(&sig), None);
        assert!(!first.has_signature(&sig));
    }

    #[test]
    fn test_new_cache_full() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = BankStatusCache::new(&blockhash);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        for _ in 0..(MAX_CACHE_ENTRIES + 1) {
            let blockhash = hash(blockhash.as_ref());
            first.new_cache(&blockhash);
        }
        assert_eq!(first.get_signature_status(&sig), None);
        assert!(!first.has_signature(&sig));
    }

    #[test]
    fn test_status_cache_squash_has_signature() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = BankStatusCache::new(&blockhash);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));

        // give first a merge
        let blockhash = hash(blockhash.as_ref());
        first.new_cache(&blockhash);

        let blockhash = hash(blockhash.as_ref());
        let mut second = BankStatusCache::new(&blockhash);

        second.squash(&[&first]);

        assert_eq!(second.get_signature_status(&sig), Some(Ok(())));
        assert!(second.has_signature(&sig));
    }

    #[test]
    #[ignore] // takes a lot of time or RAM or both..
    fn test_status_cache_squash_overflow() {
        let mut blockhash = hash(Hash::default().as_ref());
        let mut cache = BankStatusCache::new(&blockhash);

        let parents: Vec<_> = (0..MAX_CACHE_ENTRIES)
            .map(|_| {
                blockhash = hash(blockhash.as_ref());

                BankStatusCache::new(&blockhash)
            })
            .collect();

        let mut parents_refs: Vec<_> = parents.iter().collect();

        blockhash = hash(Hash::default().as_ref());
        let mut root = BankStatusCache::new(&blockhash);

        let sig = Signature::default();
        root.add(&sig);

        parents_refs.push(&root);

        assert_eq!(root.get_signature_status(&sig), Some(Ok(())));
        assert!(root.has_signature(&sig));

        // will overflow
        cache.squash(&parents_refs);

        assert_eq!(cache.get_signature_status(&sig), None);
        assert!(!cache.has_signature(&sig));
    }

    #[test]
    fn test_failure_status() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&blockhash);
        first.add(&sig);
        first.save_failure_status(&sig, TransactionError::DuplicateSignature);
        assert_eq!(first.has_signature(&sig), true);
        assert_eq!(
            first.get_signature_status(&sig),
            Some(Err(TransactionError::DuplicateSignature)),
        );
    }

    #[test]
    fn test_clear_signatures() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&blockhash);
        first.add(&sig);
        assert_eq!(first.has_signature(&sig), true);
        first.save_failure_status(&sig, TransactionError::DuplicateSignature);
        assert_eq!(
            first.get_signature_status(&sig),
            Some(Err(TransactionError::DuplicateSignature)),
        );
        first.clear();
        assert_eq!(first.has_signature(&sig), false);
        assert_eq!(first.get_signature_status(&sig), None);
    }
    #[test]
    fn test_clear_signatures_all() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&blockhash);
        first.add(&sig);
        assert_eq!(first.has_signature(&sig), true);
        let mut second = StatusCache::new(&blockhash);
        let mut checkpoints = [&mut second, &mut first];
        BankStatusCache::clear_all(&mut checkpoints);
        assert_eq!(
            BankStatusCache::has_signature_all(&checkpoints, &sig),
            false
        );
    }
}
