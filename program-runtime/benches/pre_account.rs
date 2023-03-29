#![feature(test)]

extern crate test;

use {
    log::*,
    solana_program_runtime::{pre_account::PreAccount, timings::ExecuteDetailsTimings},
    solana_sdk::{account::AccountSharedData, pubkey, rent::Rent},
    test::Bencher,
};

#[bench]
fn bench_verify_account_changes_data(bencher: &mut Bencher) {
    solana_logger::setup();

    let owner = pubkey::new_rand();
    let non_owner = pubkey::new_rand();
    let pre = PreAccount::new(
        &pubkey::new_rand(),
        AccountSharedData::new(0, BUFSIZE, &owner),
    );
    let post = AccountSharedData::new(0, BUFSIZE, &owner);
    assert_eq!(
        pre.verify(
            &owner,
            false,
            &Rent::default(),
            &post,
            &mut ExecuteDetailsTimings::default(),
            false,
        ),
        Ok(())
    );

    // this one should be faster
    bencher.iter(|| {
        pre.verify(
            &owner,
            false,
            &Rent::default(),
            &post,
            &mut ExecuteDetailsTimings::default(),
            false,
        )
        .unwrap();
    });
    let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
    info!("data no change by owner: {} ns/iter", summary.median);

    let pre_data = vec![BUFSIZE];
    let post_data = vec![BUFSIZE];
    bencher.iter(|| pre_data == post_data);
    let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
    info!("data compare {} ns/iter", summary.median);

    let pre = PreAccount::new(
        &pubkey::new_rand(),
        AccountSharedData::new(0, BUFSIZE, &owner),
    );
    bencher.iter(|| {
        pre.verify(
            &non_owner,
            false,
            &Rent::default(),
            &post,
            &mut ExecuteDetailsTimings::default(),
            false,
        )
        .unwrap();
    });
    let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
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
