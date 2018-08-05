#[macro_use]
extern crate criterion;
extern crate rand;
extern crate solana;

use criterion::Criterion;
use rand::{thread_rng, RngCore};
use solana::page_table::{Call, Context, PageTable, N};

fn bench_load_and_execute(criterion: &mut Criterion) {
    criterion.bench_function("bench_load_and_execute", move |b| {
        let pt = PageTable::default();
        let mut ttx: Vec<Vec<_>> = (0..N)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for transactions in &ttx {
            pt.force_allocate(transactions, true, 1_000_000);
        }
        let mut ctx = Context::default();
        b.iter(|| {
            let transactions = &mut ttx[thread_rng().next_u64() as usize % N];
            for tx in transactions.iter_mut() {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.allocate_keys_with_ctx(&transactions, &mut ctx);
            PageTable::execute_with_ctx(&transactions, &mut ctx);
            pt.commit_release_with_ctx(&transactions, &ctx);
        });
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(2);
    targets = bench_load_and_execute
);
criterion_main!(benches);
