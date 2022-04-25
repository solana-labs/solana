#![feature(test)]

extern crate test;

use {
    solana_perf::turbine_merkle::{TurbineMerkleHash, TurbineMerkleTree},
    test::Bencher,
};

fn create_random_packets(npackets: usize) -> Vec<Vec<u8>> {
    let mut packets = Vec::default();
    for _i in 0..npackets {
        let buf: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
        packets.push(buf);
    }
    packets
}

#[bench]
fn bench_merkle_create_tree_64(b: &mut Bencher) {
    let packets = create_random_packets(64);

    b.iter(|| {
        let leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash(&[p]))
            .collect();
        let _tree = TurbineMerkleTree::new_from_leaves(&leaves);
    });
}

#[bench]
fn bench_merkle_create_tree_96(b: &mut Bencher) {
    let packets = create_random_packets(96);

    b.iter(|| {
        let mut leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash(&[p]))
            .collect();
        leaves.extend(std::iter::repeat(TurbineMerkleHash::default()).take(32));
        let _tree = TurbineMerkleTree::new_from_leaves(&leaves);
    });
}

#[bench]
fn bench_merkle_create_tree_128(b: &mut Bencher) {
    let packets = create_random_packets(128);

    b.iter(|| {
        let leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash(&[p]))
            .collect();
        let _tree = TurbineMerkleTree::new_from_leaves(&leaves);
    });
}

#[bench]
fn bench_merkle_create_proofs_96_single(b: &mut Bencher) {
    let packets = create_random_packets(96);

    let mut leaves: Vec<TurbineMerkleHash> = packets
        .iter()
        .map(|p| TurbineMerkleHash::hash(&[p]))
        .collect();
    leaves.extend(std::iter::repeat(TurbineMerkleHash::default()).take(32));
    let tree = TurbineMerkleTree::new_from_leaves(&leaves);

    b.iter(|| {
        let _proof = tree.prove(7);
    });
}

#[bench]
fn bench_merkle_create_proofs_96_batch(b: &mut Bencher) {
    let packets = create_random_packets(96);

    let mut leaves: Vec<TurbineMerkleHash> = packets
        .iter()
        .map(|p| TurbineMerkleHash::hash(&[p]))
        .collect();
    leaves.extend(std::iter::repeat(TurbineMerkleHash::default()).take(32));
    let tree = TurbineMerkleTree::new_from_leaves(&leaves);

    b.iter(|| {
        let _proofs: Vec<_> = (0..96).map(|i| tree.prove(i)).collect();
    });
}

#[bench]
fn bench_merkle_verify_proofs_96_single(b: &mut Bencher) {
    let packets = create_random_packets(96);

    let mut leaves: Vec<TurbineMerkleHash> = packets
        .iter()
        .map(|p| TurbineMerkleHash::hash(&[p]))
        .collect();
    leaves.extend(std::iter::repeat(TurbineMerkleHash::default()).take(32));
    let tree = TurbineMerkleTree::new_from_leaves(&leaves);
    let proof = tree.prove(7);

    b.iter(|| {
        proof.verify(&tree.root(), &tree.node(7), 7);
    });
}

#[bench]
fn bench_merkle_verify_proofs_96_batch(b: &mut Bencher) {
    let packets = create_random_packets(96);

    let mut leaves: Vec<TurbineMerkleHash> = packets
        .iter()
        .map(|p| TurbineMerkleHash::hash(&[p]))
        .collect();
    leaves.extend(std::iter::repeat(TurbineMerkleHash::default()).take(32));
    let tree = TurbineMerkleTree::new_from_leaves(&leaves);
    let proofs: Vec<_> = (0..96).map(|i| tree.prove(i)).collect();

    b.iter(|| {
        let _results: Vec<_> = (0..96)
            .map(|i| proofs[i].verify(&tree.root(), &tree.node(i), i))
            .collect();
    });
}
