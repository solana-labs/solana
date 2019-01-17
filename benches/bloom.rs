#![feature(test)]

extern crate test;

use solana::bloom::{Bloom, BloomHashIndex};
use solana_sdk::hash::{hash, Hash};
use solana_sdk::signature::Signature;
//use std::collections::HashSet;
use hashbrown::HashSet;
use test::Bencher;

#[bench]
fn bench_sigs_bloom(bencher: &mut Bencher) {
    // 1M TPS * 1s (length of block in sigs) == 1M items in filter
    // 1.0E-8 false positive rate
    // https://hur.st/bloomfilter/?n=1000000&p=1.0E-8&m=&k=
    let last_id = hash(Hash::default().as_ref());
    eprintln!("last_id = {:?}", last_id);
    let keys = (0..27)
        .into_iter()
        .map(|i| last_id.hash_at_index(i))
        .collect();
    let mut sigs: Bloom<Signature> = Bloom::new(38_340_234, keys);

    let mut id = last_id;
    let mut falses = 0;
    let mut iterations = 0;
    bencher.iter(|| {
        id = hash(id.as_ref());
        let mut sigbytes = Vec::from(id.as_ref());
        id = hash(id.as_ref());
        sigbytes.extend(id.as_ref());

        let sig = Signature::new(&sigbytes);
        if sigs.contains(&sig) {
            falses += 1;
        }
        sigs.add(&sig);
        sigs.contains(&sig);
        iterations += 1;
    });
    eprintln!("bloom falses: {}/{}", falses, iterations);
}

#[bench]
fn bench_sigs_hashmap(bencher: &mut Bencher) {
    // same structure as above, new
    let last_id = hash(Hash::default().as_ref());
    eprintln!("last_id = {:?}", last_id);
    let mut sigs: HashSet<Signature> = HashSet::new();

    let mut id = last_id;
    let mut falses = 0;
    let mut iterations = 0;
    bencher.iter(|| {
        id = hash(id.as_ref());
        let mut sigbytes = Vec::from(id.as_ref());
        id = hash(id.as_ref());
        sigbytes.extend(id.as_ref());

        let sig = Signature::new(&sigbytes);
        if sigs.contains(&sig) {
            falses += 1;
        }
        sigs.insert(sig);
        sigs.contains(&sig);
        iterations += 1;
    });
    eprintln!("hashset falses: {}/{}", falses, iterations);
}
