#[macro_use]
extern crate criterion;
extern crate bincode;
extern crate rayon;
extern crate solana;

use criterion::{Bencher, Criterion};
use solana::packet::{to_packets, PacketRecycler};
use solana::sigverify;
use solana::transaction::test_tx;

fn bench_sigverify(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let packet_recycler = PacketRecycler::default();
    let batches = to_packets(&packet_recycler, &vec![tx; 128]);

    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::ed25519_verify(&batches);
    })
}

fn bench(criterion: &mut Criterion) {
    criterion.bench_function("bench_sigverify", |bencher| {
        bench_sigverify(bencher);
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(2);
    targets = bench
);
criterion_main!(benches);
