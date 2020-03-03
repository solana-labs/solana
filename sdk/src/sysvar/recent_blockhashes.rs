use crate::{
    account::Account,
    declare_sysvar_id,
    hash::{hash, Hash},
    program_error::ProgramError,
    sysvar::Sysvar,
};
use std::{collections::BinaryHeap, iter::FromIterator, ops::Deref};

const MAX_ENTRIES: usize = 32;

declare_sysvar_id!(
    "SysvarRecentB1ockHashes11111111111111111111",
    RecentBlockhashes
);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Versions {
    Current(Box<RecentBlockhashes>),
}

pub mod unversioned {
    use super::*;

    declare_sysvar_id!(
        "SysvarRecentB1ockHashes11111111111111111111",
        RecentBlockhashes
    );

    #[repr(C)]
    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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

    impl Deref for RecentBlockhashes {
        type Target = Vec<Hash>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl Sysvar for RecentBlockhashes {
        fn size_of() -> usize {
            // hard-coded so that we don't have to construct an empty
            1032 // golden, update if MAX_ENTRIES changes
        }
    }

    pub fn create_account(lamports: u64) -> Account {
        RecentBlockhashes::default().create_account(lamports)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_size_of() {
            assert_eq!(
                bincode::serialized_size(&RecentBlockhashes(vec![Hash::default(); MAX_ENTRIES]))
                    .unwrap() as usize,
                RecentBlockhashes::size_of()
            );
        }
    }
}

#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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

#[derive(Debug, PartialEq)]
enum Versioning {
    Versioned(Versions),
    Unversioned(unversioned::RecentBlockhashes),
}

impl From<unversioned::RecentBlockhashes> for RecentBlockhashes {
    fn from(other: unversioned::RecentBlockhashes) -> Self {
        RecentBlockhashes::from_iter(other.iter())
    }
}

fn resolve_versioning(data: &[u8]) -> bincode::Result<Versioning> {
    if data.len() == unversioned::RecentBlockhashes::size_of() {
        bincode::deserialize::<unversioned::RecentBlockhashes>(&data).map(Versioning::Unversioned)
    } else {
        bincode::deserialize::<Versions>(data).map(Versioning::Versioned)
    }
}

impl Sysvar for RecentBlockhashes {
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        1036 // golden, update if MAX_ENTRIES changes
    }

    fn serialize_into(&self, data: &mut [u8]) -> Option<()> {
        match resolve_versioning(data).ok()? {
            Versioning::Versioned(_) => {
                let versioned = Versions::Current(Box::new(self.clone()));
                bincode::serialize_into(data, &versioned).ok()
            }
            Versioning::Unversioned(_) => bincode::serialize_into(data, self).ok(),
        }
    }

    fn deserialize(data: &[u8]) -> Result<Self, ProgramError> {
        let versioned = match resolve_versioning(data).map_err(|_| ProgramError::InvalidArgument)? {
            Versioning::Versioned(versioned) => versioned,
            Versioning::Unversioned(unversioned) => Versions::Current(Box::new(unversioned.into())),
        };
        match versioned {
            Versions::Current(recent_blockhashes) => Ok(*recent_blockhashes),
        }
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
    let bhq: Vec<_> = (start..start + MAX_ENTRIES)
        .map(|i| hash(&bincode::serialize(&i).unwrap()))
        .collect();
    RecentBlockhashes::from_iter(bhq.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{account_utils::StateMut, hash::HASH_BYTES, pubkey::Pubkey};
    use assert_matches::assert_matches;
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    #[test]
    fn test_size_of() {
        assert_eq!(
            bincode::serialized_size(&Versions::Current(Box::new(RecentBlockhashes(
                vec![Hash::default(); MAX_ENTRIES]
            ))))
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

    #[test]
    fn test_resolve_versioning() {
        let account =
            create_account_with_data(42, vec![(0u64, &Hash::default()); MAX_ENTRIES].into_iter());
        assert_matches!(
            resolve_versioning(&account.data),
            Ok(Versioning::Versioned(_))
        );
        let unversioned_acc = unversioned::create_account(1);
        assert_matches!(
            resolve_versioning(&unversioned_acc.data),
            Ok(Versioning::Unversioned(_))
        );
        let too_small_acc = Account::new(1, 0, &Pubkey::new(&[2u8; 32]));
        assert!(resolve_versioning(&too_small_acc.data).is_err());
    }

    #[test]
    fn test_store_versioned_data_in_unversioned_account() {
        let mut unversioned_acc = unversioned::create_account(1);
        let modern_data = create_test_recent_blockhashes(0);
        // Manual write of versioned data fails
        let versioned = Versions::Current(Box::new(modern_data.clone()));
        assert!(unversioned_acc.set_state(&versioned).is_err());
        assert!(modern_data.to_account(&mut unversioned_acc).is_some());
    }
}
