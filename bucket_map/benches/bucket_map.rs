#![feature(test)]

extern crate test;
use rayon::prelude::*;
use solana_bucket_map::bucket_map::{BucketMap, BucketMapConfig};
use solana_sdk::pubkey::Pubkey;
use std::collections::hash_map::HashMap;
use std::sync::RwLock;
use test::Bencher;

type IndexValue = u64;

#[bench]
fn bucket_map_bench_hashmap_baseline(bencher: &mut Bencher) {
    let mut index = HashMap::new();
    bencher.iter(|| {
        let key = Pubkey::new_unique();
        index.insert(key, vec![(0, IndexValue::default())]);
    });
}

#[bench]
fn bucket_map_bench_insert_1(bencher: &mut Bencher) {
    let index = BucketMap::new(BucketMapConfig::from_buckets(1));
    bencher.iter(|| {
        let key = Pubkey::new_unique();
        index.update(&key, |_| Some((vec![(0, IndexValue::default())], 0)));
    });
}

#[bench]
fn bucket_map_bench_insert_16x32_baseline(bencher: &mut Bencher) {
    let index = RwLock::new(HashMap::new());
    (0..16).into_iter().into_par_iter().for_each(|_| {
        let key = Pubkey::new_unique();
        index
            .write()
            .unwrap()
            .insert(key, vec![(0, IndexValue::default())]);
    });
    bencher.iter(|| {
        (0..16).into_iter().into_par_iter().for_each(|_| {
            for _ in 0..32 {
                let key = Pubkey::new_unique();
                index
                    .write()
                    .unwrap()
                    .insert(key, vec![(0, IndexValue::default())]);
            }
        })
    });
}

#[bench]
fn bucket_map_bench_insert_16x32(bencher: &mut Bencher) {
    let index = BucketMap::new(BucketMapConfig::from_buckets(4));
    (0..16).into_iter().into_par_iter().for_each(|_| {
        let key = Pubkey::new_unique();
        index.update(&key, |_| Some((vec![(0, IndexValue::default())], 0)));
    });
    bencher.iter(|| {
        (0..16).into_iter().into_par_iter().for_each(|_| {
            for _ in 0..32 {
                let key = Pubkey::new_unique();
                index.update(&key, |_| Some((vec![(0, IndexValue::default())], 0)));
            }
        })
    });
}
