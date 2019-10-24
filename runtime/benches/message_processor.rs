#![feature(test)]

extern crate test;

use log::*;
use solana_runtime::message_processor::*;
use solana_sdk::{account::Account, pubkey::Pubkey};
use test::Bencher;

#[bench]
fn bench_has_duplicates(bencher: &mut Bencher) {
    bencher.iter(|| {
        let data = test::black_box([1, 2, 3]);
        assert!(!has_duplicates(&data));
    })
}

#[bench]
fn bench_verify_instruction_data(bencher: &mut Bencher) {
    solana_logger::setup();

    let owner = Pubkey::new_rand();
    let non_owner = Pubkey::new_rand();
    let pre = Account::new(0, BUFSIZE, &owner);
    let post = Account::new(0, BUFSIZE, &owner);
    assert_eq!(verify_instruction(true, &owner, &pre, &post), Ok(()));

    bencher.iter(|| pre.data == post.data);
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data compare {} ns/iter", summary.median);

    // this one should be faster
    bencher.iter(|| {
        verify_instruction(true, &owner, &pre, &post).unwrap();
    });
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data no change by owner: {} ns/iter", summary.median);

    bencher.iter(|| {
        verify_instruction(true, &non_owner, &pre, &post).unwrap();
    });
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data no change by non owner: {} ns/iter", summary.median);
}

const BUFSIZE: usize = 1024 * 1024 + 127;
static BUF0: [u8; BUFSIZE] = [0; BUFSIZE];
static BUF1: [u8; BUFSIZE] = [1; BUFSIZE];

#[bench]
fn bench_is_zeroed(bencher: &mut Bencher) {
    bencher.iter(|| {
        is_zeroed(&BUF0);
    });
}

#[bench]
fn bench_is_zeroed_not(bencher: &mut Bencher) {
    bencher.iter(|| {
        is_zeroed(&BUF1);
    });
}

#[bench]
fn bench_is_zeroed_by_iter(bencher: &mut Bencher) {
    bencher.iter(|| BUF0.iter().all(|item| *item == 0));
}

#[bench]
fn bench_is_zeroed_not_by_iter(bencher: &mut Bencher) {
    bencher.iter(|| BUF1.iter().all(|item| *item == 0));
}
