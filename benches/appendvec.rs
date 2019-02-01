#![cfg_attr(feature = "unstable", feature(test))]
extern crate rand;
extern crate test;

use rand::{thread_rng, Rng};
use solana_runtime::appendvec::AppendVec;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::spawn;
use test::Bencher;

const START_SIZE: u64 = 4 * 1024 * 1024;
const INC_SIZE: u64 = 1 * 1024 * 1024;

#[bench]
fn appendvec_atomic_append(bencher: &mut Bencher) {
    let path = Path::new("/media/nvme0/bench/bench_append");
    let mut vec = AppendVec::<AtomicUsize>::new(path, true, START_SIZE, INC_SIZE);
    bencher.iter(|| {
        if vec.append(AtomicUsize::new(0)).is_none() {
            assert!(vec.grow_file().is_ok());
            assert!(vec.append(AtomicUsize::new(0)).is_some());
        }
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn appendvec_atomic_random_access(bencher: &mut Bencher) {
    let path = Path::new("/media/nvme0/bench/bench_ra");
    let mut vec = AppendVec::<AtomicUsize>::new(path, true, START_SIZE, INC_SIZE);
    let size = 10_000_000;
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
fn appendvec_atomic_random_change(bencher: &mut Bencher) {
    let path = Path::new("/media/nvme0/bench/bench_rax");
    let mut vec = AppendVec::<AtomicUsize>::new(path, true, START_SIZE, INC_SIZE);
    let size = 10_000_000;
    for _ in 0..size {
        if vec.append(AtomicUsize::new(0)).is_none() {
            assert!(vec.grow_file().is_ok());
            assert!(vec.append(AtomicUsize::new(0)).is_some());
        }
    }
    bencher.iter(|| {
        let index = thread_rng().gen_range(0, size as u64);
        let atomic1 = vec.get(index * std::mem::size_of::<AtomicUsize>() as u64);
        let current1 = atomic1.load(Ordering::Relaxed);
        let next = current1 + 1;
        atomic1.store(next, Ordering::Relaxed);
        let atomic2 = vec.get(index * std::mem::size_of::<AtomicUsize>() as u64);
        let current2 = atomic2.load(Ordering::Relaxed);
        assert_eq!(current2, next);
    });
    std::fs::remove_file(path).unwrap();
}

#[bench]
fn appendvec_atomic_random_read(bencher: &mut Bencher) {
    let path = Path::new("/media/nvme0/bench/bench_read");
    let mut vec = AppendVec::<AtomicUsize>::new(path, true, START_SIZE, INC_SIZE);
    let size = 100_000_000;
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
fn appendvec_concurrent_lock_append(bencher: &mut Bencher) {
    let path = Path::new("bench_lock_append");
    let vec = Arc::new(RwLock::new(AppendVec::<AtomicUsize>::new(
        path, true, START_SIZE, INC_SIZE,
    )));
    let vec1 = vec.clone();
    let size = 100_000_000;
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
fn appendvec_concurrent_get_append(bencher: &mut Bencher) {
    let path = Path::new("bench_get_append");
    let vec = Arc::new(RwLock::new(AppendVec::<AtomicUsize>::new(
        path, true, START_SIZE, INC_SIZE,
    )));
    let vec1 = vec.clone();
    let size = 100_000_000;
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
