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

// Blockhash found in the transaction
type TransactionBlockHash = Hash
// Store forks in a single chunk of memory to avoid another lookup.
type ForkStatus<T> = Vec<(ForkId, T)>;
type SignatureMap<T> = HashMap<Signature, ForkStatusMap<T>>;
type StatusMap<T> = StatusMap<TransactionBlockHash, SignatureMap<T>>;

#[derive(Clone, Default)]
struct StatusCache<T: Default + Clone> {
    /// all signatures seen during a hash period
    cache: StatusMap<T>,

}

impl<T: Clone> StatusCache<T> {
    /// Check if the signature from a transaction is in any of the forks in the ancestors set.
    pub fn get_signature_status(&self, sig: &Signature, blockhash: TransactionBlockHash, ancestors: &HashSet<u64>) -> Option<T> {
        self.cache
            .get(blockhash)
            .and_then(|sigmap| sigmap.get(signature))
            .and_then(|stored_forks|
                      stored_forks.iter().filter(|(f,r)| ancestors.contains(f)).map(|(_,r) r.clone()).first())
    }
 
    /// Insert a new signature for a specific fork.
    pub fn insert(&mut self, hash: &TransactionBlockHash, sig: &Signature, fork: u64, res: T) {
        let sig_map = self.cache.entry(hash).or_insert(HashMap::new());
        let sig_forks = sig_map.entry(sig).or_insert(vec![]);
        sig_forks.push((fork, T));
    }

    /// Remove an expired blockhash 
    pub fn remove_blockhash(&mut self, hash: &TransactionBlockHash) {
        self.cache.remove(hash);
    }

    /// Clear for testing
    pub fn clear(&mut self) {
        self.cache.clear();
    } 
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use solana_sdk::hash::hash;
//     use solana_sdk::transaction::TransactionError;
// 
//     type BankStatusCache = StatusCache<TransactionError>;
// 
//     #[test]
//     fn test_has_signature() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut status_cache = BankStatusCache::new(&blockhash);
//         assert_eq!(status_cache.has_signature(&sig), false);
//         assert_eq!(status_cache.get_signature_status(&sig), None);
//         status_cache.add(&sig);
//         assert_eq!(status_cache.has_signature(&sig), true);
//         assert_eq!(status_cache.get_signature_status(&sig), Some(Ok(())));
//     }
// 
//     #[test]
//     fn test_has_signature_checkpoint() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = BankStatusCache::new(&blockhash);
//         first.add(&sig);
//         assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
//         let blockhash = hash(blockhash.as_ref());
//         let second = StatusCache::new(&blockhash);
//         let checkpoints = [&second, &first];
//         assert_eq!(
//             BankStatusCache::get_signature_status_all(&checkpoints, &sig),
//             Some((1, Ok(()))),
//         );
//         assert!(StatusCache::has_signature_all(&checkpoints, &sig));
//     }
// 
//     #[test]
//     fn test_new_cache() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = BankStatusCache::new(&blockhash);
//         first.add(&sig);
//         assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
//         let blockhash = hash(blockhash.as_ref());
//         first.new_cache(&blockhash);
//         assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
//         assert!(first.has_signature(&sig));
//         first.clear();
//         assert_eq!(first.get_signature_status(&sig), None);
//         assert!(!first.has_signature(&sig));
//     }
// 
//     #[test]
//     fn test_new_cache_full() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = BankStatusCache::new(&blockhash);
//         first.add(&sig);
//         assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
//         for _ in 0..(MAX_CACHE_ENTRIES + 1) {
//             let blockhash = hash(blockhash.as_ref());
//             first.new_cache(&blockhash);
//         }
//         assert_eq!(first.get_signature_status(&sig), None);
//         assert!(!first.has_signature(&sig));
//     }
// 
//     #[test]
//     fn test_status_cache_squash_has_signature() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = BankStatusCache::new(&blockhash);
//         first.add(&sig);
//         assert_eq!(first.get_signature_status(&sig), Some(Ok(())));
// 
//         // give first a merge
//         let blockhash = hash(blockhash.as_ref());
//         first.new_cache(&blockhash);
// 
//         let blockhash = hash(blockhash.as_ref());
//         let mut second = BankStatusCache::new(&blockhash);
//         first.freeze();
//         second.squash(&[&first]);
// 
//         assert_eq!(second.get_signature_status(&sig), Some(Ok(())));
//         assert!(second.has_signature(&sig));
//     }
// 
//     #[test]
//     fn test_status_cache_squash_overflow() {
//         let mut blockhash = hash(Hash::default().as_ref());
//         let mut cache = BankStatusCache::new(&blockhash);
// 
//         let parents: Vec<_> = (0..MAX_CACHE_ENTRIES)
//             .map(|_| {
//                 blockhash = hash(blockhash.as_ref());
// 
//                 let mut p = BankStatusCache::new(&blockhash);
//                 p.freeze();
//                 p
//             })
//             .collect();
// 
//         let mut parents_refs: Vec<_> = parents.iter().collect();
// 
//         blockhash = hash(Hash::default().as_ref());
//         let mut root = BankStatusCache::new(&blockhash);
// 
//         let sig = Signature::default();
//         root.add(&sig);
// 
//         parents_refs.push(&root);
// 
//         assert_eq!(root.get_signature_status(&sig), Some(Ok(())));
//         assert!(root.has_signature(&sig));
// 
//         // will overflow
//         cache.squash(&parents_refs);
// 
//         assert_eq!(cache.get_signature_status(&sig), None);
//         assert!(!cache.has_signature(&sig));
//     }
// 
//     #[test]
//     fn test_failure_status() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = StatusCache::new(&blockhash);
//         first.add(&sig);
//         first.save_failure_status(&sig, TransactionError::DuplicateSignature);
//         assert_eq!(first.has_signature(&sig), true);
//         assert_eq!(
//             first.get_signature_status(&sig),
//             Some(Err(TransactionError::DuplicateSignature)),
//         );
//     }
// 
//     #[test]
//     fn test_clear_signatures() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = StatusCache::new(&blockhash);
//         first.add(&sig);
//         assert_eq!(first.has_signature(&sig), true);
//         first.save_failure_status(&sig, TransactionError::DuplicateSignature);
//         assert_eq!(
//             first.get_signature_status(&sig),
//             Some(Err(TransactionError::DuplicateSignature)),
//         );
//         first.clear();
//         assert_eq!(first.has_signature(&sig), false);
//         assert_eq!(first.get_signature_status(&sig), None);
//     }
//     #[test]
//     fn test_clear_signatures_all() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut first = StatusCache::new(&blockhash);
//         first.add(&sig);
//         assert_eq!(first.has_signature(&sig), true);
//         let mut second = StatusCache::new(&blockhash);
//         let mut checkpoints = [&mut second, &mut first];
//         BankStatusCache::clear_all(&mut checkpoints);
//         assert_eq!(
//             BankStatusCache::has_signature_all(&checkpoints, &sig),
//             false
//         );
//     }
// 
//     #[test]
//     fn test_status_cache_freeze() {
//         let sig = Signature::default();
//         let blockhash = hash(Hash::default().as_ref());
//         let mut cache: StatusCache<()> = StatusCache::new(&blockhash);
// 
//         cache.freeze();
//         cache.freeze();
// 
//         cache.add(&sig);
//         assert_eq!(cache.has_signature(&sig), false);
//     }
// 
// }
