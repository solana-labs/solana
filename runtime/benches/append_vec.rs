#![feature(test)]

extern crate rand;
extern crate test;

use bincode::{deserialize, serialize_into, serialized_size};
use rand::{thread_rng, Rng};
use solana_runtime::append_vec::{
    deserialize_account, get_serialized_size, serialize_account, AppendVec,
};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::fs::{create_dir_all, remove_dir_all};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::spawn;
use test::Bencher;

const START_SIZE: u64 = 4 * 1024 * 1024;
const INC_SIZE: u64 = 1 * 1024 * 1024;

macro_rules! align_up {
    ($addr: expr, $align: expr) => {
        ($addr + ($align - 1)) & !($align - 1)
    };
}

fn get_append_vec_bench_path(path: &str) -> PathBuf {
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let mut buf = PathBuf::new();
    buf.push(&format!("{}/{}", out_dir, path));
    let _ignored = remove_dir_all(out_dir.clone());
    create_dir_all(out_dir).expect("Create directory failed");
    buf
}

#[bench]
fn append_vec_atomic_append(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_append");
    let mut vec = AppendVec::<AtomicUsize>::new(&path, true, START_SIZE, INC_SIZE);
    bencher.iter(|| {
        if vec.append(AtomicUsize::new(0)).is_none() {
            assert!(vec.grow_file().is_ok());
            assert!(vec.append(AtomicUsize::new(0)).is_some());
        }
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_atomic_random_access(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_ra");
    let mut vec = AppendVec::<AtomicUsize>::new(&path, true, START_SIZE, INC_SIZE);
    let size = 1_000_000;
    for _ in 0..size {
        if vec.append(AtomicUsize::new(0)).is_none() {
            assert!(vec.grow_file().is_ok());
            assert!(vec.append(AtomicUsize::new(0)).is_some());
        }
    }
    bencher.iter(|| {
        let index = thread_rng().gen_range(0, size as u64);
        vec.get(index * std::mem::size_of::<AtomicUsize>() as u64);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_atomic_random_change(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_rax");
    let mut vec = AppendVec::<AtomicUsize>::new(&path, true, START_SIZE, INC_SIZE);
    let size = 1_000_000;
    for k in 0..size {
        if vec.append(AtomicUsize::new(k)).is_none() {
            assert!(vec.grow_file().is_ok());
            assert!(vec.append(AtomicUsize::new(k)).is_some());
        }
    }
    bencher.iter(|| {
        let index = thread_rng().gen_range(0, size as u64);
        let atomic1 = vec.get(index * std::mem::size_of::<AtomicUsize>() as u64);
        let current1 = atomic1.load(Ordering::Relaxed);
        assert_eq!(current1, index as usize);
        let next = current1 + 1;
        let mut index = vec.append(AtomicUsize::new(next));
        if index.is_none() {
            assert!(vec.grow_file().is_ok());
            index = vec.append(AtomicUsize::new(next));
        }
        let atomic2 = vec.get(index.unwrap());
        let current2 = atomic2.load(Ordering::Relaxed);
        assert_eq!(current2, next);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_atomic_random_read(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_read");
    let mut vec = AppendVec::<AtomicUsize>::new(&path, true, START_SIZE, INC_SIZE);
    let size = 1_000_000;
    for _ in 0..size {
        if vec.append(AtomicUsize::new(0)).is_none() {
            assert!(vec.grow_file().is_ok());
            assert!(vec.append(AtomicUsize::new(0)).is_some());
        }
    }
    bencher.iter(|| {
        let index = thread_rng().gen_range(0, size);
        let atomic1 = vec.get((index * std::mem::size_of::<AtomicUsize>()) as u64);
        let current1 = atomic1.load(Ordering::Relaxed);
        assert_eq!(current1, 0);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_concurrent_lock_append(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_lock_append");
    let vec = Arc::new(RwLock::new(AppendVec::<AtomicUsize>::new(
        &path, true, START_SIZE, INC_SIZE,
    )));
    let vec1 = vec.clone();
    let size = 1_000_000;
    let count = Arc::new(AtomicUsize::new(0));
    let count1 = count.clone();
    spawn(move || loop {
        let mut len = count.load(Ordering::Relaxed);
        {
            let rlock = vec1.read().unwrap();
            loop {
                if rlock.append(AtomicUsize::new(0)).is_none() {
                    break;
                }
                len = count.fetch_add(1, Ordering::Relaxed);
            }
            if len >= size {
                break;
            }
        }
        {
            let mut wlock = vec1.write().unwrap();
            if len >= size {
                break;
            }
            assert!(wlock.grow_file().is_ok());
        }
    });
    bencher.iter(|| {
        let _rlock = vec.read().unwrap();
        let len = count1.load(Ordering::Relaxed);
        assert!(len < size * 2);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_concurrent_get_append(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_get_append");
    let vec = Arc::new(RwLock::new(AppendVec::<AtomicUsize>::new(
        &path, true, START_SIZE, INC_SIZE,
    )));
    let vec1 = vec.clone();
    let size = 1_000_000;
    let count = Arc::new(AtomicUsize::new(0));
    let count1 = count.clone();
    spawn(move || loop {
        let mut len = count.load(Ordering::Relaxed);
        {
            let rlock = vec1.read().unwrap();
            loop {
                if rlock.append(AtomicUsize::new(0)).is_none() {
                    break;
                }
                len = count.fetch_add(1, Ordering::Relaxed);
            }
            if len >= size {
                break;
            }
        }
        {
            let mut wlock = vec1.write().unwrap();
            if len >= size {
                break;
            }
            assert!(wlock.grow_file().is_ok());
        }
    });
    bencher.iter(|| {
        let rlock = vec.read().unwrap();
        let len = count1.load(Ordering::Relaxed);
        if len > 0 {
            let index = thread_rng().gen_range(0, len);
            rlock.get((index * std::mem::size_of::<AtomicUsize>()) as u64);
        }
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn bench_account_serialize(bencher: &mut Bencher) {
    let num: usize = 1000;
    let account = Account::new(2, 100, &Pubkey::new_rand());
    let len = get_serialized_size(&account);
    let ser_len = align_up!(len + std::mem::size_of::<u64>(), std::mem::size_of::<u64>());
    let mut memory = test::black_box(vec![0; num * ser_len]);
    bencher.iter(|| {
        for i in 0..num {
            let start = i * ser_len;
            serialize_account(&mut memory[start..start + ser_len], &account, len);
        }
    });

    let index = thread_rng().gen_range(0, num);
    let start = index * ser_len;
    let new_account = deserialize_account(&memory[start..start + ser_len], 0, num * len).unwrap();
    assert_eq!(new_account, account);
}

#[bench]
fn bench_account_serialize_bincode(bencher: &mut Bencher) {
    let num: usize = 1000;
    let account = Account::new(2, 100, &Pubkey::new_rand());
    let len = serialized_size(&account).unwrap() as usize;
    let mut memory = test::black_box(vec![0u8; num * len]);
    bencher.iter(|| {
        for i in 0..num {
            let start = i * len;
            let cursor = Cursor::new(&mut memory[start..start + len]);
            serialize_into(cursor, &account).unwrap();
        }
    });

    let index = thread_rng().gen_range(0, len);
    let start = index * len;
    let new_account: Account = deserialize(&memory[start..start + len]).unwrap();
    assert_eq!(new_account, account);
}
