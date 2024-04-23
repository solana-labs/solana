//! trait for abstracting underlying storage of pubkey and account pairs to be written
use {
    crate::{
        account_storage::meta::StoredAccountMeta,
        accounts_db::{AccountFromStorage, AccountStorageEntry, AccountsDb},
        accounts_index::ZeroLamport,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        pubkey::Pubkey,
    },
    std::sync::{Arc, RwLock},
};

/// hold a ref to an account to store. The account could be represented in memory a few different ways
#[derive(Debug, Copy, Clone)]
pub enum AccountForStorage<'a> {
    AddressAndAccount((&'a Pubkey, &'a AccountSharedData)),
    StoredAccountMeta(&'a StoredAccountMeta<'a>),
}

impl<'a> From<(&'a Pubkey, &'a AccountSharedData)> for AccountForStorage<'a> {
    fn from(source: (&'a Pubkey, &'a AccountSharedData)) -> Self {
        Self::AddressAndAccount(source)
    }
}

impl<'a> From<&'a StoredAccountMeta<'a>> for AccountForStorage<'a> {
    fn from(source: &'a StoredAccountMeta<'a>) -> Self {
        Self::StoredAccountMeta(source)
    }
}

impl<'a> ZeroLamport for AccountForStorage<'a> {
    fn is_zero_lamport(&self) -> bool {
        self.lamports() == 0
    }
}

impl<'a> AccountForStorage<'a> {
    pub fn pubkey(&self) -> &'a Pubkey {
        match self {
            AccountForStorage::AddressAndAccount((pubkey, _account)) => pubkey,
            AccountForStorage::StoredAccountMeta(account) => account.pubkey(),
        }
    }
}

impl<'a> ReadableAccount for AccountForStorage<'a> {
    fn lamports(&self) -> u64 {
        match self {
            AccountForStorage::AddressAndAccount((_pubkey, account)) => account.lamports(),
            AccountForStorage::StoredAccountMeta(account) => account.lamports(),
        }
    }
    fn data(&self) -> &[u8] {
        match self {
            AccountForStorage::AddressAndAccount((_pubkey, account)) => account.data(),
            AccountForStorage::StoredAccountMeta(account) => account.data(),
        }
    }
    fn owner(&self) -> &Pubkey {
        match self {
            AccountForStorage::AddressAndAccount((_pubkey, account)) => account.owner(),
            AccountForStorage::StoredAccountMeta(account) => account.owner(),
        }
    }
    fn executable(&self) -> bool {
        match self {
            AccountForStorage::AddressAndAccount((_pubkey, account)) => account.executable(),
            AccountForStorage::StoredAccountMeta(account) => account.executable(),
        }
    }
    fn rent_epoch(&self) -> Epoch {
        match self {
            AccountForStorage::AddressAndAccount((_pubkey, account)) => account.rent_epoch(),
            AccountForStorage::StoredAccountMeta(account) => account.rent_epoch(),
        }
    }
    fn to_account_shared_data(&self) -> AccountSharedData {
        match self {
            AccountForStorage::AddressAndAccount((_pubkey, account)) => {
                account.to_account_shared_data()
            }
            AccountForStorage::StoredAccountMeta(account) => account.to_account_shared_data(),
        }
    }
}

lazy_static! {
    static ref DEFAULT_ACCOUNT_SHARED_DATA: AccountSharedData = AccountSharedData::default();
}

#[derive(Default, Debug)]
pub struct StorableAccountsCacher {
    slot: Slot,
    storage: Option<Arc<AccountStorageEntry>>,
}

/// abstract access to pubkey, account, slot, target_slot of either:
/// a. (slot, &[&Pubkey, &ReadableAccount])
/// b. (slot, &[&Pubkey, &ReadableAccount, Slot]) (we will use this later)
/// This trait avoids having to allocate redundant data when there is a duplicated slot parameter.
/// All legacy callers do not have a unique slot per account to store.
pub trait StorableAccounts<'a>: Sync {
    /// account at 'index'
    fn account<Ret>(
        &self,
        index: usize,
        callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
    ) -> Ret;
    /// None if account is zero lamports
    fn account_default_if_zero_lamport<Ret>(
        &self,
        index: usize,
        mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
    ) -> Ret {
        self.account(index, |account| {
            callback(if account.lamports() != 0 {
                account
            } else {
                // preserve the pubkey, but use a default value for the account
                AccountForStorage::AddressAndAccount((
                    account.pubkey(),
                    &DEFAULT_ACCOUNT_SHARED_DATA,
                ))
            })
        })
    }
    // current slot for account at 'index'
    fn slot(&self, index: usize) -> Slot;
    /// slot that all accounts are to be written to
    fn target_slot(&self) -> Slot;
    /// true if no accounts to write
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// # accounts to write
    fn len(&self) -> usize;
    /// are there accounts from multiple slots
    /// only used for an assert
    fn contains_multiple_slots(&self) -> bool {
        false
    }
}

impl<'a: 'b, 'b> StorableAccounts<'a> for (Slot, &'b [(&'a Pubkey, &'a AccountSharedData)]) {
    fn account<Ret>(
        &self,
        index: usize,
        mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
    ) -> Ret {
        callback((self.1[index].0, self.1[index].1).into())
    }
    fn slot(&self, _index: usize) -> Slot {
        // per-index slot is not unique per slot when per-account slot is not included in the source data
        self.target_slot()
    }
    fn target_slot(&self) -> Slot {
        self.0
    }
    fn len(&self) -> usize {
        self.1.len()
    }
}

/// holds slices of accounts being moved FROM a common source slot to 'target_slot'
pub struct StorableAccountsBySlot<'a> {
    target_slot: Slot,
    /// each element is (source slot, accounts moving FROM source slot)
    slots_and_accounts: &'a [(Slot, &'a [&'a AccountFromStorage])],

    /// This is calculated based off slots_and_accounts.
    /// cumulative offset of all account slices prior to this one
    /// starting_offsets[0] is the starting offset of slots_and_accounts[1]
    /// The starting offset of slots_and_accounts[0] is always 0
    starting_offsets: Vec<usize>,
    /// true if there is more than 1 slot represented in slots_and_accounts
    contains_multiple_slots: bool,
    /// total len of all accounts, across all slots_and_accounts
    len: usize,
    db: &'a AccountsDb,
    /// remember the last storage we looked up for a given slot
    cached_storage: RwLock<StorableAccountsCacher>,
}

impl<'a> StorableAccountsBySlot<'a> {
    /// each element of slots_and_accounts is (source slot, accounts moving FROM source slot)
    pub fn new(
        target_slot: Slot,
        slots_and_accounts: &'a [(Slot, &'a [&'a AccountFromStorage])],
        db: &'a AccountsDb,
    ) -> Self {
        let mut cumulative_len = 0usize;
        let mut starting_offsets = Vec::with_capacity(slots_and_accounts.len());
        let first_slot = slots_and_accounts
            .first()
            .map(|(slot, _)| *slot)
            .unwrap_or_default();
        let mut contains_multiple_slots = false;
        for (slot, accounts) in slots_and_accounts {
            cumulative_len = cumulative_len.saturating_add(accounts.len());
            starting_offsets.push(cumulative_len);
            contains_multiple_slots |= &first_slot != slot;
        }
        Self {
            target_slot,
            slots_and_accounts,
            starting_offsets,
            contains_multiple_slots,
            len: cumulative_len,
            db,
            cached_storage: RwLock::default(),
        }
    }
    /// given an overall index for all accounts in self:
    /// return (slots_and_accounts index, index within those accounts)
    fn find_internal_index(&self, index: usize) -> (usize, usize) {
        // search offsets for the accounts slice that contains 'index'.
        // This could be a binary search.
        for (offset_index, next_offset) in self.starting_offsets.iter().enumerate() {
            if next_offset > &index {
                // offset of prior entry
                let prior_offset = if offset_index > 0 {
                    self.starting_offsets[offset_index.saturating_sub(1)]
                } else {
                    0
                };
                return (offset_index, index - prior_offset);
            }
        }
        panic!("failed");
    }
}

impl<'a> StorableAccounts<'a> for StorableAccountsBySlot<'a> {
    fn account<Ret>(
        &self,
        index: usize,
        mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
    ) -> Ret {
        let indexes = self.find_internal_index(index);
        let slot = self.slots_and_accounts[indexes.0].0;
        let data = self.slots_and_accounts[indexes.0].1[indexes.1];
        let offset = data.index_info.offset();
        let mut call_callback = |storage: &AccountStorageEntry| {
            storage
                .accounts
                .get_stored_account_meta_callback(offset, |account: StoredAccountMeta| {
                    callback((&account).into())
                })
                .expect("account has to exist to be able to store it")
        };
        {
            let reader = self.cached_storage.read().unwrap();
            if reader.slot == slot {
                if let Some(storage) = reader.storage.as_ref() {
                    return call_callback(storage);
                }
            }
        }
        // cache doesn't contain a storage for this slot, so lookup storage in db.
        // note we do not use file id here. We just want the normal unshrunk storage for this slot.
        let storage = self
            .db
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(slot)
            .expect("source slot has to have a storage to be able to store accounts");
        let ret = call_callback(&storage);
        let mut writer = self.cached_storage.write().unwrap();
        writer.slot = slot;
        writer.storage = Some(storage);
        ret
    }
    fn slot(&self, index: usize) -> Slot {
        let indexes = self.find_internal_index(index);
        self.slots_and_accounts[indexes.0].0
    }
    fn target_slot(&self) -> Slot {
        self.target_slot
    }
    fn len(&self) -> usize {
        self.len
    }
    fn contains_multiple_slots(&self) -> bool {
        self.contains_multiple_slots
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            account_info::{AccountInfo, StorageLocation},
            account_storage::meta::{AccountMeta, StoredAccountMeta, StoredMeta},
            accounts_db::{get_temp_accounts_paths, AccountStorageEntry},
            accounts_file::AccountsFileProvider,
            accounts_hash::AccountHash,
            append_vec::AppendVecStoredAccountMeta,
        },
        solana_sdk::{
            account::{accounts_equal, AccountSharedData, WritableAccount},
            hash::Hash,
        },
        std::sync::Arc,
    };

    /// this is used in the test for generation of storages
    /// this is no longer used in the validator.
    /// It is very tricky to get these right. There are already tests for this. It is likely worth it to leave this here for a while until everything has settled.
    impl<'a> StorableAccounts<'a> for (Slot, &'a [&'a StoredAccountMeta<'a>]) {
        fn account<Ret>(
            &self,
            index: usize,
            mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
        ) -> Ret {
            callback(self.1[index].into())
        }
        fn slot(&self, _index: usize) -> Slot {
            // per-index slot is not unique per slot when per-account slot is not included in the source data
            self.0
        }
        fn target_slot(&self) -> Slot {
            self.0
        }
        fn len(&self) -> usize {
            self.1.len()
        }
    }

    /// this is no longer used. It is very tricky to get these right. There are already tests for this. It is likely worth it to leave this here for a while until everything has settled.
    impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a> for (Slot, &'a [&'a (Pubkey, T)])
    where
        AccountForStorage<'a>: From<(&'a Pubkey, &'a T)>,
    {
        fn account<Ret>(
            &self,
            index: usize,
            mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
        ) -> Ret {
            callback((&self.1[index].0, &self.1[index].1).into())
        }
        fn slot(&self, _index: usize) -> Slot {
            // per-index slot is not unique per slot when per-account slot is not included in the source data
            self.target_slot()
        }
        fn target_slot(&self) -> Slot {
            self.0
        }
        fn len(&self) -> usize {
            self.1.len()
        }
    }

    /// this is no longer used. It is very tricky to get these right. There are already tests for this. It is likely worth it to leave this here for a while until everything has settled.
    /// this tuple contains a single different source slot that applies to all accounts
    /// accounts are StoredAccountMeta
    impl<'a> StorableAccounts<'a> for (Slot, &'a [&'a StoredAccountMeta<'a>], Slot) {
        fn account<Ret>(
            &self,
            index: usize,
            mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
        ) -> Ret {
            callback(self.1[index].into())
        }
        fn slot(&self, _index: usize) -> Slot {
            // same other slot for all accounts
            self.2
        }
        fn target_slot(&self) -> Slot {
            self.0
        }
        fn len(&self) -> usize {
            self.1.len()
        }
    }

    fn compare<'a>(a: &impl StorableAccounts<'a>, b: &impl StorableAccounts<'a>) {
        assert_eq!(a.target_slot(), b.target_slot());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.is_empty(), b.is_empty());
        (0..a.len()).for_each(|i| {
            b.account(i, |account| {
                a.account(i, |account_a| {
                    assert_eq!(account_a.pubkey(), account.pubkey());
                    assert!(accounts_equal(&account_a, &account));
                });
            });
        })
    }

    #[test]
    fn test_contains_multiple_slots() {
        let db = AccountsDb::new_single_for_tests();
        let pk = Pubkey::from([1; 32]);
        let slot = 0;
        let lamports = 1;
        let owner = Pubkey::default();
        let executable = false;
        let rent_epoch = 0;
        let meta = StoredMeta {
            write_version_obsolete: 5,
            pubkey: pk,
            data_len: 7,
        };
        let account_meta = AccountMeta {
            lamports,
            owner,
            executable,
            rent_epoch,
        };
        let data = Vec::default();
        let offset = 99 * std::mem::size_of::<u64>(); // offset needs to be 8 byte aligned
        let stored_size = 101;
        let hash = AccountHash(Hash::new_unique());
        let stored_account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });

        let account_from_storage = AccountFromStorage::new(&stored_account);

        let accounts = [&account_from_storage, &account_from_storage];
        let accounts2 = [(slot, &accounts[..])];
        let test3 = StorableAccountsBySlot::new(slot, &accounts2[..], &db);
        assert!(!test3.contains_multiple_slots());
    }

    pub fn build_accounts_from_storage<'a>(
        accounts: impl Iterator<Item = &'a StoredAccountMeta<'a>>,
    ) -> Vec<AccountFromStorage> {
        accounts
            .map(|account| AccountFromStorage::new(account))
            .collect()
    }

    #[test]
    fn test_storable_accounts() {
        let max_slots = 3_u64;
        for target_slot in 0..max_slots {
            for entries in 0..2 {
                for starting_slot in 0..max_slots {
                    let db = AccountsDb::new_single_for_tests();
                    let data = Vec::default();
                    let hash = AccountHash(Hash::new_unique());
                    let mut raw = Vec::new();
                    let mut raw2 = Vec::new();
                    let mut raw4 = Vec::new();
                    for entry in 0..entries {
                        let pk = Pubkey::from([entry; 32]);
                        let account = AccountSharedData::create(
                            (entry as u64) * starting_slot,
                            Vec::default(),
                            Pubkey::default(),
                            false,
                            0,
                        );

                        raw.push((
                            pk,
                            account.clone(),
                            starting_slot % max_slots,
                            StoredMeta {
                                write_version_obsolete: 0, // just something
                                pubkey: pk,
                                data_len: u64::MAX, // just something
                            },
                            AccountMeta {
                                lamports: account.lamports(),
                                owner: *account.owner(),
                                executable: account.executable(),
                                rent_epoch: account.rent_epoch(),
                            },
                        ));
                    }
                    for entry in 0..entries {
                        let offset = 99 * std::mem::size_of::<u64>(); // offset needs to be 8 byte aligned
                        let stored_size = 101;
                        let raw = &raw[entry as usize];
                        raw2.push(StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
                            meta: &raw.3,
                            account_meta: &raw.4,
                            data: &data,
                            offset,
                            stored_size,
                            hash: &hash,
                        }));
                        raw4.push((raw.0, raw.1.clone()));
                    }
                    let raw2_accounts_from_storage = build_accounts_from_storage(raw2.iter());

                    let mut two = Vec::new();
                    let mut three = Vec::new();
                    let mut three_accounts_from_storage_byval = Vec::new();
                    let mut four_pubkey_and_account_value = Vec::new();
                    raw.iter()
                        .zip(
                            raw2.iter()
                                .zip(raw4.iter().zip(raw2_accounts_from_storage.iter())),
                        )
                        .for_each(|(raw, (raw2, (raw4, raw2_accounts_from_storage)))| {
                            two.push((&raw.0, &raw.1)); // 2 item tuple
                            three.push(raw2);
                            three_accounts_from_storage_byval.push(*raw2_accounts_from_storage);
                            four_pubkey_and_account_value.push(raw4);
                        });
                    let test2 = (target_slot, &two[..]);
                    let test4 = (target_slot, &four_pubkey_and_account_value[..]);

                    let source_slot = starting_slot % max_slots;

                    let storage = setup_sample_storage(&db, source_slot);
                    // since we're no longer storing `StoredAccountMeta`, we have to actually store the
                    // accounts so they can be looked up later in `db`
                    if let Some(offsets) = storage
                        .accounts
                        .append_accounts(&(source_slot, &three[..]), 0)
                    {
                        three_accounts_from_storage_byval
                            .iter_mut()
                            .zip(offsets.offsets.iter())
                            .for_each(|(account, offset)| {
                                account.index_info = AccountInfo::new(
                                    StorageLocation::AppendVec(0, *offset),
                                    if account.is_zero_lamport() { 0 } else { 1 },
                                )
                            });
                    }
                    let three_accounts_from_storage =
                        three_accounts_from_storage_byval.iter().collect::<Vec<_>>();

                    let accounts_with_slots = vec![(source_slot, &three_accounts_from_storage[..])];
                    let test3 = StorableAccountsBySlot::new(target_slot, &accounts_with_slots, &db);
                    let old_slot = starting_slot;
                    let for_slice = [(old_slot, &three_accounts_from_storage[..])];
                    let test_moving_slots2 =
                        StorableAccountsBySlot::new(target_slot, &for_slice, &db);
                    compare(&test2, &test3);
                    compare(&test2, &test4);
                    compare(&test2, &test_moving_slots2);
                    for (i, raw) in raw.iter().enumerate() {
                        test3.account(i, |account| {
                            assert_eq!(raw.0, *account.pubkey());
                            assert!(accounts_equal(&raw.1, &account));
                        });
                        assert_eq!(raw.2, test3.slot(i));
                        assert_eq!(target_slot, test4.slot(i));
                        assert_eq!(target_slot, test2.slot(i));
                        assert_eq!(old_slot, test_moving_slots2.slot(i));
                    }
                    assert_eq!(target_slot, test3.target_slot());
                    assert_eq!(target_slot, test4.target_slot());
                    assert_eq!(target_slot, test_moving_slots2.target_slot());
                    assert!(!test2.contains_multiple_slots());
                    assert!(!test4.contains_multiple_slots());
                    assert_eq!(test3.contains_multiple_slots(), entries > 1);
                }
            }
        }
    }

    fn setup_sample_storage(db: &AccountsDb, slot: Slot) -> Arc<AccountStorageEntry> {
        let id = 2;
        let file_size = 10_000;
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let data = AccountStorageEntry::new(
            &paths[0],
            slot,
            id,
            file_size,
            AccountsFileProvider::AppendVec,
        );
        let storage = Arc::new(data);
        db.storage.insert(slot, storage.clone());
        storage
    }

    #[test]
    fn test_storable_accounts_by_slot() {
        for entries in 0..6 {
            let data = Vec::default();
            let hashes = (0..entries)
                .map(|_| AccountHash(Hash::new_unique()))
                .collect::<Vec<_>>();
            let mut raw = Vec::new();
            let mut raw2 = Vec::new();
            for entry in 0..entries {
                let pk = Pubkey::from([entry; 32]);
                let account = AccountSharedData::create(
                    entry as u64,
                    Vec::default(),
                    Pubkey::default(),
                    false,
                    0,
                );
                raw.push((
                    pk,
                    account.clone(),
                    StoredMeta {
                        write_version_obsolete: 500 + (entry * 3) as u64, // just something
                        pubkey: pk,
                        data_len: (entry * 2) as u64, // just something
                    },
                    AccountMeta {
                        lamports: account.lamports(),
                        owner: *account.owner(),
                        executable: account.executable(),
                        rent_epoch: account.rent_epoch(),
                    },
                ));
            }

            for entry in 0..entries {
                let offset = 99 * std::mem::size_of::<u64>(); // offset needs to be 8 byte aligned
                let stored_size = 101;
                raw2.push(StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
                    meta: &raw[entry as usize].2,
                    account_meta: &raw[entry as usize].3,
                    data: &data,
                    offset,
                    stored_size,
                    hash: &hashes[entry as usize],
                }));
            }

            let raw2_account_from_storage = raw2
                .iter()
                .map(|account| AccountFromStorage::new(account))
                .collect::<Vec<_>>();
            let raw2_refs = raw2.iter().collect::<Vec<_>>();

            // enumerate through permutations of # entries (ie. accounts) in each slot. Each one is 0..=entries.
            for entries0 in 0..=entries {
                let remaining1 = entries.saturating_sub(entries0);
                for entries1 in 0..=remaining1 {
                    let remaining2 = entries.saturating_sub(entries0 + entries1);
                    for entries2 in 0..=remaining2 {
                        let db = AccountsDb::new_single_for_tests();
                        let remaining3 = entries.saturating_sub(entries0 + entries1 + entries2);
                        let entries_by_level = [entries0, entries1, entries2, remaining3];
                        let mut overall_index = 0;
                        let mut expected_slots = Vec::default();
                        let slots_and_accounts_byval = entries_by_level
                            .iter()
                            .enumerate()
                            .filter_map(|(slot, count)| {
                                let slot = slot as Slot;
                                let count = *count as usize;
                                (overall_index < raw2.len()).then(|| {
                                    let range = overall_index..(overall_index + count);
                                    let mut result =
                                        raw2_account_from_storage[range.clone()].to_vec();
                                    // since we're no longer storing `StoredAccountMeta`, we have to actually store the
                                    // accounts so they can be looked up later in `db`
                                    let storage = setup_sample_storage(&db, slot);
                                    if let Some(offsets) = storage
                                        .accounts
                                        .append_accounts(&(slot, &raw2_refs[range.clone()]), 0)
                                    {
                                        result.iter_mut().zip(offsets.offsets.iter()).for_each(
                                            |(account, offset)| {
                                                account.index_info = AccountInfo::new(
                                                    StorageLocation::AppendVec(0, *offset),
                                                    if account.is_zero_lamport() { 0 } else { 1 },
                                                )
                                            },
                                        );
                                    }

                                    range.for_each(|_| expected_slots.push(slot));
                                    overall_index += count;
                                    (slot, result)
                                })
                            })
                            .collect::<Vec<_>>();
                        let slots_and_accounts_ref1 = slots_and_accounts_byval
                            .iter()
                            .map(|(slot, accounts)| (*slot, accounts.iter().collect::<Vec<_>>()))
                            .collect::<Vec<_>>();
                        let slots_and_accounts = slots_and_accounts_ref1
                            .iter()
                            .map(|(slot, accounts)| (*slot, &accounts[..]))
                            .collect::<Vec<_>>();

                        let storable =
                            StorableAccountsBySlot::new(99, &slots_and_accounts[..], &db);
                        assert_eq!(99, storable.target_slot());
                        assert_eq!(entries0 != entries, storable.contains_multiple_slots());
                        (0..entries).for_each(|index| {
                            let index = index as usize;
                            let mut called = false;
                            storable.account(index, |account| {
                                called = true;
                                assert!(accounts_equal(&account, &raw2[index]));
                                assert_eq!(account.pubkey(), raw2[index].pubkey());
                            });
                            assert!(called);
                            assert_eq!(storable.slot(index), expected_slots[index]);
                        })
                    }
                }
            }
        }
    }
}
