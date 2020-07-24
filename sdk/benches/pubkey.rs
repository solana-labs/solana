#![feature(test)]

extern crate test;
use solana_sdk::pubkey::Pubkey;
use test::Bencher;

#[bench]
fn bench_pubkey_create_program_address(b: &mut Bencher) {
    let program_id = Pubkey::new_rand();
    b.iter(|| {
        Pubkey::create_program_address(&[b"todo"], &program_id).unwrap();
    });
}
