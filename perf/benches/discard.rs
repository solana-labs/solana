#![feature(test)]

extern crate test;

use {
    solana_perf::{discard::discard_batches_randomly, packet::to_packet_batches, test_tx::test_tx},
    test::Bencher,
};

const NUM: usize = 1000;

#[bench]
fn bench_discard(bencher: &mut Bencher) {
    solana_logger::setup();
    let tx = test_tx();
    let num_packets = NUM;

    // generate packet vector
    let batches = to_packet_batches(
        &std::iter::repeat(tx).take(num_packets).collect::<Vec<_>>(),
        10,
    );

    bencher.iter(|| {
        let mut discarded = batches.clone();
        discard_batches_randomly(&mut discarded, 100, NUM);
        assert_eq!(discarded.len(), 10);
    })
}
