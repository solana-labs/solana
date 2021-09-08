use crate::accounts_index::AccountMapEntry;
use crate::accounts_index::{IsCached, SlotList};
use crate::bucket_map_holder::{BucketMapHolder, K};

use solana_sdk::pubkey::Pubkey;
use std::fmt::Debug;
use std::ops::{Range, RangeBounds};
use std::sync::Arc;

// one instance of this represents one bin of the accounts index.
#[derive(Debug)]
pub struct InMemAccountsIndex<V: IsCached> {
    // backing store
    disk: Arc<BucketMapHolder<V>>,
    // the bin this instance holds
    bin_index: usize,
}

impl<V: IsCached> InMemAccountsIndex<V> {
    pub fn new(bucket_map: &Arc<BucketMapHolder<V>>, bin_index: usize) -> Self {
        Self {
            disk: bucket_map.clone(),
            bin_index,
        }
    }

    pub fn new_bucket_map(bins: usize) -> Arc<BucketMapHolder<V>> {
        Arc::new(BucketMapHolder::new(bins))
    }

    pub fn iter<R>(&self, range: Option<&R>) -> Vec<(K, AccountMapEntry<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        self.disk.range(self.bin_index, range)
    }

    pub fn keys(&self) -> Vec<Pubkey> {
        self.disk
            .keys(self.bin_index, None::<&Range<Pubkey>>)
            .unwrap_or_default()
    }

    pub fn remove_if_slot_list_empty(&self, key: &Pubkey) -> bool {
        self.disk.remove_if_slot_list_empty(self.bin_index, key)
    }

    pub fn upsert(
        &self,
        pubkey: &Pubkey,
        new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        previous_slot_entry_was_cached: bool,
    ) {
        self.disk.upsert(
            self.bin_index,
            pubkey,
            new_value,
            reclaims,
            previous_slot_entry_was_cached,
        );
    }

    // return None if item was created new
    // if entry for pubkey already existed, return Some(entry).
    // Otherwise, similar to upsert. Called only at startup.
    // This will be refactored later to be async and not return with information about whether
    //  the item existed or not.
    pub fn upsert_with_lock_pubkey_result(
        &self,
        pubkey: Pubkey,
        new_entry: AccountMapEntry<V>,
    ) -> Option<Pubkey> {
        self.disk
            .upsert_with_lock_pubkey_result(self.bin_index, pubkey, new_entry)
    }

    pub fn get(&self, key: &K) -> Option<AccountMapEntry<V>> {
        self.disk.get(self.bin_index, key)
    }
    pub fn remove(&mut self, key: &K) {
        self.disk.delete_key(self.bin_index, key);
    }

    pub fn len(&self) -> usize {
        self.disk.len_bucket(self.bin_index)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
