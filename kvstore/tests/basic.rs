use std::fs;
use std::path::{Path, PathBuf};

use solana_kvstore::test::gen;
use solana_kvstore::{Config, Key, KvStore};

const KB: usize = 1024;
const HALF_KB: usize = 512;

#[test]
fn test_put_get() {
    let path = setup("test_put_get");

    let cfg = Config {
        max_mem: 64 * KB,
        max_tables: 5,
        page_size: 64 * KB,
        ..Config::default()
    };

    let lsm = KvStore::open(&path, cfg).unwrap();
    let (key, bytes) = gen::pairs(HALF_KB).take(1).next().unwrap();

    lsm.put(&key, &bytes).expect("put fail");
    let out_bytes = lsm.get(&key).expect("get fail").expect("missing");

    assert_eq!(bytes, out_bytes);

    teardown(&path);
}

#[test]
fn test_put_get_many() {
    let path = setup("test_put_get_many");

    let cfg = Config {
        max_mem: 64 * KB,
        max_tables: 5,
        page_size: 64 * KB,
        ..Config::default()
    };
    let lsm = KvStore::open(&path, cfg).unwrap();

    let mut pairs: Vec<_> = gen::pairs(HALF_KB).take(1024).collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);

    lsm.put_many(pairs.clone().drain(..))
        .expect("put_many fail");

    let retrieved: Vec<(Key, Vec<u8>)> =
        lsm.range(Key::ALL_INCLUSIVE).expect("range fail").collect();

    assert!(!retrieved.is_empty());
    assert_eq!(pairs.len(), retrieved.len());
    assert_eq!(pairs, retrieved);

    teardown(&path);
}

#[test]
fn test_delete() {
    let path = setup("test_delete");

    let cfg = Config {
        max_mem: 64 * KB,
        max_tables: 5,
        page_size: 64 * KB,
        ..Config::default()
    };
    let lsm = KvStore::open(&path, cfg).unwrap();

    let mut pairs: Vec<_> = gen::pairs(HALF_KB).take(64 * 6).collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);

    for (k, i) in pairs.iter() {
        lsm.put(k, i).expect("put fail");
    }

    // drain iterator deletes from `pairs`
    for (k, _) in pairs.drain(64..128) {
        lsm.delete(&k).expect("delete fail");
    }

    let retrieved: Vec<(Key, Vec<u8>)> =
        lsm.range(Key::ALL_INCLUSIVE).expect("range fail").collect();

    assert!(!retrieved.is_empty());
    assert_eq!(pairs.len(), retrieved.len());
    assert_eq!(pairs, retrieved);

    teardown(&path);
}

#[test]
fn test_delete_many() {
    let path = setup("test_delete_many");

    let cfg = Config {
        max_mem: 64 * KB,
        max_tables: 5,
        page_size: 64 * KB,
        ..Config::default()
    };
    let lsm = KvStore::open(&path, cfg).unwrap();

    let mut pairs: Vec<_> = gen::pairs(HALF_KB).take(64 * 6).collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);

    for (k, i) in pairs.iter() {
        lsm.put(k, i).expect("put fail");
    }

    // drain iterator deletes from `pairs`
    let keys_to_delete = pairs.drain(320..384).map(|(k, _)| k);

    lsm.delete_many(keys_to_delete).expect("delete_many fail");

    let retrieved: Vec<(Key, Vec<u8>)> =
        lsm.range(Key::ALL_INCLUSIVE).expect("range fail").collect();

    assert!(!retrieved.is_empty());
    assert_eq!(pairs.len(), retrieved.len());
    assert_eq!(pairs, retrieved);

    teardown(&path);
}

#[test]
fn test_close_reopen() {
    let path = setup("test_close_reopen");
    let cfg = Config::default();
    let lsm = KvStore::open(&path, cfg).unwrap();

    let mut pairs: Vec<_> = gen::pairs(KB).take(1024).collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);

    for (k, i) in pairs.iter() {
        lsm.put(k, i).expect("put fail");
    }

    for (k, _) in pairs.drain(64..128) {
        lsm.delete(&k).expect("delete fail");
    }

    // Drop and re-open
    drop(lsm);
    let lsm = KvStore::open(&path, cfg).unwrap();

    let retrieved: Vec<(Key, Vec<u8>)> =
        lsm.range(Key::ALL_INCLUSIVE).expect("range fail").collect();

    assert!(!retrieved.is_empty());
    assert_eq!(pairs.len(), retrieved.len());
    assert_eq!(pairs, retrieved);

    teardown(&path);
}

#[test]
fn test_partitioned() {
    let path = setup("test_partitioned");

    let cfg = Config {
        max_mem: 64 * KB,
        max_tables: 5,
        page_size: 64 * KB,
        ..Config::default()
    };

    let storage_dirs = (0..4)
        .map(|i| path.join(format!("parition-{}", i)))
        .collect::<Vec<_>>();

    let lsm = KvStore::partitioned(&path, &storage_dirs, cfg).unwrap();

    let mut pairs: Vec<_> = gen::pairs(HALF_KB).take(64 * 12).collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);

    lsm.put_many(pairs.iter()).expect("put_many fail");

    // drain iterator deletes from `pairs`
    let keys_to_delete = pairs.drain(320..384).map(|(k, _)| k);

    lsm.delete_many(keys_to_delete).expect("delete_many fail");

    let retrieved: Vec<(Key, Vec<u8>)> =
        lsm.range(Key::ALL_INCLUSIVE).expect("range fail").collect();

    assert!(!retrieved.is_empty());
    assert_eq!(pairs.len(), retrieved.len());
    assert_eq!(pairs, retrieved);

    teardown(&path);
}

#[test]
fn test_in_memory() {
    let path = setup("test_in_memory");

    let cfg = Config {
        max_mem: 64 * KB,
        max_tables: 5,
        page_size: 64 * KB,
        in_memory: true,
        ..Config::default()
    };
    let lsm = KvStore::open(&path, cfg).unwrap();

    let mut pairs: Vec<_> = gen::pairs(HALF_KB).take(64 * 12).collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);

    lsm.put_many(pairs.iter()).expect("put_many fail");

    // drain iterator deletes from `pairs`
    let keys_to_delete = pairs.drain(320..384).map(|(k, _)| k);

    lsm.delete_many(keys_to_delete).expect("delete_many fail");

    let retrieved: Vec<(Key, Vec<u8>)> =
        lsm.range(Key::ALL_INCLUSIVE).expect("range fail").collect();

    assert!(!retrieved.is_empty());
    assert_eq!(pairs.len(), retrieved.len());
    assert_eq!(pairs, retrieved);

    teardown(&path);
}

fn setup(test_name: &str) -> PathBuf {
    let dir = Path::new("kvstore-test").join(test_name);

    let _ig = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    dir
}

fn teardown(p: &Path) {
    KvStore::destroy(p).expect("Expect successful store destruction");
}
