#![feature(test)]

extern crate test;

use log::*;
use solana_runtime::message_processor::PreAccount;
use solana_sdk::{account::Account, pubkey::Pubkey, rent::Rent};
use test::Bencher;

#[bench]
fn bench_verify_account_changes_data(bencher: &mut Bencher) {
    solana_logger::setup();

    let owner = Pubkey::new_rand();
    let non_owner = Pubkey::new_rand();
    let pre = PreAccount::new(
        &Pubkey::new_rand(),
        &Account::new(0, BUFSIZE, &owner),
        true,
        false,
    );
    let post = Account::new(0, BUFSIZE, &owner);
    assert_eq!(pre.verify(&owner, &Rent::default(), &post), Ok(()));

    // this one should be faster
    bencher.iter(|| {
        pre.verify(&owner, &Rent::default(), &post).unwrap();
    });
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data no change by owner: {} ns/iter", summary.median);

    let pre_data = vec![BUFSIZE];
    let post_data = vec![BUFSIZE];
    bencher.iter(|| pre_data == post_data);
    let summary = bencher.bench(|_bencher| {}).unwrap();
    info!("data compare {} ns/iter", summary.median);

    let pre = PreAccount::new(
        &Pubkey::new_rand(),
        &Account::new(0, BUFSIZE, &owner),
        true,
        false,
    );
    bencher.iter(|| {
        pre.verify(&non_owner, &Rent::default(), &post).unwrap();
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
        PreAccount::is_zeroed(&BUF0);
    });
}

#[bench]
fn bench_is_zeroed_not(bencher: &mut Bencher) {
    bencher.iter(|| {
        PreAccount::is_zeroed(&BUF1);
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
