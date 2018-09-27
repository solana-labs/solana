#![feature(test)]
extern crate bincode;
extern crate rayon;
extern crate solana;
extern crate test;

use solana::packet::{to_packets, PacketRecycler};
use solana::sigverify;
use solana::system_transaction::test_tx;
use test::Bencher;

#[bench]
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
