#[macro_use]
extern crate criterion;
extern crate solana;

use criterion::{Bencher, Criterion};
use solana::signature::GenKeys;

fn bench_gen_keys(b: &mut Bencher) {
    let rnd = GenKeys::new([0u8; 32]);
    b.iter(|| rnd.gen_n_keypairs(1000));
}

fn bench(criterion: &mut Criterion) {
    criterion.bench_function("bench_gen_keys", |bencher| {
        bench_gen_keys(bencher);
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
