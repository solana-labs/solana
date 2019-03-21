use crate::bloom::{Bloom, BloomHashIndex};
use hashbrown::HashMap;
use log::*;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::VecDeque;
use std::ops::Deref;
#[cfg(test)]
use std::ops::DerefMut;
use std::sync::Arc;

/// Each cache entry is designed to span ~1 second of signatures
const MAX_CACHE_ENTRIES: usize = solana_sdk::timing::MAX_HASH_AGE_IN_SECONDS;

type FailureMap<T> = HashMap<Signature, T>;

#[derive(Clone)]
struct Status<T> {
    /// all signatures seen during a hash period
    signatures: Bloom<Signature>,

    /// failures
    failures: FailureMap<T>,
}

impl<T: Clone> Status<T> {
    // new needs to be called once per second to be useful for the Bank
    // 1M TPS * 1s (length of block in sigs) == 1M items in filter
    // 1.0E-8 false positive rate
    // https://hur.st/bloomfilter/?n=1000000&p=1.0E-8&m=&k=
    fn new(blockhash: &Hash) -> Self {
        let keys = (0..27).map(|i| blockhash.hash_at_index(i)).collect();
        Status {
            signatures: Bloom::new(38_340_234, keys),
            failures: HashMap::default(),
        }
    }
    fn has_signature(&self, sig: &Signature) -> bool {
        self.signatures.contains(&sig)
    }

    fn add(&mut self, sig: &Signature) {
        self.signatures.add(&sig);
    }

    fn clear(&mut self) {
        self.failures.clear();
        self.signatures.clear();
    }
    pub fn get_signature_status(&self, sig: &Signature) -> Option<Result<(), T>> {
        if let Some(res) = self.failures.get(sig) {
            return Some(Err(res.clone()));
        } else if self.signatures.contains(sig) {
            return Some(Ok(()));
        }
        None
    }
}

#[derive(Clone)]
pub struct StatusCache<T> {
    /// currently active status
    active: Option<Status<T>>,

    /// merges cover previous periods, and are read-only
    merges: VecDeque<Arc<Status<T>>>,
}

impl<T: Clone> Default for StatusCache<T> {
    fn default() -> Self {
        Self::new(&Hash::default())
    }
}

impl<T: Clone> StatusCache<T> {
    pub fn new(blockhash: &Hash) -> Self {
        Self {
            active: Some(Status::new(blockhash)),
            merges: VecDeque::default(),
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
        self.active
            .as_ref()
            .map_or(false, |active| active.has_signature(&sig))
            || self.has_signature_merged(sig)
    }

    /// add a signature
    pub fn add(&mut self, sig: &Signature) {
        if let Some(active) = self.active.as_mut() {
            active.add(&sig);
        }
    }

    /// Save an error status for a signature
    pub fn save_failure_status(&mut self, sig: &Signature, err: T) {
        assert!(
            self.active
                .as_ref()
                .map_or(false, |active| active.has_signature(sig)),
            "sig not found"
        );

        self.active
            .as_mut()
            .map(|active| active.failures.insert(*sig, err));
    }
    /// Forget all signatures. Useful for benchmarking.
    pub fn clear(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.clear();
        }
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
        self.active
            .as_ref()
            .and_then(|active| active.get_signature_status(sig))
            .or_else(|| self.get_signature_status_merged(sig))
    }

    fn squash_parent_is_full(&mut self, parent: &Self) -> bool {
        // flatten and squash the parent and its merges into self.merges,
        //  returns true if self is full
        if parent.active.is_some() {
            warn!("=========== FIXME: squash() on an active parent! ================");
        }
        // TODO: put this assert back in
        //assert!(parent.active.is_none());

        if self.merges.len() < MAX_CACHE_ENTRIES {
            for merge in parent
                .merges
                .iter()
                .take(MAX_CACHE_ENTRIES - self.merges.len())
            {
                self.merges.push_back(merge.clone());
            }
        }
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
        assert!(self.active.is_some());
        let merge = self.active.replace(Status::new(blockhash));

        self.merges.push_front(Arc::new(merge.unwrap()));
        if self.merges.len() > MAX_CACHE_ENTRIES {
            self.merges.pop_back();
        }
    }

    pub fn freeze(&mut self) {
        if let Some(active) = self.active.take() {
            self.merges.push_front(Arc::new(active));
        }
    }

    pub fn get_signature_status_all<U>(
        checkpoints: &[U],
        signature: &Signature,
    ) -> Option<(usize, Result<(), T>)>
    where
        U: Deref<Target = Self>,
    {
        for (i, c) in checkpoints.iter().enumerate() {
            if let Some(status) = c.get_signature_status(signature) {
                return Some((i, status));
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
            Some((1, Ok(()))),
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
        first.freeze();
        second.squash(&[&first]);

        assert_eq!(second.get_signature_status(&sig), Some(Ok(())));
        assert!(second.has_signature(&sig));
    }

    #[test]
    fn test_status_cache_squash_overflow() {
        let mut blockhash = hash(Hash::default().as_ref());
        let mut cache = BankStatusCache::new(&blockhash);

        let parents: Vec<_> = (0..MAX_CACHE_ENTRIES)
            .map(|_| {
                blockhash = hash(blockhash.as_ref());

                let mut p = BankStatusCache::new(&blockhash);
                p.freeze();
                p
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

    #[test]
    fn test_status_cache_freeze() {
        let sig = Signature::default();
        let blockhash = hash(Hash::default().as_ref());
        let mut cache: StatusCache<()> = StatusCache::new(&blockhash);

        cache.freeze();
        cache.freeze();

        cache.add(&sig);
        assert_eq!(cache.has_signature(&sig), false);
    }

}
