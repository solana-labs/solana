//! Helpers for the recent blockhashes sysvar.

#[allow(deprecated)]
use solana_program::sysvar::recent_blockhashes::{
    IntoIterSorted, IterItem, RecentBlockhashes, MAX_ENTRIES,
};
use {
    crate::{
        account::{
            create_account_shared_data_with_fields, to_account, AccountSharedData,
            InheritableAccountFields, DUMMY_INHERITABLE_ACCOUNT_FIELDS,
        },
        clock::INITIAL_RENT_EPOCH,
    },
    std::{collections::BinaryHeap, iter::FromIterator},
};

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[allow(deprecated)]
pub fn update_account<'a, I>(
    account: &mut AccountSharedData,
    recent_blockhash_iter: I,
) -> Option<()>
where
    I: IntoIterator<Item = IterItem<'a>>,
{
    let sorted = BinaryHeap::from_iter(recent_blockhash_iter);
    #[allow(deprecated)]
    let sorted_iter = IntoIterSorted::new(sorted);
    #[allow(deprecated)]
    let recent_blockhash_iter = sorted_iter.take(MAX_ENTRIES);
    #[allow(deprecated)]
    let recent_blockhashes: RecentBlockhashes = recent_blockhash_iter.collect();
    to_account(&recent_blockhashes, account)
}

#[deprecated(
    since = "1.5.17",
    note = "Please use `create_account_with_data_for_test` instead"
)]
#[allow(deprecated)]
pub fn create_account_with_data<'a, I>(lamports: u64, recent_blockhash_iter: I) -> AccountSharedData
where
    I: IntoIterator<Item = IterItem<'a>>,
{
    #[allow(deprecated)]
    create_account_with_data_and_fields(recent_blockhash_iter, (lamports, INITIAL_RENT_EPOCH))
}

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[allow(deprecated)]
pub fn create_account_with_data_and_fields<'a, I>(
    recent_blockhash_iter: I,
    fields: InheritableAccountFields,
) -> AccountSharedData
where
    I: IntoIterator<Item = IterItem<'a>>,
{
    let mut account = create_account_shared_data_with_fields::<RecentBlockhashes>(
        &RecentBlockhashes::default(),
        fields,
    );
    update_account(&mut account, recent_blockhash_iter).unwrap();
    account
}

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[allow(deprecated)]
pub fn create_account_with_data_for_test<'a, I>(recent_blockhash_iter: I) -> AccountSharedData
where
    I: IntoIterator<Item = IterItem<'a>>,
{
    create_account_with_data_and_fields(recent_blockhash_iter, DUMMY_INHERITABLE_ACCOUNT_FIELDS)
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]
    use {
        super::*,
        crate::account::from_account,
        rand::{seq::SliceRandom, thread_rng},
        solana_program::{
            hash::{Hash, HASH_BYTES},
            sysvar::recent_blockhashes::Entry,
        },
    };

    #[test]
    fn test_create_account_empty() {
        let account = create_account_with_data_for_test(vec![]);
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();
        assert_eq!(recent_blockhashes, RecentBlockhashes::default());
    }

    #[test]
    fn test_create_account_full() {
        let def_hash = Hash::default();
        let def_lamports_per_signature = 0;
        let account = create_account_with_data_for_test(vec![
            IterItem(
                0u64,
                &def_hash,
                def_lamports_per_signature
            );
            MAX_ENTRIES
        ]);
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_truncate() {
        let def_hash = Hash::default();
        let def_lamports_per_signature = 0;
        let account = create_account_with_data_for_test(vec![
            IterItem(
                0u64,
                &def_hash,
                def_lamports_per_signature
            );
            MAX_ENTRIES + 1
        ]);
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_unsorted() {
        let def_lamports_per_signature = 0;
        let mut unsorted_blocks: Vec<_> = (0..MAX_ENTRIES)
            .map(|i| {
                (i as u64, {
                    // create hash with visibly recognizable ordering
                    let mut h = [0; HASH_BYTES];
                    h[HASH_BYTES - 1] = i as u8;
                    Hash::new(&h)
                })
            })
            .collect();
        unsorted_blocks.shuffle(&mut thread_rng());

        let account = create_account_with_data_for_test(
            unsorted_blocks
                .iter()
                .map(|(i, hash)| IterItem(*i, hash, def_lamports_per_signature)),
        );
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();

        let mut unsorted_recent_blockhashes: Vec<_> = unsorted_blocks
            .iter()
            .map(|(i, hash)| IterItem(*i, hash, def_lamports_per_signature))
            .collect();
        unsorted_recent_blockhashes.sort();
        unsorted_recent_blockhashes.reverse();
        let expected_recent_blockhashes: Vec<_> = (unsorted_recent_blockhashes
            .into_iter()
            .map(|IterItem(_, b, f)| Entry::new(b, f)))
        .collect();

        assert_eq!(*recent_blockhashes, expected_recent_blockhashes);
    }
}
