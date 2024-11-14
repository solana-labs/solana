#![feature(test)]
extern crate test;

use {
    bincode::serialize,
    rand::{rngs::SmallRng, Rng, SeedableRng},
    solana_accounts_db::ancestors::Ancestors,
    solana_runtime::{bank::BankStatusCache, status_cache::*},
    solana_sdk::{
        hash::{hash, Hash, HASH_BYTES},
        signature::{Signature, SIGNATURE_BYTES},
    },
    test::Bencher,
};

#[bench]
fn bench_status_cache_serialize(bencher: &mut Bencher) {
    let mut status_cache = BankStatusCache::default();
    status_cache.add_root(0);
    status_cache.clear();
    for hash_index in 0..100 {
        let blockhash = Hash::new_from_array([hash_index; HASH_BYTES]);
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

fn fill_status_cache(status_cache: &mut BankStatusCache, max_cache_entries: u64, num_txs: usize) {
    for slot in 0..max_cache_entries {
        let blockhash = Hash::new_unique();
        fill_status_cache_slot(status_cache, &blockhash, slot, num_txs);
    }
}

fn fill_status_cache_slot(
    status_cache: &mut BankStatusCache,
    blockhash: &Hash,
    slot: u64,
    num_txs: usize,
) {
    for _ in 0..num_txs {
        let tx_hash = Hash::new_unique();
        status_cache.insert(blockhash, tx_hash, slot, Ok(()));
    }
}

#[bench]
fn bench_status_cache_check_and_insert(bencher: &mut Bencher) {
    // Fill up the status cache to better match what intense runtime usage would
    // look like.
    let max_cache_entries = MAX_CACHE_ENTRIES as u64;
    let mut status_cache = BankStatusCache::default();
    fill_status_cache(&mut status_cache, max_cache_entries - 1, 100_000);

    // Manually fill the last slot so we can save off the blockhash to use for
    // querying and inserting into.
    let blockhash = Hash::new_unique();
    fill_status_cache_slot(&mut status_cache, &blockhash, max_cache_entries, 100_000);

    let slot = max_cache_entries + 1;
    let ancestors = Ancestors::from((slot - 32..slot).collect::<Vec<u64>>());

    // Pre-generate unique tx_hashes so we don't spend benchmark time generating
    // them.
    let batch_size = 1_000;
    let mut tx_hashes = Vec::with_capacity(batch_size);
    let mut rng = SmallRng::seed_from_u64(0);
    for _ in 0..batch_size {
        let mut sigbytes = [0u8; SIGNATURE_BYTES];
        rng.fill(&mut sigbytes);
        tx_hashes.push(Signature::from(sigbytes));
    }

    bencher.iter(|| {
        for tx_hash in &tx_hashes {
            if status_cache
                .get_status(*tx_hash, &blockhash, &ancestors)
                .is_none()
            {
                status_cache.insert(&blockhash, *tx_hash, slot, Ok(()));
            }
        }
    });
}

#[bench]
fn bench_status_cache_add_roots(bencher: &mut Bencher) {
    // Fill up the status cache to better match what intense runtime usage would
    // look like.
    let max_cache_entries = MAX_CACHE_ENTRIES as u64;
    let mut status_cache = BankStatusCache::default();
    fill_status_cache(&mut status_cache, max_cache_entries, 100_000);
    let start_slot = max_cache_entries + 1;
    bencher.iter(|| {
        for root in start_slot..start_slot + max_cache_entries {
            status_cache.add_root(root);
        }
    });
}
