use crate::bank::{BankError, Result};
use crate::bloom::{Bloom, BloomHashIndex};
use crate::last_id_queue::MAX_ENTRY_IDS;
use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

type FailureMap<T> = HashMap<Signature, T>;

pub struct StatusCache {
    /// all signatures seen at this checkpoint
    signatures: Bloom<Signature>,

    /// failures
    failures: FailureMap<BankError>,

    /// Merges are empty unless this is the root checkpoint which cannot be unrolled
    merges: VecDeque<StatusCache>,
}

impl StatusCache {
    pub fn new(last_id: &Hash) -> Self {
        let keys = (0..27)
            .into_iter()
            .map(|i| last_id.hash_at_index(i))
            .collect();
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
        // any mutable cache is "live" and should not be merged into
        // since it cannot be a valid root checkpoint
        assert!(self.merges.is_empty());

        self.signatures.add(&sig)
    }
    /// Save an error status for a signature
    pub fn save_failure_status(&mut self, sig: &Signature, err: BankError) {
        assert!(self.has_signature(sig), "sig not found with err {:?}", err);
        // any mutable cache is "live" and should not be merged into
        // since it cannot be a valid root checkpoint
        assert!(self.merges.is_empty());
        self.failures.insert(*sig, err);
    }
    /// Forget all signatures. Useful for benchmarking.
    pub fn clear(&mut self) {
        self.failures.clear();
        self.signatures.clear();
    }
    fn get_signature_status_merged(&self, sig: &Signature) -> Option<Result<()>> {
        for c in &self.merges {
            if c.has_signature(sig) {
                return c.get_signature_status(sig);
            }
        }
        None
    }
    pub fn get_signature_status(&self, sig: &Signature) -> Option<Result<()>> {
        if let Some(res) = self.failures.get(sig) {
            return Some(Err(res.clone()));
        } else if self.signatures.contains(sig) {
            return Some(Ok(()));
        }
        self.get_signature_status_merged(sig)
    }
    /// like accounts, status cache starts with an new data structure for every checkpoint
    /// so only merge is implemented
    /// but the merges maintains a history
    pub fn merge_into_root(&mut self, other: Self) {
        // merges should be empty for every other checkpoint accept the root
        // which cannot be rolled back
        assert!(other.merges.is_empty());
        self.merges.push_front(other);
        if self.merges.len() > MAX_ENTRY_IDS {
            //TODO check if this is the right size ^
            self.merges.pop_back();
        }
    }
    pub fn get_signature_status_all<U>(
        checkpoints: &[U],
        signature: &Signature,
    ) -> Option<Result<()>>
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
    //use bincode::serialize;
    use solana_sdk::hash::hash;
    #[test]
    fn test_has_signature() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut status_cache = StatusCache::new(&last_id);
        assert_eq!(status_cache.has_signature(&sig), false);
        assert_eq!(status_cache.get_signature_status(&sig), None,);
        status_cache.add(&sig);
        assert_eq!(status_cache.has_signature(&sig), true);
        assert_eq!(status_cache.get_signature_status(&sig), Some(Ok(())),);
    }

    #[test]
    fn test_has_signature_checkpoint() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&last_id);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        let last_id = hash(last_id.as_ref());
        let second = StatusCache::new(&last_id);
        let checkpoints = [&second, &first];
        assert_eq!(
            StatusCache::get_signature_status_all(&checkpoints, &sig),
            Some(Ok(())),
        );
        assert!(StatusCache::has_signature_all(&checkpoints, &sig));
    }

    #[test]
    fn test_has_signature_merged1() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&last_id);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        let last_id = hash(last_id.as_ref());
        let second = StatusCache::new(&last_id);
        first.merge_into_root(second);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())),);
        assert!(first.has_signature(&sig));
    }

    #[test]
    fn test_has_signature_merged2() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&last_id);
        first.add(&sig);
        assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
        let last_id = hash(last_id.as_ref());
        let mut second = StatusCache::new(&last_id);
        second.merge_into_root(first);
        assert_eq!(second.get_signature_status(&sig), Some(Ok(())),);
        assert!(second.has_signature(&sig));
    }

    #[test]
    fn test_failure_status() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&last_id);
        first.add(&sig);
        first.save_failure_status(&sig, BankError::DuplicateSignature);
        assert_eq!(first.has_signature(&sig), true);
        assert_eq!(
            first.get_signature_status(&sig),
            Some(Err(BankError::DuplicateSignature)),
        );
    }

    #[test]
    fn test_clear_signatures() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&last_id);
        first.add(&sig);
        assert_eq!(first.has_signature(&sig), true);
        first.save_failure_status(&sig, BankError::DuplicateSignature);
        assert_eq!(
            first.get_signature_status(&sig),
            Some(Err(BankError::DuplicateSignature)),
        );
        first.clear();
        assert_eq!(first.has_signature(&sig), false);
        assert_eq!(first.get_signature_status(&sig), None,);
    }
    #[test]
    fn test_clear_signatures_all() {
        let sig = Default::default();
        let last_id = hash(Hash::default().as_ref());
        let mut first = StatusCache::new(&last_id);
        first.add(&sig);
        assert_eq!(first.has_signature(&sig), true);
        let mut second = StatusCache::new(&last_id);
        let mut checkpoints = [&mut second, &mut first];
        StatusCache::clear_all(&mut checkpoints);
        assert_eq!(StatusCache::has_signature_all(&checkpoints, &sig), false);
    }
}
