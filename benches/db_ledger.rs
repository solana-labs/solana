#![feature(test)]
extern crate rand;
extern crate rocksdb;
extern crate solana;
extern crate test;

use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use rocksdb::{Options, DB};
use solana::db_ledger::{DataCf, DbLedger, LedgerColumnFamilyRaw};
use solana::ledger::{get_tmp_ledger_path, make_large_test_entries, make_tiny_test_entries, Block};
use solana::packet::{Blob, BLOB_HEADER_SIZE};
use test::Bencher;

// Given some blobs and a ledger at ledger_path, benchmark writing the blobs to the ledger
fn bench_write_blobs(bench: &mut Bencher, blobs: &mut [&mut Blob], ledger_path: &str) {
    let db_ledger =
        DbLedger::open(&ledger_path).expect("Expected to be able to open database ledger");
    let slot = 0;
    let num_blobs = blobs.len();
    bench.iter(move || {
        for blob in blobs.iter_mut() {
            let index = blob.index().unwrap();
            let key = DataCf::key(slot, index);
            let size = blob.size().unwrap();
            db_ledger
                .data_cf
                .put(&db_ledger.db, &key, &blob.data[..BLOB_HEADER_SIZE + size])
                .unwrap();
            blob.set_index(index + num_blobs as u64).unwrap();
        }
    });

    DB::destroy(&Options::default(), &ledger_path)
        .expect("Expected successful database destruction");
}

// Insert some blobs into the ledger in preparation for read benchmarks
fn setup_read_bench(
    db_ledger: &mut DbLedger,
    num_small_blobs: u64,
    num_large_blobs: u64,
    slot: u64,
) {
    // Make some big and small entries
    let mut entries = make_large_test_entries(num_large_blobs as usize);
    entries.extend(make_tiny_test_entries(num_small_blobs as usize));

    // Convert the entries to blobs, wrsite the blobs to the ledger
    let shared_blobs = entries.to_blobs();
    db_ledger
        .write_shared_blobs(slot, &shared_blobs)
        .expect("Expectd successful insertion of blobs into ledger");
}

// Write small blobs to the ledger
#[bench]
#[ignore]
fn bench_write_small(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path("bench_write_small");
    let num_entries = 32 * 1024;
    let entries = make_tiny_test_entries(num_entries);
    let shared_blobs = entries.to_blobs();
    let mut blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.write().unwrap()).collect();
    let mut blobs: Vec<&mut Blob> = blob_locks.iter_mut().map(|b| &mut **b).collect();
    bench_write_blobs(bench, &mut blobs, &ledger_path);
}

// Write big blobs to the ledger
#[bench]
#[ignore]
fn bench_write_big(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path("bench_write_big");
    let num_entries = 32 * 1024;
    let entries = make_tiny_test_entries(num_entries);
    let shared_blobs = entries.to_blobs();
    let mut blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.write().unwrap()).collect();
    let mut blobs: Vec<&mut Blob> = blob_locks.iter_mut().map(|b| &mut **b).collect();
    bench_write_blobs(bench, &mut blobs, &ledger_path);
}

#[bench]
#[ignore]
fn bench_read_sequential(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path("bench_read_sequential");
    let mut db_ledger =
        DbLedger::open(&ledger_path).expect("Expected to be able to open database ledger");

    // Insert some big and small blobs into the ledger
    let num_small_blobs = 32 * 1024;
    let num_large_blobs = 32 * 1024;
    let total_blobs = num_small_blobs + num_large_blobs;
    let slot = 0;
    setup_read_bench(&mut db_ledger, num_small_blobs, num_large_blobs, slot);

    let num_reads = total_blobs / 15;
    let mut rng = rand::thread_rng();
    bench.iter(move || {
        // Generate random starting point in the range [0, total_blobs - 1], read num_reads blobs sequentially
        let start_index = rng.gen_range(0, num_small_blobs + num_large_blobs);
        for i in start_index..start_index + num_reads {
            let _ =
                db_ledger
                    .data_cf
                    .get_by_slot_index(&db_ledger.db, slot, i as u64 % total_blobs);
        }
    });

    DB::destroy(&Options::default(), &ledger_path)
        .expect("Expected successful database destruction");
}

#[bench]
#[ignore]
fn bench_read_random(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path("bench_read_random");
    let mut db_ledger =
        DbLedger::open(&ledger_path).expect("Expected to be able to open database ledger");

    // Insert some big and small blobs into the ledger
    let num_small_blobs = 32 * 1024;
    let num_large_blobs = 32 * 1024;
    let total_blobs = num_small_blobs + num_large_blobs;
    let slot = 0;
    setup_read_bench(&mut db_ledger, num_small_blobs, num_large_blobs, slot);

    let num_reads = total_blobs / 15;

    // Generate a num_reads sized random sample of indexes in range [0, total_blobs - 1],
    // simulating random reads
    let mut rng = rand::thread_rng();
    let indexes: Vec<usize> = (0..num_reads)
        .map(|_| rng.gen_range(0, total_blobs) as usize)
        .collect();
    bench.iter(move || {
        for i in indexes.iter() {
            let _ = db_ledger
                .data_cf
                .get_by_slot_index(&db_ledger.db, slot, *i as u64);
        }
    });

    DB::destroy(&Options::default(), &ledger_path)
        .expect("Expected successful database destruction");
}

#[bench]
#[ignore]
fn bench_insert_data_blob_small(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path("bench_insert_data_blob_small");
    let db_ledger =
        DbLedger::open(&ledger_path).expect("Expected to be able to open database ledger");
    let num_entries = 32 * 1024;
    let entries = make_tiny_test_entries(num_entries);
    let shared_blobs = entries.to_blobs();
    let mut blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.write().unwrap()).collect();
    let mut blobs: Vec<&mut Blob> = blob_locks.iter_mut().map(|b| &mut **b).collect();
    blobs.shuffle(&mut thread_rng());
    let slot = 0;

    bench.iter(move || {
        for blob in blobs.iter_mut() {
            let index = blob.index().unwrap();
            let key = DataCf::key(slot, index);
            db_ledger.insert_data_blob(&key, blob).unwrap();
            blob.set_index(index + num_entries as u64).unwrap();
        }
    });

    DB::destroy(&Options::default(), &ledger_path)
        .expect("Expected successful database destruction");
}

#[bench]
#[ignore]
fn bench_insert_data_blob_big(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path("bench_insert_data_blob_big");
    let db_ledger =
        DbLedger::open(&ledger_path).expect("Expected to be able to open database ledger");
    let num_entries = 32 * 1024;
    let entries = make_large_test_entries(num_entries);
    let shared_blobs = entries.to_blobs();
    let mut blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.write().unwrap()).collect();
    let mut blobs: Vec<&mut Blob> = blob_locks.iter_mut().map(|b| &mut **b).collect();
    blobs.shuffle(&mut thread_rng());
    let slot = 0;

    bench.iter(move || {
        for blob in blobs.iter_mut() {
            let index = blob.index().unwrap();
            let key = DataCf::key(slot, index);
            db_ledger.insert_data_blob(&key, blob).unwrap();
            blob.set_index(index + num_entries as u64).unwrap();
        }
    });

    DB::destroy(&Options::default(), &ledger_path)
        .expect("Expected successful database destruction");
}
