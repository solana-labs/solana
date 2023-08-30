#![feature(test)]
extern crate test;
use {
    solana_entry::entry::{self, VerifyRecyclers},
    solana_perf::test_tx::test_tx,
    solana_sdk::{
        hash::Hash,
        transaction::{
            Result, SanitizedTransaction, SimpleAddressLoader, TransactionVerificationMode,
            VersionedTransaction,
        },
    },
    std::sync::Arc,
    test::Bencher,
};

#[bench]
fn bench_gpusigverify(bencher: &mut Bencher) {
    let entries = (0..131072)
        .map(|_| {
            let transaction = test_tx();
            entry::next_entry_mut(&mut Hash::default(), 0, vec![transaction])
        })
        .collect::<Vec<_>>();

    let verify_transaction = {
        move |versioned_tx: VersionedTransaction,
              verification_mode: TransactionVerificationMode|
              -> Result<SanitizedTransaction> {
            let sanitized_tx = {
                let message_hash =
                    if verification_mode == TransactionVerificationMode::FullVerification {
                        versioned_tx.verify_and_hash_message()?
                    } else {
                        versioned_tx.message.hash()
                    };

                SanitizedTransaction::try_create(
                    versioned_tx,
                    message_hash,
                    None,
                    SimpleAddressLoader::Disabled,
                )
            }?;

            Ok(sanitized_tx)
        }
    };

    let recycler = VerifyRecyclers::default();

    bencher.iter(|| {
        let res = entry::start_verify_transactions(
            entries.clone(),
            false,
            recycler.clone(),
            Arc::new(verify_transaction),
        );

        if let Ok(mut res) = res {
            let _ans = res.finish_verify();
        }
    })
}

#[bench]
fn bench_cpusigverify(bencher: &mut Bencher) {
    let entries = (0..131072)
        .map(|_| {
            let transaction = test_tx();
            entry::next_entry_mut(&mut Hash::default(), 0, vec![transaction])
        })
        .collect::<Vec<_>>();

    let verify_transaction = {
        move |versioned_tx: VersionedTransaction| -> Result<SanitizedTransaction> {
            let sanitized_tx = {
                let message_hash = versioned_tx.verify_and_hash_message()?;
                SanitizedTransaction::try_create(
                    versioned_tx,
                    message_hash,
                    None,
                    SimpleAddressLoader::Disabled,
                )
            }?;

            Ok(sanitized_tx)
        }
    };

    bencher.iter(|| {
        let _ans = entry::verify_transactions(entries.clone(), Arc::new(verify_transaction));
    })
}
