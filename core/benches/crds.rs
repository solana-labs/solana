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
    let now = CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS + CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS / 1000;
    std::iter::repeat_with(|| (CrdsValue::new_rand(&mut rng), rng.gen_range(0, now)))
        .take(50_000)
        .for_each(|(v, ts)| assert!(crds.insert(v, ts).is_ok()));
    let mut timeouts = HashMap::new();
    timeouts.insert(Pubkey::default(), CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS);
    bencher.iter(|| {
        let out = crds.find_old_labels(&thread_pool, now, &timeouts);
        assert!(out.len() > 10);
        assert!(out.len() < 250);
        out
    });
}
