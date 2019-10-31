#![feature(test)]

extern crate test;

use solana_core::test_tx;
use solana_ledger::entry::{create_ticks, Entry};
use solana_ledger::shred::{
    max_entries_per_n_shred, max_ticks_per_n_shreds, Shred, Shredder, RECOMMENDED_FEC_RATE,
    SIZE_OF_DATA_SHRED_PAYLOAD,
};
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::sync::Arc;
use test::Bencher;

fn make_test_entry(txs_per_entry: u64) -> Entry {
    Entry {
        num_hashes: 100_000,
        hash: Hash::default(),
        transactions: vec![test_tx::test_tx(); txs_per_entry as usize],
    }
}
fn make_large_unchained_entries(txs_per_entry: u64, num_entries: u64) -> Vec<Entry> {
    (0..num_entries)
        .map(|_| make_test_entry(txs_per_entry))
        .collect()
}

#[bench]
fn bench_shredder_ticks(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    let shred_size = SIZE_OF_DATA_SHRED_PAYLOAD;
    let num_shreds = ((1000 * 1000) + (shred_size - 1)) / shred_size;
    // ~1Mb
    let num_ticks = max_ticks_per_n_shreds(1) * num_shreds as u64;
    let entries = create_ticks(num_ticks, 0, Hash::default());
    bencher.iter(|| {
        let shredder = Shredder::new(1, 0, RECOMMENDED_FEC_RATE, kp.clone()).unwrap();
        shredder.entries_to_shreds(&entries, true, 0);
    })
}

#[bench]
fn bench_shredder_large_entries(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    let shred_size = SIZE_OF_DATA_SHRED_PAYLOAD;
    let num_shreds = ((1000 * 1000) + (shred_size - 1)) / shred_size;
    let txs_per_entry = 128;
    let num_entries = max_entries_per_n_shred(&make_test_entry(txs_per_entry), num_shreds as u64);
    let entries = make_large_unchained_entries(txs_per_entry, num_entries);
    // 1Mb
    bencher.iter(|| {
        let shredder = Shredder::new(1, 0, RECOMMENDED_FEC_RATE, kp.clone()).unwrap();
        shredder.entries_to_shreds(&entries, true, 0);
    })
}

#[bench]
fn bench_deshredder(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    let shred_size = SIZE_OF_DATA_SHRED_PAYLOAD;
    // ~10Mb
    let num_shreds = ((10000 * 1000) + (shred_size - 1)) / shred_size;
    let num_ticks = max_ticks_per_n_shreds(1) * num_shreds as u64;
    let entries = create_ticks(num_ticks, 0, Hash::default());
    let shredder = Shredder::new(1, 0, RECOMMENDED_FEC_RATE, kp).unwrap();
    let data_shreds = shredder.entries_to_shreds(&entries, true, 0).0;
    bencher.iter(|| {
        let raw = &mut Shredder::deshred(&data_shreds).unwrap();
        assert_ne!(raw.len(), 0);
    })
}

#[bench]
fn bench_deserialize_hdr(bencher: &mut Bencher) {
    let data = vec![0; SIZE_OF_DATA_SHRED_PAYLOAD];

    let shred = Shred::new_from_data(2, 1, 1, Some(&data), true, true);

    bencher.iter(|| {
        let payload = shred.payload.clone();
        let _ = Shred::new_from_serialized_shred(payload).unwrap();
    })
}
