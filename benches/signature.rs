#![feature(test)]
extern crate solana;
extern crate solana_sdk;
extern crate test;

use solana::signature::GenKeys;
use test::Bencher;

#[bench]
fn bench_gen_keys(b: &mut Bencher) {
    let mut rnd = GenKeys::new([0u8; 32]);
    b.iter(|| rnd.gen_n_keypairs(1000));
}
