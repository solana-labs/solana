#![feature(test)]

extern crate test;
use solana_sdk::sanitized_transaction::SanitizedTransaction;
use test::Bencher;

#[bench]
fn bench_has_duplicates(bencher: &mut Bencher) {
    bencher.iter(|| {
        let data = test::black_box([1, 2, 3]);
        assert!(!SanitizedTransaction::has_duplicates(&data));
    })
}
