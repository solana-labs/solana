#![feature(test)]

macro_rules! DEFINE_NxM_BENCH {
    ($i:ident, $n:literal, $m:literal) => {
        mod $i {
            use super::*;

            #[bench]
            fn bench_insert_baseline_hashmap(bencher: &mut Bencher) {
                do_bench_insert_baseline_hashmap(bencher, $n, $m);
            }

            #[bench]
            fn bench_insert_bucket_map(bencher: &mut Bencher) {
                do_bench_insert_bucket_map(bencher, $n, $m);
            }
        }
    };
}

extern crate test;
use {
    rayon::prelude::*,
    solana_bucket_map::bucket_map::{BucketMap, BucketMapConfig},
    solana_sdk::pubkey::Pubkey,
    std::{collections::hash_map::HashMap, sync::RwLock},
    test::Bencher,
};

type IndexValue = u64;

DEFINE_NxM_BENCH!(dim_01x02, 1, 2);
DEFINE_NxM_BENCH!(dim_02x04, 2, 4);
DEFINE_NxM_BENCH!(dim_04x08, 4, 8);
DEFINE_NxM_BENCH!(dim_08x16, 8, 16);
DEFINE_NxM_BENCH!(dim_16x32, 16, 32);
DEFINE_NxM_BENCH!(dim_32x64, 32, 64);

/// Benchmark insert with Hashmap as baseline for N threads inserting M keys each
fn do_bench_insert_baseline_hashmap(bencher: &mut Bencher, n: usize, m: usize) {
    let index = RwLock::new(HashMap::new());
    (0..n).into_par_iter().for_each(|i| {
        let key = Pubkey::new_unique();
        index
            .write()
            .unwrap()
            .insert(key, vec![(i, IndexValue::default())]);
    });
    bencher.iter(|| {
        (0..n).into_par_iter().for_each(|_| {
            for j in 0..m {
                let key = Pubkey::new_unique();
                index
                    .write()
                    .unwrap()
                    .insert(key, vec![(j, IndexValue::default())]);
            }
        })
    });
}

/// Benchmark insert with BucketMap with N buckets for N threads inserting M keys each
fn do_bench_insert_bucket_map(bencher: &mut Bencher, n: usize, m: usize) {
    let index = BucketMap::new(BucketMapConfig::new(n));
    (0..n).into_par_iter().for_each(|i| {
        let key = Pubkey::new_unique();
        index.update(&key, |_| Some((vec![(i, IndexValue::default())], 0)));
    });
    bencher.iter(|| {
        (0..n).into_par_iter().for_each(|_| {
            for j in 0..m {
                let key = Pubkey::new_unique();
                index.update(&key, |_| Some((vec![(j, IndexValue::default())], 0)));
            }
        })
    });
}
