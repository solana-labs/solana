#[macro_use]
extern crate criterion;
extern crate solana;

use criterion::{Bencher, Criterion};
use solana::hash::{hash, Hash};
use solana::ledger::{next_entries, reconstruct_entries_from_blobs, Block};
use solana::packet::BlobRecycler;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::collections::VecDeque;

fn bench_block_to_blobs_to_block(bencher: &mut Bencher) {
    let zero = Hash::default();
    let one = hash(&zero);
    let keypair = KeyPair::new();
    let tx0 = Transaction::new(&keypair, keypair.pubkey(), 1, one);
    let transactions = vec![tx0; 10];
    let entries = next_entries(&zero, 1, transactions);

    let blob_recycler = BlobRecycler::default();
    bencher.iter(|| {
        let mut blob_q = VecDeque::new();
        entries.to_blobs(&blob_recycler, &mut blob_q);
        assert_eq!(reconstruct_entries_from_blobs(blob_q).unwrap(), entries);
    });
}

fn bench(criterion: &mut Criterion) {
    criterion.bench_function("bench_block_to_blobs_to_block", |bencher| {
        bench_block_to_blobs_to_block(bencher);
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(2);
    targets = bench
);
criterion_main!(benches);
