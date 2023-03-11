//! Signature verification for [`Entry`]es.
//!
//! TODO Add support for GPU.
//!
//! See `Entry::start_verify_transactions_gpu`.

use {
    super::{TodoItem, TodoOp},
    rayon::{prelude::*, ThreadPool},
    solana_sdk::{clock::Slot, transaction::VersionedTransaction},
    std::ops::Not,
};

/// Runs signature verification for all items from the `todo` list, returning a vector of slots that
/// failed the verification.
///
/// Returned value may contain duplicates, if slot failed verification base on multiple items,
/// referencing the same slot.
pub(super) fn verify(pool: &ThreadPool, todo: &[TodoItem]) -> Vec<Slot> {
    fn transaction_valid(tx: &VersionedTransaction) -> bool {
        let valid = tx.verify_with_results().iter().all(|valid| *valid);
        if !valid {
            warn!("Signature verification failed for tx: {tx:?}");
        }
        valid
    }

    pool.install(|| {
        todo.par_iter()
            .filter_map(|TodoItem { slot, op }| match op {
                TodoOp::All { entries, .. } | TodoOp::AllButPohOnFirstEntry { entries } => entries
                    .par_iter()
                    .map(|entry| entry.transactions.iter())
                    .flatten_iter()
                    .all(transaction_valid)
                    .not()
                    .then_some(slot),
                TodoOp::OnlyFirstEntryPoh { .. } => None,
            })
            .copied()
            .collect()
    })
}
