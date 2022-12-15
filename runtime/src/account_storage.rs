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
