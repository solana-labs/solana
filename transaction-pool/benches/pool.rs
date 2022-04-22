#![feature(test)]

extern crate test;
use ahash::AHasher;
use rand::thread_rng;
use rand::Rng;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_pool::{Item, Pool, Table};
use test::Bencher;

#[bench]
fn bench_pool_insert(bencher: &mut Bencher) {
    let mut pool = Pool::default();
    let mut i = 0;
    bencher.iter(move || {
        pool.insert(Item {
            priority: i,
            units: i,
            time: i,
            id: (0, 0),
        });
        i += 1;
    });
}

struct Test<'a> {
    keys: &'a [&'a Pubkey],
}

impl Table for Test<'_> {
    fn keys(&self, item: &Item) -> &[&Pubkey] {
        &self.keys[item.priority as usize % 5..self.keys.len()]
    }
}

#[bench]
fn bench_pool_baseline(bencher: &mut Bencher) {
    let mut pool = Pool::default();
    let max = 10_000usize;
    for i in 0..max {
        pool.insert(Item {
            priority: thread_rng().gen_range(1, 100),
            units: thread_rng().gen_range(1, 100),
            time: thread_rng().gen_range(1, 100),
            id: (i as u32, i as u32),
        });
    }
    bencher.iter(move || {
        let mut pool1 = pool.clone();
        pool1.count += 1;
    });
}

#[bench]
fn bench_pool_pop(bencher: &mut Bencher) {
    let mut pool = Pool::default();
    let max = 10_000usize;
    for i in 0..max {
        pool.insert(Item {
            priority: thread_rng().gen_range(1, 100),
            units: thread_rng().gen_range(1, 100),
            time: thread_rng().gen_range(1, 100),
            id: (i as u32, i as u32),
        });
    }
    //10x smaller then a block
    //ave units is 32
    pool.max_block_cu = 100_000;
    pool.max_bucket_cu = 25_000;
    pool.max_age = 128;
    let keys: &[&Pubkey] = &[
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
    ];
    let test = Test { keys };
    bencher.iter(move || {
        let mut pool1 = pool.clone();
        let rv = pool1.pop_block(0, &test);
        assert_eq!(rv.len() + pool1.count, pool.count);
    });
}
