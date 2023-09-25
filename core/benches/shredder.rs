#![allow(clippy::arithmetic_side_effects)]
#![feature(test)]

extern crate test;

use {
    rand::seq::SliceRandom,
    raptorq::{Decoder, Encoder},
    solana_entry::entry::{create_ticks, Entry},
    solana_ledger::shred::{
        max_entries_per_n_shred, max_ticks_per_n_shreds, ProcessShredsStats, ReedSolomonCache,
        Shred, ShredFlags, Shredder, DATA_SHREDS_PER_FEC_BLOCK, LEGACY_SHRED_DATA_CAPACITY,
    },
    solana_perf::test_tx,
    solana_sdk::{hash::Hash, packet::PACKET_DATA_SIZE, signature::Keypair},
    test::Bencher,
};

// Copied these values here to avoid exposing shreds
// internals only for the sake of benchmarks.

// size of nonce: 4
// size of common shred header: 83
// size of coding shred header: 6
const VALID_SHRED_DATA_LEN: usize = PACKET_DATA_SIZE - 4 - 83 - 6;

fn make_test_entry(txs_per_entry: u64) -> Entry {
    Entry {
        num_hashes: 100_000,
        hash: Hash::default(),
        transactions: vec![test_tx::test_tx().into(); txs_per_entry as usize],
    }
}
fn make_large_unchained_entries(txs_per_entry: u64, num_entries: u64) -> Vec<Entry> {
    (0..num_entries)
        .map(|_| make_test_entry(txs_per_entry))
        .collect()
}

fn make_shreds(num_shreds: usize) -> Vec<Shred> {
    let txs_per_entry = 128;
    let num_entries = max_entries_per_n_shred(
        &make_test_entry(txs_per_entry),
        2 * num_shreds as u64,
        Some(LEGACY_SHRED_DATA_CAPACITY),
    );
    let entries = make_large_unchained_entries(txs_per_entry, num_entries);
    let shredder = Shredder::new(1, 0, 0, 0).unwrap();
    let (data_shreds, _) = shredder.entries_to_shreds(
        &Keypair::new(),
        &entries,
        true,  // is_last_in_slot
        0,     // next_shred_index
        0,     // next_code_index
        false, // merkle_variant
        &ReedSolomonCache::default(),
        &mut ProcessShredsStats::default(),
    );
    assert!(data_shreds.len() >= num_shreds);
    data_shreds
}

fn make_concatenated_shreds(num_shreds: usize) -> Vec<u8> {
    let data_shreds = make_shreds(num_shreds);
    let mut data: Vec<u8> = vec![0; num_shreds * VALID_SHRED_DATA_LEN];
    for (i, shred) in (data_shreds[0..num_shreds]).iter().enumerate() {
        data[i * VALID_SHRED_DATA_LEN..(i + 1) * VALID_SHRED_DATA_LEN]
            .copy_from_slice(&shred.payload()[..VALID_SHRED_DATA_LEN]);
    }

    data
}

#[bench]
fn bench_shredder_ticks(bencher: &mut Bencher) {
    let kp = Keypair::new();
    let shred_size = LEGACY_SHRED_DATA_CAPACITY;
    let num_shreds = ((1000 * 1000) + (shred_size - 1)) / shred_size;
    // ~1Mb
    let num_ticks = max_ticks_per_n_shreds(1, Some(LEGACY_SHRED_DATA_CAPACITY)) * num_shreds as u64;
    let entries = create_ticks(num_ticks, 0, Hash::default());
    let reed_solomon_cache = ReedSolomonCache::default();
    bencher.iter(|| {
        let shredder = Shredder::new(1, 0, 0, 0).unwrap();
        shredder.entries_to_shreds(
            &kp,
            &entries,
            true,
            0,
            0,
            true, // merkle_variant
            &reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );
    })
}

#[bench]
fn bench_shredder_large_entries(bencher: &mut Bencher) {
    let kp = Keypair::new();
    let shred_size = LEGACY_SHRED_DATA_CAPACITY;
    let num_shreds = ((1000 * 1000) + (shred_size - 1)) / shred_size;
    let txs_per_entry = 128;
    let num_entries = max_entries_per_n_shred(
        &make_test_entry(txs_per_entry),
        num_shreds as u64,
        Some(shred_size),
    );
    let entries = make_large_unchained_entries(txs_per_entry, num_entries);
    let reed_solomon_cache = ReedSolomonCache::default();
    // 1Mb
    bencher.iter(|| {
        let shredder = Shredder::new(1, 0, 0, 0).unwrap();
        shredder.entries_to_shreds(
            &kp,
            &entries,
            true,
            0,
            0,
            true, // merkle_variant
            &reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );
    })
}

#[bench]
fn bench_deshredder(bencher: &mut Bencher) {
    let kp = Keypair::new();
    let shred_size = LEGACY_SHRED_DATA_CAPACITY;
    // ~10Mb
    let num_shreds = ((10000 * 1000) + (shred_size - 1)) / shred_size;
    let num_ticks = max_ticks_per_n_shreds(1, Some(shred_size)) * num_shreds as u64;
    let entries = create_ticks(num_ticks, 0, Hash::default());
    let shredder = Shredder::new(1, 0, 0, 0).unwrap();
    let (data_shreds, _) = shredder.entries_to_shreds(
        &kp,
        &entries,
        true,
        0,
        0,
        true, // merkle_variant
        &ReedSolomonCache::default(),
        &mut ProcessShredsStats::default(),
    );
    bencher.iter(|| {
        let raw = &mut Shredder::deshred(&data_shreds).unwrap();
        assert_ne!(raw.len(), 0);
    })
}

#[bench]
fn bench_deserialize_hdr(bencher: &mut Bencher) {
    let data = vec![0; LEGACY_SHRED_DATA_CAPACITY];

    let shred = Shred::new_from_data(2, 1, 1, &data, ShredFlags::LAST_SHRED_IN_SLOT, 0, 0, 1);

    bencher.iter(|| {
        let payload = shred.payload().clone();
        let _ = Shred::new_from_serialized_shred(payload).unwrap();
    })
}

#[bench]
fn bench_shredder_coding(bencher: &mut Bencher) {
    let symbol_count = DATA_SHREDS_PER_FEC_BLOCK;
    let data_shreds = make_shreds(symbol_count);
    let reed_solomon_cache = ReedSolomonCache::default();
    bencher.iter(|| {
        Shredder::generate_coding_shreds(
            &data_shreds[..symbol_count],
            0, // next_code_index
            &reed_solomon_cache,
        )
        .len();
    })
}

#[bench]
fn bench_shredder_decoding(bencher: &mut Bencher) {
    let symbol_count = DATA_SHREDS_PER_FEC_BLOCK;
    let data_shreds = make_shreds(symbol_count);
    let reed_solomon_cache = ReedSolomonCache::default();
    let coding_shreds = Shredder::generate_coding_shreds(
        &data_shreds[..symbol_count],
        0, // next_code_index
        &reed_solomon_cache,
    );
    bencher.iter(|| {
        Shredder::try_recovery(coding_shreds[..].to_vec(), &reed_solomon_cache).unwrap();
    })
}

#[bench]
fn bench_shredder_coding_raptorq(bencher: &mut Bencher) {
    let symbol_count = DATA_SHREDS_PER_FEC_BLOCK;
    let data = make_concatenated_shreds(symbol_count);
    bencher.iter(|| {
        let encoder = Encoder::with_defaults(&data, VALID_SHRED_DATA_LEN as u16);
        encoder.get_encoded_packets(symbol_count as u32);
    })
}

#[bench]
fn bench_shredder_decoding_raptorq(bencher: &mut Bencher) {
    let symbol_count = DATA_SHREDS_PER_FEC_BLOCK;
    let data = make_concatenated_shreds(symbol_count);
    let encoder = Encoder::with_defaults(&data, VALID_SHRED_DATA_LEN as u16);
    let mut packets = encoder.get_encoded_packets(symbol_count as u32);
    packets.shuffle(&mut rand::thread_rng());

    // Here we simulate losing 1 less than 50% of the packets randomly
    packets.truncate(packets.len() - packets.len() / 2 + 1);

    bencher.iter(|| {
        let mut decoder = Decoder::new(encoder.get_config());
        let mut result = None;
        for packet in &packets {
            result = decoder.decode(packet.clone());
            if result.is_some() {
                break;
            }
        }
        assert_eq!(result.unwrap(), data);
    })
}
