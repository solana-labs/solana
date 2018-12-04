#![feature(test)]
extern crate bincode;
extern crate rayon;
extern crate solana;
extern crate test;

use solana::packet::to_packets;
use solana::sigverify;
use solana::test_tx::test_tx;
use test::Bencher;

#[bench]
fn bench_sigverify(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let batches = to_packets(&vec![tx; 128]);

    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::ed25519_verify(&batches);
    })
}
