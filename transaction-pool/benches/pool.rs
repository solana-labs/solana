#![feature(test)]

extern crate test;
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
fn bench_pool_pop(bencher: &mut Bencher) {
    let mut pool = Pool::default();
    for i in 0..100_000_000 {
        pool.insert(Item {
            priority: i % 63,
            units: i % 63,
            time: i % 127,
            id: (0, 0),
        });
    }
    //10x smaller then a block
    //ave units is 32
    pool.max_block_cu = 100_000;
    pool.max_bucket_cu = 25_000;
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
        let _rv = pool.pop_block(100, &test);
    });
}
