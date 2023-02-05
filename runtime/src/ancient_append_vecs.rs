//! helpers for squashing append vecs into ancient append vecs
//! an ancient append vec is:
//! 1. a slot that is older than an epoch old
//! 2. multiple 'slots' squashed into a single older (ie. ancient) slot for convenience and performance
//! Otherwise, an ancient append vec is the same as any other append vec
use {
    crate::{
        accounts_db::{AccountStorageEntry, AccountsDb},
        append_vec::{AppendVec, StoredAccountMeta},
    },
    rand::{thread_rng, Rng},
    solana_sdk::{clock::Slot, saturating_add_assign},
    std::sync::Arc,
};

/// info about a storage eligible to be combined into an ancient append vec.
/// Useful to help sort vecs of storages.
#[derive(Debug)]
#[allow(dead_code)]
struct SlotInfo {
    storage: Arc<AccountStorageEntry>,
    /// slot of storage
    slot: Slot,
    /// total capacity of storage
    capacity: u64,
    /// # alive bytes in storage
    alive_bytes: u64,
    /// true if this should be shrunk due to ratio
    should_shrink: bool,
}

/// info for all storages in ancient slots
/// 'all_infos' contains all slots and storages that are ancient
#[derive(Default, Debug)]
struct AncientSlotInfos {
    /// info on all ancient storages
    all_infos: Vec<SlotInfo>,
    /// indexes to 'all_info' for storages that should be shrunk because alive ratio is too low.
    /// subset of all_infos
    shrink_indexes: Vec<usize>,
    /// total alive bytes across contents of 'shrink_indexes'
    total_alive_bytes_shrink: u64,
    /// total alive bytes across all slots
    total_alive_bytes: u64,
}

impl AncientSlotInfos {
    /// add info for 'storage'
    fn add(&mut self, slot: Slot, storage: Arc<AccountStorageEntry>, can_randomly_shrink: bool) {
        let alive_bytes = storage.alive_bytes() as u64;
        if alive_bytes > 0 {
            let capacity = storage.accounts.capacity();
            let should_shrink = if capacity > 0 {
                let alive_ratio = alive_bytes * 100 / capacity;
                (alive_ratio < 90) || (can_randomly_shrink && thread_rng().gen_range(0, 10000) == 0)
            } else {
                false
            };
            // two criteria we're shrinking by later:
            // 1. alive ratio so that we don't consume too much disk space with dead accounts
            // 2. # of active ancient roots, so that we don't consume too many open file handles

            if should_shrink {
                // alive ratio is too low, so prioritize combining this slot with others
                // to reduce disk space used
                saturating_add_assign!(self.total_alive_bytes_shrink, alive_bytes);
                self.shrink_indexes.push(self.all_infos.len());
            }
            self.all_infos.push(SlotInfo {
                slot,
                capacity,
                storage,
                alive_bytes,
                should_shrink,
            });
            self.total_alive_bytes += alive_bytes;
        }
    }
}

impl AccountsDb {
    /// go through all slots and populate 'SlotInfo', per slot
    /// This provides the list of possible ancient slots to sort, filter, and then combine.
    #[allow(dead_code)]
    fn calc_ancient_slot_info(
        &self,
        slots: Vec<Slot>,
        can_randomly_shrink: bool,
    ) -> AncientSlotInfos {
        let len = slots.len();
        let mut infos = AncientSlotInfos {
            shrink_indexes: Vec::with_capacity(len),
            all_infos: Vec::with_capacity(len),
            ..AncientSlotInfos::default()
        };

        for slot in &slots {
            if let Some(storage) = self.storage.get_slot_storage_entry(*slot) {
                infos.add(*slot, storage, can_randomly_shrink);
            }
        }
        infos
    }
}

/// a set of accounts need to be stored.
/// If there are too many to fit in 'Primary', the rest are put in 'Overflow'
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StorageSelector {
    Primary,
    Overflow,
}

/// reference a set of accounts to store
/// The accounts may have to be split between 2 storages (primary and overflow) if there is not enough room in the primary storage.
/// The 'store' functions need data stored in a slice of specific type.
/// We need 1-2 of these slices constructed based on available bytes and individual account sizes.
/// The slice arithmetic accross both hashes and account data gets messy. So, this struct abstracts that.
pub struct AccountsToStore<'a> {
    accounts: &'a [&'a StoredAccountMeta<'a>],
    /// if 'accounts' contains more items than can be contained in the primary storage, then we have to split these accounts.
    /// 'index_first_item_overflow' specifies the index of the first item in 'accounts' that will go into the overflow storage
    index_first_item_overflow: usize,
    pub slot: Slot,
}

impl<'a> AccountsToStore<'a> {
    /// break 'stored_accounts' into primary and overflow
    /// available_bytes: how many bytes remain in the primary storage. Excess accounts will be directed to an overflow storage
    pub fn new(
        mut available_bytes: u64,
        accounts: &'a [&'a StoredAccountMeta<'a>],
        alive_total_bytes: usize,
        slot: Slot,
    ) -> Self {
        let num_accounts = accounts.len();
        // index of the first account that doesn't fit in the current append vec
        let mut index_first_item_overflow = num_accounts; // assume all fit
        if alive_total_bytes > available_bytes as usize {
            // not all the alive bytes fit, so we have to find how many accounts fit within available_bytes
            for (i, account) in accounts.iter().enumerate() {
                let account_size = account.stored_size as u64;
                if available_bytes >= account_size {
                    available_bytes = available_bytes.saturating_sub(account_size);
                } else if index_first_item_overflow == num_accounts {
                    // the # of accounts we have so far seen is the most that will fit in the current ancient append vec
                    index_first_item_overflow = i;
                    break;
                }
            }
        }
        Self {
            accounts,
            index_first_item_overflow,
            slot,
        }
    }

    /// true if a request to 'get' 'Overflow' would return accounts & hashes
    pub fn has_overflow(&self) -> bool {
        self.index_first_item_overflow < self.accounts.len()
    }

    /// get the accounts to store in the given 'storage'
    pub fn get(&self, storage: StorageSelector) -> &[&'a StoredAccountMeta<'a>] {
        let range = match storage {
            StorageSelector::Primary => 0..self.index_first_item_overflow,
            StorageSelector::Overflow => self.index_first_item_overflow..self.accounts.len(),
        };
        &self.accounts[range]
    }
}

/// capacity of an ancient append vec
pub fn get_ancient_append_vec_capacity() -> u64 {
    use crate::append_vec::MAXIMUM_APPEND_VEC_FILE_SIZE;
    // smaller than max by a bit just in case
    // some functions add slop on allocation
    // The bigger an append vec is, the more unwieldy it becomes to shrink, create, write.
    // 1/10 of max is a reasonable size in practice.
    MAXIMUM_APPEND_VEC_FILE_SIZE / 10 - 2048
}

/// is this a max-size append vec designed to be used as an ancient append vec?
pub fn is_ancient(storage: &AppendVec) -> bool {
    storage.capacity() >= get_ancient_append_vec_capacity()
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            accounts_db::{
                get_temp_accounts_paths,
                tests::{
                    create_db_with_storages_and_index, create_storages_and_update_index,
                    remove_account_for_tests,
                },
            },
            append_vec::{AccountMeta, StoredAccountMeta, StoredMeta},
        },
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            hash::Hash,
            pubkey::Pubkey,
        },
    };

    #[test]
    fn test_accounts_to_store_simple() {
        let map = vec![];
        let slot = 1;
        let accounts_to_store = AccountsToStore::new(0, &map, 0, slot);
        for selector in [StorageSelector::Primary, StorageSelector::Overflow] {
            let accounts = accounts_to_store.get(selector);
            assert!(accounts.is_empty());
        }
        assert!(!accounts_to_store.has_overflow());
    }

    #[test]
    fn test_accounts_to_store_more() {
        let pubkey = Pubkey::from([1; 32]);
        let account_size = 3;

        let account = AccountSharedData::default();

        let account_meta = AccountMeta {
            lamports: 1,
            owner: Pubkey::from([2; 32]),
            executable: false,
            rent_epoch: 0,
        };
        let offset = 3;
        let hash = Hash::new(&[2; 32]);
        let stored_meta = StoredMeta {
            /// global write version
            write_version_obsolete: 0,
            /// key for the account
            pubkey,
            data_len: 43,
        };
        let account = StoredAccountMeta {
            meta: &stored_meta,
            /// account data
            account_meta: &account_meta,
            data: account.data(),
            offset,
            stored_size: account_size,
            hash: &hash,
        };
        let map = vec![&account];
        for (selector, available_bytes) in [
            (StorageSelector::Primary, account_size),
            (StorageSelector::Overflow, account_size - 1),
        ] {
            let slot = 1;
            let alive_total_bytes = account_size;
            let accounts_to_store =
                AccountsToStore::new(available_bytes as u64, &map, alive_total_bytes, slot);
            let accounts = accounts_to_store.get(selector);
            assert_eq!(
                accounts.iter().collect::<Vec<_>>(),
                map.iter().collect::<Vec<_>>(),
                "mismatch"
            );
            let accounts = accounts_to_store.get(get_opposite(&selector));
            assert_eq!(
                selector == StorageSelector::Overflow,
                accounts_to_store.has_overflow()
            );
            assert!(accounts.is_empty());
        }
    }
    fn get_opposite(selector: &StorageSelector) -> StorageSelector {
        match selector {
            StorageSelector::Overflow => StorageSelector::Primary,
            StorageSelector::Primary => StorageSelector::Overflow,
        }
    }

    #[test]
    fn test_get_ancient_append_vec_capacity() {
        assert_eq!(
            get_ancient_append_vec_capacity(),
            crate::append_vec::MAXIMUM_APPEND_VEC_FILE_SIZE / 10 - 2048
        );
    }

    #[test]
    fn test_is_ancient() {
        for (size, expected_ancient) in [
            (get_ancient_append_vec_capacity() + 1, true),
            (get_ancient_append_vec_capacity(), true),
            (get_ancient_append_vec_capacity() - 1, false),
        ] {
            let tf = crate::append_vec::test_utils::get_append_vec_path("test_is_ancient");
            let (_temp_dirs, _paths) = get_temp_accounts_paths(1).unwrap();
            let av = AppendVec::new(&tf.path, true, size as usize);

            assert_eq!(expected_ancient, is_ancient(&av));
        }
    }

    fn assert_storage_info(info: &SlotInfo, storage: &AccountStorageEntry, should_shrink: bool) {
        assert_eq!(storage.append_vec_id(), info.storage.append_vec_id());
        assert_eq!(storage.slot(), info.slot);
        assert_eq!(storage.capacity(), info.capacity);
        assert_eq!(storage.alive_bytes(), info.alive_bytes as usize);
        assert_eq!(should_shrink, info.should_shrink);
    }

    #[test]
    fn test_calc_ancient_slot_info_one_alive() {
        let can_randomly_shrink = false;
        let alive = true;
        let slots = 1;
        for call_add in [false, true] {
            // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
            for data_size in [None, Some(1_040_000)] {
                let (db, slot1) = create_db_with_storages_and_index(alive, slots, data_size);
                let mut infos = AncientSlotInfos::default();
                let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
                let alive_bytes_expected = storage.alive_bytes();
                if call_add {
                    // test lower level 'add'
                    infos.add(slot1, Arc::clone(&storage), can_randomly_shrink);
                } else {
                    infos = db.calc_ancient_slot_info(vec![slot1], can_randomly_shrink);
                }
                assert_eq!(infos.all_infos.len(), 1);
                let should_shrink = data_size.is_none();
                assert_storage_info(infos.all_infos.first().unwrap(), &storage, should_shrink);
                if should_shrink {
                    // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                    assert_eq!(infos.shrink_indexes, vec![0]);
                    assert_eq!(infos.total_alive_bytes, alive_bytes_expected as u64);
                    assert_eq!(infos.total_alive_bytes_shrink, alive_bytes_expected as u64);
                } else {
                    assert!(infos.shrink_indexes.is_empty());
                    assert_eq!(infos.total_alive_bytes, alive_bytes_expected as u64);
                    assert_eq!(infos.total_alive_bytes_shrink, 0);
                }
            }
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_one_dead() {
        let can_randomly_shrink = false;
        let alive = false;
        let slots = 1;
        for call_add in [false, true] {
            let (db, slot1) = create_db_with_storages_and_index(alive, slots, None);
            let mut infos = AncientSlotInfos::default();
            let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
            if call_add {
                infos.add(slot1, Arc::clone(&storage), can_randomly_shrink);
            } else {
                infos = db.calc_ancient_slot_info(vec![slot1], can_randomly_shrink);
            }
            assert!(infos.all_infos.is_empty());
            assert!(infos.shrink_indexes.is_empty());
            assert_eq!(infos.total_alive_bytes, 0);
            assert_eq!(infos.total_alive_bytes_shrink, 0);
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_several() {
        let can_randomly_shrink = false;
        for alive in [true, false] {
            for slots in 2..4 {
                // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
                for data_size in [None, Some(1_040_000)] {
                    let (db, slot1) = create_db_with_storages_and_index(alive, slots, data_size);
                    let slot_vec = (slot1..(slot1 + slots as Slot)).collect::<Vec<_>>();
                    let storages = slot_vec
                        .iter()
                        .map(|slot| db.storage.get_slot_storage_entry(*slot).unwrap())
                        .collect::<Vec<_>>();
                    let alive_bytes_expected = storages
                        .iter()
                        .map(|storage| storage.alive_bytes() as u64)
                        .sum::<u64>();
                    let infos = db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink);
                    if !alive {
                        assert!(infos.all_infos.is_empty());
                        assert!(infos.shrink_indexes.is_empty());
                        assert_eq!(infos.total_alive_bytes, 0);
                        assert_eq!(infos.total_alive_bytes_shrink, 0);
                    } else {
                        assert_eq!(infos.all_infos.len(), slots);
                        let should_shrink = data_size.is_none();
                        storages
                            .iter()
                            .zip(infos.all_infos.iter())
                            .for_each(|(storage, info)| {
                                assert_storage_info(info, storage, should_shrink);
                            });
                        if should_shrink {
                            // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                            assert_eq!(
                                infos.shrink_indexes,
                                slot_vec
                                    .iter()
                                    .enumerate()
                                    .map(|(i, _)| i)
                                    .collect::<Vec<_>>()
                            );
                            assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                            assert_eq!(infos.total_alive_bytes_shrink, alive_bytes_expected);
                        } else {
                            assert!(infos.shrink_indexes.is_empty());
                            assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                            assert_eq!(infos.total_alive_bytes_shrink, 0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_one_alive_one_dead() {
        let can_randomly_shrink = false;
        for slot1_is_alive in [false, true] {
            let alives = vec![false /*dummy*/, slot1_is_alive, !slot1_is_alive];
            let slots = 2;
            // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
            for data_size in [None, Some(1_040_000)] {
                let (db, slot1) =
                    create_db_with_storages_and_index(true /*alive*/, slots, data_size);
                assert_eq!(slot1, 1); // make sure index into alives will be correct
                assert_eq!(alives[slot1 as usize], slot1_is_alive);
                let slot_vec = (slot1..(slot1 + slots as Slot)).collect::<Vec<_>>();
                let storages = slot_vec
                    .iter()
                    .map(|slot| db.storage.get_slot_storage_entry(*slot).unwrap())
                    .collect::<Vec<_>>();
                storages.iter().for_each(|storage| {
                    let slot = storage.slot();
                    let alive = alives[slot as usize];
                    if !alive {
                        // make this storage not alive
                        remove_account_for_tests(storage, storage.written_bytes() as usize, false);
                    }
                });
                let alive_storages = storages
                    .iter()
                    .filter_map(|storage| alives[storage.slot() as usize].then_some(storage))
                    .collect::<Vec<_>>();
                let alive_bytes_expected = alive_storages
                    .iter()
                    .map(|storage| storage.alive_bytes() as u64)
                    .sum::<u64>();
                let infos = db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink);
                assert_eq!(infos.all_infos.len(), 1);
                let should_shrink = data_size.is_none();
                alive_storages
                    .iter()
                    .zip(infos.all_infos.iter())
                    .for_each(|(storage, info)| {
                        assert_storage_info(info, storage, should_shrink);
                    });
                if should_shrink {
                    // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                    assert_eq!(infos.shrink_indexes, vec![0]);
                    assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                    assert_eq!(infos.total_alive_bytes_shrink, alive_bytes_expected);
                } else {
                    assert!(infos.shrink_indexes.is_empty());
                    assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                    assert_eq!(infos.total_alive_bytes_shrink, 0);
                }
            }
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_one_shrink_one_not() {
        let can_randomly_shrink = false;
        for slot1_shrink in [false, true] {
            let shrinks = vec![false /*dummy*/, slot1_shrink, !slot1_shrink];
            let slots = 2;
            // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
            let data_sizes = shrinks
                .iter()
                .map(|shrink| (!shrink).then_some(1_040_000))
                .collect::<Vec<_>>();
            let (db, slot1) =
                create_db_with_storages_and_index(true /*alive*/, 1, data_sizes[1]);
            let dead_bytes = 184; // constant based on None data size
            create_storages_and_update_index(
                &db,
                None,
                slot1 + 1,
                1,
                true,
                data_sizes[(slot1 + 1) as usize],
            );

            assert_eq!(slot1, 1); // make sure index into shrinks will be correct
            assert_eq!(shrinks[slot1 as usize], slot1_shrink);
            let slot_vec = (slot1..(slot1 + slots as Slot)).collect::<Vec<_>>();
            let storages = slot_vec
                .iter()
                .map(|slot| {
                    let storage = db.storage.get_slot_storage_entry(*slot).unwrap();
                    assert_eq!(*slot, storage.slot());
                    storage
                })
                .collect::<Vec<_>>();
            let alive_bytes_expected = storages
                .iter()
                .map(|storage| storage.alive_bytes() as u64)
                .sum::<u64>();
            let infos = db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink);
            assert_eq!(infos.all_infos.len(), 2);
            storages
                .iter()
                .zip(infos.all_infos.iter())
                .for_each(|(storage, info)| {
                    assert_storage_info(info, storage, shrinks[storage.slot() as usize]);
                });
            // data size is so small compared to min aligned file size that the storage is marked as should_shrink
            assert_eq!(
                infos.shrink_indexes,
                shrinks
                    .iter()
                    .skip(1)
                    .enumerate()
                    .filter_map(|(i, shrink)| shrink.then_some(i))
                    .collect::<Vec<_>>()
            );
            assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
            assert_eq!(infos.total_alive_bytes_shrink, dead_bytes);
        }
    }
}
