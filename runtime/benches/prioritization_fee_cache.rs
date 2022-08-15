#![feature(test)]
extern crate test;

use {
    solana_runtime::prioritization_fee_cache::*,
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction, message::Message, pubkey::Pubkey,
        system_instruction, transaction::SanitizedTransaction, transaction::Transaction,
    },
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
    let mut prioritization_fee_cache = PrioritizationFeeCache::default();

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
