use {
    criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput},
    solana_runtime_transaction::instructions_processor::process_compute_budget_instructions,
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        instruction::Instruction,
        message::Message,
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        system_instruction::{self},
        transaction::{SanitizedTransaction, Transaction},
    },
    solana_svm_transaction::svm_message::SVMMessage,
};

const NUM_TRANSACTIONS_PER_ITER: usize = 1024;
const DUMMY_PROGRAM_ID: &str = "dummmy1111111111111111111111111111111111111";

fn build_sanitized_transaction(
    payer_keypair: &Keypair,
    instructions: &[Instruction],
) -> SanitizedTransaction {
    SanitizedTransaction::from_transaction_for_tests(Transaction::new_unsigned(Message::new(
        instructions,
        Some(&payer_keypair.pubkey()),
    )))
}

fn bench_process_compute_budget_instructions_empty(c: &mut Criterion) {
    c.benchmark_group("bench_process_compute_budget_instructions_empty")
        .throughput(Throughput::Elements(NUM_TRANSACTIONS_PER_ITER as u64))
        .bench_function("0 instructions", |bencher| {
            let tx = build_sanitized_transaction(&Keypair::new(), &[]);
            bencher.iter(|| {
                (0..NUM_TRANSACTIONS_PER_ITER).for_each(|_| {
                    assert!(process_compute_budget_instructions(black_box(
                        SVMMessage::program_instructions_iter(&tx)
                    ))
                    .is_ok())
                })
            });
        });
}

fn bench_process_compute_budget_instructions_no_builtins(c: &mut Criterion) {
    let num_instructions = 4;
    c.benchmark_group("bench_process_compute_budget_instructions_no_builtins")
        .throughput(Throughput::Elements(NUM_TRANSACTIONS_PER_ITER as u64))
        .bench_function(
            format!("{num_instructions} dummy Instructions"),
            |bencher| {
                let ixs: Vec<_> = (0..num_instructions)
                    .map(|_| {
                        Instruction::new_with_bincode(
                            DUMMY_PROGRAM_ID.parse().unwrap(),
                            &(),
                            vec![],
                        )
                    })
                    .collect();
                let tx = build_sanitized_transaction(&Keypair::new(), &ixs);
                bencher.iter(|| {
                    (0..NUM_TRANSACTIONS_PER_ITER).for_each(|_| {
                        assert!(process_compute_budget_instructions(black_box(
                            SVMMessage::program_instructions_iter(&tx)
                        ))
                        .is_ok())
                    })
                });
            },
        );
}

fn bench_process_compute_budget_instructions_compute_budgets(c: &mut Criterion) {
    c.benchmark_group("bench_process_compute_budget_instructions_compute_budgets")
        .throughput(Throughput::Elements(NUM_TRANSACTIONS_PER_ITER as u64))
        .bench_function("4 compute-budget instructions", |bencher| {
            let ixs = vec![
                ComputeBudgetInstruction::request_heap_frame(40 * 1024),
                ComputeBudgetInstruction::set_compute_unit_limit(u32::MAX),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(u32::MAX),
            ];
            let tx = build_sanitized_transaction(&Keypair::new(), &ixs);
            bencher.iter(|| {
                (0..NUM_TRANSACTIONS_PER_ITER).for_each(|_| {
                    assert!(process_compute_budget_instructions(black_box(
                        SVMMessage::program_instructions_iter(&tx)
                    ))
                    .is_ok())
                })
            });
        });
}

fn bench_process_compute_budget_instructions_builtins(c: &mut Criterion) {
    c.benchmark_group("bench_process_compute_budget_instructions_builtins")
        .throughput(Throughput::Elements(NUM_TRANSACTIONS_PER_ITER as u64))
        .bench_function("4 dummy builtins", |bencher| {
            let ixs = vec![
                Instruction::new_with_bincode(solana_sdk::bpf_loader::id(), &(), vec![]),
                Instruction::new_with_bincode(solana_sdk::secp256k1_program::id(), &(), vec![]),
                Instruction::new_with_bincode(
                    solana_sdk::address_lookup_table::program::id(),
                    &(),
                    vec![],
                ),
                Instruction::new_with_bincode(solana_sdk::loader_v4::id(), &(), vec![]),
            ];
            let tx = build_sanitized_transaction(&Keypair::new(), &ixs);
            bencher.iter(|| {
                (0..NUM_TRANSACTIONS_PER_ITER).for_each(|_| {
                    assert!(process_compute_budget_instructions(black_box(
                        SVMMessage::program_instructions_iter(&tx)
                    ))
                    .is_ok())
                })
            });
        });
}

fn bench_process_compute_budget_instructions_mixed(c: &mut Criterion) {
    let num_instructions = 355;
    c.benchmark_group("bench_process_compute_budget_instructions_mixed")
        .throughput(Throughput::Elements(NUM_TRANSACTIONS_PER_ITER as u64))
        .bench_function(
            format!("{num_instructions} mixed instructions"),
            |bencher| {
                let payer_keypair = Keypair::new();
                let mut ixs: Vec<_> = (0..num_instructions)
                    .map(|_| {
                        Instruction::new_with_bincode(
                            DUMMY_PROGRAM_ID.parse().unwrap(),
                            &(),
                            vec![],
                        )
                    })
                    .collect();
                ixs.extend(vec![
                    ComputeBudgetInstruction::request_heap_frame(40 * 1024),
                    ComputeBudgetInstruction::set_compute_unit_limit(u32::MAX),
                    ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(u32::MAX),
                    system_instruction::transfer(&payer_keypair.pubkey(), &Pubkey::new_unique(), 1),
                ]);
                let tx = build_sanitized_transaction(&payer_keypair, &ixs);

                bencher.iter(|| {
                    (0..NUM_TRANSACTIONS_PER_ITER).for_each(|_| {
                        assert!(process_compute_budget_instructions(black_box(
                            SVMMessage::program_instructions_iter(&tx)
                        ))
                        .is_ok())
                    })
                });
            },
        );
}

criterion_group!(
    benches,
    bench_process_compute_budget_instructions_empty,
    bench_process_compute_budget_instructions_no_builtins,
    bench_process_compute_budget_instructions_compute_budgets,
    bench_process_compute_budget_instructions_builtins,
    bench_process_compute_budget_instructions_mixed,
);
criterion_main!(benches);
