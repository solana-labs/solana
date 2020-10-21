#![feature(test)]

extern crate test;

use rand::{thread_rng, Rng};
use rayon::ThreadPoolBuilder;
use solana_core::crds::Crds;
use solana_core::crds_gossip_pull::CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
use solana_core::crds_value::CrdsValue;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use test::Bencher;

#[bench]
fn bench_find_old_labels(bencher: &mut Bencher) {
    let thread_pool = ThreadPoolBuilder::new().build().unwrap();
    let mut rng = thread_rng();
    let mut crds = Crds::default();
    let mut num_inserts = 0;
    let now = CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS + CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS / 1000;
    for _ in 0..50_000 {
        if crds
            .insert(CrdsValue::new_rand(&mut rng), rng.gen_range(0, now))
            .is_ok()
        {
            num_inserts += 1;
        }
    }
    assert_eq!(num_inserts, 50_000);
    let mut timeouts = HashMap::new();
    timeouts.insert(Pubkey::default(), CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS);
    let mut fail = 0;
    bencher.iter(|| {
        let out = crds.find_old_labels(&thread_pool, now, &timeouts).len();
        if out < 10 || out > 250 {
            fail += 1;
        }
        out
    });
    assert_eq!(fail, 0);
}
