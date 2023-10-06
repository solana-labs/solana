//! Information about recent blocks and their fee calculators.
//!
//! The _recent blockhashes sysvar_ provides access to the [`RecentBlockhashes`],
//! which contains recent blockhahes and their [`FeeCalculator`]s.
//!
//! [`RecentBlockhashes`] does not implement [`Sysvar::get`].
//!
//! This sysvar is deprecated and should not be used. Transaction fees should be
//! determined with the [`getFeeForMessage`] RPC method. For additional context
//! see the [Comprehensive Compute Fees proposal][ccf].
//!
//! [`getFeeForMessage`]: https://docs.solana.com/developing/clients/jsonrpc-api#getfeeformessage
//! [ccf]: https://docs.solana.com/proposals/comprehensive-compute-fees
//!
//! See also the Solana [documentation on the recent blockhashes sysvar][sdoc].
//!
//! [sdoc]: https://docs.solana.com/developing/runtime-facilities/sysvars#recentblockhashes

#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]
use {
    crate::{
        declare_deprecated_sysvar_id, fee_calculator::FeeCalculator, hash::Hash, sysvar::Sysvar,
    },
    std::{cmp::Ordering, collections::BinaryHeap, iter::FromIterator, ops::Deref},
};

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
pub const MAX_ENTRIES: usize = 150;

declare_deprecated_sysvar_id!(
    "SysvarRecentB1ockHashes11111111111111111111",
    RecentBlockhashes
);

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub blockhash: Hash,
    pub fee_calculator: FeeCalculator,
}
impl Entry {
    pub fn new(blockhash: &Hash, lamports_per_signature: u64) -> Self {
        Self {
            blockhash: *blockhash,
            fee_calculator: FeeCalculator::new(lamports_per_signature),
        }
    }
}

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[derive(Clone, Debug)]
pub struct IterItem<'a>(pub u64, pub &'a Hash, pub u64);

impl<'a> Eq for IterItem<'a> {}

impl<'a> PartialEq for IterItem<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<'a> Ord for IterItem<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<'a> PartialOrd for IterItem<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Contains recent block hashes and fee calculators.
///
/// The entries are ordered by descending block height, so the first entry holds
/// the most recent block hash, and the last entry holds an old block hash.
#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RecentBlockhashes(Vec<Entry>);

impl Default for RecentBlockhashes {
    fn default() -> Self {
        Self(Vec::with_capacity(MAX_ENTRIES))
    }
}

impl<'a> FromIterator<IterItem<'a>> for RecentBlockhashes {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = IterItem<'a>>,
    {
        let mut new = Self::default();
        for i in iter {
            new.0.push(Entry::new(i.1, i.2))
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
impl<T> IntoIterSorted<T> {
    pub fn new(binary_heap: BinaryHeap<T>) -> Self {
        Self { inner: binary_heap }
    }
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
        6008 // golden, update if MAX_ENTRIES changes
    }
}

impl Deref for RecentBlockhashes {
    type Target = Vec<Entry>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::clock::MAX_PROCESSING_AGE};

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_sysvar_can_hold_all_active_blockhashes() {
        // Ensure we can still hold all of the active entries in `BlockhashQueue`
        assert!(MAX_PROCESSING_AGE <= MAX_ENTRIES);
    }

    #[test]
    fn test_size_of() {
        let entry = Entry::new(&Hash::default(), 0);
        assert_eq!(
            bincode::serialized_size(&RecentBlockhashes(vec![entry; MAX_ENTRIES])).unwrap()
                as usize,
            RecentBlockhashes::size_of()
        );
    }
}
