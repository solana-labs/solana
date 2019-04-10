#![feature(test)]
extern crate test;

use rand::{thread_rng, Rng};
use solana_runtime::append_vec::AppendVec;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::thread::spawn;
use std::time::Duration;
use test::Bencher;

fn test_account(ix: usize) -> Account {
    let data_len = ix % 256;
    let mut account = Account::new(ix as u64, 0, &Pubkey::default());
    account.data = (0..data_len).into_iter().map(|_| data_len as u8).collect();
    account
}

fn get_append_vec_bench_path(path: &str) -> PathBuf {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let mut buf = PathBuf::new();
    buf.push(&format!("{}/{}", out_dir, path));
    buf
}

#[bench]
fn append_vec_append(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("bench_append");
    let vec = AppendVec::new(&path, true, 64 * 1024);
    bencher.iter(|| {
        let val = test_account(0);
        if vec.append_account(&val).is_none() {
            vec.reset();
        }
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_sequential_read(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("seq_read");
    let vec = AppendVec::new(&path, true, 64 * 1024);
    let size = 1_000;
    let mut indexes = vec![];
    for ix in 0..size {
        let val = test_account(ix);
        if let Some(pos) = vec.append_account(&val) {
            indexes.push((ix, pos))
        }
    }
    bencher.iter(|| {
        let (ix, pos) = indexes.pop().unwrap();
        let account = vec.get_account(pos);
        let test = test_account(ix);
        assert_eq!(*account, test);
        indexes.push((ix, pos));
    });
    std::fs::remove_file(path).unwrap();
}
#[bench]
fn append_vec_random_read(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("random_read");
    let vec = AppendVec::new(&path, true, 64 * 1024);
    let size = 1_000;
    let mut indexes = vec![];
    for ix in 0..size {
        let val = test_account(ix);
        if let Some(pos) = vec.append_account(&val) {
            indexes.push((ix, pos))
        }
    }
    bencher.iter(|| {
        let random_index: usize = thread_rng().gen_range(0, indexes.len());
        let (ix, pos) = &indexes[random_index];
        let account = vec.get_account(*pos);
        let test = test_account(*ix);
        assert_eq!(*account, test);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_concurrent_append_read(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("concurrent_read");
    let vec = Arc::new(AppendVec::new(&path, true, 1024 * 1024));
    let vec1 = vec.clone();
    let indexes: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(vec![]));
    let indexes1 = indexes.clone();
    spawn(move || loop {
        let ix = indexes1.lock().unwrap().len();
        let account = test_account(ix);
        if let Some(pos) = vec1.append_account(&account) {
            indexes1.lock().unwrap().push((ix, pos))
        } else {
            break;
        }
    });
    while indexes.lock().unwrap().is_empty() {
        sleep(Duration::from_millis(100));
    }
    bencher.iter(|| {
        let len = indexes.lock().unwrap().len();
        let random_index: usize = thread_rng().gen_range(0, len);
        let (ix, pos) = indexes.lock().unwrap().get(random_index).unwrap().clone();
        let account = vec.get_account(pos);
        let test = test_account(ix);
        assert_eq!(*account, test);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn append_vec_concurrent_read_append(bencher: &mut Bencher) {
    let path = get_append_vec_bench_path("concurrent_read");
    let vec = Arc::new(AppendVec::new(&path, true, 1024 * 1024));
    let vec1 = vec.clone();
    let indexes: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(vec![]));
    let indexes1 = indexes.clone();
    spawn(move || loop {
        let len = indexes1.lock().unwrap().len();
        let random_index: usize = thread_rng().gen_range(0, len + 1);
        let (ix, pos) = indexes1
            .lock()
            .unwrap()
            .get(random_index % len)
            .unwrap()
            .clone();
        let account = vec1.get_account(pos);
        let test = test_account(ix);
        assert_eq!(*account, test);
    });
    bencher.iter(|| {
        let ix: usize = thread_rng().gen_range(0, 256);
        let account = test_account(ix);
        if let Some(pos) = vec.append_account(&account) {
            indexes.lock().unwrap().push((ix, pos))
        }
    });
    std::fs::remove_file(path).unwrap();
}
