//! Manage the map of slot -> append vecs

use {
    crate::accounts_db::{AccountStorageEntry, AppendVecId, SlotStores, SnapshotStorage},
    dashmap::DashMap,
    solana_sdk::clock::Slot,
    std::{
        collections::{hash_map::RandomState, HashMap},
        sync::{Arc, RwLock},
    },
};

pub type AccountStorageMap = DashMap<Slot, SlotStores>;

#[derive(Clone, Default, Debug)]
pub struct AccountStorage {
    map: AccountStorageMap,
}

impl AccountStorage {
    pub(crate) fn get_account_storage_entry(
        &self,
        slot: Slot,
        store_id: AppendVecId,
    ) -> Option<Arc<AccountStorageEntry>> {
        self.get_slot_stores(slot)
            .and_then(|storage_map| storage_map.read().unwrap().get(&store_id).cloned())
    }

    pub fn get_slot_stores(&self, slot: Slot) -> Option<SlotStores> {
        self.map.get(&slot).map(|result| result.value().clone())
    }

    pub(crate) fn get_slot_storage_entries(&self, slot: Slot) -> Option<SnapshotStorage> {
        self.get_slot_stores(slot)
            .map(|res| res.read().unwrap().values().cloned().collect())
    }

    pub(crate) fn slot_store_count(&self, slot: Slot, store_id: AppendVecId) -> Option<usize> {
        self.get_account_storage_entry(slot, store_id)
            .map(|store| store.count())
    }

    pub(crate) fn all_slots(&self) -> Vec<Slot> {
        self.map.iter().map(|iter_item| *iter_item.key()).collect()
    }

    pub(crate) fn extend(&mut self, source: AccountStorageMap) {
        self.map.extend(source.into_iter())
    }

    pub(crate) fn remove(&self, slot: &Slot) -> Option<(Slot, SlotStores)> {
        self.map.remove(slot)
    }

    pub(crate) fn iter(&self) -> dashmap::iter::Iter<Slot, SlotStores> {
        self.map.iter()
    }
    pub(crate) fn get(
        &self,
        slot: &Slot,
    ) -> Option<dashmap::mapref::one::Ref<'_, Slot, SlotStores, RandomState>> {
        self.map.get(slot)
    }
    pub(crate) fn insert(&self, slot: Slot, store: Arc<AccountStorageEntry>) {
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

        assert!(slot_storages
            .write()
            .unwrap()
            .insert(store.append_vec_id(), store)
            .is_none());
    }

    /// called when shrinking begins on a slot and append vec.
    /// When 'ShrinkInProgress' is dropped by caller, the old store will be removed from the storage map.
    pub(crate) fn shrinking_in_progress(
        &self,
        slot: Slot,
        new_store: Arc<AccountStorageEntry>,
    ) -> ShrinkInProgress<'_> {
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

        let shrinking_store = Arc::clone(slot_storages.read().unwrap().iter().next().unwrap().1);

        let new_id = new_store.append_vec_id();
        let mut storages = slot_storages.write().unwrap();
        // insert 'new_store' into 'map'
        assert!(storages.insert(new_id, Arc::clone(&new_store)).is_none());

        ShrinkInProgress {
            storage: self,
            slot,
            new_store,
            old_store: shrinking_store,
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_empty_at_slot(&self, slot: Slot) {
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
        // the slot must be in the map
        let slot_storages: SlotStores = self.storage.get_slot_stores(self.slot).unwrap();

        let mut storages = slot_storages.write().unwrap();
        // the id must be in the hashmap
        assert!(
            storages.remove(&self.old_store.append_vec_id()).is_some(),
            "slot: {}, len: {}",
            self.slot,
            storages.len()
        );
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
