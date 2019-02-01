#![feature(test)]
use rand;

extern crate test;

use solana::blob_store::{get_tmp_store_path, BlobStore};
use solana::entry::{make_large_test_entries, make_tiny_test_entries, EntrySlice};
use solana::packet::Blob;

use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use test::Bencher;

// Given some blobs and a ledger at ledger_path, benchmark writing the blobs to the ledger
fn bench_write_blobs(bench: &mut Bencher, blobs: &mut [Blob], ledger_path: &str) {
    let mut store = BlobStore::open(&ledger_path).unwrap();

    bench.iter(move || {
        store.put_blobs(&blobs[..]).expect("Failed to insert blobs");
    });

    BlobStore::destroy(&ledger_path).expect("Expected successful database destruction");
}

// Insert some blobs into the ledger in preparation for read benchmarks
fn setup_read_bench(store: &mut BlobStore, num_small_blobs: u64, num_large_blobs: u64, slot: u64) {
    // Make some big and small entries
    let mut entries = make_large_test_entries(num_large_blobs as usize);
    entries.extend(make_tiny_test_entries(num_small_blobs as usize));

    // Convert the entries to blobs, write the blobs to the ledger
    let mut blobs = entries.to_blobs();
    for (index, b) in blobs.iter_mut().enumerate() {
        b.set_index(index as u64);
        b.set_slot(slot);
    }

    store
        .put_blobs(&blobs)
        .expect("Expectd successful insertion of blobs into ledger");
}

// Write small blobs to the ledger
#[bench]
#[ignore]
fn bench_write_small(bench: &mut Bencher) {
    let ledger_path = get_tmp_store_path("bench_write_small").unwrap();
    let num_entries = 32 * 1024;
    let entries = make_tiny_test_entries(num_entries);
    let mut blobs = entries.to_blobs();
    for (index, b) in blobs.iter_mut().enumerate() {
        b.set_index(index as u64);
    }
    bench_write_blobs(bench, &mut blobs, &ledger_path.to_string_lossy());
}

// Write big blobs to the ledger
#[bench]
#[ignore]
fn bench_write_big(bench: &mut Bencher) {
    let ledger_path = get_tmp_store_path("bench_write_big").unwrap();
    let num_entries = 1 * 1024;
    let entries = make_large_test_entries(num_entries);
    let mut blobs = entries.to_blobs();
    for (index, b) in blobs.iter_mut().enumerate() {
        b.set_index(index as u64);
    }

    bench_write_blobs(bench, &mut blobs, &ledger_path.to_string_lossy());
}

#[bench]
#[ignore]
fn bench_read_sequential(bench: &mut Bencher) {
    let ledger_path = get_tmp_store_path("bench_read_sequential").unwrap();
    let mut store = BlobStore::open(&ledger_path).unwrap();

    // Insert some big and small blobs into the ledger
    let num_small_blobs = 32 * 1024;
    let num_large_blobs = 32 * 1024;
    let total_blobs = num_small_blobs + num_large_blobs;
    let slot = 0;
    setup_read_bench(&mut store, num_small_blobs, num_large_blobs, slot);

    let num_reads = total_blobs / 15;
    let mut rng = rand::thread_rng();
    bench.iter(move || {
        // Generate random starting point in the range [0, total_blobs - 1], read num_reads blobs sequentially
        let start_index = rng.gen_range(0, num_small_blobs + num_large_blobs);
        for i in start_index..start_index + num_reads {
            let _ = store.get_blob_data(slot, i as u64 % total_blobs);
        }
    });

    BlobStore::destroy(&ledger_path).expect("Expected successful database destruction");
}

#[bench]
#[ignore]
fn bench_read_random(bench: &mut Bencher) {
    let ledger_path = get_tmp_store_path("bench_read_random").unwrap();
    let mut store = BlobStore::open(&ledger_path).unwrap();

    // Insert some big and small blobs into the ledger
    let num_small_blobs = 32 * 1024;
    let num_large_blobs = 32 * 1024;
    let total_blobs = num_small_blobs + num_large_blobs;
    let slot = 0;
    setup_read_bench(&mut store, num_small_blobs, num_large_blobs, slot);

    let num_reads = total_blobs / 15;

    // Generate a num_reads sized random sample of indexes in range [0, total_blobs - 1],
    // simulating random reads
    let mut rng = rand::thread_rng();
    let indexes: Vec<usize> = (0..num_reads)
        .map(|_| rng.gen_range(0, total_blobs) as usize)
        .collect();
    bench.iter(move || {
        for i in indexes.iter() {
            let _ = store.get_blob_data(slot, *i as u64);
        }
    });

    BlobStore::destroy(&ledger_path).expect("Expected successful database destruction");
}

#[bench]
#[ignore]
fn bench_insert_data_blob_small(bench: &mut Bencher) {
    let ledger_path = get_tmp_store_path("bench_insert_data_blob_small").unwrap();
    let mut store = BlobStore::open(&ledger_path).unwrap();
    let num_entries = 32 * 1024;
    let entries = make_tiny_test_entries(num_entries);
    let mut blobs = entries.to_blobs();

    blobs.shuffle(&mut thread_rng());

    bench.iter(move || {
        for blob in blobs.iter_mut() {
            let index = blob.index();
            blob.set_index(index + num_entries as u64);
        }
        store.put_blobs(&blobs).unwrap();
    });

    BlobStore::destroy(&ledger_path).expect("Expect successful destruction");
}

#[bench]
#[ignore]
fn bench_insert_data_blob_big(bench: &mut Bencher) {
    let ledger_path = get_tmp_store_path("bench_insert_data_blob_big").unwrap();
    let mut store = BlobStore::open(&ledger_path).unwrap();
    let num_entries = 32 * 1024;
    let entries = make_large_test_entries(num_entries);
    let mut blobs = entries.to_blobs();
    blobs.shuffle(&mut thread_rng());

    bench.iter(move || {
        let mut i = 0;
        for blob in blobs.iter_mut() {
            blob.set_index(i + num_entries as u64);
            i += 1;
        }

        store.put_blobs(&blobs).expect("failed to insert blobs");
    });

    BlobStore::destroy(&ledger_path).expect("Expect successful destruction");
}
