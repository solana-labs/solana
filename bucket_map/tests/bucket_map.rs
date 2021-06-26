use rayon::prelude::*;
use solana_bucket_map::bucket_map::BucketMap;
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
#[ignore]
fn bucket_map_test_mt() {
    let threads = 4096;
    let items = 4096;
    let tmpdir1 = std::env::temp_dir().join("bucket_map_test_mt");
    let tmpdir2 = PathBuf::from("/mnt/data/aeyakovenko").join("bucket_map_test_mt");
    let drives = Arc::new(vec![tmpdir1, tmpdir2]);
    for tmpdir in drives.iter() {
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
    }
    let index = BucketMap::new(12, drives.clone());
    (0..threads).into_iter().into_par_iter().for_each(|_| {
        let key = Pubkey::new_unique();
        index.update(&key, |_| Some(vec![0u64]));
    });
    let mut timer = Measure::start("bucket_map_test");
    (0..threads).into_iter().into_par_iter().for_each(|_| {
        for _ in 0..items {
            let key = Pubkey::new_unique();
            let ix: u64 = index.bucket_ix(&key) as u64;
            index.update(&key, |_| Some(vec![ix]));
            assert_eq!(index.read_value(&key), Some(vec![ix]));
        }
    });
    timer.stop();
    println!("time: {}ns per item", timer.as_ns() / (threads * items));
    let mut total = 0;
    for tmpdir in drives.iter() {
        let folder_size = fs_extra::dir::get_size(tmpdir).unwrap();
        total += folder_size;
        std::fs::remove_dir_all(tmpdir).unwrap();
    }
    println!("overhead: {}", total / (threads * items));
}
