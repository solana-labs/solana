use {
    rayon::prelude::*,
    solana_bucket_map::bucket_map::{BucketMap, BucketMapConfig},
    solana_measure::measure::Measure,
    solana_sdk::pubkey::Pubkey,
    std::path::PathBuf,
};
#[test]
#[ignore]
fn bucket_map_test_mt() {
    let threads = 4096;
    let items = 4096;
    let tmpdir1 = std::env::temp_dir().join("bucket_map_test_mt");
    let tmpdir2 = PathBuf::from("/mnt/data/0").join("bucket_map_test_mt");
    let paths: Vec<PathBuf> = [tmpdir1, tmpdir2]
        .iter()
        .filter(|x| std::fs::create_dir_all(x).is_ok())
        .cloned()
        .collect();
    assert!(!paths.is_empty());
    let index = BucketMap::new(BucketMapConfig {
        max_buckets: 1 << 12,
        drives: Some(paths.clone()),
        ..BucketMapConfig::default()
    });
    (0..threads).into_par_iter().for_each(|_| {
        let key = Pubkey::new_unique();
        index.update(&key, |_| Some((vec![0u64], 0)));
    });
    let mut timer = Measure::start("bucket_map_test_mt");
    (0..threads).into_par_iter().for_each(|_| {
        for _ in 0..items {
            let key = Pubkey::new_unique();
            let ix: u64 = index.bucket_ix(&key) as u64;
            index.update(&key, |_| Some((vec![ix], 0)));
            assert_eq!(index.read_value(&key), Some((vec![ix], 0)));
        }
    });
    timer.stop();
    println!("time: {}ns per item", timer.as_ns() / (threads * items));
    let mut total = 0;
    for tmpdir in paths.iter() {
        let folder_size = fs_extra::dir::get_size(tmpdir).unwrap();
        total += folder_size;
        std::fs::remove_dir_all(tmpdir).unwrap();
    }
    println!("overhead: {}bytes per item", total / (threads * items));
}
