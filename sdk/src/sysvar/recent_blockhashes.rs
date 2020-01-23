use crate::{
    account::Account,
    hash::{hash, Hash},
    sysvar::Sysvar,
};
use bincode::serialize;
use std::collections::BinaryHeap;
use std::iter::FromIterator;
use std::ops::Deref;

const MAX_ENTRIES: usize = 32;

crate::declare_sysvar_id!(
    "SysvarRecentB1ockHashes11111111111111111111",
    RecentBlockhashes
);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RecentBlockhashes(Vec<Hash>);

impl Default for RecentBlockhashes {
    fn default() -> Self {
        Self(Vec::with_capacity(MAX_ENTRIES))
    }
}

impl<'a> FromIterator<&'a Hash> for RecentBlockhashes {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'a Hash>,
    {
        let mut new = Self::default();
        for i in iter {
            new.0.push(*i)
        }
        new
    }
}

// This is cherry-picked from HEAD of rust-lang's master (ref1) because it's
// a nightly-only experimental API.
// (binary_heap_into_iter_sorted [rustc issue #59278])
// Remove this and use the standard API once BinaryHeap::into_iter_sorted (ref2)
// is stabilized.
// ref1: https://github.com/rust-lang/rust/blob/2f688ac602d50129388bb2a5519942049096cbff/src/liballoc/collections/binary_heap.rs#L1149
// ref2: https://doc.rust-lang.org/std/collections/struct.BinaryHeap.html#into_iter_sorted.v

#[derive(Clone, Debug)]
pub struct IntoIterSorted<T> {
    inner: BinaryHeap<T>,
}

impl<T: Ord> Iterator for IntoIterSorted<T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.inner.pop()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = self.inner.len();
        (exact, Some(exact))
    }
}

impl Sysvar for RecentBlockhashes {
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        1032 // golden, update if MAX_ENTRIES changes
    }
}

impl Deref for RecentBlockhashes {
    type Target = Vec<Hash>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn create_account(lamports: u64) -> Account {
    RecentBlockhashes::default().create_account(lamports)
}

pub fn update_account<'a, I>(account: &mut Account, recent_blockhash_iter: I) -> Option<()>
where
    I: IntoIterator<Item = (u64, &'a Hash)>,
{
    let sorted = BinaryHeap::from_iter(recent_blockhash_iter);
    let sorted_iter = IntoIterSorted { inner: sorted };
    let recent_blockhash_iter = sorted_iter.take(MAX_ENTRIES).map(|(_, hash)| hash);
    let recent_blockhashes = RecentBlockhashes::from_iter(recent_blockhash_iter);
    recent_blockhashes.to_account(account)
}

pub fn create_account_with_data<'a, I>(lamports: u64, recent_blockhash_iter: I) -> Account
where
    I: IntoIterator<Item = (u64, &'a Hash)>,
{
    let mut account = create_account(lamports);
    update_account(&mut account, recent_blockhash_iter).unwrap();
    account
}

pub fn create_test_recent_blockhashes(start: usize) -> RecentBlockhashes {
    let bhq: Vec<_> = (start..start + (MAX_ENTRIES - 1))
        .map(|i| hash(&serialize(&i).unwrap()))
        .collect();
    RecentBlockhashes::from_iter(bhq.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HASH_BYTES;
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    #[test]
    fn test_size_of() {
        assert_eq!(
            bincode::serialized_size(&RecentBlockhashes(vec![Hash::default(); MAX_ENTRIES]))
                .unwrap() as usize,
            RecentBlockhashes::size_of()
        );
    }

    #[test]
    fn test_create_account_empty() {
        let account = create_account_with_data(42, vec![].into_iter());
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes, RecentBlockhashes::default());
    }

    #[test]
    fn test_create_account_full() {
        let def_hash = Hash::default();
        let account =
            create_account_with_data(42, vec![(0u64, &def_hash); MAX_ENTRIES].into_iter());
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_truncate() {
        let def_hash = Hash::default();
        let account =
            create_account_with_data(42, vec![(0u64, &def_hash); MAX_ENTRIES + 1].into_iter());
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_unsorted() {
        let mut unsorted_recent_blockhashes: Vec<_> = (0..MAX_ENTRIES)
            .map(|i| {
                (i as u64, {
                    // create hash with visibly recognizable ordering
                    let mut h = [0; HASH_BYTES];
                    h[HASH_BYTES - 1] = i as u8;
                    Hash::new(&h)
                })
            })
            .collect();
        unsorted_recent_blockhashes.shuffle(&mut thread_rng());

        let account = create_account_with_data(
            42,
            unsorted_recent_blockhashes
                .iter()
                .map(|(i, hash)| (*i, hash)),
        );
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();

        let mut expected_recent_blockhashes: Vec<_> =
            (unsorted_recent_blockhashes.into_iter().map(|(_, b)| b)).collect();
        expected_recent_blockhashes.sort();
        expected_recent_blockhashes.reverse();

        assert_eq!(*recent_blockhashes, expected_recent_blockhashes);
    }
}
