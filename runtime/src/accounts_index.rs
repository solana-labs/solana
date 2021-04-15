use crate::{
    contains::Contains,
    inline_spl_token_v2_0::{self, SPL_TOKEN_ACCOUNT_MINT_OFFSET, SPL_TOKEN_ACCOUNT_OWNER_OFFSET},
    secondary_index::*,
};
use dashmap::DashSet;
use ouroboros::self_referencing;
use solana_measure::measure::Measure;
use solana_sdk::{
    clock::Slot,
    pubkey::{Pubkey, PUBKEY_BYTES},
};
use std::{
    collections::{
        btree_map::{self, BTreeMap},
        HashMap, HashSet,
    },
    ops::{
        Bound,
        Bound::{Excluded, Included, Unbounded},
        Range, RangeBounds,
    },
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock, RwLockReadGuard, RwLockWriteGuard,
    },
};

pub const ITER_BATCH_SIZE: usize = 1000;

pub type SlotList<T> = Vec<(Slot, T)>;
pub type SlotSlice<'s, T> = &'s [(Slot, T)];
pub type Ancestors = HashMap<Slot, usize>;

pub type RefCount = u64;
pub type AccountMap<K, V> = BTreeMap<K, V>;

type AccountMapEntry<T> = Arc<AccountMapEntryInner<T>>;

pub trait IsCached {
    fn is_cached(&self) -> bool;
}

impl IsCached for bool {
    fn is_cached(&self) -> bool {
        false
    }
}

impl IsCached for u64 {
    fn is_cached(&self) -> bool {
        false
    }
}

enum ScanTypes<R: RangeBounds<Pubkey>> {
    Unindexed(Option<R>),
    Indexed(IndexKey),
}

#[derive(Debug, Clone, Copy)]
pub enum IndexKey {
    ProgramId(Pubkey),
    SplTokenMint(Pubkey),
    SplTokenOwner(Pubkey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AccountIndex {
    ProgramId,
    SplTokenMint,
    SplTokenOwner,
}

#[derive(Debug)]
pub struct AccountMapEntryInner<T> {
    ref_count: AtomicU64,
    pub slot_list: RwLock<SlotList<T>>,
}

impl<T> AccountMapEntryInner<T> {
    pub fn ref_count(&self) -> u64 {
        self.ref_count.load(Ordering::Relaxed)
    }
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

    pub fn unref(&self) {
        self.ref_count().fetch_sub(1, Ordering::Relaxed);
    }
}

#[self_referencing]
pub struct WriteAccountMapEntry<T: 'static> {
    owned_entry: AccountMapEntry<T>,
    #[borrows(owned_entry)]
    slot_list_guard: RwLockWriteGuard<'this, SlotList<T>>,
}

impl<T: 'static + Clone + IsCached> WriteAccountMapEntry<T> {
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
            let is_flush_from_cache =
                previous_update_value.is_cached() && !account_info.is_cached();
            reclaims.push((*s, previous_update_value.clone()));
            self.slot_list_mut(|list| list.remove(list_index));
            if is_flush_from_cache {
                self.ref_count().fetch_add(1, Ordering::Relaxed);
            }
        } else if !account_info.is_cached() {
            // If it's the first non-cache insert, also bump the stored ref count
            self.ref_count().fetch_add(1, Ordering::Relaxed);
        }

        self.slot_list_mut(|list| list.push((slot, account_info)));
    }
}

#[derive(Debug, Default)]
pub struct RootsTracker {
    roots: HashSet<Slot>,
    max_root: Slot,
    uncleaned_roots: HashSet<Slot>,
    previous_uncleaned_roots: HashSet<Slot>,
}

#[derive(Debug, Default)]
pub struct AccountsIndexRootsStats {
    pub roots_len: usize,
    pub uncleaned_roots_len: usize,
    pub previous_uncleaned_roots_len: usize,
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

pub trait ZeroLamport {
    fn is_zero_lamport(&self) -> bool;
}

#[derive(Debug, Default)]
pub struct AccountsIndex<T> {
    pub account_maps: RwLock<AccountMap<Pubkey, AccountMapEntry<T>>>,
    program_id_index: SecondaryIndex<DashMapSecondaryIndexEntry>,
    spl_token_mint_index: SecondaryIndex<DashMapSecondaryIndexEntry>,
    spl_token_owner_index: SecondaryIndex<RwLockSecondaryIndexEntry>,
    roots_tracker: RwLock<RootsTracker>,
    ongoing_scan_roots: RwLock<BTreeMap<Slot, u64>>,
    zero_lamport_pubkeys: DashSet<Pubkey>,
}

impl<T: 'static + Clone + IsCached + ZeroLamport> AccountsIndex<T> {
    fn iter<R>(&self, range: Option<R>) -> AccountsIndexIterator<T>
    where
        R: RangeBounds<Pubkey>,
    {
        AccountsIndexIterator::new(&self.account_maps, range)
    }

    fn do_checked_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        func: F,
        scan_type: ScanTypes<R>,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        let max_root = {
            let mut w_ongoing_scan_roots = self
                // This lock is also grabbed by clean_accounts(), so clean
                // has at most cleaned up to the current `max_root` (since
                // clean only happens *after* BankForks::set_root() which sets
                // the `max_root`)
                .ongoing_scan_roots
                .write()
                .unwrap();
            // `max_root()` grabs a lock while
            // the `ongoing_scan_roots` lock is held,
            // make sure inverse doesn't happen to avoid
            // deadlock
            let max_root = self.max_root();
            *w_ongoing_scan_roots.entry(max_root).or_default() += 1;

            max_root
        };

        // First we show that for any bank `B` that is a descendant of
        // the current `max_root`, it must be true that and `B.ancestors.contains(max_root)`,
        // regardless of the pattern of `squash()` behavior, where `ancestors` is the set
        // of ancestors that is tracked in each bank.
        //
        // Proof: At startup, if starting from a snapshot, generate_index() adds all banks
        // in the snapshot to the index via `add_root()` and so `max_root` will be the
        // greatest of these. Thus, so the claim holds at startup since there are no
        // descendants of `max_root`.
        //
        // Now we proceed by induction on each `BankForks::set_root()`.
        // Assume the claim holds when the `max_root` is `R`. Call the set of
        // descendants of `R` present in BankForks `R_descendants`.
        //
        // Then for any banks `B` in `R_descendants`, it must be that `B.ancestors.contains(S)`,
        // where `S` is any ancestor of `B` such that `S >= R`.
        //
        // For example:
        //          `R` -> `A` -> `C` -> `B`
        // Then `B.ancestors == {R, A, C}`
        //
        // Next we call `BankForks::set_root()` at some descendant of `R`, `R_new`,
        // where `R_new > R`.
        //
        // When we squash `R_new`, `max_root` in the AccountsIndex here is now set to `R_new`,
        // and all nondescendants of `R_new` are pruned.
        //
        // Now consider any outstanding references to banks in the system that are descended from
        // `max_root == R_new`. Take any one of these references and call it `B`. Because `B` is
        // a descendant of `R_new`, this means `B` was also a descendant of `R`. Thus `B`
        // must be a member of `R_descendants` because `B` was constructed and added to
        // BankForks before the `set_root`.
        //
        // This means by the guarantees of `R_descendants` described above, because
        // `R_new` is an ancestor of `B`, and `R < R_new < B`, then `B.ancestors.contains(R_new)`.
        //
        // Now until the next `set_root`, any new banks constructed from `new_from_parent` will
        // also have `max_root == R_new` in their ancestor set, so the claim holds for those descendants
        // as well. Once the next `set_root` happens, we once again update `max_root` and the same
        // inductive argument can be applied again to show the claim holds.

        // Check that the `max_root` is present in `ancestors`. From the proof above, if
        // `max_root` is not present in `ancestors`, this means the bank `B` with the
        // given `ancestors` is not descended from `max_root, which means
        // either:
        // 1) `B` is on a different fork or
        // 2) `B` is an ancestor of `max_root`.
        // In both cases we can ignore the given ancestors and instead just rely on the roots
        // present as `max_root` indicates the roots present in the index are more up to date
        // than the ancestors given.
        let empty = Ancestors::default();
        let ancestors = if ancestors.contains_key(&max_root) {
            ancestors
        } else {
            /*
            This takes of edge cases like:

            Diagram 1:

                        slot 0
                          |
                        slot 1
                      /        \
                 slot 2         |
                    |       slot 3 (max root)
            slot 4 (scan)

            By the time the scan on slot 4 is called, slot 2 may already have been
            cleaned by a clean on slot 3, but slot 4 may not have been cleaned.
            The state in slot 2 would have been purged and is not saved in any roots.
            In this case, a scan on slot 4 wouldn't accurately reflect the state when bank 4
            was frozen. In cases like this, we default to a scan on the latest roots by
            removing all `ancestors`.
            */
            &empty
        };

        /*
        Now there are two cases, either `ancestors` is empty or nonempty:

        1) If ancestors is empty, then this is the same as a scan on a rooted bank,
        and `ongoing_scan_roots` provides protection against cleanup of roots necessary
        for the scan, and  passing `Some(max_root)` to `do_scan_accounts()` ensures newer
        roots don't appear in the scan.

        2) If ancestors is non-empty, then from the `ancestors_contains(&max_root)` above, we know
        that the fork structure must look something like:

        Diagram 2:

                Build fork structure:
                        slot 0
                          |
                    slot 1 (max_root)
                    /            \
             slot 2              |
                |            slot 3 (potential newer max root)
              slot 4
                |
             slot 5 (scan)

        Consider both types of ancestors, ancestor <= `max_root` and
        ancestor > `max_root`, where `max_root == 1` as illustrated above.

        a) The set of `ancestors <= max_root` are all rooted, which means their state
        is protected by the same guarantees as 1).

        b) As for the `ancestors > max_root`, those banks have at least one reference discoverable
        through the chain of `Bank::BankRc::parent` starting from the calling bank. For instance
        bank 5's parent reference keeps bank 4 alive, which will prevent the `Bank::drop()` from
        running and cleaning up bank 4. Furthermore, no cleans can happen past the saved max_root == 1,
        so a potential newer max root at 3 will not clean up any of the ancestors > 1, so slot 4
        will not be cleaned in the middle of the scan either.
        */
        match scan_type {
            ScanTypes::Unindexed(range) => {
                // Pass "" not to log metrics, so RPC doesn't get spammy
                self.do_scan_accounts(metric_name, ancestors, func, range, Some(max_root));
            }
            ScanTypes::Indexed(IndexKey::ProgramId(program_id)) => {
                self.do_scan_secondary_index(
                    ancestors,
                    func,
                    &self.program_id_index,
                    &program_id,
                    Some(max_root),
                );
            }
            ScanTypes::Indexed(IndexKey::SplTokenMint(mint_key)) => {
                self.do_scan_secondary_index(
                    ancestors,
                    func,
                    &self.spl_token_mint_index,
                    &mint_key,
                    Some(max_root),
                );
            }
            ScanTypes::Indexed(IndexKey::SplTokenOwner(owner_key)) => {
                self.do_scan_secondary_index(
                    ancestors,
                    func,
                    &self.spl_token_owner_index,
                    &owner_key,
                    Some(max_root),
                );
            }
        }

        {
            let mut ongoing_scan_roots = self.ongoing_scan_roots.write().unwrap();
            let count = ongoing_scan_roots.get_mut(&max_root).unwrap();
            *count -= 1;
            if *count == 0 {
                ongoing_scan_roots.remove(&max_root);
            }
        }
    }

    fn do_unchecked_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        func: F,
        range: Option<R>,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        self.do_scan_accounts(metric_name, ancestors, func, range, None);
    }

    // Scan accounts and return latest version of each account that is either:
    // 1) rooted or
    // 2) present in ancestors
    fn do_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        mut func: F,
        range: Option<R>,
        max_root: Option<Slot>,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        // TODO: expand to use mint index to find the `pubkey_list` below more efficiently
        // instead of scanning the entire range
        let mut total_elapsed_timer = Measure::start("total");
        let mut num_keys_iterated = 0;
        let mut latest_slot_elapsed = 0;
        let mut load_account_elapsed = 0;
        let mut read_lock_elapsed = 0;
        let mut iterator_elapsed = 0;
        let mut iterator_timer = Measure::start("iterator_elapsed");
        for pubkey_list in self.iter(range) {
            iterator_timer.stop();
            iterator_elapsed += iterator_timer.as_us();
            for (pubkey, list) in pubkey_list {
                num_keys_iterated += 1;
                let mut read_lock_timer = Measure::start("read_lock");
                let list_r = &list.slot_list.read().unwrap();
                read_lock_timer.stop();
                read_lock_elapsed += read_lock_timer.as_us();
                let mut latest_slot_timer = Measure::start("latest_slot");
                if let Some(index) = self.latest_slot(Some(ancestors), &list_r, max_root) {
                    latest_slot_timer.stop();
                    latest_slot_elapsed += latest_slot_timer.as_us();
                    let mut load_account_timer = Measure::start("load_account");
                    func(&pubkey, (&list_r[index].1, list_r[index].0));
                    load_account_timer.stop();
                    load_account_elapsed += load_account_timer.as_us();
                }
            }
            iterator_timer = Measure::start("iterator_elapsed");
        }

        total_elapsed_timer.stop();
        if !metric_name.is_empty() {
            datapoint_info!(
                metric_name,
                ("total_elapsed", total_elapsed_timer.as_us(), i64),
                ("latest_slot_elapsed", latest_slot_elapsed, i64),
                ("read_lock_elapsed", read_lock_elapsed, i64),
                ("load_account_elapsed", load_account_elapsed, i64),
                ("iterator_elapsed", iterator_elapsed, i64),
                ("num_keys_iterated", num_keys_iterated, i64),
            )
        }
    }

    fn do_scan_secondary_index<
        F,
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        &self,
        ancestors: &Ancestors,
        mut func: F,
        index: &SecondaryIndex<SecondaryIndexEntryType>,
        index_key: &Pubkey,
        max_root: Option<Slot>,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        for pubkey in index.get(index_key) {
            // Maybe these reads from the AccountsIndex can be batched every time it
            // grabs the read lock as well...
            if let Some((list_r, index)) = self.get(&pubkey, Some(ancestors), max_root) {
                func(
                    &pubkey,
                    (&list_r.slot_list()[index].1, list_r.slot_list()[index].0),
                );
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

    fn insert_new_entry_if_missing(&self, pubkey: &Pubkey) -> (WriteAccountMapEntry<T>, bool) {
        let new_entry = Arc::new(AccountMapEntryInner {
            ref_count: AtomicU64::new(0),
            slot_list: RwLock::new(SlotList::with_capacity(1)),
        });
        let mut w_account_maps = self.account_maps.write().unwrap();
        let mut is_newly_inserted = false;
        let account_entry = w_account_maps.entry(*pubkey).or_insert_with(|| {
            is_newly_inserted = true;
            new_entry
        });
        let w_account_entry = WriteAccountMapEntry::from_account_map_entry(account_entry.clone());
        (w_account_entry, is_newly_inserted)
    }

    fn get_account_write_entry_else_create(
        &self,
        pubkey: &Pubkey,
    ) -> (WriteAccountMapEntry<T>, bool) {
        let mut w_account_entry = self.get_account_write_entry(pubkey);
        let mut is_newly_inserted = false;
        if w_account_entry.is_none() {
            let entry_is_new = self.insert_new_entry_if_missing(pubkey);
            w_account_entry = Some(entry_is_new.0);
            is_newly_inserted = entry_is_new.1;
        }

        (w_account_entry.unwrap(), is_newly_inserted)
    }

    pub fn handle_dead_keys(&self, dead_keys: &[&Pubkey], account_indexes: &HashSet<AccountIndex>) {
        if !dead_keys.is_empty() {
            for key in dead_keys.iter() {
                let mut w_index = self.account_maps.write().unwrap();
                if let btree_map::Entry::Occupied(index_entry) = w_index.entry(**key) {
                    if index_entry.get().slot_list.read().unwrap().is_empty() {
                        index_entry.remove();

                        // Note passing `None` to remove all the entries for this key
                        // is only safe because we have the lock for this key's entry
                        // in the AccountsIndex, so no other thread is also updating
                        // the index
                        self.purge_secondary_indexes_by_inner_key(
                            key,
                            None::<&Slot>,
                            account_indexes,
                        );
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
        // Pass "" not to log metrics, so RPC doesn't get spammy
        self.do_checked_scan_accounts(
            "",
            ancestors,
            func,
            ScanTypes::Unindexed(None::<Range<Pubkey>>),
        );
    }

    pub(crate) fn unchecked_scan_accounts<F>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        func: F,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        self.do_unchecked_scan_accounts(metric_name, ancestors, func, None::<Range<Pubkey>>);
    }

    /// call func with every pubkey and index visible from a given set of ancestors with range
    pub(crate) fn range_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        range: R,
        func: F,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey>,
    {
        // Only the rent logic should be calling this, which doesn't need the safety checks
        self.do_unchecked_scan_accounts(metric_name, ancestors, func, Some(range));
    }

    /// call func with every pubkey and index visible from a given set of ancestors
    pub(crate) fn index_scan_accounts<F>(&self, ancestors: &Ancestors, index_key: IndexKey, func: F)
    where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        // Pass "" not to log metrics, so RPC doesn't get spammy
        self.do_checked_scan_accounts(
            "",
            ancestors,
            func,
            ScanTypes::<Range<Pubkey>>::Indexed(index_key),
        );
    }

    pub fn get_rooted_entries(&self, slice: SlotSlice<T>, max: Option<Slot>) -> SlotList<T> {
        let max = max.unwrap_or(Slot::MAX);
        let lock = &self.roots_tracker.read().unwrap().roots;
        slice
            .iter()
            .filter(|(slot, _)| *slot <= max && lock.contains(slot))
            .cloned()
            .collect()
    }

    // returns the rooted entries and the storage ref count
    pub fn roots_and_ref_count(
        &self,
        locked_account_entry: &ReadAccountMapEntry<T>,
        max: Option<Slot>,
    ) -> (SlotList<T>, RefCount) {
        (
            self.get_rooted_entries(&locked_account_entry.slot_list(), max),
            locked_account_entry.ref_count().load(Ordering::Relaxed),
        )
    }

    pub fn purge_exact<'a, C>(
        &'a self,
        pubkey: &Pubkey,
        slots_to_purge: &'a C,
        reclaims: &mut SlotList<T>,
        account_indexes: &HashSet<AccountIndex>,
    ) -> bool
    where
        C: Contains<'a, Slot>,
    {
        let res = {
            let mut write_account_map_entry = self.get_account_write_entry(pubkey).unwrap();
            write_account_map_entry.slot_list_mut(|slot_list| {
                slot_list.retain(|(slot, item)| {
                    let should_purge = slots_to_purge.contains(&slot);
                    if should_purge {
                        reclaims.push((*slot, item.clone()));
                    }
                    !should_purge
                });
                slot_list.is_empty()
            })
        };
        self.purge_secondary_indexes_by_inner_key(pubkey, Some(slots_to_purge), account_indexes);
        res
    }

    pub fn min_ongoing_scan_root(&self) -> Option<Slot> {
        self.ongoing_scan_roots
            .read()
            .unwrap()
            .keys()
            .next()
            .cloned()
    }

    // Given a SlotSlice `L`, a list of ancestors and a maximum slot, find the latest element
    // in `L`, where the slot `S` is an ancestor or root, and if `S` is a root, then `S <= max_root`
    fn latest_slot(
        &self,
        ancestors: Option<&Ancestors>,
        slice: SlotSlice<T>,
        max_root: Option<Slot>,
    ) -> Option<usize> {
        let mut current_max = 0;
        let mut rv = None;
        if let Some(ancestors) = ancestors {
            if !ancestors.is_empty() {
                for (i, (slot, _t)) in slice.iter().rev().enumerate() {
                    if (rv.is_none() || *slot > current_max) && ancestors.contains_key(slot) {
                        rv = Some(i);
                        current_max = *slot;
                    }
                }
            }
        }

        let max_root = max_root.unwrap_or(Slot::MAX);
        let mut tracker = None;

        for (i, (slot, _t)) in slice.iter().rev().enumerate() {
            if (rv.is_none() || *slot > current_max) && *slot <= max_root {
                let lock = match tracker {
                    Some(inner) => inner,
                    None => self.roots_tracker.read().unwrap(),
                };
                if lock.roots.contains(&slot) {
                    rv = Some(i);
                    current_max = *slot;
                }
                tracker = Some(lock);
            }
        }

        rv.map(|index| slice.len() - 1 - index)
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

    fn update_secondary_indexes(
        &self,
        pubkey: &Pubkey,
        slot: Slot,
        account_owner: &Pubkey,
        account_data: &[u8],
        account_indexes: &HashSet<AccountIndex>,
    ) {
        if account_indexes.is_empty() {
            return;
        }

        if account_indexes.contains(&AccountIndex::ProgramId) {
            self.program_id_index.insert(account_owner, pubkey, slot);
        }
        // Note because of the below check below on the account data length, when an
        // account hits zero lamports and is reset to AccountSharedData::Default, then we skip
        // the below updates to the secondary indexes.
        //
        // Skipping means not updating secondary index to mark the account as missing.
        // This doesn't introduce false positives during a scan because the caller to scan
        // provides the ancestors to check. So even if a zero-lamport account is not yet
        // removed from the secondary index, the scan function will:
        // 1) consult the primary index via `get(&pubkey, Some(ancestors), max_root)`
        // and find the zero-lamport version
        // 2) When the fetch from storage occurs, it will return AccountSharedData::Default
        // (as persisted tombstone for snapshots). This will then ultimately be
        // filtered out by post-scan filters, like in `get_filtered_spl_token_accounts_by_owner()`.
        if *account_owner == inline_spl_token_v2_0::id()
            && account_data.len() == inline_spl_token_v2_0::state::Account::get_packed_len()
        {
            if account_indexes.contains(&AccountIndex::SplTokenOwner) {
                let owner_key = Pubkey::new(
                    &account_data[SPL_TOKEN_ACCOUNT_OWNER_OFFSET
                        ..SPL_TOKEN_ACCOUNT_OWNER_OFFSET + PUBKEY_BYTES],
                );
                self.spl_token_owner_index.insert(&owner_key, pubkey, slot);
            }

            if account_indexes.contains(&AccountIndex::SplTokenMint) {
                let mint_key = Pubkey::new(
                    &account_data[SPL_TOKEN_ACCOUNT_MINT_OFFSET
                        ..SPL_TOKEN_ACCOUNT_MINT_OFFSET + PUBKEY_BYTES],
                );
                self.spl_token_mint_index.insert(&mint_key, pubkey, slot);
            }
        }
    }

    // Same functionally to upsert, but doesn't take the read lock
    // initially on the accounts_map
    // Can save time when inserting lots of new keys
    pub fn insert_new_if_missing(
        &self,
        slot: Slot,
        pubkey: &Pubkey,
        account_owner: &Pubkey,
        account_data: &[u8],
        account_indexes: &HashSet<AccountIndex>,
        account_info: T,
        reclaims: &mut SlotList<T>,
    ) {
        {
            let (mut w_account_entry, _is_new) = self.insert_new_entry_if_missing(pubkey);
            if account_info.is_zero_lamport() {
                self.zero_lamport_pubkeys.insert(*pubkey);
            }
            w_account_entry.update(slot, account_info, reclaims);
        }
        self.update_secondary_indexes(pubkey, slot, account_owner, account_data, account_indexes);
    }

    // Updates the given pubkey at the given slot with the new account information.
    // Returns true if the pubkey was newly inserted into the index, otherwise, if the
    // pubkey updates an existing entry in the index, returns false.
    pub fn upsert(
        &self,
        slot: Slot,
        pubkey: &Pubkey,
        account_owner: &Pubkey,
        account_data: &[u8],
        account_indexes: &HashSet<AccountIndex>,
        account_info: T,
        reclaims: &mut SlotList<T>,
    ) -> bool {
        let is_newly_inserted = {
            let (mut w_account_entry, is_newly_inserted) =
                self.get_account_write_entry_else_create(pubkey);
            // We don't atomically update both primary index and secondary index together.
            // This certainly creates small time window with inconsistent state across the two indexes.
            // However, this is acceptable because:
            //
            //  - A strict consistent view at any given moment of time is not necessary, because the only
            //  use case for the secondary index is `scan`, and `scans` are only supported/require consistency
            //  on frozen banks, and this inconsistency is only possible on working banks.
            //
            //  - The secondary index is never consulted as primary source of truth for gets/stores.
            //  So, what the accounts_index sees alone is sufficient as a source of truth for other non-scan
            //  account operations.
            if account_info.is_zero_lamport() {
                self.zero_lamport_pubkeys.insert(*pubkey);
            }
            w_account_entry.update(slot, account_info, reclaims);
            is_newly_inserted
        };
        self.update_secondary_indexes(pubkey, slot, account_owner, account_data, account_indexes);
        is_newly_inserted
    }

    pub fn remove_zero_lamport_key(&self, pubkey: &Pubkey) {
        self.zero_lamport_pubkeys.remove(pubkey);
    }

    pub fn zero_lamport_pubkeys(&self) -> &DashSet<Pubkey> {
        &self.zero_lamport_pubkeys
    }

    pub fn unref_from_storage(&self, pubkey: &Pubkey) {
        if let Some(locked_entry) = self.get_account_read_entry(pubkey) {
            locked_entry.unref();
        }
    }

    pub fn ref_count_from_storage(&self, pubkey: &Pubkey) -> RefCount {
        if let Some(locked_entry) = self.get_account_read_entry(pubkey) {
            locked_entry.ref_count().load(Ordering::Relaxed)
        } else {
            0
        }
    }

    fn purge_secondary_indexes_by_inner_key<'a, C>(
        &'a self,
        inner_key: &Pubkey,
        slots_to_remove: Option<&'a C>,
        account_indexes: &HashSet<AccountIndex>,
    ) where
        C: Contains<'a, Slot>,
    {
        if account_indexes.contains(&AccountIndex::ProgramId) {
            self.program_id_index
                .remove_by_inner_key(inner_key, slots_to_remove);
        }

        if account_indexes.contains(&AccountIndex::SplTokenOwner) {
            self.spl_token_owner_index
                .remove_by_inner_key(inner_key, slots_to_remove);
        }

        if account_indexes.contains(&AccountIndex::SplTokenMint) {
            self.spl_token_mint_index
                .remove_by_inner_key(inner_key, slots_to_remove);
        }
    }

    fn purge_older_root_entries(
        &self,
        pubkey: &Pubkey,
        list: &mut SlotList<T>,
        reclaims: &mut SlotList<T>,
        max_clean_root: Option<Slot>,
        account_indexes: &HashSet<AccountIndex>,
    ) {
        let roots_tracker = &self.roots_tracker.read().unwrap();
        let max_root = Self::get_max_root(&roots_tracker.roots, &list, max_clean_root);

        let mut purged_slots: HashSet<Slot> = HashSet::new();
        list.retain(|(slot, value)| {
            let should_purge = Self::can_purge(max_root, *slot) && !value.is_cached();
            if should_purge {
                reclaims.push((*slot, value.clone()));
                purged_slots.insert(*slot);
            }
            !should_purge
        });

        self.purge_secondary_indexes_by_inner_key(pubkey, Some(&purged_slots), account_indexes);
    }

    // `is_cached` closure is needed to work around the generic (`T`) indexed type.
    pub fn clean_rooted_entries(
        &self,
        pubkey: &Pubkey,
        reclaims: &mut SlotList<T>,
        max_clean_root: Option<Slot>,
        account_indexes: &HashSet<AccountIndex>,
    ) {
        if let Some(mut locked_entry) = self.get_account_write_entry(pubkey) {
            locked_entry.slot_list_mut(|slot_list| {
                self.purge_older_root_entries(
                    pubkey,
                    slot_list,
                    reclaims,
                    max_clean_root,
                    account_indexes,
                );
            });
        }
    }

    pub fn can_purge(max_root: Slot, slot: Slot) -> bool {
        slot < max_root
    }

    pub fn is_root(&self, slot: Slot) -> bool {
        self.roots_tracker.read().unwrap().roots.contains(&slot)
    }

    pub fn add_root(&self, slot: Slot, caching_enabled: bool) {
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.roots.insert(slot);
        // we delay cleaning until flushing!
        if !caching_enabled {
            w_roots_tracker.uncleaned_roots.insert(slot);
        }
        // `AccountsDb::flush_accounts_cache()` relies on roots being added in order
        assert!(slot >= w_roots_tracker.max_root);
        w_roots_tracker.max_root = slot;
    }

    pub fn add_uncleaned_roots<I>(&self, roots: I)
    where
        I: IntoIterator<Item = Slot>,
    {
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.uncleaned_roots.extend(roots);
    }

    pub fn max_root(&self) -> Slot {
        self.roots_tracker.read().unwrap().max_root
    }

    /// Remove the slot when the storage for the slot is freed
    /// Accounts no longer reference this slot.
    pub fn clean_dead_slot(&self, slot: Slot) -> Option<AccountsIndexRootsStats> {
        if self.is_root(slot) {
            let (roots_len, uncleaned_roots_len, previous_uncleaned_roots_len) = {
                let mut w_roots_tracker = self.roots_tracker.write().unwrap();
                w_roots_tracker.roots.remove(&slot);
                w_roots_tracker.uncleaned_roots.remove(&slot);
                w_roots_tracker.previous_uncleaned_roots.remove(&slot);
                (
                    w_roots_tracker.roots.len(),
                    w_roots_tracker.uncleaned_roots.len(),
                    w_roots_tracker.previous_uncleaned_roots.len(),
                )
            };
            Some(AccountsIndexRootsStats {
                roots_len,
                uncleaned_roots_len,
                previous_uncleaned_roots_len,
            })
        } else {
            None
        }
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

    #[cfg(test)]
    pub fn clear_uncleaned_roots(&self, max_clean_root: Option<Slot>) -> HashSet<Slot> {
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
        cleaned_roots
    }

    pub fn is_uncleaned_root(&self, slot: Slot) -> bool {
        self.roots_tracker
            .read()
            .unwrap()
            .uncleaned_roots
            .contains(&slot)
    }

    pub fn num_roots(&self) -> usize {
        self.roots_tracker.read().unwrap().roots.len()
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

    #[cfg(test)]
    // filter any rooted entries and return them along with a bool that indicates
    // if this account has no more entries. Note this does not update the secondary
    // indexes!
    pub fn purge_roots(&self, pubkey: &Pubkey) -> (SlotList<T>, bool) {
        let mut write_account_map_entry = self.get_account_write_entry(pubkey).unwrap();
        write_account_map_entry.slot_list_mut(|slot_list| {
            let reclaims = self.get_rooted_entries(slot_list, None);
            slot_list.retain(|(slot, _)| !self.is_root(*slot));
            (reclaims, slot_list.is_empty())
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_sdk::signature::{Keypair, Signer};

    pub enum SecondaryIndexTypes<'a> {
        RwLock(&'a SecondaryIndex<RwLockSecondaryIndexEntry>),
        DashMap(&'a SecondaryIndex<DashMapSecondaryIndexEntry>),
    }

    pub fn spl_token_mint_index_enabled() -> HashSet<AccountIndex> {
        let mut account_indexes = HashSet::new();
        account_indexes.insert(AccountIndex::SplTokenMint);
        account_indexes
    }

    pub fn spl_token_owner_index_enabled() -> HashSet<AccountIndex> {
        let mut account_indexes = HashSet::new();
        account_indexes.insert(AccountIndex::SplTokenOwner);
        account_indexes
    }

    fn create_dashmap_secondary_index_state() -> (usize, usize, HashSet<AccountIndex>) {
        {
            // Check that we're actually testing the correct variant
            let index = AccountsIndex::<bool>::default();
            let _type_check = SecondaryIndexTypes::DashMap(&index.spl_token_mint_index);
        }

        (0, PUBKEY_BYTES, spl_token_mint_index_enabled())
    }

    fn create_rwlock_secondary_index_state() -> (usize, usize, HashSet<AccountIndex>) {
        {
            // Check that we're actually testing the correct variant
            let index = AccountsIndex::<bool>::default();
            let _type_check = SecondaryIndexTypes::RwLock(&index.spl_token_owner_index);
        }

        (
            SPL_TOKEN_ACCOUNT_OWNER_OFFSET,
            SPL_TOKEN_ACCOUNT_OWNER_OFFSET + PUBKEY_BYTES,
            spl_token_owner_index_enabled(),
        )
    }

    #[test]
    fn test_get_empty() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let ancestors = Ancestors::default();
        assert!(index.get(&key.pubkey(), Some(&ancestors), None).is_none());
        assert!(index.get(&key.pubkey(), None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts("", &ancestors, |_pubkey, _index| num += 1);
        assert_eq!(num, 0);
    }

    #[test]
    fn test_insert_no_ancestors() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());

        let ancestors = Ancestors::default();
        assert!(index.get(&key.pubkey(), Some(&ancestors), None).is_none());
        assert!(index.get(&key.pubkey(), None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts("", &ancestors, |_pubkey, _index| num += 1);
        assert_eq!(num, 0);
    }

    #[test]
    fn test_insert_wrong_ancestors() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert!(index.get(&key.pubkey(), Some(&ancestors), None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts("", &ancestors, |_pubkey, _index| num += 1);
        assert_eq!(num, 0);
    }

    #[test]
    fn test_insert_with_ancestors() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());

        let ancestors = vec![(0, 0)].into_iter().collect();
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));

        let mut num = 0;
        let mut found_key = false;
        index.unchecked_scan_accounts("", &ancestors, |pubkey, _index| {
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
            index.upsert(
                root_slot,
                &new_pubkey,
                &Pubkey::default(),
                &[],
                &HashSet::new(),
                true,
                &mut vec![],
            );
            new_pubkey
        })
        .take(num_pubkeys.saturating_sub(1))
        .collect();

        if num_pubkeys != 0 {
            pubkeys.push(Pubkey::default());
            index.upsert(
                root_slot,
                &Pubkey::default(),
                &Pubkey::default(),
                &[],
                &HashSet::new(),
                true,
                &mut vec![],
            );
        }

        index.add_root(root_slot, false);

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

        let ancestors = Ancestors::default();
        let mut scanned_keys = HashSet::new();
        index.range_scan_accounts("", &ancestors, pubkey_range, |pubkey, _index| {
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
            Some(ITER_BATCH_SIZE - 1_usize),
            Some(2 * ITER_BATCH_SIZE as usize + 1),
        );
    }

    fn run_test_scan_accounts(num_pubkeys: usize) {
        let (index, _) = setup_accounts_index_keys(num_pubkeys);
        let ancestors = Ancestors::default();

        let mut scanned_keys = HashSet::new();
        index.unchecked_scan_accounts("", &ancestors, |pubkey, _index| {
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
        index.upsert(
            0,
            &solana_sdk::pubkey::new_rand(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_is_root() {
        let index = AccountsIndex::<bool>::default();
        assert!(!index.is_root(0));
        index.add_root(0, false);
        assert!(index.is_root(0));
    }

    #[test]
    fn test_insert_with_root() {
        let key = Keypair::new();
        let index = AccountsIndex::<bool>::default();
        let mut gc = Vec::new();
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());

        index.add_root(0, false);
        let (list, idx) = index.get(&key.pubkey(), None, None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
    }

    #[test]
    fn test_clean_first() {
        let index = AccountsIndex::<bool>::default();
        index.add_root(0, false);
        index.add_root(1, false);
        index.clean_dead_slot(0);
        assert!(index.is_root(1));
        assert!(!index.is_root(0));
    }

    #[test]
    fn test_clean_last() {
        //this behavior might be undefined, clean up should only occur on older slots
        let index = AccountsIndex::<bool>::default();
        index.add_root(0, false);
        index.add_root(1, false);
        index.clean_dead_slot(1);
        assert!(!index.is_root(1));
        assert!(index.is_root(0));
    }

    #[test]
    fn test_clean_and_unclean_slot() {
        let index = AccountsIndex::<bool>::default();
        assert_eq!(0, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        index.add_root(0, false);
        index.add_root(1, false);
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

        index.add_root(2, false);
        index.add_root(3, false);
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
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());
        let (list, idx) = index.get(&key.pubkey(), Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
        drop(list);

        let mut gc = Vec::new();
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            false,
            &mut gc,
        );
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
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());
        index.upsert(
            1,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            false,
            &mut gc,
        );
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
        index.upsert(
            0,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        assert!(gc.is_empty());
        index.upsert(
            1,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            false,
            &mut gc,
        );
        index.upsert(
            2,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        index.upsert(
            3,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );
        index.add_root(0, false);
        index.add_root(1, false);
        index.add_root(3, false);
        index.upsert(
            4,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            true,
            &mut gc,
        );

        // Updating index should not purge older roots, only purges
        // previous updates within the same slot
        assert_eq!(gc, vec![]);
        let (list, idx) = index.get(&key.pubkey(), None, None).unwrap();
        assert_eq!(list.slot_list()[idx], (3, true));

        let mut num = 0;
        let mut found_key = false;
        index.unchecked_scan_accounts("", &Ancestors::default(), |pubkey, _index| {
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
        assert!(index.upsert(
            1,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            12,
            &mut gc
        ));

        assert!(!index.upsert(
            1,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            10,
            &mut gc
        ));

        let purges = index.purge_roots(&key.pubkey());
        assert_eq!(purges, (vec![], false));
        index.add_root(1, false);

        let purges = index.purge_roots(&key.pubkey());
        assert_eq!(purges, (vec![(1, 10)], true));

        assert!(!index.upsert(
            1,
            &key.pubkey(),
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            9,
            &mut gc
        ));
    }

    #[test]
    fn test_latest_slot() {
        let slot_slice = vec![(0, true), (5, true), (3, true), (7, true)];
        let index = AccountsIndex::<bool>::default();

        // No ancestors, no root, should return None
        assert!(index.latest_slot(None, &slot_slice, None).is_none());

        // Given a root, should return the root
        index.add_root(5, false);
        assert_eq!(index.latest_slot(None, &slot_slice, None).unwrap(), 1);

        // Given a max_root == root, should still return the root
        assert_eq!(index.latest_slot(None, &slot_slice, Some(5)).unwrap(), 1);

        // Given a max_root < root, should filter out the root
        assert!(index.latest_slot(None, &slot_slice, Some(4)).is_none());

        // Given a max_root, should filter out roots < max_root, but specified
        // ancestors should not be affected
        let ancestors = vec![(3, 1), (7, 1)].into_iter().collect();
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(4))
                .unwrap(),
            3
        );
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(7))
                .unwrap(),
            3
        );

        // Given no max_root, should just return the greatest ancestor or root
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, None)
                .unwrap(),
            3
        );
    }

    fn run_test_purge_exact_secondary_index<
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        index: &AccountsIndex<bool>,
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        key_start: usize,
        key_end: usize,
        account_index: &HashSet<AccountIndex>,
    ) {
        // No roots, should be no reclaims
        let slots = vec![1, 2, 5, 9];
        let index_key = Pubkey::new_unique();
        let account_key = Pubkey::new_unique();

        let mut account_data = vec![0; inline_spl_token_v2_0::state::Account::get_packed_len()];
        account_data[key_start..key_end].clone_from_slice(&(index_key.clone().to_bytes()));

        // Insert slots into secondary index
        for slot in &slots {
            index.upsert(
                *slot,
                &account_key,
                // Make sure these accounts are added to secondary index
                &inline_spl_token_v2_0::id(),
                &account_data,
                account_index,
                true,
                &mut vec![],
            );
        }

        // Only one top level index entry exists
        assert_eq!(secondary_index.index.get(&index_key).unwrap().len(), 1);

        // In the reverse index, one account maps across multiple slots
        // to the same top level key
        assert_eq!(
            secondary_index
                .reverse_index
                .get(&account_key)
                .unwrap()
                .value()
                .read()
                .unwrap()
                .len(),
            slots.len()
        );

        index.purge_exact(
            &account_key,
            &slots.into_iter().collect::<HashSet<Slot>>(),
            &mut vec![],
            account_index,
        );

        assert!(secondary_index.index.is_empty());
        assert!(secondary_index.reverse_index.is_empty());
    }

    #[test]
    fn test_purge_exact_dashmap_secondary_index() {
        let (key_start, key_end, account_index) = create_dashmap_secondary_index_state();
        let index = AccountsIndex::<bool>::default();
        run_test_purge_exact_secondary_index(
            &index,
            &index.spl_token_mint_index,
            key_start,
            key_end,
            &account_index,
        );
    }

    #[test]
    fn test_purge_exact_rwlock_secondary_index() {
        let (key_start, key_end, account_index) = create_rwlock_secondary_index_state();
        let index = AccountsIndex::<bool>::default();
        run_test_purge_exact_secondary_index(
            &index,
            &index.spl_token_owner_index,
            key_start,
            key_end,
            &account_index,
        );
    }

    #[test]
    fn test_purge_older_root_entries() {
        // No roots, should be no reclaims
        let index = AccountsIndex::<bool>::default();
        let mut slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        let mut reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            None,
            &HashSet::new(),
        );
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Add a later root, earlier slots should be reclaimed
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        index.add_root(1, false);
        // Note 2 is not a root
        index.add_root(5, false);
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            None,
            &HashSet::new(),
        );
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Add a later root that is not in the list, should not affect the outcome
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        index.add_root(6, false);
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            None,
            &HashSet::new(),
        );
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root >= than any root in the slot list, should not affect
        // outcome
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            Some(6),
            &HashSet::new(),
        );
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root, earlier slots should be reclaimed
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            Some(5),
            &HashSet::new(),
        );
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root 2. This means the latest root < 2 is 1 because 2 is not a root
        // so nothing will be purged
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            Some(2),
            &HashSet::new(),
        );
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Pass a max root 1. This means the latest root < 3 is 1 because 2 is not a root
        // so nothing will be purged
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            Some(1),
            &HashSet::new(),
        );
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Pass a max root that doesn't exist in the list but is greater than
        // some of the roots in the list, shouldn't return those smaller roots
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(
            &Pubkey::default(),
            &mut slot_list,
            &mut reclaims,
            Some(7),
            &HashSet::new(),
        );
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);
    }

    fn check_secondary_index_unique<SecondaryIndexEntryType>(
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        slot: Slot,
        key: &Pubkey,
        account_key: &Pubkey,
    ) where
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    {
        // Check secondary index has unique mapping from secondary index key
        // to the account key and slot
        assert_eq!(secondary_index.index.len(), 1);
        let inner_key_map = secondary_index.index.get(key).unwrap();
        assert_eq!(inner_key_map.len(), 1);
        inner_key_map
            .value()
            .get(account_key, &|slots_map: Option<&RwLock<HashSet<Slot>>>| {
                let slots_map = slots_map.unwrap();
                assert_eq!(slots_map.read().unwrap().len(), 1);
                assert!(slots_map.read().unwrap().contains(&slot));
            });

        // Check reverse index is unique
        let slots_map = secondary_index.reverse_index.get(account_key).unwrap();
        assert_eq!(slots_map.value().read().unwrap().get(&slot).unwrap(), key);
    }

    fn run_test_secondary_indexes<
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        index: &AccountsIndex<bool>,
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        key_start: usize,
        key_end: usize,
        account_index: &HashSet<AccountIndex>,
    ) {
        let account_key = Pubkey::new_unique();
        let index_key = Pubkey::new_unique();
        let slot = 1;
        let mut account_data = vec![0; inline_spl_token_v2_0::state::Account::get_packed_len()];
        account_data[key_start..key_end].clone_from_slice(&(index_key.clone().to_bytes()));

        // Wrong program id
        index.upsert(
            0,
            &account_key,
            &Pubkey::default(),
            &account_data,
            account_index,
            true,
            &mut vec![],
        );
        assert!(index.spl_token_mint_index.index.is_empty());
        assert!(index.spl_token_mint_index.reverse_index.is_empty());

        // Wrong account data size
        index.upsert(
            0,
            &account_key,
            &inline_spl_token_v2_0::id(),
            &account_data[1..],
            account_index,
            true,
            &mut vec![],
        );
        assert!(index.spl_token_mint_index.index.is_empty());
        assert!(index.spl_token_mint_index.reverse_index.is_empty());

        // Just right. Inserting the same index multiple times should be ok
        for _ in 0..2 {
            index.update_secondary_indexes(
                &account_key,
                slot,
                &inline_spl_token_v2_0::id(),
                &account_data,
                account_index,
            );
            check_secondary_index_unique(secondary_index, slot, &index_key, &account_key);
        }

        index
            .get_account_write_entry(&account_key)
            .unwrap()
            .slot_list_mut(|slot_list| slot_list.clear());

        // Everything should be deleted
        index.handle_dead_keys(&[&account_key], account_index);
        assert!(index.spl_token_mint_index.index.is_empty());
        assert!(index.spl_token_mint_index.reverse_index.is_empty());
    }

    #[test]
    fn test_dashmap_secondary_index() {
        let (key_start, key_end, account_index) = create_dashmap_secondary_index_state();
        let index = AccountsIndex::<bool>::default();
        run_test_secondary_indexes(
            &index,
            &index.spl_token_mint_index,
            key_start,
            key_end,
            &account_index,
        );
    }

    #[test]
    fn test_rwlock_secondary_index() {
        let (key_start, key_end, account_index) = create_rwlock_secondary_index_state();
        let index = AccountsIndex::<bool>::default();
        run_test_secondary_indexes(
            &index,
            &index.spl_token_owner_index,
            key_start,
            key_end,
            &account_index,
        );
    }

    fn run_test_secondary_indexes_same_slot_and_forks<
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        index: &AccountsIndex<bool>,
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        index_key_start: usize,
        index_key_end: usize,
        account_index: &HashSet<AccountIndex>,
    ) {
        let account_key = Pubkey::new_unique();
        let secondary_key1 = Pubkey::new_unique();
        let secondary_key2 = Pubkey::new_unique();
        let slot = 1;
        let mut account_data1 = vec![0; inline_spl_token_v2_0::state::Account::get_packed_len()];
        account_data1[index_key_start..index_key_end]
            .clone_from_slice(&(secondary_key1.clone().to_bytes()));
        let mut account_data2 = vec![0; inline_spl_token_v2_0::state::Account::get_packed_len()];
        account_data2[index_key_start..index_key_end]
            .clone_from_slice(&(secondary_key2.clone().to_bytes()));

        // First write one mint index
        index.upsert(
            slot,
            &account_key,
            &inline_spl_token_v2_0::id(),
            &account_data1,
            account_index,
            true,
            &mut vec![],
        );

        // Now write a different mint index
        index.upsert(
            slot,
            &account_key,
            &inline_spl_token_v2_0::id(),
            &account_data2,
            account_index,
            true,
            &mut vec![],
        );

        // Check correctness
        check_secondary_index_unique(&secondary_index, slot, &secondary_key2, &account_key);
        assert!(secondary_index.get(&secondary_key1).is_empty());
        assert_eq!(secondary_index.get(&secondary_key2), vec![account_key]);

        // If another fork reintroduces secondary_key1, then it should be re-added to the
        // index
        let fork = slot + 1;
        index.upsert(
            fork,
            &account_key,
            &inline_spl_token_v2_0::id(),
            &account_data1,
            account_index,
            true,
            &mut vec![],
        );
        assert_eq!(secondary_index.get(&secondary_key1), vec![account_key]);

        // If we set a root at fork, and clean, then the secondary_key1 should no longer
        // be findable
        index.add_root(fork, false);
        index
            .get_account_write_entry(&account_key)
            .unwrap()
            .slot_list_mut(|slot_list| {
                index.purge_older_root_entries(
                    &account_key,
                    slot_list,
                    &mut vec![],
                    None,
                    account_index,
                )
            });
        assert!(secondary_index.get(&secondary_key2).is_empty());
        assert_eq!(secondary_index.get(&secondary_key1), vec![account_key]);

        // Check correctness
        check_secondary_index_unique(secondary_index, fork, &secondary_key1, &account_key);
    }

    #[test]
    fn test_dashmap_secondary_index_same_slot_and_forks() {
        let (key_start, key_end, account_index) = create_dashmap_secondary_index_state();
        let index = AccountsIndex::<bool>::default();
        run_test_secondary_indexes_same_slot_and_forks(
            &index,
            &index.spl_token_mint_index,
            key_start,
            key_end,
            &account_index,
        );
    }

    #[test]
    fn test_rwlock_secondary_index_same_slot_and_forks() {
        let (key_start, key_end, account_index) = create_rwlock_secondary_index_state();
        let index = AccountsIndex::<bool>::default();
        run_test_secondary_indexes_same_slot_and_forks(
            &index,
            &index.spl_token_owner_index,
            key_start,
            key_end,
            &account_index,
        );
    }

    impl ZeroLamport for bool {
        fn is_zero_lamport(&self) -> bool {
            false
        }
    }

    impl ZeroLamport for u64 {
        fn is_zero_lamport(&self) -> bool {
            false
        }
    }
}
