#![feature(test)]
extern crate test;

use {
    rand::{thread_rng, Rng},
    solana_runtime::prioritization_fee_cache::*,
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        message::Message,
        pubkey::Pubkey,
        system_instruction,
        transaction::{SanitizedTransaction, Transaction},
    },
    std::sync::Arc,
    test::Bencher,
};
const TRANSFER_TRANSACTION_COMPUTE_UNIT: u32 = 200;

fn build_sanitized_transaction(
    compute_unit_price: u64,
    signer_account: &Pubkey,
    write_account: &Pubkey,
) -> SanitizedTransaction {
    let transfer_lamports = 1;
    let transaction = Transaction::new_unsigned(Message::new(
        &[
            system_instruction::transfer(signer_account, write_account, transfer_lamports),
            ComputeBudgetInstruction::set_compute_unit_limit(TRANSFER_TRANSACTION_COMPUTE_UNIT),
            ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
        ],
        Some(signer_account),
    ));

    SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap()
}

#[bench]
fn bench_process_transactions_single_slot(bencher: &mut Bencher) {
    let prioritization_fee_cache = PrioritizationFeeCache::default();

    let slot = 101;
    // build test transactions
    let transactions: Vec<_> = (0..5000)
        .map(|n| {
            let compute_unit_price = n % 7;
            build_sanitized_transaction(
                compute_unit_price,
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
            )
        })
        .collect();

    bencher.iter(|| {
        prioritization_fee_cache.update_transactions(slot, transactions.iter());
    });
}

fn process_transactions_multiple_slots() {
    const NUM_SLOTS: u64 = 5;
    const NUM_THREADS: usize = 3;

    let prioritization_fee_cache = Arc::new(PrioritizationFeeCache::default());

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(NUM_THREADS)
        .build()
        .unwrap();

    // each threads updates a slot a batch of 50 transactions, for 100 times
    for _ in 0..100 {
        pool.install(|| {
            let transactions: Vec<_> = (0..50)
                .map(|n| {
                    let compute_unit_price = n % 7;
                    build_sanitized_transaction(
                        compute_unit_price,
                        &Pubkey::new_unique(),
                        &Pubkey::new_unique(),
                    )
                })
                .collect();

            let slot = thread_rng().gen_range(0, NUM_SLOTS);

            prioritization_fee_cache.update_transactions(slot, transactions.iter());
        })
    }
}

#[bench]
fn bench_process_transactions_multiple_slots(bencher: &mut Bencher) {
    bencher.iter(|| {
        process_transactions_multiple_slots();
    });
}
