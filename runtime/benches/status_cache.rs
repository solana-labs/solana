#![feature(test)]
extern crate test;

use {
    bincode::serialize,
    solana_runtime::{bank::BankStatusCache, status_cache::*},
    solana_sdk::{
        hash::{hash, Hash},
        signature::Signature,
    },
    test::Bencher,
};

#[bench]
fn bench_status_cache_serialize(bencher: &mut Bencher) {
    let mut status_cache = BankStatusCache::default();
    status_cache.add_root(0);
    status_cache.clear();
    for hash_index in 0..100 {
        let blockhash = Hash::new(&vec![hash_index; std::mem::size_of::<Hash>()]);
        let mut id = blockhash;
        for _ in 0..100 {
            id = hash(id.as_ref());
            let mut sigbytes = Vec::from(id.as_ref());
            id = hash(id.as_ref());
            sigbytes.extend(id.as_ref());
            let sig = Signature::try_from(sigbytes).unwrap();
            status_cache.insert(&blockhash, sig, 0, Ok(()));
        }
    }
    assert!(status_cache.roots().contains(&0));
    bencher.iter(|| {
        let _ = serialize(&status_cache.root_slot_deltas()).unwrap();
    });
}

#[bench]
fn bench_status_cache_root_slot_deltas(bencher: &mut Bencher) {
    let mut status_cache = BankStatusCache::default();

    // fill the status cache
    let slots: Vec<_> = (42..).take(MAX_CACHE_ENTRIES).collect();
    for slot in &slots {
        for _ in 0..5 {
            status_cache.insert(&Hash::new_unique(), Hash::new_unique(), *slot, Ok(()));
        }
        status_cache.add_root(*slot);
    }

    bencher.iter(|| test::black_box(status_cache.root_slot_deltas()));
}
