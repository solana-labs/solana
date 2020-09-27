use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::RangeBounds,
    sync::{RwLock, RwLockReadGuard},
};

pub type SlotList<T> = Vec<(Slot, T)>;
pub type SlotSlice<'s, T> = &'s [(Slot, T)];
pub type Ancestors = HashMap<Slot, usize>;

pub type RefCount = u64;
type AccountMapEntry<T> = (AtomicU64, RwLock<SlotList<T>>);
pub type AccountMap<K, V> = BTreeMap<K, V>;

#[derive(Debug, Default)]
pub struct AccountsIndex<T> {
    pub account_maps: AccountMap<Pubkey, AccountMapEntry<T>>,

    pub roots: HashSet<Slot>,
    pub uncleaned_roots: HashSet<Slot>,
    pub previous_uncleaned_roots: HashSet<Slot>,
}

impl<'a, T: 'a + Clone> AccountsIndex<T> {
    fn do_scan_accounts<F, I>(&self, ancestors: &Ancestors, mut func: F, iter: I)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
        I: Iterator<Item = (&'a Pubkey, &'a AccountMapEntry<T>)>,
    {
        for (pubkey, list) in iter {
            let list_r = &list.1.read().unwrap();
            if let Some(index) = self.latest_slot(Some(ancestors), &list_r, None) {
                func(pubkey, (&list_r[index].1, list_r[index].0));
            }
        }
    }

    /// call func with every pubkey and index visible from a given set of ancestors
    pub(crate) fn scan_accounts<F>(&self, ancestors: &Ancestors, func: F)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        self.do_scan_accounts(ancestors, func, self.account_maps.iter());
    }

    /// call func with every pubkey and index visible from a given set of ancestors with range
    pub(crate) fn range_scan_accounts<F, R>(&self, ancestors: &Ancestors, range: R, func: F)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        self.do_scan_accounts(ancestors, func, self.account_maps.range(range));
    }

    fn get_rooted_entries(&self, slice: SlotSlice<T>) -> SlotList<T> {
        slice
            .iter()
            .filter(|(slot, _)| self.is_root(*slot))
            .cloned()
            .collect()
    }

    // returns the rooted entries and the storage ref count
    pub fn would_purge(&self, pubkey: &Pubkey) -> (SlotList<T>, RefCount) {
        let (ref_count, slots_list) = self.account_maps.get(&pubkey).unwrap();
        let slots_list_r = &slots_list.read().unwrap();
        (
            self.get_rooted_entries(&slots_list_r),
            ref_count.load(Ordering::Relaxed),
        )
    }

    // filter any rooted entries and return them along with a bool that indicates
    // if this account has no more entries.
    pub fn purge(&self, pubkey: &Pubkey) -> (SlotList<T>, bool) {
        let list = &mut self.account_maps.get(&pubkey).unwrap().1.write().unwrap();
        let reclaims = self.get_rooted_entries(&list);
        list.retain(|(slot, _)| !self.is_root(*slot));
        (reclaims, list.is_empty())
    }

    pub fn purge_exact(&self, pubkey: &Pubkey, slots: HashSet<Slot>) -> (SlotList<T>, bool) {
        let list = &mut self.account_maps.get(&pubkey).unwrap().1.write().unwrap();
        let reclaims = list
            .iter()
            .filter(|(slot, _)| slots.contains(&slot))
            .cloned()
            .collect();
        list.retain(|(slot, _)| !slots.contains(slot));
        (reclaims, list.is_empty())
    }

    // Given a SlotSlice `L`, a list of ancestors and a maximum slot, find the latest element
    // in `L`, where the slot `S < max_slot`, and `S` is an ancestor or root.
    fn latest_slot(
        &self,
        ancestors: Option<&Ancestors>,
        slice: SlotSlice<T>,
        max_slot: Option<Slot>,
    ) -> Option<usize> {
        let mut current_max = 0;
        let max_slot = max_slot.unwrap_or(std::u64::MAX);

        let mut rv = None;
        for (i, (slot, _t)) in slice.iter().rev().enumerate() {
            if *slot >= current_max
                && *slot <= max_slot
                && self.is_ancestor_or_root(ancestors, *slot)
            {
                rv = Some((slice.len() - 1) - i);
                current_max = *slot;
            }
        }

        rv
    }

    // Checks that the given slot is either:
    // 1) in the `ancestors` set
    // 2) or is a root
    fn is_ancestor_or_root(&self, ancestors: Option<&Ancestors>, slot: Slot) -> bool {
        ancestors.map_or(false, |ancestors| ancestors.contains_key(&slot)) || (self.is_root(slot))
    }

    /// Get an account
    /// The latest account that appears in `ancestors` or `roots` is returned.
    pub(crate) fn get(
        &self,
        pubkey: &Pubkey,
        ancestors: Option<&Ancestors>,
        max_root: Option<Slot>,
    ) -> Option<(RwLockReadGuard<SlotList<T>>, usize)> {
        self.account_maps.get(pubkey).and_then(|list| {
            let list_r = list.1.read().unwrap();
            let lock = &list_r;
            let found_index = self.latest_slot(ancestors, &lock, max_root)?;
            Some((list_r, found_index))
        })
    }

    pub fn get_max_root(roots: &HashSet<Slot>, slice: SlotSlice<T>) -> Slot {
        let mut max_root = 0;
        for (f, _) in slice.iter() {
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
        reclaims: &mut SlotList<T>,
    ) {
        self.account_maps
            .entry(*pubkey)
            .or_insert_with(|| (AtomicU64::new(0), RwLock::new(SlotList::with_capacity(32))));
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
        reclaims: &mut SlotList<T>,
    ) -> Option<T> {
        if let Some(lock) = self.account_maps.get(pubkey) {
            let list = &mut lock.1.write().unwrap();
            // filter out other dirty entries from the same slot
            let mut same_slot_previous_updates: Vec<(usize, (Slot, &T))> = list
                .iter()
                .enumerate()
                .filter_map(|(i, (s, value))| {
                    if *s == slot {
                        Some((i, (*s, value)))
                    } else {
                        None
                    }
                })
                .collect();
            assert!(same_slot_previous_updates.len() <= 1);

            if let Some((list_index, (s, previous_update_value))) = same_slot_previous_updates.pop()
            {
                reclaims.push((s, previous_update_value.clone()));
                list.remove(list_index);
            } else {
                // Only increment ref count if the account was not prevously updated in this slot
                lock.0.fetch_add(1, Ordering::Relaxed);
            }
            list.push((slot, account_info));
            None
        } else {
            Some(account_info)
        }
    }

    pub fn unref_from_storage(&self, pubkey: &Pubkey) {
        let locked_entry = self.account_maps.get(pubkey);
        if let Some(entry) = locked_entry {
            entry.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn ref_count_from_storage(&self, pubkey: &Pubkey) -> RefCount {
        let locked_entry = self.account_maps.get(pubkey);
        if let Some(entry) = locked_entry {
            entry.0.load(Ordering::Relaxed)
        } else {
            0
        }
    }

    fn purge_older_root_entries(
        &self,
        list: &mut SlotList<T>,
        reclaims: &mut SlotList<T>,
        max_clean_root: Option<Slot>,
    ) {
        let roots = &self.roots;

        let mut max_root = Self::get_max_root(roots, &list);
        if let Some(max_clean_root) = max_clean_root {
            max_root = std::cmp::min(max_root, max_clean_root);
        }

        reclaims.extend(
            list.iter()
                .filter(|(slot, _)| Self::can_purge(max_root, *slot))
                .cloned(),
        );
        list.retain(|(slot, _)| !Self::can_purge(max_root, *slot));
    }

    pub fn clean_rooted_entries(
        &self,
        pubkey: &Pubkey,
        reclaims: &mut SlotList<T>,
        max_clean_root: Option<Slot>,
    ) {
        if let Some(locked_entry) = self.account_maps.get(pubkey) {
            let mut list = locked_entry.1.write().unwrap();
            self.purge_older_root_entries(&mut list, reclaims, max_clean_root);
        }
    }

    pub fn clean_unrooted_entries_by_slot(
        &self,
        purge_slot: Slot,
        pubkey: &Pubkey,
        reclaims: &mut SlotList<T>,
    ) {
        if let Some(entry) = self.account_maps.get(pubkey) {
            let mut list = entry.1.write().unwrap();
            list.retain(|(slot, entry)| {
                if *slot == purge_slot {
                    reclaims.push((*slot, entry.clone()));
                }
                *slot != purge_slot
            });
        }
    }

    pub fn add_index(&mut self, slot: Slot, pubkey: &Pubkey, account_info: T) {
        let entry = self
            .account_maps
            .entry(*pubkey)
            .or_insert_with(|| (AtomicU64::new(1), RwLock::new(vec![])));
        entry.1.write().unwrap().push((slot, account_info));
    }

    pub fn can_purge(max_root: Slot, slot: Slot) -> bool {
        slot < max_root
    }

    pub fn is_root(&self, slot: Slot) -> bool {
        self.roots.contains(&slot)
    }

    pub fn add_root(&mut self, slot: Slot) {
        self.roots.insert(slot);
        self.uncleaned_roots.insert(slot);
    }
    /// Remove the slot when the storage for the slot is freed
    /// Accounts no longer reference this slot.
    pub fn clean_dead_slot(&mut self, slot: Slot) {
        self.roots.remove(&slot);
        self.uncleaned_roots.remove(&slot);
        self.previous_uncleaned_roots.remove(&slot);
    }

    pub fn reset_uncleaned_roots(&mut self, max_clean_root: Option<Slot>) -> HashSet<Slot> {
        let mut cleaned_roots = HashSet::new();
        self.uncleaned_roots.retain(|root| {
            let is_cleaned = max_clean_root
                .map(|max_clean_root| *root <= max_clean_root)
                .unwrap_or(true);
            if is_cleaned {
                cleaned_roots.insert(*root);
            }
            // Only keep the slots that have yet to be cleaned
            !is_cleaned
        });
        std::mem::replace(&mut self.previous_uncleaned_roots, cleaned_roots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::{Keypair, Signer};

    #[test]
    fn test_get_empty() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let ancestors = HashMap::new();
        assert!(index.get(&key.pubkey(), Some(&ancestors), None).is_none());
        assert!(index.get(&key.pubkey(), None, None).is_none());

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
        assert!(index.get(&key.pubkey(), Some(&ancestors), None).is_none());
        assert!(index.get(&key.pubkey(), None, None).is_none());

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
        assert!(index.get(&key.pubkey(), Some(&ancestors), None).is_none());

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
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
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

        index.add_root(0);
        let (list, idx) = index.get(&key.pubkey(), None, None).unwrap();
        assert_eq!(list[idx], (0, true));
    }

    #[test]
    fn test_clean_first() {
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.clean_dead_slot(0);
        assert!(index.is_root(1));
        assert!(!index.is_root(0));
    }

    #[test]
    fn test_clean_last() {
        //this behavior might be undefined, clean up should only occur on older slots
        let mut index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.clean_dead_slot(1);
        assert!(!index.is_root(1));
        assert!(index.is_root(0));
    }

    #[test]
    fn test_clean_and_unclean_slot() {
        let mut index = AccountsIndex::<bool>::default();
        assert_eq!(0, index.uncleaned_roots.len());
        index.add_root(0);
        index.add_root(1);
        assert_eq!(2, index.uncleaned_roots.len());

        assert_eq!(0, index.previous_uncleaned_roots.len());
        index.reset_uncleaned_roots(None);
        assert_eq!(2, index.roots.len());
        assert_eq!(0, index.uncleaned_roots.len());
        assert_eq!(2, index.previous_uncleaned_roots.len());

        index.add_root(2);
        index.add_root(3);
        assert_eq!(4, index.roots.len());
        assert_eq!(2, index.uncleaned_roots.len());
        assert_eq!(2, index.previous_uncleaned_roots.len());

        index.clean_dead_slot(1);
        assert_eq!(3, index.roots.len());
        assert_eq!(2, index.uncleaned_roots.len());
        assert_eq!(1, index.previous_uncleaned_roots.len());

        index.clean_dead_slot(2);
        assert_eq!(2, index.roots.len());
        assert_eq!(1, index.uncleaned_roots.len());
        assert_eq!(1, index.previous_uncleaned_roots.len());
    }

    #[test]
    fn test_update_last_wins() {
        let key = Keypair::new();
        let mut index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list[idx], (0, true));
        drop(list);

        let mut gc = Vec::new();
        index.insert(0, &key.pubkey(), false, &mut gc);
        assert_eq!(gc, vec![(0, true)]);
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
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
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list[idx], (0, true));
        let ancestors = vec![(1, 0)].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
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

        // Updating index should not purge older roots, only purges
        // previous updates within the same slot
        assert_eq!(gc, vec![]);
        let (list, idx) = index.get(&key.pubkey(), None, None).unwrap();
        assert_eq!(list[idx], (3, true));

        let mut num = 0;
        let mut found_key = false;
        index.scan_accounts(&Ancestors::new(), |pubkey, _index| {
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

    #[test]
    fn test_latest_slot() {
        let slot_slice = vec![(0, true), (5, true), (3, true), (7, true)];
        let mut index = AccountsIndex::<bool>::default();

        // No ancestors, no root, should return None
        assert!(index.latest_slot(None, &slot_slice, None).is_none());

        // Given a root, should return the root
        index.add_root(5);
        assert_eq!(index.latest_slot(None, &slot_slice, None).unwrap(), 1);

        // Given a maximum -= root, should still return the root
        assert_eq!(index.latest_slot(None, &slot_slice, Some(5)).unwrap(), 1);

        // Given a maximum < root, should filter out the root
        assert!(index.latest_slot(None, &slot_slice, Some(4)).is_none());

        // Given a maximum, should filter out the ancestors > maximum
        let ancestors: HashMap<Slot, usize> = vec![(3, 1), (7, 1)].into_iter().collect();
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(4))
                .unwrap(),
            2
        );
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(7))
                .unwrap(),
            3
        );

        // Given no maximum, should just return the greatest ancestor or root
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, None)
                .unwrap(),
            3
        );

        // Because the given maximum `m == root`, ancestors > root
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(5))
                .unwrap(),
            1
        );
    }
}
