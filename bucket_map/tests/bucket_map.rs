use rayon::prelude::*;
use solana_bucket_map::bucket_map::BucketMap;
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn bucket_map_test_mt() {
    let threads = 256;
    let items = 256;
    let tmpdir1 = std::env::temp_dir().join("bucket_map_test_mt");
    let tmpdir2 = PathBuf::from("/mnt/data/aeyakovenko").join("bucket_map_test_mt");
    let drives = Arc::new(vec![tmpdir1, tmpdir2]);
    for tmpdir in drives.iter() {
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
    }
    let index = BucketMap::new(9, drives.clone());
    (0..threads).into_iter().into_par_iter().for_each(|_| {
        let key = Pubkey::new_unique();
        index.update(&key, |_| Some(vec![0u64]));
    });
    let mut timer = Measure::start("bucket_map_test");
    (0..threads).into_iter().into_par_iter().for_each(|_| {
        for _ in 0..items {
            let key = Pubkey::new_unique();
            index.update(&key, |_| Some(vec![0u64]));
        }
    });
    timer.stop();
    println!("time: {}ns per item", timer.as_ns() / (threads * items));
    for tmpdir in drives.iter() {
        std::fs::remove_dir_all(tmpdir).unwrap();
    }
}
