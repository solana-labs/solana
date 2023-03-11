//! PoH verification for [`Entry`]es.
//!
//! TODO Add support SIMD optimizations.
//!
//! See [`EntrySlice::verify_cpu_x86_simd()`].
//!
//! [`EntrySlice::verify_cpu_x86_simd()`]: solana_entry::entry::EntrySlice::verify_cpu_x86_simd()

use {
    super::{TodoItem, TodoOp},
    rayon::{iter::once, prelude::*, ThreadPool},
    solana_entry::entry::Entry,
    solana_sdk::{clock::Slot, hash::Hash},
    std::ops::Not,
};

/// Runs PoH verification for all items from the `todo` list, returning a vector of slots that
/// failed the verification.
///
/// Returned value may contain duplicates, if slot failed verification base on multiple items,
/// referencing the same slot.
pub(super) fn verify(pool: &ThreadPool, todo: &[TodoItem]) -> Vec<Slot> {
    fn poh_valid((entry, hash): (&Entry, &Hash)) -> bool {
        let valid = entry.verify(hash);
        if !valid {
            warn!(
                "Entry failed PoH: start hash: {:?}, entry hash: {:?} num txs: {}",
                hash,
                entry.hash,
                entry.transactions.len()
            );
        }
        valid
    }

    pool.install(|| {
        todo.par_iter()
            .filter_map(|TodoItem { slot, op }| match op {
                TodoOp::All {
                    poh_start_hash,
                    entries,
                } => {
                    let entry_hashes = entries.par_iter().map(|entry| &entry.hash);
                    let hashes = once(poh_start_hash).chain(entry_hashes);

                    entries
                        .par_iter()
                        .zip(hashes)
                        .all(poh_valid)
                        .not()
                        .then_some(slot)
                }
                TodoOp::AllButPohOnFirstEntry { entries } => {
                    let hashes = entries.par_iter().map(|entry| &entry.hash);

                    entries
                        .par_iter()
                        .skip(1)
                        .zip(hashes)
                        .all(poh_valid)
                        .not()
                        .then_some(slot)
                }
                TodoOp::OnlyFirstEntryPoh {
                    poh_start_hash,
                    entry,
                } => entry.verify(poh_start_hash).not().then_some(slot),
            })
            .copied()
            .collect()
    })
}
