//! Manage the map of slot -> append vecs

use {
    crate::accounts_db::{AccountStorageEntry, AppendVecId, SlotStores, SnapshotStorage},
    dashmap::DashMap,
    solana_sdk::clock::Slot,
    std::sync::Arc,
};

pub type AccountStorageMap = DashMap<Slot, SlotStores>;

#[derive(Clone, Default, Debug)]
pub struct AccountStorage {
    pub map: AccountStorageMap,
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
