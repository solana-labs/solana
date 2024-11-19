use {
    criterion::{black_box, criterion_group, criterion_main, Criterion},
    solana_compute_budget::compute_budget_limits::ComputeBudgetLimits,
    solana_runtime_transaction::instructions_processor::process_compute_budget_instructions,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, instruction::CompiledInstruction},
    solana_svm_transaction::instruction::SVMInstruction,
    std::num::NonZero,
};

const ONE_PAGE: u32 = 32 * 1024;
const SIXTY_FOUR_MB: u32 = 64 * 1024 * 1024;

fn bench_request_heap_frame(c: &mut Criterion) {
    let instruction = [(
        solana_sdk::compute_budget::id(),
        CompiledInstruction::new_from_raw_parts(
            0,
            ComputeBudgetInstruction::request_heap_frame(ONE_PAGE).data,
            vec![],
        ),
    )];

    c.bench_function("request_heap_limit", |bencher| {
        bencher.iter(|| {
            assert_eq!(
                process_compute_budget_instructions(black_box(
                    instruction
                        .iter()
                        .map(|(id, ix)| (id, SVMInstruction::from(ix)))
                )),
                Ok(ComputeBudgetLimits {
                    updated_heap_bytes: ONE_PAGE,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                    loaded_accounts_bytes: NonZero::new(SIXTY_FOUR_MB).unwrap()
                })
            )
        })
    });
}

fn bench_set_compute_unit_limit(c: &mut Criterion) {
    let instruction = [(
        solana_sdk::compute_budget::id(),
        CompiledInstruction::new_from_raw_parts(
            0,
            ComputeBudgetInstruction::set_compute_unit_limit(1024).data,
            vec![],
        ),
    )];

    c.bench_function("set_compute_unit_limit", |bencher| {
        bencher.iter(|| {
            assert_eq!(
                process_compute_budget_instructions(black_box(
                    instruction
                        .iter()
                        .map(|(id, ix)| (id, SVMInstruction::from(ix)))
                )),
                Ok(ComputeBudgetLimits {
                    updated_heap_bytes: ONE_PAGE,
                    compute_unit_limit: 1024,
                    compute_unit_price: 0,
                    loaded_accounts_bytes: NonZero::new(SIXTY_FOUR_MB).unwrap()
                })
            )
        })
    });
}

fn bench_set_compute_unit_price(c: &mut Criterion) {
    let instruction = [(
        solana_sdk::compute_budget::id(),
        CompiledInstruction::new_from_raw_parts(
            0,
            ComputeBudgetInstruction::set_compute_unit_price(1).data,
            vec![],
        ),
    )];

    c.bench_function("set_compute_unit_price", |bencher| {
        bencher.iter(|| {
            assert_eq!(
                process_compute_budget_instructions(black_box(
                    instruction
                        .iter()
                        .map(|(id, ix)| (id, SVMInstruction::from(ix)))
                )),
                Ok(ComputeBudgetLimits {
                    updated_heap_bytes: ONE_PAGE,
                    compute_unit_limit: 0,
                    compute_unit_price: 1,
                    loaded_accounts_bytes: NonZero::new(SIXTY_FOUR_MB).unwrap()
                })
            )
        })
    });
}

fn bench_set_loaded_accounts_data_size_limit(c: &mut Criterion) {
    let instruction = [(
        solana_sdk::compute_budget::id(),
        CompiledInstruction::new_from_raw_parts(
            0,
            ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(1).data,
            vec![],
        ),
    )];

    c.bench_function("set_loaded_accounts_data_size_limit", |bencher| {
        bencher.iter(|| {
            assert_eq!(
                process_compute_budget_instructions(black_box(
                    instruction
                        .iter()
                        .map(|(id, ix)| (id, SVMInstruction::from(ix)))
                )),
                Ok(ComputeBudgetLimits {
                    updated_heap_bytes: ONE_PAGE,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                    loaded_accounts_bytes: NonZero::new(1).unwrap()
                })
            )
        })
    });
}

criterion_group!(
    benches,
    bench_request_heap_frame,
    bench_set_compute_unit_limit,
    bench_set_compute_unit_price,
    bench_set_loaded_accounts_data_size_limit,
);
criterion_main!(benches);
