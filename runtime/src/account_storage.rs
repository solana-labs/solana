//! Manage the map of slot -> append vec

use {
    crate::accounts_db::{AccountStorageEntry, AppendVecId},
    dashmap::DashMap,
    solana_sdk::clock::Slot,
    std::sync::Arc,
};

pub mod meta;

#[derive(Clone, Debug)]
pub struct AccountStorageReference {
    /// the single storage for a given slot
    pub(crate) storage: Arc<AccountStorageEntry>,
    /// id can be read from 'storage', but it is an atomic read.
    /// id will never change while a storage is held, so we store it separately here for faster runtime lookup in 'get_account_storage_entry'
    pub(crate) id: AppendVecId,
}

pub type AccountStorageMap = DashMap<Slot, AccountStorageReference>;

#[derive(Default, Debug)]
pub struct AccountStorage {
    /// map from Slot -> the single append vec for the slot
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
    /// This fn looks in 'map' first, then in 'shrink_in_progress_map', then in 'map' again because
    /// 'shrinking_in_progress' first inserts the new append vec into 'shrink_in_progress_map'
    /// Then, when 'shrink_in_progress' is dropped,
    /// the old append vec is replaced in 'map' with the new append vec
    /// then the new append vec is dropped from 'shrink_in_progress_map'.
    /// So, it is possible for a race with this fn and dropping 'shrink_in_progress'.
    /// Callers to this function have 2 choices:
    /// 1. hold the account index read lock for the pubkey so that the account index entry cannot be changed prior to or during this call. (scans do this)
    /// 2. expect to be ready to start over and read the index again if this function returns None
    /// Operations like shrinking or write cache flushing may have updated the index between when the caller read the index and called this function to
    /// load from the append vec specified in the index.
    /// In practice, this fn will return the entry from the map in the very first lookup unless a shrink is in progress.
    /// The third lookup will only be called if a requesting thread exactly interposes itself between the 2 map manipulations in the drop of 'shrink_in_progress'.
    pub(crate) fn get_account_storage_entry(
        &self,
        slot: Slot,
        store_id: AppendVecId,
    ) -> Option<Arc<AccountStorageEntry>> {
        let lookup_in_map = || {
            self.map
                .get(&slot)
                .and_then(|r| (r.id == store_id).then_some(Arc::clone(&r.storage)))
        };

        lookup_in_map()
            .or_else(|| {
                self.shrink_in_progress_map.get(&slot).and_then(|entry| {
                    (entry.value().append_vec_id() == store_id).then(|| Arc::clone(entry.value()))
                })
            })
            .or_else(lookup_in_map)
    }

    /// returns true if shrink in progress is NOT active
    pub(crate) fn no_shrink_in_progress(&self) -> bool {
        self.shrink_in_progress_map.is_empty()
    }

    /// return the append vec for 'slot' if it exists
    /// This is only ever called when shrink is not possibly running and there is a max of 1 append vec per slot.
    pub(crate) fn get_slot_storage_entry(&self, slot: Slot) -> Option<Arc<AccountStorageEntry>> {
        assert!(self.no_shrink_in_progress());
        self.get_slot_storage_entry_shrinking_in_progress_ok(slot)
    }

    /// return the append vec for 'slot' if it exists
    pub(crate) fn get_slot_storage_entry_shrinking_in_progress_ok(
        &self,
        slot: Slot,
    ) -> Option<Arc<AccountStorageEntry>> {
        self.map.get(&slot).map(|entry| Arc::clone(&entry.storage))
    }

    pub(crate) fn all_slots(&self) -> Vec<Slot> {
        assert!(self.no_shrink_in_progress());
        self.map.iter().map(|iter_item| *iter_item.key()).collect()
    }

    /// returns true if there is no entry for 'slot'
    #[cfg(test)]
    pub(crate) fn is_empty_entry(&self, slot: Slot) -> bool {
        assert!(self.no_shrink_in_progress());
        self.map.get(&slot).is_none()
    }

    /// initialize the storage map to 'all_storages'
    pub(crate) fn initialize(&mut self, all_storages: AccountStorageMap) {
        assert!(self.map.is_empty());
        assert!(self.no_shrink_in_progress());
        self.map.extend(all_storages.into_iter())
    }

    /// remove the append vec at 'slot'
    /// returns the current contents
    pub(crate) fn remove(
        &self,
        slot: &Slot,
        shrink_can_be_active: bool,
    ) -> Option<Arc<AccountStorageEntry>> {
        assert!(shrink_can_be_active || self.shrink_in_progress_map.is_empty());
        self.map.remove(slot).map(|(_, entry)| entry.storage)
    }

    /// iterate through all (slot, append-vec)
    pub(crate) fn iter(&self) -> AccountStorageIter<'_> {
        assert!(self.no_shrink_in_progress());
        AccountStorageIter::new(self)
    }

    pub(crate) fn insert(&self, slot: Slot, store: Arc<AccountStorageEntry>) {
        assert!(self.no_shrink_in_progress());
        assert!(self
            .map
            .insert(
                slot,
                AccountStorageReference {
                    id: store.append_vec_id(),
                    storage: store,
                }
            )
            .is_none());
    }

    /// called when shrinking begins on a slot and append vec.
    /// When 'ShrinkInProgress' is dropped by caller, the old store will be replaced with 'new_store' in the storage map.
    /// Fails if there are no existing stores at the slot.
    /// 'new_store' will be replacing the current store at 'slot' in 'map'
    /// So, insert 'new_store' into 'shrink_in_progress_map'.
    /// This allows tx processing loads to find the items in 'shrink_in_progress_map' after the index is updated and item is now located in 'new_store'.
    pub(crate) fn shrinking_in_progress(
        &self,
        slot: Slot,
        new_store: Arc<AccountStorageEntry>,
    ) -> ShrinkInProgress<'_> {
        let shrinking_store = Arc::clone(
            &self
                .map
                .get(&slot)
                .expect("no pre-existing storage for shrinking slot")
                .value()
                .storage,
        );

        // insert 'new_store' into 'shrink_in_progress_map'
        assert!(
            self.shrink_in_progress_map
                .insert(slot, Arc::clone(&new_store))
                .is_none(),
            "duplicate call"
        );

        ShrinkInProgress {
            storage: self,
            slot,
            new_store,
            old_store: shrinking_store,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }
}

/// iterate contents of AccountStorage without exposing internals
pub struct AccountStorageIter<'a> {
    iter: dashmap::iter::Iter<'a, Slot, AccountStorageReference>,
}

impl<'a> AccountStorageIter<'a> {
    pub fn new(storage: &'a AccountStorage) -> Self {
        Self {
            iter: storage.map.iter(),
        }
    }
}

impl<'a> Iterator for AccountStorageIter<'a> {
    type Item = (Slot, Arc<AccountStorageEntry>);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(entry) = self.iter.next() {
            let slot = entry.key();
            let store = entry.value();
            return Some((*slot, Arc::clone(&store.storage)));
        }
        None
    }
}

/// exists while there is a shrink in progress
/// keeps track of the 'new_store' being created and the 'old_store' being replaced.
#[derive(Debug)]
pub(crate) struct ShrinkInProgress<'a> {
    storage: &'a AccountStorage,
    /// old store which will be shrunk and replaced
    old_store: Arc<AccountStorageEntry>,
    /// newly shrunk store with a subset of contents from 'old_store'
    new_store: Arc<AccountStorageEntry>,
    slot: Slot,
}

/// called when the shrink is no longer in progress. This means we can release the old append vec and update the map of slot -> append vec
impl<'a> Drop for ShrinkInProgress<'a> {
    fn drop(&mut self) {
        assert_eq!(
            self.storage
                .map
                .insert(
                    self.slot,
                    AccountStorageReference {
                        storage: Arc::clone(&self.new_store),
                        id: self.new_store.append_vec_id()
                    }
                )
                .map(|store| store.id),
            Some(self.old_store.append_vec_id())
        );

        // The new store can be removed from 'shrink_in_progress_map'
        assert!(self
            .storage
            .shrink_in_progress_map
            .remove(&self.slot)
            .is_some());
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

#[cfg(test)]
pub(crate) mod tests {
    use {super::*, std::path::Path};

    #[test]
    /// The new cold storage will fail this test as it does not have
    /// the concept of capacity yet.
    fn test_shrink_in_progress() {
        // test that we check in order map then shrink_in_progress_map
        let storage = AccountStorage::default();
        let slot = 0;
        let id = 0;
        // empty everything
        assert!(storage.get_account_storage_entry(slot, id).is_none());

        // add a map store
        let common_store_path = Path::new("");
        let store_file_size = 4000;
        let store_file_size2 = store_file_size * 2;
        // 2 append vecs with same id, but different sizes
        let entry = Arc::new(AccountStorageEntry::new_av(
            common_store_path,
            slot,
            id,
            store_file_size,
        ));
        let entry2 = Arc::new(AccountStorageEntry::new_av(
            common_store_path,
            slot,
            id,
            store_file_size2,
        ));
        storage
            .map
            .insert(slot, AccountStorageReference { id, storage: entry });

        // look in map
        assert_eq!(
            store_file_size,
            storage
                .get_account_storage_entry(slot, id)
                .map(|entry| entry.accounts.capacity())
                .unwrap_or_default()
        );

        // look in shrink_in_progress_map
        storage.shrink_in_progress_map.insert(slot, entry2);

        // look in map
        assert_eq!(
            store_file_size,
            storage
                .get_account_storage_entry(slot, id)
                .map(|entry| entry.accounts.capacity())
                .unwrap_or_default()
        );

        // remove from map
        storage.map.remove(&slot).unwrap();

        // look in shrink_in_progress_map
        assert_eq!(
            store_file_size2,
            storage
                .get_account_storage_entry(slot, id)
                .map(|entry| entry.accounts.capacity())
                .unwrap_or_default()
        );
    }

    impl AccountStorage {
        fn get_test_storage_with_id(&self, id: AppendVecId) -> Arc<AccountStorageEntry> {
            let slot = 0;
            // add a map store
            let common_store_path = Path::new("");
            let store_file_size = 4000;
            Arc::new(AccountStorageEntry::new(
                common_store_path,
                slot,
                id,
                store_file_size,
            ))
        }
        fn get_test_storage(&self) -> Arc<AccountStorageEntry> {
            self.get_test_storage_with_id(0)
        }
    }

    #[test]
    #[should_panic(expected = "assertion failed: self.no_shrink_in_progress()")]
    fn test_get_slot_storage_entry_fail() {
        let storage = AccountStorage::default();
        storage
            .shrink_in_progress_map
            .insert(0, storage.get_test_storage());
        storage.get_slot_storage_entry(0);
    }

    #[test]
    #[should_panic(expected = "assertion failed: self.no_shrink_in_progress()")]
    fn test_all_slots_fail() {
        let storage = AccountStorage::default();
        storage
            .shrink_in_progress_map
            .insert(0, storage.get_test_storage());
        storage.all_slots();
    }

    #[test]
    #[should_panic(expected = "assertion failed: self.no_shrink_in_progress()")]
    fn test_initialize_fail() {
        let mut storage = AccountStorage::default();
        storage
            .shrink_in_progress_map
            .insert(0, storage.get_test_storage());
        storage.initialize(AccountStorageMap::default());
    }

    #[test]
    #[should_panic(
        expected = "assertion failed: shrink_can_be_active || self.shrink_in_progress_map.is_empty()"
    )]
    fn test_remove_fail() {
        let storage = AccountStorage::default();
        storage
            .shrink_in_progress_map
            .insert(0, storage.get_test_storage());
        storage.remove(&0, false);
    }

    #[test]
    #[should_panic(expected = "assertion failed: self.no_shrink_in_progress()")]
    fn test_iter_fail() {
        let storage = AccountStorage::default();
        storage
            .shrink_in_progress_map
            .insert(0, storage.get_test_storage());
        storage.iter();
    }

    #[test]
    #[should_panic(expected = "assertion failed: self.no_shrink_in_progress()")]
    fn test_insert_fail() {
        let storage = AccountStorage::default();
        let sample = storage.get_test_storage();
        storage.shrink_in_progress_map.insert(0, sample.clone());
        storage.insert(0, sample);
    }

    #[test]
    #[should_panic(expected = "duplicate call")]
    fn test_shrinking_in_progress_fail3() {
        // already entry in shrink_in_progress_map
        let storage = AccountStorage::default();
        let sample = storage.get_test_storage();
        storage.map.insert(
            0,
            AccountStorageReference {
                id: 0,
                storage: sample.clone(),
            },
        );
        storage.shrink_in_progress_map.insert(0, sample.clone());
        storage.shrinking_in_progress(0, sample);
    }

    #[test]
    #[should_panic(expected = "duplicate call")]
    fn test_shrinking_in_progress_fail4() {
        // already called 'shrink_in_progress' on this slot and it is still active
        let storage = AccountStorage::default();
        let sample_to_shrink = storage.get_test_storage();
        let sample = storage.get_test_storage();
        storage.map.insert(
            0,
            AccountStorageReference {
                id: 0,
                storage: sample_to_shrink,
            },
        );
        let _shrinking_in_progress = storage.shrinking_in_progress(0, sample.clone());
        storage.shrinking_in_progress(0, sample);
    }

    #[test]
    fn test_shrinking_in_progress_second_call() {
        // already called 'shrink_in_progress' on this slot, but it finished, so we succeed
        // verify data structures during and after shrink and then with subsequent shrink call
        let storage = AccountStorage::default();
        let slot = 0;
        let id_to_shrink = 1;
        let id_shrunk = 0;
        let sample_to_shrink = storage.get_test_storage_with_id(id_to_shrink);
        let sample = storage.get_test_storage();
        storage.map.insert(
            slot,
            AccountStorageReference {
                id: id_to_shrink,
                storage: sample_to_shrink,
            },
        );
        let shrinking_in_progress = storage.shrinking_in_progress(slot, sample.clone());
        assert!(storage.map.contains_key(&slot));
        assert_eq!(
            id_to_shrink,
            storage.map.get(&slot).unwrap().storage.append_vec_id()
        );
        assert_eq!(
            (slot, id_shrunk),
            storage
                .shrink_in_progress_map
                .iter()
                .next()
                .map(|r| (*r.key(), r.value().append_vec_id()))
                .unwrap()
        );
        drop(shrinking_in_progress);
        assert!(storage.map.contains_key(&slot));
        assert_eq!(
            id_shrunk,
            storage.map.get(&slot).unwrap().storage.append_vec_id()
        );
        assert!(storage.shrink_in_progress_map.is_empty());
        storage.shrinking_in_progress(slot, sample);
    }

    #[test]
    #[should_panic(expected = "no pre-existing storage for shrinking slot")]
    fn test_shrinking_in_progress_fail1() {
        // nothing in slot currently
        let storage = AccountStorage::default();
        let sample = storage.get_test_storage();
        storage.shrinking_in_progress(0, sample);
    }

    #[test]
    #[should_panic(expected = "no pre-existing storage for shrinking slot")]
    fn test_shrinking_in_progress_fail2() {
        // nothing in slot currently, but there is an empty map entry
        let storage = AccountStorage::default();
        let sample = storage.get_test_storage();
        storage.shrinking_in_progress(0, sample);
    }

    #[test]
    fn test_missing() {
        // already called 'shrink_in_progress' on this slot, but it finished, so we succeed
        // verify data structures during and after shrink and then with subsequent shrink call
        let storage = AccountStorage::default();
        let sample = storage.get_test_storage();
        let id = sample.append_vec_id();
        let missing_id = 9999;
        let slot = sample.slot();
        // id is missing since not in maps at all
        assert!(storage.get_account_storage_entry(slot, id).is_none());
        // missing should always be missing
        assert!(storage
            .get_account_storage_entry(slot, missing_id)
            .is_none());
        storage.map.insert(
            slot,
            AccountStorageReference {
                id,
                storage: sample.clone(),
            },
        );
        // id is found in map
        assert!(storage.get_account_storage_entry(slot, id).is_some());
        assert!(storage
            .get_account_storage_entry(slot, missing_id)
            .is_none());
        storage
            .shrink_in_progress_map
            .insert(slot, Arc::clone(&sample));
        // id is found in map
        assert!(storage
            .get_account_storage_entry(slot, missing_id)
            .is_none());
        assert!(storage.get_account_storage_entry(slot, id).is_some());
        storage.map.remove(&slot);
        // id is found in shrink_in_progress_map
        assert!(storage
            .get_account_storage_entry(slot, missing_id)
            .is_none());
        assert!(storage.get_account_storage_entry(slot, id).is_some());
    }
}
