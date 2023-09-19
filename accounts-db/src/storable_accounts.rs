//! trait for abstracting underlying storage of pubkey and account pairs to be written
use {
    crate::{account_storage::meta::StoredAccountMeta, accounts_db::IncludeSlotInHash},
    solana_sdk::{account::ReadableAccount, clock::Slot, hash::Hash, pubkey::Pubkey},
};

/// abstract access to pubkey, account, slot, target_slot of either:
/// a. (slot, &[&Pubkey, &ReadableAccount])
/// b. (slot, &[&Pubkey, &ReadableAccount, Slot]) (we will use this later)
/// This trait avoids having to allocate redundant data when there is a duplicated slot parameter.
/// All legacy callers do not have a unique slot per account to store.
pub trait StorableAccounts<'a, T: ReadableAccount + Sync>: Sync {
    /// pubkey at 'index'
    fn pubkey(&self, index: usize) -> &Pubkey;
    /// account at 'index'
    fn account(&self, index: usize) -> &T;
    /// None if account is zero lamports
    fn account_default_if_zero_lamport(&self, index: usize) -> Option<&T> {
        let account = self.account(index);
        (account.lamports() != 0).then_some(account)
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
    /// true iff hashing these accounts should include the slot
    fn include_slot_in_hash(&self) -> IncludeSlotInHash;

    /// true iff the impl can provide hash and write_version
    /// Otherwise, hash and write_version have to be provided separately to store functions.
    fn has_hash_and_write_version(&self) -> bool {
        false
    }

    /// return hash for account at 'index'
    /// Should only be called if 'has_hash_and_write_version' = true
    fn hash(&self, _index: usize) -> &Hash {
        // this should never be called if has_hash_and_write_version returns false
        unimplemented!();
    }

    /// return write_version for account at 'index'
    /// Should only be called if 'has_hash_and_write_version' = true
    fn write_version(&self, _index: usize) -> u64 {
        // this should never be called if has_hash_and_write_version returns false
        unimplemented!();
    }
}

/// accounts that are moving from 'old_slot' to 'target_slot'
/// since all accounts are from the same old slot, we don't need to create a slice with per-account slot
/// but, we need slot(_) to return 'old_slot' for all accounts
/// Created a struct instead of a tuple to make the code easier to read.
pub struct StorableAccountsMovingSlots<'a, T: ReadableAccount + Sync> {
    pub accounts: &'a [(&'a Pubkey, &'a T)],
    /// accounts will be written to this slot
    pub target_slot: Slot,
    /// slot where accounts are currently stored
    pub old_slot: Slot,
    /// This is temporarily here until feature activation.
    pub include_slot_in_hash: IncludeSlotInHash,
}

impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T> for StorableAccountsMovingSlots<'a, T> {
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.accounts[index].0
    }
    fn account(&self, index: usize) -> &T {
        self.accounts[index].1
    }
    fn slot(&self, _index: usize) -> Slot {
        // per-index slot is not unique per slot, but it is different than 'target_slot'
        self.old_slot
    }
    fn target_slot(&self) -> Slot {
        self.target_slot
    }
    fn len(&self) -> usize {
        self.accounts.len()
    }
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.include_slot_in_hash
    }
}

/// The last parameter exists until this feature is activated:
///  ignore slot when calculating an account hash #28420
impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T>
    for (Slot, &'a [(&'a Pubkey, &'a T)], IncludeSlotInHash)
{
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.1[index].0
    }
    fn account(&self, index: usize) -> &T {
        self.1[index].1
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
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.2
    }
}

#[allow(dead_code)]
/// The last parameter exists until this feature is activated:
///  ignore slot when calculating an account hash #28420
impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T>
    for (Slot, &'a [&'a (Pubkey, T)], IncludeSlotInHash)
{
    fn pubkey(&self, index: usize) -> &Pubkey {
        &self.1[index].0
    }
    fn account(&self, index: usize) -> &T {
        &self.1[index].1
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
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.2
    }
}

/// The last parameter exists until this feature is activated:
///  ignore slot when calculating an account hash #28420
impl<'a> StorableAccounts<'a, StoredAccountMeta<'a>>
    for (Slot, &'a [&'a StoredAccountMeta<'a>], IncludeSlotInHash)
{
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.account(index).pubkey()
    }
    fn account(&self, index: usize) -> &StoredAccountMeta<'a> {
        self.1[index]
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
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.2
    }
    fn has_hash_and_write_version(&self) -> bool {
        true
    }
    fn hash(&self, index: usize) -> &Hash {
        self.account(index).hash()
    }
    fn write_version(&self, index: usize) -> u64 {
        self.account(index).write_version()
    }
}

/// holds slices of accounts being moved FROM a common source slot to 'target_slot'
pub struct StorableAccountsBySlot<'a> {
    target_slot: Slot,
    /// each element is (source slot, accounts moving FROM source slot)
    slots_and_accounts: &'a [(Slot, &'a [&'a StoredAccountMeta<'a>])],
    include_slot_in_hash: IncludeSlotInHash,

    /// This is calculated based off slots_and_accounts.
    /// cumulative offset of all account slices prior to this one
    /// starting_offsets[0] is the starting offset of slots_and_accounts[1]
    /// The starting offset of slots_and_accounts[0] is always 0
    starting_offsets: Vec<usize>,
    /// true if there is more than 1 slot represented in slots_and_accounts
    contains_multiple_slots: bool,
    /// total len of all accounts, across all slots_and_accounts
    len: usize,
}

impl<'a> StorableAccountsBySlot<'a> {
    #[allow(dead_code)]
    /// each element of slots_and_accounts is (source slot, accounts moving FROM source slot)
    pub fn new(
        target_slot: Slot,
        slots_and_accounts: &'a [(Slot, &'a [&'a StoredAccountMeta<'a>])],
        include_slot_in_hash: IncludeSlotInHash,
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
            include_slot_in_hash,
            contains_multiple_slots,
            len: cumulative_len,
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

/// The last parameter exists until this feature is activated:
///  ignore slot when calculating an account hash #28420
impl<'a> StorableAccounts<'a, StoredAccountMeta<'a>> for StorableAccountsBySlot<'a> {
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.account(index).pubkey()
    }
    fn account(&self, index: usize) -> &StoredAccountMeta<'a> {
        let indexes = self.find_internal_index(index);
        self.slots_and_accounts[indexes.0].1[indexes.1]
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
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.include_slot_in_hash
    }
    fn has_hash_and_write_version(&self) -> bool {
        true
    }
    fn hash(&self, index: usize) -> &Hash {
        self.account(index).hash()
    }
    fn write_version(&self, index: usize) -> u64 {
        self.account(index).write_version()
    }
}

/// this tuple contains a single different source slot that applies to all accounts
/// accounts are StoredAccountMeta
impl<'a> StorableAccounts<'a, StoredAccountMeta<'a>>
    for (
        Slot,
        &'a [&'a StoredAccountMeta<'a>],
        IncludeSlotInHash,
        Slot,
    )
{
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.account(index).pubkey()
    }
    fn account(&self, index: usize) -> &StoredAccountMeta<'a> {
        self.1[index]
    }
    fn slot(&self, _index: usize) -> Slot {
        // same other slot for all accounts
        self.3
    }
    fn target_slot(&self) -> Slot {
        self.0
    }
    fn len(&self) -> usize {
        self.1.len()
    }
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.2
    }
    fn has_hash_and_write_version(&self) -> bool {
        true
    }
    fn hash(&self, index: usize) -> &Hash {
        self.account(index).hash()
    }
    fn write_version(&self, index: usize) -> u64 {
        self.account(index).write_version()
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            account_storage::meta::{AccountMeta, StoredAccountMeta, StoredMeta},
            accounts_db::INCLUDE_SLOT_IN_HASH_TESTS,
            append_vec::AppendVecStoredAccountMeta,
        },
        solana_sdk::{
            account::{accounts_equal, AccountSharedData, WritableAccount},
            hash::Hash,
        },
    };

    fn compare<
        'a,
        T: ReadableAccount + Sync + PartialEq + std::fmt::Debug,
        U: ReadableAccount + Sync + PartialEq + std::fmt::Debug,
    >(
        a: &impl StorableAccounts<'a, T>,
        b: &impl StorableAccounts<'a, U>,
    ) {
        assert_eq!(a.target_slot(), b.target_slot());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.is_empty(), b.is_empty());
        assert_eq!(a.include_slot_in_hash(), b.include_slot_in_hash());
        (0..a.len()).for_each(|i| {
            assert_eq!(a.pubkey(i), b.pubkey(i));
            assert!(accounts_equal(a.account(i), b.account(i)));
        })
    }

    #[test]
    fn test_contains_multiple_slots() {
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
        let offset = 99;
        let stored_size = 101;
        let hash = Hash::new_unique();
        let stored_account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });

        let test3 = (
            slot,
            &vec![&stored_account, &stored_account][..],
            INCLUDE_SLOT_IN_HASH_TESTS,
            slot,
        );
        assert!(!test3.contains_multiple_slots());
    }

    #[test]
    fn test_storable_accounts() {
        let max_slots = 3_u64;
        for target_slot in 0..max_slots {
            for entries in 0..2 {
                for starting_slot in 0..max_slots {
                    let data = Vec::default();
                    let hash = Hash::new_unique();
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
                        let offset = 99;
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

                    let mut two = Vec::new();
                    let mut three = Vec::new();
                    let mut four_pubkey_and_account_value = Vec::new();
                    raw.iter()
                        .zip(raw2.iter().zip(raw4.iter()))
                        .for_each(|(raw, (raw2, raw4))| {
                            two.push((&raw.0, &raw.1)); // 2 item tuple
                            three.push(raw2);
                            four_pubkey_and_account_value.push(raw4);
                        });
                    let test2 = (target_slot, &two[..], INCLUDE_SLOT_IN_HASH_TESTS);
                    let test4 = (
                        target_slot,
                        &four_pubkey_and_account_value[..],
                        INCLUDE_SLOT_IN_HASH_TESTS,
                    );

                    let source_slot = starting_slot % max_slots;
                    let test3 = (
                        target_slot,
                        &three[..],
                        INCLUDE_SLOT_IN_HASH_TESTS,
                        source_slot,
                    );
                    let old_slot = starting_slot;
                    let test_moving_slots = StorableAccountsMovingSlots {
                        accounts: &two[..],
                        target_slot,
                        old_slot,
                        include_slot_in_hash: INCLUDE_SLOT_IN_HASH_TESTS,
                    };
                    let for_slice = [(old_slot, &three[..])];
                    let test_moving_slots2 = StorableAccountsBySlot::new(
                        target_slot,
                        &for_slice,
                        INCLUDE_SLOT_IN_HASH_TESTS,
                    );
                    compare(&test2, &test3);
                    compare(&test2, &test4);
                    compare(&test2, &test_moving_slots);
                    compare(&test2, &test_moving_slots2);
                    for (i, raw) in raw.iter().enumerate() {
                        assert_eq!(raw.0, *test3.pubkey(i));
                        assert!(accounts_equal(&raw.1, test3.account(i)));
                        assert_eq!(raw.2, test3.slot(i));
                        assert_eq!(target_slot, test4.slot(i));
                        assert_eq!(target_slot, test2.slot(i));
                        assert_eq!(old_slot, test_moving_slots.slot(i));
                        assert_eq!(old_slot, test_moving_slots2.slot(i));
                    }
                    assert_eq!(target_slot, test3.target_slot());
                    assert_eq!(target_slot, test4.target_slot());
                    assert_eq!(target_slot, test_moving_slots2.target_slot());
                    assert!(!test2.contains_multiple_slots());
                    assert!(!test4.contains_multiple_slots());
                    assert!(!test_moving_slots.contains_multiple_slots());
                    assert_eq!(test3.contains_multiple_slots(), entries > 1);
                }
            }
        }
    }

    #[test]
    fn test_storable_accounts_by_slot() {
        solana_logger::setup();
        // slots 0..4
        // each one containing a subset of the overall # of entries (0..4)
        for entries in 0..6 {
            let data = Vec::default();
            let hashes = (0..entries).map(|_| Hash::new_unique()).collect::<Vec<_>>();
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
                let offset = 99;
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
            let raw2_refs = raw2.iter().collect::<Vec<_>>();

            // enumerate through permutations of # entries (ie. accounts) in each slot. Each one is 0..=entries.
            for entries0 in 0..=entries {
                let remaining1 = entries.saturating_sub(entries0);
                for entries1 in 0..=remaining1 {
                    let remaining2 = entries.saturating_sub(entries0 + entries1);
                    for entries2 in 0..=remaining2 {
                        let remaining3 = entries.saturating_sub(entries0 + entries1 + entries2);
                        let entries_by_level = [entries0, entries1, entries2, remaining3];
                        let mut overall_index = 0;
                        let mut expected_slots = Vec::default();
                        let slots_and_accounts = entries_by_level
                            .iter()
                            .enumerate()
                            .filter_map(|(slot, count)| {
                                let slot = slot as Slot;
                                let count = *count as usize;
                                (overall_index < raw2.len()).then(|| {
                                    let range = overall_index..(overall_index + count);
                                    let result = &raw2_refs[range.clone()];
                                    range.for_each(|_| expected_slots.push(slot));
                                    overall_index += count;
                                    (slot, result)
                                })
                            })
                            .collect::<Vec<_>>();
                        let storable = StorableAccountsBySlot::new(
                            99,
                            &slots_and_accounts[..],
                            INCLUDE_SLOT_IN_HASH_TESTS,
                        );
                        assert!(storable.has_hash_and_write_version());
                        assert_eq!(99, storable.target_slot());
                        assert_eq!(entries0 != entries, storable.contains_multiple_slots());
                        (0..entries).for_each(|index| {
                            let index = index as usize;
                            assert_eq!(storable.account(index), &raw2[index]);
                            assert_eq!(storable.pubkey(index), raw2[index].pubkey());
                            assert_eq!(storable.hash(index), raw2[index].hash());
                            assert_eq!(storable.slot(index), expected_slots[index]);
                            assert_eq!(storable.write_version(index), raw2[index].write_version());
                        })
                    }
                }
            }
        }
    }
}
