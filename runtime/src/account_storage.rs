//! Manage the map of slot -> append vecs

use {
    crate::accounts_db::{AccountStorageEntry, AppendVecId, SlotStores},
    dashmap::DashMap,
    solana_sdk::clock::Slot,
    std::{
        collections::HashMap,
        sync::{Arc, RwLock},
    },
};

pub type AccountStorageMap = DashMap<Slot, SlotStores>;

#[derive(Clone, Default, Debug)]
pub struct AccountStorage {
    map: AccountStorageMap,
    /// while shrink is operating on a slot, there can be 2 append vecs active for that slot
    /// Once the index has been updated to only refer to the new append vec, the single entry for the slot in 'map' can be updated.
    /// Entries in 'shrink_in_progress_map' can be found by 'get_account_storage_entry'
    shrink_in_progress_map: DashMap<Slot, Arc<AccountStorageEntry>>,
}

impl AccountStorage {
    /// Return the append vec in 'slot' and with id='store_id'.
    /// can look in 'map' and 'shrink_in_progress_map' to find the specified append vec
    /// when shrinking begins, shrinking_in_progress is called.
    /// This fn looks in 'map' first, then in 'shrink_in_progress_map' because
    /// 'shrink_in_progress_map' first inserts the old append vec into 'shrink_in_progress_map'
    /// and then removes the old append vec from 'map'
    /// Then, the index is updated for all entries to refer to the new id.
    /// Callers to this function have 2 choices:
    /// 1. hold the account index read lock for the pubkey so that the account index entry cannot be changed prior to or during this call. (scans do this)
    /// 2. expect to be ready to start over and read the index again if this function returns None
    /// Operations like shrinking or write cache flushing may have updated the index between when the caller read the index and called this function to
    /// load from the append vec specified in the index.
    pub(crate) fn get_account_storage_entry(
        &self,
        slot: Slot,
        store_id: AppendVecId,
    ) -> Option<Arc<AccountStorageEntry>> {
        self.get_slot_stores_shrinking_in_progress_ok(slot)
            .and_then(|storage_map| storage_map.read().unwrap().get(&store_id).cloned())
            .or_else(|| {
                self.shrink_in_progress_map
                    .get(&slot)
                    .map(|entry| Arc::clone(entry.value()))
            })
    }

    /// public api, should only be called when shrinking is not in progress
    pub fn get_slot_stores(&self, slot: Slot) -> Option<SlotStores> {
        assert!(self.shrink_in_progress_map.is_empty());
        self.get_slot_stores_shrinking_in_progress_ok(slot)
    }

    /// safe to call while shrinking is in progress
    fn get_slot_stores_shrinking_in_progress_ok(&self, slot: Slot) -> Option<SlotStores> {
        self.map.get(&slot).map(|result| result.value().clone())
    }

    /// return the append vec for 'slot' if it exists
    /// This is only ever called when shrink is not possibly running and there is a max of 1 append vec per slot.
    pub(crate) fn get_slot_storage_entry(&self, slot: Slot) -> Option<Arc<AccountStorageEntry>> {
        assert!(self.shrink_in_progress_map.is_empty());
        self.get_slot_stores(slot).and_then(|res| {
            let read = res.read().unwrap();
            assert!(read.len() <= 1);
            read.values().next().cloned()
        })
    }

    pub(crate) fn all_slots(&self) -> Vec<Slot> {
        assert!(self.shrink_in_progress_map.is_empty());
        self.map.iter().map(|iter_item| *iter_item.key()).collect()
    }

    /// returns true if there is an entry in the map for 'slot', but it contains no append vec
    #[cfg(test)]
    pub(crate) fn is_empty_entry(&self, slot: Slot) -> bool {
        assert!(self.shrink_in_progress_map.is_empty());
        self.get_slot_stores(slot)
            .map(|storages| storages.read().unwrap().is_empty())
            .unwrap_or(false)
    }

    /// initialize the storage map to 'all_storages'
    pub(crate) fn initialize(&mut self, all_storages: AccountStorageMap) {
        assert!(self.map.is_empty());
        assert!(self.shrink_in_progress_map.is_empty());
        self.map.extend(all_storages.into_iter())
    }

    /// remove all append vecs at 'slot'
    /// returns the current contents
    pub(crate) fn remove(&self, slot: &Slot) -> Option<(Slot, SlotStores)> {
        assert!(self.shrink_in_progress_map.is_empty());
        self.map.remove(slot)
    }

    /// iterate through all (slot, append-vecs)
    pub(crate) fn iter(&self) -> dashmap::iter::Iter<Slot, SlotStores> {
        assert!(self.shrink_in_progress_map.is_empty());
        self.map.iter()
    }

    pub(crate) fn insert(&self, slot: Slot, store: Arc<AccountStorageEntry>) {
        assert!(self.shrink_in_progress_map.is_empty());
        let slot_storages: SlotStores = self.get_slot_stores(slot).unwrap_or_else(||
            // DashMap entry.or_insert() returns a RefMut, essentially a write lock,
            // which is dropped after this block ends, minimizing time held by the lock.
            // However, we still want to persist the reference to the `SlotStores` behind
            // the lock, hence we clone it out, (`SlotStores` is an Arc so is cheap to clone).
            self
                .map
                .entry(slot)
                .or_insert(Arc::new(RwLock::new(HashMap::new())))
                .clone());

        let mut write = slot_storages.write().unwrap();
        assert!(write.insert(store.append_vec_id(), store).is_none());
    }

    /// called when shrinking begins on a slot and append vec.
    /// When 'ShrinkInProgress' is dropped by caller, the old store will be removed from the storage map.
    /// Fails if there are no existing stores at the slot.
    /// 'new_store' will be replacing the current store at 'slot' in 'map'
    /// 1. insert 'shrinking_store' into 'shrink_in_progress_map'
    /// 2. remove 'shrinking_store' from 'map'
    /// 3. insert 'new_store' into 'map' (atomic with #2)
    /// #1 allows tx processing loads to find the item in 'shrink_in_progress_map' even when it is removed from 'map'
    /// #3 allows tx processing loads to find the item in 'map' after the index is updated and it is now located in 'new_store'
    /// loading for tx must check
    /// a. 'map', because it is usually there
    /// b. 'shrink_in_progress_map' because it may have moved there (#1) before it was removed from 'map' (#3)
    /// Note that if it fails step a and b, then the retry code in accounts_db will look in the index again and should find the updated index entry to 'new_store'
    pub(crate) fn shrinking_in_progress(
        &self,
        slot: Slot,
        new_store: Arc<AccountStorageEntry>,
    ) -> ShrinkInProgress<'_> {
        let slot_storages = self.get_slot_stores_shrinking_in_progress_ok(slot).unwrap();
        let shrinking_store = Arc::clone(slot_storages.read().unwrap().iter().next().unwrap().1);

        let previous_id = shrinking_store.append_vec_id();
        let new_id = new_store.append_vec_id();
        // 1. insert 'shrinking_store' into 'shrink_in_progress_map'
        assert!(self
            .shrink_in_progress_map
            .insert(slot, Arc::clone(&shrinking_store))
            .is_none());

        let mut storages = slot_storages.write().unwrap();
        // 2. remove 'shrinking_store' from 'map'
        assert!(storages.remove(&previous_id).is_some());
        // 3. insert 'new_store' into 'map' (atomic with #2)
        assert!(storages.insert(new_id, Arc::clone(&new_store)).is_none());

        ShrinkInProgress {
            storage: self,
            slot,
            new_store,
            old_store: shrinking_store,
        }
    }

    /// 'id' can now be forgotten from the storage map
    /// It will have been in 'shrink_in_progress_map'.
    /// 'id' will have been removed from 'map' in a prior call to 'shrinking_in_progress'
    fn remove_shrunk_storage(&self, slot: Slot) {
        assert!(self.shrink_in_progress_map.remove(&slot).is_some());
    }

    #[cfg(test)]
    pub(crate) fn insert_empty_at_slot(&self, slot: Slot) {
        assert!(self.shrink_in_progress_map.is_empty());
        self.map
            .entry(slot)
            .or_insert(Arc::new(RwLock::new(HashMap::new())));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }
}

/// exists while there is a shrink in progress
/// keeps track of the 'new_store' being created and the 'old_store' being replaced.
pub(crate) struct ShrinkInProgress<'a> {
    storage: &'a AccountStorage,
    /// newly shrunk store with a subset of contents from 'old_store'
    new_store: Arc<AccountStorageEntry>,
    /// old store which will be shrunk and replaced
    old_store: Arc<AccountStorageEntry>,
    slot: Slot,
}

/// called when the shrink is no longer in progress. This means we can release the old append vec and update the map of slot -> append vec
impl<'a> Drop for ShrinkInProgress<'a> {
    fn drop(&mut self) {
        self.storage.remove_shrunk_storage(self.slot);
    }
}

impl<'a> ShrinkInProgress<'a> {
    pub(crate) fn new_storage(&self) -> &Arc<AccountStorageEntry> {
        &self.new_store
    }
    pub(crate) fn old_storage(&self) -> &Arc<AccountStorageEntry> {
        &self.old_store
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Deserialize, Serialize, AbiExample, AbiEnumVisitor)]
pub enum AccountStorageStatus {
    Available = 0,
    Full = 1,
    Candidate = 2,
}

impl Default for AccountStorageStatus {
    fn default() -> Self {
        Self::Available
    }
}
