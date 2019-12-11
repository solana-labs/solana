use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::{RwLock, RwLockReadGuard};

pub type Slot = u64;
type SlotList<T> = Vec<(Slot, T)>;

#[derive(Debug, Default)]
pub struct AccountsIndex<T> {
    pub account_maps: HashMap<Pubkey, RwLock<SlotList<T>>>,

    pub roots: HashSet<Slot>,

    //This value that needs to be stored to recover the index from AppendVec
    pub last_root: Slot,
}

impl<T: Clone> AccountsIndex<T> {
    /// call func with every pubkey and index visible from a given set of ancestors
    pub fn scan_accounts<F>(&self, ancestors: &HashMap<Slot, usize>, mut func: F)
    where
        F: FnMut(&Pubkey, (&T, Slot)) -> (),
    {
        for (pubkey, list) in self.account_maps.iter() {
            let list_r = list.read().unwrap();
            if let Some(index) = self.latest_slot(ancestors, &list_r) {
                func(pubkey, (&list_r[index].1, list_r[index].0));
            }
        }
    }

    fn get_rooted_entries(&self, list: &[(Slot, T)]) -> Vec<(Slot, T)> {
        list.iter()
            .filter(|(slot, _)| self.is_root(*slot))
            .cloned()
            .collect()
    }

    pub fn would_purge(&self, pubkey: &Pubkey) -> Vec<(Slot, T)> {
        let list = self.account_maps.get(&pubkey).unwrap().read().unwrap();
        self.get_rooted_entries(&list)
    }

    // filter any rooted entries and return them along with a bool that indicates
    // if this account has no more entries.
    pub fn purge(&self, pubkey: &Pubkey) -> (Vec<(Slot, T)>, bool) {
        let mut list = self.account_maps.get(&pubkey).unwrap().write().unwrap();
        let reclaims = self.get_rooted_entries(&list);
        list.retain(|(slot, _)| !self.is_root(*slot));
        (reclaims, list.is_empty())
    }

    // find the latest slot and T in a list for a given ancestor
    // returns index into 'list' if found, None if not.
    fn latest_slot(&self, ancestors: &HashMap<Slot, usize>, list: &[(Slot, T)]) -> Option<usize> {
        let mut max = 0;
        let mut rv = None;
        for (i, (slot, _t)) in list.iter().rev().enumerate() {
            if *slot >= max && (ancestors.get(slot).is_some() || self.is_root(*slot)) {
                rv = Some((list.len() - 1) - i);
                max = *slot;
            }
        }
        rv
    }

    /// Get an account
    /// The latest account that appears in `ancestors` or `roots` is returned.
    pub fn get(
        &self,
        pubkey: &Pubkey,
        ancestors: &HashMap<Slot, usize>,
    ) -> Option<(RwLockReadGuard<SlotList<T>>, usize)> {
        self.account_maps.get(pubkey).and_then(|list| {
            let lock = list.read().unwrap();
            if let Some(found_index) = self.latest_slot(ancestors, &lock) {
                Some((lock, found_index))
            } else {
                None
            }
        })
    }

    pub fn get_max_root(roots: &HashSet<Slot>, slot_vec: &[(Slot, T)]) -> Slot {
        let mut max_root = 0;
        for (f, _) in slot_vec.iter() {
            if *f > max_root && roots.contains(f) {
                max_root = *f;
            }
        }
        max_root
    }

    pub fn insert(
        &mut self,
        slot: Slot,
        pubkey: &Pubkey,
        account_info: T,
        reclaims: &mut Vec<(Slot, T)>,
    ) {
        let _slot_vec = self
            .account_maps
            .entry(*pubkey)
            .or_insert_with(|| RwLock::new(Vec::with_capacity(32)));
        self.update(slot, pubkey, account_info, reclaims);
    }

    // Try to update an item in account_maps. If the account is not
    // already present, then the function will return back Some(account_info) which
    // the caller can then take the write lock and do an 'insert' with the item.
    // It returns None if the item is already present and thus successfully updated.
    pub fn update(
        &self,
        slot: Slot,
        pubkey: &Pubkey,
        account_info: T,
        reclaims: &mut Vec<(Slot, T)>,
    ) -> Option<T> {
        let roots = &self.roots;
        if let Some(lock) = self.account_maps.get(pubkey) {
            let mut slot_vec = lock.write().unwrap();
            // filter out old entries
            reclaims.extend(slot_vec.iter().filter(|(f, _)| *f == slot).cloned());
            slot_vec.retain(|(f, _)| *f != slot);

            // add the new entry
            slot_vec.push((slot, account_info));

            let max_root = Self::get_max_root(roots, &slot_vec);

            reclaims.extend(
                slot_vec
                    .iter()
                    .filter(|(slot, _)| Self::can_purge(max_root, *slot))
                    .cloned(),
            );
            slot_vec.retain(|(slot, _)| !Self::can_purge(max_root, *slot));
            None
        } else {
            Some(account_info)
        }
    }

    pub fn add_index(&mut self, slot: Slot, pubkey: &Pubkey, account_info: T) {
        let entry = self
            .account_maps
            .entry(*pubkey)
            .or_insert_with(|| RwLock::new(vec![]));
        entry.write().unwrap().push((slot, account_info));
    }

    pub fn is_purged(&self, slot: Slot) -> bool {
        slot < self.last_root
    }

    pub fn can_purge(max_root: Slot, slot: Slot) -> bool {
        slot < max_root
    }

    pub fn is_root(&self, slot: Slot) -> bool {
        self.roots.contains(&slot)
    }

    pub fn add_root(&mut self, slot: Slot) {
        assert!(
            (self.last_root == 0 && slot == 0) || (slot >= self.last_root),
            "new roots must be increasing"
        );
        self.last_root = slot;
        self.roots.insert(slot);
    }
    /// Remove the slot when the storage for the slot is freed
    /// Accounts no longer reference this slot.
    pub fn cleanup_dead_slot(&mut self, slot: Slot) {
        self.roots.remove(&slot);
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
        index.cleanup_dead_slot(0);
        assert!(index.is_root(1));
        assert!(!index.is_root(0));
    }

    #[test]
    fn test_cleanup_last() {
        //this behavior might be undefined, clean up should only occur on older slots
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.cleanup_dead_slot(1);
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
    fn test_update_new_slot() {
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
    fn test_update_gc_purged_slot() {
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

    #[test]
    fn test_purge() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<u64>::default();
        let mut gc = Vec::new();
        assert_eq!(Some(12), index.update(1, &key.pubkey(), 12, &mut gc));

        index.insert(1, &key.pubkey(), 12, &mut gc);

        assert_eq!(None, index.update(1, &key.pubkey(), 10, &mut gc));

        let purges = index.purge(&key.pubkey());
        assert_eq!(purges, (vec![], false));
        index.add_root(1);

        let purges = index.purge(&key.pubkey());
        assert_eq!(purges, (vec![(1, 10)], true));

        assert_eq!(None, index.update(1, &key.pubkey(), 9, &mut gc));
    }
}
