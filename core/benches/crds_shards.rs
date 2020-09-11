#![feature(test)]

extern crate test;

use rand::{thread_rng, Rng};
use solana_core::contact_info::ContactInfo;
use solana_core::crds::VersionedCrdsValue;
use solana_core::crds_shards::CrdsShards;
use solana_core::crds_value::{CrdsData, CrdsValue};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::timestamp;
use test::Bencher;

const CRDS_SHARDS_BITS: u32 = 8;

fn new_test_crds_value() -> VersionedCrdsValue {
    let data = CrdsData::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp()));
    VersionedCrdsValue::new(timestamp(), CrdsValue::new_unsigned(data))
}

fn bench_crds_shards_find(bencher: &mut Bencher, num_values: usize, mask_bits: u32) {
    let values: Vec<VersionedCrdsValue> = std::iter::repeat_with(new_test_crds_value)
        .take(num_values)
        .collect();
    let mut shards = CrdsShards::new(CRDS_SHARDS_BITS);
    for (index, value) in values.iter().enumerate() {
        assert!(shards.insert(index, value));
    }
    let mut rng = thread_rng();
    bencher.iter(|| {
        let mask = rng.gen();
        let _hits = shards.find(mask, mask_bits).count();
    });
}

#[bench]
fn bench_crds_shards_find_0(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 0);
}

#[bench]
fn bench_crds_shards_find_1(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 1);
}

#[bench]
fn bench_crds_shards_find_3(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 3);
}

#[bench]
fn bench_crds_shards_find_5(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 5);
}

#[bench]
fn bench_crds_shards_find_7(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 7);
}

#[bench]
fn bench_crds_shards_find_8(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 8);
}

#[bench]
fn bench_crds_shards_find_9(bencher: &mut Bencher) {
    bench_crds_shards_find(bencher, 100_000, 9);
}
