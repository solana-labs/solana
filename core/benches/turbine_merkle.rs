#![feature(test)]

extern crate test;

use {
    rand::Rng,
    solana_core::turbine_merkle::{TurbineMerkleHash, TurbineMerkleProof, TurbineMerkleTree},
    test::Bencher,
};

fn create_random_packets() -> Vec<Vec<u8>> {
    let rng = rand::thread_rng();
    let mut packets = Vec::default();
    for i in 0..16 {
        let buf: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
        packets.push(buf);
    }
    packets
}

#[bench]
fn bench_merkle_1(b: &mut Bencher) {
    let packets = create_random_packets();

    b.iter(|| {
        let leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash(&[&p]))
            .collect();
        //let tree = gen_tree(&leaves);
    });
}
