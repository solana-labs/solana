#![feature(test)]

extern crate test;

use solana_core::packet::to_packets;
use solana_core::recycler::Recycler;
use solana_core::sigverify;
use solana_core::test_tx::test_tx;
use test::Bencher;

#[bench]
fn bench_sigverify(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let batches = to_packets(&vec![tx; 128]);

    let recycler = Recycler::default();
    let recycler_out = Recycler::default();
    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::ed25519_verify(&batches, &recycler, &recycler_out);
    })
}

#[bench]
fn bench_get_offsets(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let batches = to_packets(&vec![tx; 1024]);

    let recycler = Recycler::default();
    // verify packets
    bencher.iter(|| {
        let ans = sigverify::generate_offsets(&batches, &recycler);
        assert!(ans.is_ok());
        let ans = ans.unwrap();
        recycler.recycle(ans.0);
        recycler.recycle(ans.1);
        recycler.recycle(ans.2);
        recycler.recycle(ans.3);
    })
}
