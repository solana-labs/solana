#![feature(test)]
extern crate test;

use {
    rand::{thread_rng, Rng},
    solana_runtime::{
        account_storage::meta::{StorableAccountsWithHashesAndWriteVersions, StoredMeta},
        accounts_db::INCLUDE_SLOT_IN_HASH_TESTS,
        append_vec::{
            test_utils::{create_test_account, get_append_vec_path},
            AppendVec,
        },
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        hash::Hash,
    },
    std::{
        sync::{Arc, Mutex},
        thread::{sleep, spawn},
        time::Duration,
    },
    test::Bencher,
};

/// Copy the account metadata, account and hash to the internal buffer.
/// Return the starting offset of the account metadata.
/// After the account is appended, the internal `current_len` is updated.
fn append_account(
    vec: &AppendVec,
    storage_meta: StoredMeta,
    account: &AccountSharedData,
    hash: Hash,
) -> Option<usize> {
    let slot_ignored = Slot::MAX;
    let accounts = [(&storage_meta.pubkey, account)];
    let slice = &accounts[..];
    let accounts = (slot_ignored, slice, INCLUDE_SLOT_IN_HASH_TESTS);
    let storable_accounts =
        StorableAccountsWithHashesAndWriteVersions::new_with_hashes_and_write_versions(
            &accounts,
            vec![&hash],
            vec![storage_meta.write_version_obsolete],
        );
    let res = vec.append_accounts(&storable_accounts, 0);
    res.and_then(|res| res.first().cloned())
}

#[bench]
fn append_vec_append(bencher: &mut Bencher) {
    let path = get_append_vec_path("bench_append");
    let vec = AppendVec::new(&path.path, true, 64 * 1024);
    bencher.iter(|| {
        let (meta, account) = create_test_account(0);
        if append_account(&vec, meta, &account, Hash::default()).is_none() {
            vec.reset();
        }
    });
}

fn add_test_accounts(vec: &AppendVec, size: usize) -> Vec<(usize, usize)> {
    (0..size)
        .filter_map(|sample| {
            let (meta, account) = create_test_account(sample);
            append_account(vec, meta, &account, Hash::default()).map(|pos| (sample, pos))
        })
        .collect()
}

#[bench]
fn append_vec_sequential_read(bencher: &mut Bencher) {
    let path = get_append_vec_path("seq_read");
    let vec = AppendVec::new(&path.path, true, 64 * 1024);
    let size = 1_000;
    let mut indexes = add_test_accounts(&vec, size);
    bencher.iter(|| {
        let (sample, pos) = indexes.pop().unwrap();
        println!("reading pos {sample} {pos}");
        let (account, _next) = vec.get_account(pos).unwrap();
        let (_meta, test) = create_test_account(sample);
        assert_eq!(account.data(), test.data());
        indexes.push((sample, pos));
    });
}
#[bench]
fn append_vec_random_read(bencher: &mut Bencher) {
    let path = get_append_vec_path("random_read");
    let vec = AppendVec::new(&path.path, true, 64 * 1024);
    let size = 1_000;
    let indexes = add_test_accounts(&vec, size);
    bencher.iter(|| {
        let random_index: usize = thread_rng().gen_range(0, indexes.len());
        let (sample, pos) = &indexes[random_index];
        let (account, _next) = vec.get_account(*pos).unwrap();
        let (_meta, test) = create_test_account(*sample);
        assert_eq!(account.data(), test.data());
    });
}

#[bench]
fn append_vec_concurrent_append_read(bencher: &mut Bencher) {
    let path = get_append_vec_path("concurrent_read");
    let vec = Arc::new(AppendVec::new(&path.path, true, 1024 * 1024));
    let vec1 = vec.clone();
    let indexes: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(vec![]));
    let indexes1 = indexes.clone();
    spawn(move || loop {
        let sample = indexes1.lock().unwrap().len();
        let (meta, account) = create_test_account(sample);
        if let Some(pos) = append_account(&vec1, meta, &account, Hash::default()) {
            indexes1.lock().unwrap().push((sample, pos))
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
        let (sample, pos) = *indexes.lock().unwrap().get(random_index).unwrap();
        let (account, _next) = vec.get_account(pos).unwrap();
        let (_meta, test) = create_test_account(sample);
        assert_eq!(account.data(), test.data());
    });
}

#[bench]
fn append_vec_concurrent_read_append(bencher: &mut Bencher) {
    let path = get_append_vec_path("concurrent_read");
    let vec = Arc::new(AppendVec::new(&path.path, true, 1024 * 1024));
    let vec1 = vec.clone();
    let indexes: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(vec![]));
    let indexes1 = indexes.clone();
    spawn(move || loop {
        let len = indexes1.lock().unwrap().len();
        if len == 0 {
            continue;
        }
        let random_index: usize = thread_rng().gen_range(0, len + 1);
        let (sample, pos) = *indexes1.lock().unwrap().get(random_index % len).unwrap();
        let (account, _next) = vec1.get_account(pos).unwrap();
        let (_meta, test) = create_test_account(sample);
        assert_eq!(account.data(), test.data());
    });
    bencher.iter(|| {
        let sample: usize = thread_rng().gen_range(0, 256);
        let (meta, account) = create_test_account(sample);
        if let Some(pos) = append_account(&vec, meta, &account, Hash::default()) {
            indexes.lock().unwrap().push((sample, pos))
        }
    });
}
