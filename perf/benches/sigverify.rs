#![feature(test)]

extern crate test;

use {
    solana_perf::{packet::to_packet_batches, recycler::Recycler, sigverify, test_tx::test_tx},
    test::Bencher,
};

#[bench]
fn bench_sigverify(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let mut batches = to_packet_batches(&std::iter::repeat(tx).take(128).collect::<Vec<_>>(), 128);

    let recycler = Recycler::default();
    let recycler_out = Recycler::default();
    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::ed25519_verify(&mut batches, &recycler, &recycler_out, false);
    })
}

#[bench]
fn bench_get_offsets(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let mut batches =
        to_packet_batches(&std::iter::repeat(tx).take(1024).collect::<Vec<_>>(), 1024);

    let recycler = Recycler::default();
    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::generate_offsets(&mut batches, &recycler, false);
    })
}
