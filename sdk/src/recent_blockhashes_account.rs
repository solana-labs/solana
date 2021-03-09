use crate::account::{create_account_shared_data, to_account, AccountSharedData};
use solana_program::sysvar::recent_blockhashes::{
    IntoIterSorted, IterItem, RecentBlockhashes, MAX_ENTRIES,
};
use std::{collections::BinaryHeap, iter::FromIterator};

pub fn update_account<'a, I>(
    account: &mut AccountSharedData,
    recent_blockhash_iter: I,
) -> Option<()>
where
    I: IntoIterator<Item = IterItem<'a>>,
{
    let sorted = BinaryHeap::from_iter(recent_blockhash_iter);
    let sorted_iter = IntoIterSorted::new(sorted);
    let recent_blockhash_iter = sorted_iter.take(MAX_ENTRIES);
    let recent_blockhashes: RecentBlockhashes = recent_blockhash_iter.collect();
    to_account(&recent_blockhashes, account)
}

pub fn create_account_with_data<'a, I>(lamports: u64, recent_blockhash_iter: I) -> AccountSharedData
where
    I: IntoIterator<Item = IterItem<'a>>,
{
    let mut account =
        create_account_shared_data::<RecentBlockhashes>(&RecentBlockhashes::default(), lamports);
    update_account(&mut account, recent_blockhash_iter).unwrap();
    account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::from_account;
    use rand::{seq::SliceRandom, thread_rng};
    use solana_program::{
        fee_calculator::FeeCalculator,
        hash::{Hash, HASH_BYTES},
        sysvar::recent_blockhashes::Entry,
    };

    #[test]
    fn test_create_account_empty() {
        let account = create_account_with_data(42, vec![].into_iter());
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();
        assert_eq!(recent_blockhashes, RecentBlockhashes::default());
    }

    #[test]
    fn test_create_account_full() {
        let def_hash = Hash::default();
        let def_fees = FeeCalculator::default();
        let account = create_account_with_data(
            42,
            vec![IterItem(0u64, &def_hash, &def_fees); MAX_ENTRIES].into_iter(),
        );
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_truncate() {
        let def_hash = Hash::default();
        let def_fees = FeeCalculator::default();
        let account = create_account_with_data(
            42,
            vec![IterItem(0u64, &def_hash, &def_fees); MAX_ENTRIES + 1].into_iter(),
        );
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_unsorted() {
        let def_fees = FeeCalculator::default();
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

        let account = create_account_with_data(
            42,
            unsorted_blocks
                .iter()
                .map(|(i, hash)| IterItem(*i, hash, &def_fees)),
        );
        let recent_blockhashes = from_account::<RecentBlockhashes, _>(&account).unwrap();

        let mut unsorted_recent_blockhashes: Vec<_> = unsorted_blocks
            .iter()
            .map(|(i, hash)| IterItem(*i, hash, &def_fees))
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
