#![feature(test)]

extern crate test;

use solana_perf::packet::to_packets_chunked;
use solana_perf::recycler::Recycler;
use solana_perf::sigverify;
use solana_perf::test_tx::test_tx;
use test::Bencher;

#[bench]
fn bench_sigverify(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let batches = to_packets_chunked(&std::iter::repeat(tx).take(128).collect::<Vec<_>>(), 128);

    let recycler = Recycler::new_without_limit("");
    let recycler_out = Recycler::new_without_limit("");
    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::ed25519_verify(&batches, &recycler, &recycler_out);
    })
}

#[bench]
fn bench_get_offsets(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let batches = to_packets_chunked(&std::iter::repeat(tx).take(1024).collect::<Vec<_>>(), 1024);

    let recycler = Recycler::new_without_limit("");
    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::generate_offsets(&batches, &recycler);
    })
}
