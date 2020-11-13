use ouroboros::self_referencing;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::ops::{
    Bound,
    Bound::{Excluded, Included, Unbounded},
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::{
        btree_map::{self, BTreeMap},
        HashMap, HashSet,
    },
    ops::{Range, RangeBounds},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};
const ITER_BATCH_SIZE: usize = 1000;

pub type SlotList<T> = Vec<(Slot, T)>;
pub type SlotSlice<'s, T> = &'s [(Slot, T)];
pub type Ancestors = HashMap<Slot, usize>;

pub type RefCount = u64;
pub type AccountMap<K, V> = BTreeMap<K, V>;

type AccountMapEntry<T> = Arc<AccountMapEntryInner<T>>;

#[derive(Debug)]
pub struct AccountMapEntryInner<T> {
    ref_count: AtomicU64,
    pub slot_list: RwLock<SlotList<T>>,
}

#[self_referencing]
pub struct ReadAccountMapEntry<T: 'static> {
    owned_entry: AccountMapEntry<T>,
    #[borrows(owned_entry)]
    slot_list_guard: RwLockReadGuard<'this, SlotList<T>>,
}

impl<T: Clone> ReadAccountMapEntry<T> {
    pub fn from_account_map_entry(account_map_entry: AccountMapEntry<T>) -> Self {
        ReadAccountMapEntryBuilder {
            owned_entry: account_map_entry,
            slot_list_guard_builder: |lock| lock.slot_list.read().unwrap(),
        }
        .build()
    }

    pub fn slot_list(&self) -> &SlotList<T> {
        &*self.borrow_slot_list_guard()
    }

    pub fn ref_count(&self) -> &AtomicU64 {
        &self.borrow_owned_entry_contents().ref_count
    }
}

#[self_referencing]
pub struct WriteAccountMapEntry<T: 'static> {
    owned_entry: AccountMapEntry<T>,
    #[borrows(owned_entry)]
    slot_list_guard: RwLockWriteGuard<'this, SlotList<T>>,
}

impl<T: 'static + Clone> WriteAccountMapEntry<T> {
    pub fn from_account_map_entry(account_map_entry: AccountMapEntry<T>) -> Self {
        WriteAccountMapEntryBuilder {
            owned_entry: account_map_entry,
            slot_list_guard_builder: |lock| lock.slot_list.write().unwrap(),
        }
        .build()
    }

    pub fn slot_list(&mut self) -> &SlotList<T> {
        &*self.borrow_slot_list_guard()
    }

    pub fn slot_list_mut<RT>(
        &mut self,
        user: impl for<'this> FnOnce(&mut RwLockWriteGuard<'this, SlotList<T>>) -> RT,
    ) -> RT {
        self.with_slot_list_guard_mut(user)
    }

    pub fn ref_count(&self) -> &AtomicU64 {
        &self.borrow_owned_entry_contents().ref_count
    }

    // Try to update an item in the slot list the given `slot` If an item for the slot
    // already exists in the list, remove the older item, add it to `reclaims`, and insert
    // the new item.
    pub fn update(&mut self, slot: Slot, account_info: T, reclaims: &mut SlotList<T>) {
        // filter out other dirty entries from the same slot
        let mut same_slot_previous_updates: Vec<(usize, &(Slot, T))> = self
            .slot_list()
            .iter()
            .enumerate()
            .filter(|(_, (s, _))| *s == slot)
            .collect();
        assert!(same_slot_previous_updates.len() <= 1);
        if let Some((list_index, (s, previous_update_value))) = same_slot_previous_updates.pop() {
            reclaims.push((*s, previous_update_value.clone()));
            self.slot_list_mut(|list| list.remove(list_index));
        } else {
            // Only increment ref count if the account was not prevously updated in this slot
            self.ref_count().fetch_add(1, Ordering::Relaxed);
        }
        self.slot_list_mut(|list| list.push((slot, account_info)));
    }
}

#[derive(Debug, Default)]
pub struct RootsTracker {
    roots: HashSet<Slot>,
    uncleaned_roots: HashSet<Slot>,
    previous_uncleaned_roots: HashSet<Slot>,
}

pub struct AccountsIndexIterator<'a, T> {
    account_maps: &'a RwLock<AccountMap<Pubkey, AccountMapEntry<T>>>,
    start_bound: Bound<Pubkey>,
    end_bound: Bound<Pubkey>,
    is_finished: bool,
}

impl<'a, T> AccountsIndexIterator<'a, T> {
    fn clone_bound(bound: Bound<&Pubkey>) -> Bound<Pubkey> {
        match bound {
            Unbounded => Unbounded,
            Included(k) => Included(*k),
            Excluded(k) => Excluded(*k),
        }
    }

    pub fn new<R>(
        account_maps: &'a RwLock<AccountMap<Pubkey, AccountMapEntry<T>>>,
        range: Option<R>,
    ) -> Self
    where
        R: RangeBounds<Pubkey>,
    {
        Self {
            start_bound: range
                .as_ref()
                .map(|r| Self::clone_bound(r.start_bound()))
                .unwrap_or(Unbounded),
            end_bound: range
                .as_ref()
                .map(|r| Self::clone_bound(r.end_bound()))
                .unwrap_or(Unbounded),
            account_maps,
            is_finished: false,
        }
    }
}

impl<'a, T: 'static + Clone> Iterator for AccountsIndexIterator<'a, T> {
    type Item = Vec<(Pubkey, AccountMapEntry<T>)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_finished {
            return None;
        }

        let chunk: Vec<(Pubkey, AccountMapEntry<T>)> = self
            .account_maps
            .read()
            .unwrap()
            .range((self.start_bound, self.end_bound))
            .map(|(pubkey, account_map_entry)| (*pubkey, account_map_entry.clone()))
            .take(ITER_BATCH_SIZE)
            .collect();

        if chunk.is_empty() {
            self.is_finished = true;
            return None;
        }

        self.start_bound = Excluded(chunk.last().unwrap().0);
        Some(chunk)
    }
}

#[derive(Debug, Default)]
pub struct AccountsIndex<T> {
    pub account_maps: RwLock<AccountMap<Pubkey, AccountMapEntry<T>>>,
    roots_tracker: RwLock<RootsTracker>,
}

impl<T: 'static + Clone> AccountsIndex<T> {
    fn iter<R>(&self, range: Option<R>) -> AccountsIndexIterator<T>
    where
        R: RangeBounds<Pubkey>,
    {
        AccountsIndexIterator::new(&self.account_maps, range)
    }

    fn do_scan_accounts<'a, F, R>(&'a self, ancestors: &Ancestors, mut func: F, range: Option<R>)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        for pubkey_list in self.iter(range) {
            for (pubkey, list) in pubkey_list {
                let list_r = &list.slot_list.read().unwrap();
                if let Some(index) = self.latest_slot(Some(ancestors), &list_r, None) {
                    func(&pubkey, (&list_r[index].1, list_r[index].0));
                }
            }
        }
    }

    pub fn get_account_read_entry(&self, pubkey: &Pubkey) -> Option<ReadAccountMapEntry<T>> {
        self.account_maps
            .read()
            .unwrap()
            .get(pubkey)
            .cloned()
            .map(ReadAccountMapEntry::from_account_map_entry)
    }

    fn get_account_write_entry(&self, pubkey: &Pubkey) -> Option<WriteAccountMapEntry<T>> {
        self.account_maps
            .read()
            .unwrap()
            .get(pubkey)
            .cloned()
            .map(WriteAccountMapEntry::from_account_map_entry)
    }

    fn get_account_write_entry_else_create(
        &self,
        pubkey: &Pubkey,
    ) -> (WriteAccountMapEntry<T>, bool) {
        let mut w_account_entry = self.get_account_write_entry(pubkey);
        let mut is_newly_inserted = false;
        if w_account_entry.is_none() {
            let new_entry = Arc::new(AccountMapEntryInner {
                ref_count: AtomicU64::new(0),
                slot_list: RwLock::new(SlotList::with_capacity(32)),
            });
            let mut w_account_maps = self.account_maps.write().unwrap();
            let account_entry = w_account_maps.entry(*pubkey).or_insert_with(|| {
                is_newly_inserted = true;
                new_entry
            });
            w_account_entry = Some(WriteAccountMapEntry::from_account_map_entry(
                account_entry.clone(),
            ));
        }

        (w_account_entry.unwrap(), is_newly_inserted)
    }

    pub fn handle_dead_keys(&self, dead_keys: &[Pubkey]) {
        if !dead_keys.is_empty() {
            for key in dead_keys.iter() {
                let mut w_index = self.account_maps.write().unwrap();
                if let btree_map::Entry::Occupied(index_entry) = w_index.entry(*key) {
                    if index_entry.get().slot_list.read().unwrap().is_empty() {
                        index_entry.remove();
                    }
                }
            }
        }
    }

    /// call func with every pubkey and index visible from a given set of ancestors
    pub(crate) fn scan_accounts<F>(&self, ancestors: &Ancestors, func: F)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        self.do_scan_accounts(ancestors, func, None::<Range<Pubkey>>);
    }

    /// call func with every pubkey and index visible from a given set of ancestors with range
    pub(crate) fn range_scan_accounts<F, R>(&self, ancestors: &Ancestors, range: R, func: F)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        self.do_scan_accounts(ancestors, func, Some(range));
    }

    pub fn get_rooted_entries(&self, slice: SlotSlice<T>) -> SlotList<T> {
        slice
            .iter()
            .filter(|(slot, _)| self.is_root(*slot))
            .cloned()
            .collect()
    }

    // returns the rooted entries and the storage ref count
    pub fn roots_and_ref_count(
        &self,
        locked_account_entry: &ReadAccountMapEntry<T>,
    ) -> (SlotList<T>, RefCount) {
        (
            self.get_rooted_entries(&locked_account_entry.slot_list()),
            locked_account_entry.ref_count().load(Ordering::Relaxed),
        )
    }

    // filter any rooted entries and return them along with a bool that indicates
    // if this account has no more entries.
    pub fn purge(&self, pubkey: &Pubkey) -> (SlotList<T>, bool) {
        let mut write_account_map_entry = self.get_account_write_entry(pubkey).unwrap();
        write_account_map_entry.slot_list_mut(|slot_list| {
            let reclaims = self.get_rooted_entries(slot_list);
            slot_list.retain(|(slot, _)| !self.is_root(*slot));
            (reclaims, slot_list.is_empty())
        })
    }

    pub fn purge_exact(&self, pubkey: &Pubkey, slots: HashSet<Slot>) -> (SlotList<T>, bool) {
        let mut write_account_map_entry = self.get_account_write_entry(pubkey).unwrap();
        write_account_map_entry.slot_list_mut(|slot_list| {
            let reclaims = slot_list
                .iter()
                .filter(|(slot, _)| slots.contains(&slot))
                .cloned()
                .collect();
            slot_list.retain(|(slot, _)| !slots.contains(slot));
            (reclaims, slot_list.is_empty())
        })
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
    ) -> Option<(ReadAccountMapEntry<T>, usize)> {
        self.get_account_read_entry(pubkey)
            .and_then(|locked_entry| {
                let found_index =
                    self.latest_slot(ancestors, &locked_entry.slot_list(), max_root)?;
                Some((locked_entry, found_index))
            })
    }

    // Get the maximum root <= `max_allowed_root` from the given `slice`
    fn get_max_root(
        roots: &HashSet<Slot>,
        slice: SlotSlice<T>,
        max_allowed_root: Option<Slot>,
    ) -> Slot {
        let mut max_root = 0;
        for (f, _) in slice.iter() {
            if let Some(max_allowed_root) = max_allowed_root {
                if *f > max_allowed_root {
                    continue;
                }
            }
            if *f > max_root && roots.contains(f) {
                max_root = *f;
            }
        }
        max_root
    }

    // Updates the given pubkey at the given slot with the new account information.
    // Returns true if the pubkey was newly inserted into the index, otherwise, if the
    // pubkey updates an existing entry in the index, returns false.
    pub fn upsert(
        &self,
        slot: Slot,
        pubkey: &Pubkey,
        account_info: T,
        reclaims: &mut SlotList<T>,
    ) -> bool {
        let (mut w_account_entry, is_newly_inserted) =
            self.get_account_write_entry_else_create(pubkey);
        w_account_entry.update(slot, account_info, reclaims);
        is_newly_inserted
    }

    pub fn unref_from_storage(&self, pubkey: &Pubkey) {
        if let Some(locked_entry) = self.get_account_read_entry(pubkey) {
            locked_entry.ref_count().fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn ref_count_from_storage(&self, pubkey: &Pubkey) -> RefCount {
        if let Some(locked_entry) = self.get_account_read_entry(pubkey) {
            locked_entry.ref_count().load(Ordering::Relaxed)
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
        let roots_traker = &self.roots_tracker.read().unwrap();

        let max_root = Self::get_max_root(&roots_traker.roots, &list, max_clean_root);

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
        if let Some(mut locked_entry) = self.get_account_write_entry(pubkey) {
            locked_entry.slot_list_mut(|slot_list| {
                self.purge_older_root_entries(slot_list, reclaims, max_clean_root);
            });
        }
    }

    pub fn clean_unrooted_entries_by_slot(
        &self,
        purge_slot: Slot,
        pubkey: &Pubkey,
        reclaims: &mut SlotList<T>,
    ) {
        if let Some(mut locked_entry) = self.get_account_write_entry(pubkey) {
            locked_entry.slot_list_mut(|slot_list| {
                slot_list.retain(|(slot, entry)| {
                    if *slot == purge_slot {
                        reclaims.push((*slot, entry.clone()));
                    }
                    *slot != purge_slot
                });
            });
        }
    }

    pub fn can_purge(max_root: Slot, slot: Slot) -> bool {
        slot < max_root
    }

    pub fn is_root(&self, slot: Slot) -> bool {
        self.roots_tracker.read().unwrap().roots.contains(&slot)
    }

    pub fn add_root(&self, slot: Slot) {
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.roots.insert(slot);
        w_roots_tracker.uncleaned_roots.insert(slot);
    }
    /// Remove the slot when the storage for the slot is freed
    /// Accounts no longer reference this slot.
    pub fn clean_dead_slot(&self, slot: Slot) {
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.roots.remove(&slot);
        w_roots_tracker.uncleaned_roots.remove(&slot);
        w_roots_tracker.previous_uncleaned_roots.remove(&slot);
    }

    pub fn reset_uncleaned_roots(&self, max_clean_root: Option<Slot>) -> HashSet<Slot> {
        let mut cleaned_roots = HashSet::new();
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.uncleaned_roots.retain(|root| {
            let is_cleaned = max_clean_root
                .map(|max_clean_root| *root <= max_clean_root)
                .unwrap_or(true);
            if is_cleaned {
                cleaned_roots.insert(*root);
            }
            // Only keep the slots that have yet to be cleaned
            !is_cleaned
        });
        std::mem::replace(&mut w_roots_tracker.previous_uncleaned_roots, cleaned_roots)
    }

    pub fn is_uncleaned_root(&self, slot: Slot) -> bool {
        self.roots_tracker
            .read()
            .unwrap()
            .uncleaned_roots
            .contains(&slot)
    }

    pub fn all_roots(&self) -> Vec<Slot> {
        self.roots_tracker
            .read()
            .unwrap()
            .roots
            .iter()
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub fn clear_roots(&self) {
        self.roots_tracker.write().unwrap().roots.clear()
    }

    #[cfg(test)]
    pub fn uncleaned_roots_len(&self) -> usize {
        self.roots_tracker.read().unwrap().uncleaned_roots.len()
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
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
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
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
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
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());

        let ancestors = vec![(0, 0)].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));

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

    fn setup_accounts_index_keys(num_pubkeys: usize) -> (AccountsIndex<bool>, Vec<Pubkey>) {
        let index = AccountsIndex::<bool>::default();
        let root_slot = 0;

        let mut pubkeys: Vec<Pubkey> = std::iter::repeat_with(|| {
            let new_pubkey = solana_sdk::pubkey::new_rand();
            index.upsert(root_slot, &new_pubkey, true, &mut vec![]);
            new_pubkey
        })
        .take(num_pubkeys.saturating_sub(1))
        .collect();

        if num_pubkeys != 0 {
            pubkeys.push(Pubkey::default());
            index.upsert(root_slot, &Pubkey::default(), true, &mut vec![]);
        }

        index.add_root(root_slot);

        (index, pubkeys)
    }

    fn run_test_range(
        index: &AccountsIndex<bool>,
        pubkeys: &[Pubkey],
        start_bound: Bound<usize>,
        end_bound: Bound<usize>,
    ) {
        // Exclusive `index_start`
        let (pubkey_start, index_start) = match start_bound {
            Unbounded => (Unbounded, 0),
            Included(i) => (Included(pubkeys[i]), i),
            Excluded(i) => (Excluded(pubkeys[i]), i + 1),
        };

        // Exclusive `index_end`
        let (pubkey_end, index_end) = match end_bound {
            Unbounded => (Unbounded, pubkeys.len()),
            Included(i) => (Included(pubkeys[i]), i + 1),
            Excluded(i) => (Excluded(pubkeys[i]), i),
        };
        let pubkey_range = (pubkey_start, pubkey_end);

        let ancestors: Ancestors = HashMap::new();
        let mut scanned_keys = HashSet::new();
        index.range_scan_accounts(&ancestors, pubkey_range, |pubkey, _index| {
            scanned_keys.insert(*pubkey);
        });

        let mut expected_len = 0;
        for key in &pubkeys[index_start..index_end] {
            expected_len += 1;
            assert!(scanned_keys.contains(key));
        }

        assert_eq!(scanned_keys.len(), expected_len);
    }

    fn run_test_range_indexes(
        index: &AccountsIndex<bool>,
        pubkeys: &[Pubkey],
        start: Option<usize>,
        end: Option<usize>,
    ) {
        let start_options = start
            .map(|i| vec![Included(i), Excluded(i)])
            .unwrap_or_else(|| vec![Unbounded]);
        let end_options = end
            .map(|i| vec![Included(i), Excluded(i)])
            .unwrap_or_else(|| vec![Unbounded]);

        for start in &start_options {
            for end in &end_options {
                run_test_range(index, pubkeys, *start, *end);
            }
        }
    }

    #[test]
    fn test_range_scan_accounts() {
        let (index, mut pubkeys) = setup_accounts_index_keys(3 * ITER_BATCH_SIZE);
        pubkeys.sort();

        run_test_range_indexes(&index, &pubkeys, None, None);

        run_test_range_indexes(&index, &pubkeys, Some(ITER_BATCH_SIZE), None);

        run_test_range_indexes(&index, &pubkeys, None, Some(2 * ITER_BATCH_SIZE as usize));

        run_test_range_indexes(
            &index,
            &pubkeys,
            Some(ITER_BATCH_SIZE as usize),
            Some(2 * ITER_BATCH_SIZE as usize),
        );

        run_test_range_indexes(
            &index,
            &pubkeys,
            Some(ITER_BATCH_SIZE as usize),
            Some(2 * ITER_BATCH_SIZE as usize - 1),
        );

        run_test_range_indexes(
            &index,
            &pubkeys,
            Some(ITER_BATCH_SIZE - 1 as usize),
            Some(2 * ITER_BATCH_SIZE as usize + 1),
        );
    }

    fn run_test_scan_accounts(num_pubkeys: usize) {
        let (index, _) = setup_accounts_index_keys(num_pubkeys);
        let ancestors: Ancestors = HashMap::new();

        let mut scanned_keys = HashSet::new();
        index.scan_accounts(&ancestors, |pubkey, _index| {
            scanned_keys.insert(*pubkey);
        });
        assert_eq!(scanned_keys.len(), num_pubkeys);
    }

    #[test]
    fn test_scan_accounts() {
        run_test_scan_accounts(0);
        run_test_scan_accounts(1);
        run_test_scan_accounts(ITER_BATCH_SIZE * 10);
        run_test_scan_accounts(ITER_BATCH_SIZE * 10 - 1);
        run_test_scan_accounts(ITER_BATCH_SIZE * 10 + 1);
    }

    #[test]
    fn test_accounts_iter_finished() {
        let (index, _) = setup_accounts_index_keys(0);
        let mut iter = index.iter(None::<Range<Pubkey>>);
        assert!(iter.next().is_none());
        let mut gc = vec![];
        index.upsert(0, &solana_sdk::pubkey::new_rand(), true, &mut gc);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_is_root() {
        let index = AccountsIndex::<bool>::default();
        assert!(!index.is_root(0));
        index.add_root(0);
        assert!(index.is_root(0));
    }

    #[test]
    fn test_insert_with_root() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());

        index.add_root(0);
        let (list, idx) = index.get(&key.pubkey(), None, None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
    }

    #[test]
    fn test_clean_first() {
        let index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.clean_dead_slot(0);
        assert!(index.is_root(1));
        assert!(!index.is_root(0));
    }

    #[test]
    fn test_clean_last() {
        //this behavior might be undefined, clean up should only occur on older slots
        let index = AccountsIndex::<bool>::default();
        index.add_root(0);
        index.add_root(1);
        index.clean_dead_slot(1);
        assert!(!index.is_root(1));
        assert!(index.is_root(0));
    }

    #[test]
    fn test_clean_and_unclean_slot() {
        let index = AccountsIndex::<bool>::default();
        assert_eq!(0, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        index.add_root(0);
        index.add_root(1);
        assert_eq!(2, index.roots_tracker.read().unwrap().uncleaned_roots.len());

        assert_eq!(
            0,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );
        index.reset_uncleaned_roots(None);
        assert_eq!(2, index.roots_tracker.read().unwrap().roots.len());
        assert_eq!(0, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            2,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );

        index.add_root(2);
        index.add_root(3);
        assert_eq!(4, index.roots_tracker.read().unwrap().roots.len());
        assert_eq!(2, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            2,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );

        index.clean_dead_slot(1);
        assert_eq!(3, index.roots_tracker.read().unwrap().roots.len());
        assert_eq!(2, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            1,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );

        index.clean_dead_slot(2);
        assert_eq!(2, index.roots_tracker.read().unwrap().roots.len());
        assert_eq!(1, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            1,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );
    }

    #[test]
    fn test_update_last_wins() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
        drop(list);

        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), false, &mut gc);
        assert_eq!(gc, vec![(0, true)]);
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, false));
    }

    #[test]
    fn test_update_new_slot() {
        solana_logger::setup();
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        index.upsert(1, &key.pubkey(), false, &mut gc);
        assert!(gc.is_empty());
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
        let ancestors = vec![(1, 0)].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (1, false));
    }

    #[test]
    fn test_update_gc_purged_slot() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(0, &key.pubkey(), true, &mut gc);
        assert!(gc.is_empty());
        index.upsert(1, &key.pubkey(), false, &mut gc);
        index.upsert(2, &key.pubkey(), true, &mut gc);
        index.upsert(3, &key.pubkey(), true, &mut gc);
        index.add_root(0);
        index.add_root(1);
        index.add_root(3);
        index.upsert(4, &key.pubkey(), true, &mut gc);

        // Updating index should not purge older roots, only purges
        // previous updates within the same slot
        assert_eq!(gc, vec![]);
        let (list, idx) = index.get(&key.pubkey(), None, None).unwrap();
        assert_eq!(list.slot_list()[idx], (3, true));

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
        let index = AccountsIndex::<u64>::default();
        let mut gc = Vec::new();
        assert!(index.upsert(1, &key.pubkey(), 12, &mut gc));

        assert!(!index.upsert(1, &key.pubkey(), 10, &mut gc));

        let purges = index.purge(&key.pubkey());
        assert_eq!(purges, (vec![], false));
        index.add_root(1);

        let purges = index.purge(&key.pubkey());
        assert_eq!(purges, (vec![(1, 10)], true));

        assert!(!index.upsert(1, &key.pubkey(), 9, &mut gc));
    }

    #[test]
    fn test_latest_slot() {
        let slot_slice = vec![(0, true), (5, true), (3, true), (7, true)];
        let index = AccountsIndex::<bool>::default();

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

    #[test]
    fn test_purge_older_root_entries() {
        // No roots, should be no reclaims
        let index = AccountsIndex::<bool>::default();
        let mut slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        let mut reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, None);
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Add a later root, earlier slots should be reclaimed
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        index.add_root(1);
        // Note 2 is not a root
        index.add_root(5);
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, None);
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Add a later root that is not in the list, should not affect the outcome
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        index.add_root(6);
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, None);
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root >= than any root in the slot list, should not affect
        // outcome
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(6));
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root, earlier slots should be reclaimed
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(5));
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root 2. This means the latest root < 2 is 1 because 2 is not a root
        // so nothing will be purged
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(2));
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Pass a max root 1. This means the latest root < 3 is 1 because 2 is not a root
        // so nothing will be purged
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(1));
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Pass a max root that doesn't exist in the list but is greater than
        // some of the roots in the list, shouldn't return those smaller roots
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(7));
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);
    }
}
