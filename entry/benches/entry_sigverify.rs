#![feature(test)]
extern crate test;
use test::Bencher;

use std::sync::Arc;

use solana_perf::test_tx::test_tx;
use solana_sdk::transaction::{SanitizedTransaction, TransactionError, VersionedTransaction, Result};

use solana_entry::entry::{
    self, VerifyRecyclers,
};

use solana_sdk::hash::Hash;

#[bench]
fn bench_gpusigverify(bencher: &mut Bencher) {
    let verify_transaction = {
        move |versioned_tx: VersionedTransaction,
              skip_verification: bool,
              verify_precompiles: bool|
              -> Result<SanitizedTransaction> {
            let sanitized_tx = {
                let message_hash = if !skip_verification {
                    versioned_tx.verify_and_hash_message()?
                } else {
                    versioned_tx.message.hash()
                };

                SanitizedTransaction::try_create(versioned_tx, message_hash, |_| {
                    Err(TransactionError::UnsupportedVersion)
                })
            }?;

            Ok(sanitized_tx)
        }
    };

    let recycler = VerifyRecyclers::default();

    bencher.iter(|| {
        let entries = (0..128)
            .map(|_| {
                let transaction = test_tx();
                entry::next_entry_mut(&mut Hash::default(), 0, vec![transaction])
            })
            .collect::<Vec<_>>();

        let _ans = entry::start_verify_transactions(
            entries,
            false,
            recycler.clone(),
            Arc::new(verify_transaction),
        );
    })
}