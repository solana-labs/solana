use crate::bank::{BankError, Result};
use crate::bloom::{Bloom, BloomHashIndex};
use crate::entry_queue::MAX_ENTRY_IDS;
use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use std::collections::VecDeque;
use std::ops::Deref;

type FailureMap<T> = HashMap<Signature, T>;

pub struct StatusCache {
    /// all signatures seen at this checkpoint
    signatures: Bloom<Signature>,

    /// failures
    failures: FailureMap<BankError>,

    /// Merges are empty unless this is the trunk checkpoint which cannot be unrolled
    merges: VecDeque<StatusCache>,
}

impl StatusCache {
    pub fn new(last_id: &Hash) -> Self {
        let keys = (0..27)
            .into_iter()
            .map(|i| last_id.hash_at_index(i))
            .collect();
        Self {
            signatures: Bloom::new(38340234, keys),
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
        return false;
    }
    /// test if a signature is known
    pub fn has_signature(&self, sig: &Signature) -> bool {
        self.signatures.contains(&sig) || self.has_signature_merged(sig)
    }
    /// add a signature
    pub fn add(&mut self, sig: &Signature) {
        // any mutable cache is "live" and should not be merged into
        // since it cannot be a valid trunk checkpoint
        assert!(self.merges.is_empty());
        self.signatures.add(&sig)
    }
    /// Save an error status for a signature
    pub fn save_failure_status(&mut self, sig: &Signature, err: BankError) {
        assert!(self.has_signature(sig));
        // any mutable cache is "live" and should not be merged into
        // since it cannot be a valid trunk checkpoint
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
    pub fn merge_into_trunk(&mut self, other: Self) {
        // merges should be empty for every other checkpoint accept the trunk
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
}
#[cfg(test)]
mod tests {
    use super::*;
    //use bincode::serialize;
    use solana_sdk::hash::hash;
    #[test]
    fn test_duplicate_transaction_signature() {
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
    fn test_duplicate_transaction_signature_checkpoint() {
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
    //
    //     #[test]
    //     fn test_count_valid_ids() {
    //         let first_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         status_deque.register_tick(&first_id);
    //         let ids: Vec<_> = (0..MAX_ENTRY_IDS)
    //             .map(|i| {
    //                 let last_id = hash(&serialize(&i).unwrap()); // Unique hash
    //                 status_deque.register_tick(&last_id);
    //                 last_id
    //             })
    //             .collect();
    //         assert_eq!(status_deque.count_valid_ids(&[]).len(), 0);
    //         assert_eq!(status_deque.count_valid_ids(&[first_id]).len(), 0);
    //         for (i, id) in status_deque.count_valid_ids(&ids).iter().enumerate() {
    //             assert_eq!(id.0, i);
    //         }
    //     }
    //
    //     #[test]
    //     fn test_clear_signatures() {
    //         let signature = Signature::default();
    //         let last_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         status_deque.register_tick(&last_id);
    //         status_deque.get_signature(&last_id, &signature).unwrap();
    //         status_deque.clear_signatures();
    //         assert_eq!(status_deque.get_signature(&last_id, &signature), Ok(()));
    //     }
    //
    //     #[test]
    //     fn test_clear_signatures_checkpoint() {
    //         let signature = Signature::default();
    //         let last_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         status_deque.register_tick(&last_id);
    //         status_deque.get_signature(&last_id, &signature).unwrap();
    //         status_deque.checkpoint();
    //         status_deque.clear_signatures();
    //         assert_eq!(status_deque.get_signature(&last_id, &signature), Ok(()));
    //     }
    //
    //     #[test]
    //     fn test_get_signature_status() {
    //         let signature = Signature::default();
    //         let last_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         status_deque.register_tick(&last_id);
    //         status_deque
    //             .get_signature(&last_id, &signature)
    //             .expect("reserve signature");
    //         assert_eq!(status_deque.get_signature_status(&signature), Some(Ok()),);
    //     }
    //
    //     #[test]
    //     fn test_register_tick() {
    //         let signature = Signature::default();
    //         let last_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         assert_eq!(
    //             status_deque.get_signature(&last_id, &signature),
    //             Err(StatusDequeError::LastIdNotFound)
    //         );
    //         status_deque.register_tick(&last_id);
    //         assert_eq!(status_deque.get_signature(&last_id, &signature), Ok(()));
    //     }
    //
    //     #[test]
    //     fn test_has_signature() {
    //         let signature = Signature::default();
    //         let last_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         status_deque.register_tick(&last_id);
    //         status_deque
    //             .get_signature(&last_id, &signature)
    //             .expect("reserve signature");
    //         assert!(status_deque.has_signature(&signature));
    //     }
    //
    //     #[test]
    //     fn test_reject_old_last_id() {
    //         let signature = Signature::default();
    //         let last_id = Default::default();
    //         let mut status_deque: StatusDeque<()> = StatusDeque::default();
    //         for i in 0..MAX_ENTRY_IDS {
    //             let last_id = hash(&serialize(&i).unwrap()); // Unique hash
    //             status_deque.register_tick(&last_id);
    //         }
    //         // Assert we're no longer able to use the oldest entry ID.
    //         assert_eq!(
    //             status_deque.get_signature(&last_id, &signature),
    //             Err(StatusDequeError::LastIdNotFound)
    //         );
    //     }
}
