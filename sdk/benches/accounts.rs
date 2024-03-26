#![feature(test)]
#![allow(clippy::arithmetic_side_effects)]

use solana_sdk::{entrypoint::MAX_PERMITTED_DATA_INCREASE, pubkey::Pubkey};

extern crate test;
use {solana_sdk::account::AccountSharedData, test::Bencher};

fn bench_unchanged(bencher: &mut Bencher, size: usize) {
    let mut account = AccountSharedData::new(42, 0, &Pubkey::new_unique());
    let new_data = vec![42; size];
    account.set_data(new_data.clone());

    bencher.iter(|| {
        account.set_data_from_slice(&new_data);
    });
}

fn bench_changed(bencher: &mut Bencher, size: usize) {
    let mut account = AccountSharedData::new(42, 0, &Pubkey::new_unique());
    let initial_data = vec![42; size];
    account.set_data(initial_data);

    let new_data = (0..10)
        .map(|i| {
            let mut data = vec![42; size];
            data[size / 10 * i] = 43;
            data
        })
        .collect::<Vec<_>>();

    let mut new_data = new_data.iter().cycle();

    bencher.iter(|| {
        account.set_data_from_slice(new_data.next().unwrap());
    });
}

fn bench_grow(bencher: &mut Bencher, size: usize) {
    let mut account = AccountSharedData::new(42, 0, &Pubkey::new_unique());
    let initial_data = vec![42; size];
    account.set_data(initial_data);

    let new_data = (0..10)
        .map(|i| {
            let mut data = vec![42; size];
            data.resize(size + (i * MAX_PERMITTED_DATA_INCREASE), 42);
            data
        })
        .collect::<Vec<_>>();

    let mut new_data = new_data.iter().cycle();

    bencher.iter(|| {
        account.set_data_from_slice(new_data.next().unwrap());
    });
}

fn bench_shrink(bencher: &mut Bencher, size: usize) {
    let mut account = AccountSharedData::new(42, 0, &Pubkey::new_unique());
    let initial_data = vec![42; size];
    account.set_data(initial_data);

    let new_data = (0..10)
        .map(|i| {
            let mut data = vec![42; size];
            data.resize(size + (i * MAX_PERMITTED_DATA_INCREASE), 42);
            data
        })
        .collect::<Vec<_>>();

    let mut new_data = new_data.iter().rev().cycle();

    bencher.iter(|| {
        account.set_data_from_slice(new_data.next().unwrap());
    });
}

#[bench]
fn bench_set_data_from_slice_unchanged_1k(b: &mut Bencher) {
    bench_unchanged(b, 1024)
}

#[bench]
fn bench_set_data_from_slice_unchanged_100k(b: &mut Bencher) {
    bench_unchanged(b, 1024 * 100)
}

#[bench]
fn bench_set_data_from_slice_unchanged_1mb(b: &mut Bencher) {
    bench_unchanged(b, 1024 * 1024)
}

#[bench]
fn bench_set_data_from_slice_unchanged_10mb(b: &mut Bencher) {
    bench_unchanged(b, 1024 * 1024 * 10)
}

#[bench]
fn bench_set_data_from_slice_changed_1k(b: &mut Bencher) {
    bench_changed(b, 1024)
}

#[bench]
fn bench_set_data_from_slice_changed_100k(b: &mut Bencher) {
    bench_changed(b, 1024 * 100)
}

#[bench]
fn bench_set_data_from_slice_changed_1mb(b: &mut Bencher) {
    bench_changed(b, 1024 * 1024)
}

#[bench]
fn bench_set_data_from_slice_changed_10mb(b: &mut Bencher) {
    bench_changed(b, 1024 * 1024 * 10)
}

#[bench]
fn bench_set_data_from_slice_grow_1k(b: &mut Bencher) {
    bench_grow(b, 1024)
}

#[bench]
fn bench_set_data_from_slice_grow_100k(b: &mut Bencher) {
    bench_grow(b, 1024 * 100)
}

#[bench]
fn bench_set_data_from_slice_grow_1mb(b: &mut Bencher) {
    bench_grow(b, 1024 * 1024)
}

#[bench]
fn bench_set_data_from_slice_grow_10mb(b: &mut Bencher) {
    bench_grow(b, 1024 * 1024 * 10)
}

#[bench]
fn bench_set_data_from_slice_shrink_100k(b: &mut Bencher) {
    bench_shrink(b, 1024 * 100)
}

#[bench]
fn bench_set_data_from_slice_shrink_1mb(b: &mut Bencher) {
    bench_shrink(b, 1024 * 1024)
}

#[bench]
fn bench_set_data_from_slice_shrink_10mb(b: &mut Bencher) {
    bench_shrink(b, 1024 * 1024 * 10)
}
