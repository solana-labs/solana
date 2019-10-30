#![feature(test)]

extern crate test;

use log::*;
use solana_runtime::message_processor::*;
use solana_sdk::{account::Account, pubkey::Pubkey};
use test::Bencher;

#[bench]
fn bench_verify_account_changes_data(bencher: &mut Bencher) {
    solana_logger::setup();

    let owner = Pubkey::new_rand();
    let non_owner = Pubkey::new_rand();
    let pre = PreInstructionAccount::new(
        &Account::new(0, BUFSIZE, &owner),
        true,
        need_account_data_checked(&owner, &owner, true),
    );
    let post = Account::new(0, BUFSIZE, &owner);
    assert_eq!(verify_account_changes(&owner, &pre, &post), Ok(()));

    // this one should be faster
    bencher.iter(|| {
        verify_account_changes(&owner, &pre, &post).unwrap();
    });
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data no change by owner: {} ns/iter", summary.median);

    let pre = PreInstructionAccount::new(
        &Account::new(0, BUFSIZE, &owner),
        true,
        need_account_data_checked(&owner, &non_owner, true),
    );
    match pre.data {
        Some(ref data) => bencher.iter(|| *data == post.data),
        None => panic!("No data!"),
    }
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data compare {} ns/iter", summary.median);
    bencher.iter(|| {
        verify_account_changes(&non_owner, &pre, &post).unwrap();
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
