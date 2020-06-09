#![feature(test)]

extern crate test;
use bincode::deserialize;
use solana_sdk::short_vec::ShortVec;
use test::Bencher;

// Return a ShortVec with 127 bytes
fn create_encoded_short_vec() -> Vec<u8> {
    let mut bytes = vec![127];
    bytes.extend_from_slice(&[0u8; 127]);
    bytes
}

// Return a Vec with 127 bytes
fn create_encoded_vec() -> Vec<u8> {
    let mut bytes = vec![127, 0, 0, 0, 0, 0, 0, 0];
    bytes.extend_from_slice(&[0u8; 127]);
    bytes
}

#[bench]
fn bench_short_vec(b: &mut Bencher) {
    b.iter(|| {
        let bytes = test::black_box(create_encoded_short_vec());
        deserialize::<ShortVec<u8>>(&bytes).unwrap();
    });
}

#[bench]
fn bench_vec(b: &mut Bencher) {
    b.iter(|| {
        let bytes = test::black_box(create_encoded_vec());
        deserialize::<Vec<u8>>(&bytes).unwrap();
    });
}
