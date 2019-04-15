use hashbrown::{HashMap, HashSet};
use log::*;
use solana_sdk::pubkey::Pubkey;

pub type Fork = u64;

#[derive(Default)]
pub struct AccountsIndex<T> {
    account_maps: HashMap<Pubkey, Vec<(Fork, T)>>,
    roots: HashSet<Fork>,
    //This value that needs to be stored to recover the index from AppendVec
    last_root: Fork,
}

impl<T: Clone> AccountsIndex<T> {
    /// Get an account
    /// The latest account that appears in `ancestors` or `roots` is returned.
    pub fn get(&self, pubkey: &Pubkey, ancestors: &HashMap<Fork, usize>) -> Option<&T> {
        let list = self.account_maps.get(pubkey)?;
        let mut max = 0;
        let mut rv = None;
        for e in list.iter().rev() {
            if e.0 >= max && (ancestors.get(&e.0).is_some() || self.is_root(e.0)) {
                trace!("GET {} {:?}", e.0, ancestors);
                rv = Some(&e.1);
                max = e.0;
            }
        }
        rv
    }

    /// Insert a new fork.  
    /// @retval - The return value contains any squashed accounts that can freed from storage.
    pub fn insert(&mut self, fork: Fork, pubkey: &Pubkey, account_info: T) -> Vec<(Fork, T)> {
        let mut rv = vec![];
        let mut fork_vec: Vec<(Fork, T)> = vec![];
        {
            let entry = self.account_maps.entry(*pubkey).or_insert(vec![]);
            std::mem::swap(entry, &mut fork_vec);
        };

        // filter out old entries
        rv.extend(fork_vec.iter().filter(|(f, _)| *f == fork).cloned());
        fork_vec.retain(|(f, _)| *f != fork);

        // add the new entry
        fork_vec.push((fork, account_info));

        rv.extend(
            fork_vec
                .iter()
                .filter(|(fork, _)| self.is_purged(*fork))
                .cloned(),
        );
        fork_vec.retain(|(fork, _)| !self.is_purged(*fork));
        {
            let entry = self.account_maps.entry(*pubkey).or_insert(vec![]);
            std::mem::swap(entry, &mut fork_vec);
        };
        rv
    }
    fn is_purged(&self, fork: Fork) -> bool {
        !self.is_root(fork) && fork < self.last_root
    }
    pub fn is_root(&self, fork: Fork) -> bool {
        self.roots.contains(&fork)
    }
    pub fn add_root(&mut self, fork: Fork) {
        if fork > self.last_root {
            self.last_root = fork;
        }
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
        assert_eq!(index.get(&key.pubkey(), &ancestors), None);
    }

    #[test]
    fn test_insert_no_ancestors() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());

        let ancestors = HashMap::new();
        assert_eq!(index.get(&key.pubkey(), &ancestors), None);
    }

    #[test]
    fn test_insert_wrong_ancestors() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(index.get(&key.pubkey(), &ancestors), None);
    }

    #[test]
    fn test_insert_with_ancestors() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());

        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&true));
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
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());

        let ancestors = vec![].into_iter().collect();
        index.add_root(0);
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&true));
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
        index.add_root(0);
        assert_eq!(index.last_root, 1);
    }

    #[test]
    fn test_cleanup_first() {
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(1);
        index.add_root(0);
        index.cleanup_dead_fork(0);
        assert!(index.is_root(1));
        assert!(!index.is_root(0));
    }

    #[test]
    fn test_cleanup_last() {
        //this behavior might be undefined, clean up should only occur on older forks
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(1);
        index.add_root(0);
        index.cleanup_dead_fork(1);
        assert!(!index.is_root(1));
        assert!(index.is_root(0));
    }

    #[test]
    fn test_update_last_wins() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&true));

        let gc = index.insert(0, &key.pubkey(), false);
        assert_eq!(gc, vec![(0, true)]);
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&false));
    }

    #[test]
    fn test_update_new_fork() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());
        let gc = index.insert(1, &key.pubkey(), false);
        assert!(gc.is_empty());
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&true));
        let ancestors = vec![(1, 0)].into_iter().collect();
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&false));
    }

    #[test]
    fn test_update_gc_purged_fork() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let gc = index.insert(0, &key.pubkey(), true);
        assert!(gc.is_empty());
        index.add_root(1);
        let gc = index.insert(1, &key.pubkey(), false);
        assert_eq!(gc, vec![(0, true)]);
        let ancestors = vec![].into_iter().collect();
        assert_eq!(index.get(&key.pubkey(), &ancestors), Some(&false));
    }
}
