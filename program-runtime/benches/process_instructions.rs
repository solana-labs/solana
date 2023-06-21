#![feature(test)]

extern crate test;

use {
    solana_program_runtime::compute_budget::ComputeBudget,
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        message::Message,
        signature::Signer,
        signer::keypair::Keypair,
        transaction::{SanitizedTransaction, Transaction},
    },
    test::Bencher,
};

fn process_instructions_n_times(
    n: usize,
    tx: &SanitizedTransaction,
    default_units_per_instruction: bool,
    support_request_units_deprecated: bool,
    enable_request_heap_frame_ix: bool,
    support_set_loaded_accounts_data_size_limit_ix: bool,
    round_compute_unit_price: bool,
) {
        for _ in 0..n {
            let mut compute_budget = ComputeBudget::default();
            let _ = compute_budget.process_instructions(
                tx.message().program_instructions_iter(),
                default_units_per_instruction,
                support_request_units_deprecated,
                enable_request_heap_frame_ix,
                support_set_loaded_accounts_data_size_limit_ix,
                round_compute_unit_price,
            );
        }
}

#[bench]
fn bench_set_compute_unit_price_and_round(bencher: &mut Bencher) {
    let payer_keypair = Keypair::new();
    let tx_set_compute_unit_price = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
        &[&payer_keypair],
        Message::new(
            &[ComputeBudgetInstruction::set_compute_unit_price(5_432)],
            Some(&payer_keypair.pubkey()),
        ),
        Hash::default(),
    ));
    bencher.iter(|| {
        process_instructions_n_times(5_000, 
                &tx_set_compute_unit_price,
                true, // default_units_per_instruction
                true, // support_request_units_deprecated
                true, // enable_request_heap_frame_ix
                true, // support_set_loaded_accounts_data_size_limit_ix
                true, // round_compute_unit_price
            );
    });
}

#[bench]
fn bench_set_compute_unit_price_not_round(bencher: &mut Bencher) {
    let payer_keypair = Keypair::new();
    let tx_set_compute_unit_price = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
        &[&payer_keypair],
        Message::new(
            &[ComputeBudgetInstruction::set_compute_unit_price(5_432)],
            Some(&payer_keypair.pubkey()),
        ),
        Hash::default(),
    ));
    bencher.iter(|| {
        process_instructions_n_times(5_000, 
                &tx_set_compute_unit_price,
                true, // default_units_per_instruction
                true, // support_request_units_deprecated
                true, // enable_request_heap_frame_ix
                true, // support_set_loaded_accounts_data_size_limit_ix
                false, // round_compute_unit_price
            );
    });
}
