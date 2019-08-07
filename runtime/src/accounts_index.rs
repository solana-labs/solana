use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::{RwLock, RwLockReadGuard};

pub type Fork = u64;
type ForkList<T> = Vec<(Fork, T)>;

#[derive(Debug, Default)]
pub struct AccountsIndex<T> {
    pub account_maps: hashbrown::HashMap<Pubkey, RwLock<ForkList<T>>>,

    pub roots: HashSet<Fork>,

    //This value that needs to be stored to recover the index from AppendVec
    pub last_root: Fork,
}

impl<T: Clone> AccountsIndex<T> {
    /// call func with every pubkey and index visible from a given set of ancestors
    pub fn scan_accounts<F>(&self, ancestors: &HashMap<Fork, usize>, mut func: F)
    where
        F: FnMut(&Pubkey, (&T, Fork)) -> (),
    {
        for (pubkey, list) in self.account_maps.iter() {
            let list_r = list.read().unwrap();
            if let Some(index) = self.latest_fork(ancestors, &list_r) {
                func(pubkey, (&list_r[index].1, list_r[index].0));
            }
        }
    }

    // find the latest fork and T in a list for a given ancestor
    // returns index into 'list' if found, None if not.
    fn latest_fork(&self, ancestors: &HashMap<Fork, usize>, list: &[(Fork, T)]) -> Option<usize> {
        let mut max = 0;
        let mut rv = None;
        for (i, (fork, _t)) in list.iter().rev().enumerate() {
            if *fork >= max && (ancestors.get(fork).is_some() || self.is_root(*fork)) {
                rv = Some((list.len() - 1) - i);
                max = *fork;
            }
        }
        rv
    }

    /// Get an account
    /// The latest account that appears in `ancestors` or `roots` is returned.
    pub fn get(
        &self,
        pubkey: &Pubkey,
        ancestors: &HashMap<Fork, usize>,
    ) -> Option<(RwLockReadGuard<ForkList<T>>, usize)> {
        self.account_maps.get(pubkey).and_then(|list| {
            let lock = list.read().unwrap();
            if let Some(found_index) = self.latest_fork(ancestors, &lock) {
                Some((lock, found_index))
            } else {
                None
            }
        })
    }

    pub fn get_max_root(roots: &HashSet<Fork>, fork_vec: &[(Fork, T)]) -> Fork {
        let mut max_root = 0;
        for (f, _) in fork_vec.iter() {
            if *f > max_root && roots.contains(f) {
                max_root = *f;
            }
        }
        max_root
    }

    pub fn insert(
        &mut self,
        fork: Fork,
        pubkey: &Pubkey,
        account_info: T,
        reclaims: &mut Vec<(Fork, T)>,
    ) {
        let _fork_vec = self
            .account_maps
            .entry(*pubkey)
            .or_insert_with(|| RwLock::new(Vec::with_capacity(32)));
        self.update(fork, pubkey, account_info, reclaims);
    }

    // Try to update an item in account_maps. If the account is not
    // already present, then the function will return back Some(account_info) which
    // the caller can then take the write lock and do an 'insert' with the item.
    // It returns None if the item is already present and thus successfully updated.
    pub fn update(
        &self,
        fork: Fork,
        pubkey: &Pubkey,
        account_info: T,
        reclaims: &mut Vec<(Fork, T)>,
    ) -> Option<T> {
        let roots = &self.roots;
        if let Some(lock) = self.account_maps.get(pubkey) {
            let mut fork_vec = lock.write().unwrap();
            // filter out old entries
            reclaims.extend(fork_vec.iter().filter(|(f, _)| *f == fork).cloned());
            fork_vec.retain(|(f, _)| *f != fork);

            // add the new entry
            fork_vec.push((fork, account_info));

            let max_root = Self::get_max_root(roots, &fork_vec);

            reclaims.extend(
                fork_vec
                    .iter()
                    .filter(|(fork, _)| Self::can_purge(max_root, *fork))
                    .cloned(),
            );
            fork_vec.retain(|(fork, _)| !Self::can_purge(max_root, *fork));

            return None;
        } else {
            return Some(account_info);
        }
    }

    pub fn add_index(&mut self, fork: Fork, pubkey: &Pubkey, account_info: T) {
        let entry = self
            .account_maps
            .entry(*pubkey)
            .or_insert_with(|| RwLock::new(vec![]));
        entry.write().unwrap().push((fork, account_info));
    }

    pub fn is_purged(&self, fork: Fork) -> bool {
        fork < self.last_root
    }

    pub fn can_purge(max_root: Fork, fork: Fork) -> bool {
        fork < max_root
    }

    pub fn is_root(&self, fork: Fork) -> bool {
        self.roots.contains(&fork)
    }

    pub fn add_root(&mut self, fork: Fork) {
        assert!(
            (self.last_root == 0 && fork == 0) || (fork >= self.last_root),
            "new roots must be increasing"
        );
        self.last_root = fork;
        self.roots.insert(fork);
    }
    /// Remove the fork when the storage for the fork is freed
    /// Accounts no longer reference this fork.
    pub fn cleanup_dead_fork(&mut self, fork: Fork) {
        self.roots.remove(&fork);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_get_empty() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let ancestors = HashMap::new();
        assert!(index.get(&key.pubkey(), &ancestors).is_none());

        let mut num = 0;
        index.scan_accounts(&ancestors, |_pubkey, _index| num += 1);
        assert_eq!(num, 0);
    }

    #[test]
    fn test_insert_no_ancestors() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());

        let ancestors = HashMap::new();
        assert!(index.get(&key.pubkey(), &ancestors).is_none());

        let mut num = 0;
        index.scan_accounts(&ancestors, |_pubkey, _index| num += 1);
        assert_eq!(num, 0);
    }

    #[test]
    fn test_insert_wrong_ancestors() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert!(index.get(&key.pubkey(), &ancestors).is_none());

        let mut num = 0;
        index.scan_accounts(&ancestors, |_pubkey, _index| num += 1);
        assert_eq!(num, 0);
    }

    #[test]
    fn test_insert_with_ancestors() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());

        let ancestors = vec![(0, 0)].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (0, true));

        let mut num = 0;
        let mut found_key = false;
        index.scan_accounts(&ancestors, |pubkey, _index| {
            if pubkey == &key.pubkey() {
                found_key = true
            };
            num += 1
        });
        assert_eq!(num, 1);
        assert!(found_key);
    }

    #[test]
    fn test_is_root() {
        let mut index = AccountsIndex::<bool>::default();
        assert!(!index.is_root(0));
        index.add_root(0);
        assert!(index.is_root(0));
    }

    #[test]
    fn test_insert_with_root() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());

        let ancestors = vec![].into_iter().collect();
        index.add_root(0);
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (0, true));
    }

    #[test]
    fn test_is_purged() {
        let mut index = AccountsIndex::<bool>::default();
        assert!(!index.is_purged(0));
        index.add_root(1);
        assert!(index.is_purged(0));
    }

    #[test]
    fn test_max_last_root() {
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(1);
        assert_eq!(index.last_root, 1);
    }

    #[test]
    #[should_panic]
    fn test_max_last_root_old() {
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(1);
        index.add_root(0);
    }

    #[test]
    fn test_cleanup_first() {
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.cleanup_dead_fork(0);
        assert!(index.is_root(1));
        assert!(!index.is_root(0));
    }

    #[test]
    fn test_cleanup_last() {
        //this behavior might be undefined, clean up should only occur on older forks
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.cleanup_dead_fork(1);
        assert!(!index.is_root(1));
        assert!(index.is_root(0));
    }

    #[test]
    fn test_update_last_wins() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (0, true));
        drop(list);

        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), false, &mut gc);
        assert_eq!(gc, vec![(0, true)]);
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (0, false));
    }

    #[test]
    fn test_update_new_fork() {
        solana_logger::setup();
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        index.insert(1, &key.pubkey(), false, &mut gc);
        assert!(gc.is_empty());
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (0, true));
        let ancestors = vec![(1, 0)].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (1, false));
    }

    #[test]
    fn test_update_gc_purged_fork() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        index.insert(1, &key.pubkey(), false, &mut gc);
        index.insert(2, &key.pubkey(), true, &mut gc);
        index.insert(3, &key.pubkey(), true, &mut gc);
        index.add_root(0);
        index.add_root(1);
        index.add_root(3);
        index.insert(4, &key.pubkey(), true, &mut gc);
        assert_eq!(gc, vec![(0, true), (1, false), (2, true)]);
        let ancestors = vec![].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), &ancestors).unwrap();
        assert_eq!(list[idx], (3, true));

        let mut num = 0;
        let mut found_key = false;
        index.scan_accounts(&ancestors, |pubkey, _index| {
            if pubkey == &key.pubkey() {
                found_key = true;
                assert_eq!(_index, (&true, 3));
            };
            num += 1
        });
        assert_eq!(num, 1);
        assert!(found_key);
    }
}
