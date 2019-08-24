#![feature(test)]
extern crate test;

use std::fs;
use std::path::{Path, PathBuf};

use rand::{self, Rng};

use test::Bencher;

use solana_kvstore::{test::gen, Config, Key, KvStore};

const SMALL_SIZE: usize = 512;
const LARGE_SIZE: usize = 32 * 1024;
const HUGE_SIZE: usize = 64 * 1024;

fn bench_write(bench: &mut Bencher, rows: &[(Key, Vec<u8>)], ledger_path: &str) {
    let store = KvStore::open_default(&ledger_path).unwrap();

    bench.iter(move || {
        store.put_many(rows.iter()).expect("Failed to insert rows");
    });

    teardown(&ledger_path);
}

fn bench_write_partitioned(bench: &mut Bencher, rows: &[(Key, Vec<u8>)], ledger_path: &str) {
    let path = Path::new(ledger_path);
    let storage_dirs = (0..4)
        .map(|i| path.join(format!("parition-{}", i)))
        .collect::<Vec<_>>();

    let store = KvStore::partitioned(&ledger_path, &storage_dirs, Config::default()).unwrap();

    bench.iter(move || {
        store.put_many(rows.iter()).expect("Failed to insert rows");
    });

    teardown(&ledger_path);
}

#[bench]
#[ignore]
fn bench_write_small(bench: &mut Bencher) {
    let ledger_path = setup("bench_write_small");
    let num_entries = 32 * 1024;
    let rows = gen::pairs(SMALL_SIZE).take(num_entries).collect::<Vec<_>>();
    bench_write(bench, &rows, &ledger_path.to_string_lossy());
}

#[bench]
#[ignore]
fn bench_write_small_partitioned(bench: &mut Bencher) {
    let ledger_path = setup("bench_write_small_partitioned");
    let num_entries = 32 * 1024;
    let rows = gen::pairs(SMALL_SIZE).take(num_entries).collect::<Vec<_>>();
    bench_write_partitioned(bench, &rows, &ledger_path.to_string_lossy());
}

#[bench]
#[ignore]
fn bench_write_large(bench: &mut Bencher) {
    let ledger_path = setup("bench_write_large");
    let num_entries = 32 * 1024;
    let rows = gen::pairs(LARGE_SIZE).take(num_entries).collect::<Vec<_>>();
    bench_write(bench, &rows, &ledger_path.to_string_lossy());
}

#[bench]
#[ignore]
fn bench_write_huge(bench: &mut Bencher) {
    let ledger_path = setup("bench_write_huge");
    let num_entries = 32 * 1024;
    let rows = gen::pairs(HUGE_SIZE).take(num_entries).collect::<Vec<_>>();
    bench_write(bench, &rows, &ledger_path.to_string_lossy());
}

#[bench]
#[ignore]
fn bench_read_sequential(bench: &mut Bencher) {
    let ledger_path = setup("bench_read_sequential");
    let store = KvStore::open_default(&ledger_path).unwrap();

    // Insert some big and small blobs into the ledger
    let num_small_blobs = 32 * 1024;
    let num_large_blobs = 32 * 1024;
    let total_blobs = num_small_blobs + num_large_blobs;

    let small = gen::data(SMALL_SIZE).take(num_small_blobs);
    let large = gen::data(LARGE_SIZE).take(num_large_blobs);
    let rows = gen_seq_keys().zip(small.chain(large));

    let _ = store.put_many(rows);

    let num_reads = total_blobs / 15;
    let mut rng = rand::thread_rng();

    bench.iter(move || {
        // Generate random starting point in the range [0, total_blobs - 1], read num_reads blobs sequentially
        let start_index = rng.gen_range(0, num_small_blobs + num_large_blobs);
        for i in start_index..start_index + num_reads {
            let i = i as u64;
            let k = Key::from((i, i, i));
            let _ = store.get(&k);
        }
    });

    teardown(&ledger_path);
}

#[bench]
#[ignore]
fn bench_read_random(bench: &mut Bencher) {
    let ledger_path = setup("bench_read_sequential");
    let store = KvStore::open_default(&ledger_path).unwrap();

    // Insert some big and small blobs into the ledger
    let num_small_blobs = 32 * 1024;
    let num_large_blobs = 32 * 1024;
    let total_blobs = num_small_blobs + num_large_blobs;

    let small = gen::data(SMALL_SIZE).take(num_small_blobs);
    let large = gen::data(LARGE_SIZE).take(num_large_blobs);
    let rows = gen_seq_keys().zip(small.chain(large));

    let _ = store.put_many(rows);

    let num_reads = total_blobs / 15;
    let mut rng = rand::thread_rng();

    // Generate a num_reads sized random sample of indexes in range [0, total_blobs - 1],
    // simulating random reads
    let indexes: Vec<u64> = (0..num_reads)
        .map(|_| rng.gen_range(0, total_blobs as u64))
        .collect();

    bench.iter(move || {
        for &i in indexes.iter() {
            let i = i as u64;
            let k = Key::from((i, i, i));
            let _ = store.get(&k);
        }
    });

    teardown(&ledger_path);
}

fn setup(test_name: &str) -> PathBuf {
    let dir = Path::new("kvstore-bench").join(test_name);

    let _ig = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    dir
}

fn gen_seq_keys() -> impl Iterator<Item = Key> {
    let mut n = 0;

    std::iter::repeat_with(move || {
        let key = Key::from((n, n, n));
        n += 1;

        key
    })
}

fn teardown<P: AsRef<Path>>(p: P) {
    KvStore::destroy(p).expect("Expect successful store destruction");
}
