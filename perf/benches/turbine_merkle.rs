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

fn bench_merkle_create_tree_from_leaves(b: &mut Bencher, pkt_count: usize) {
    let packets = create_random_packets(pkt_count);
    b.iter(|| {
        let leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash(&[p]))
            .collect();
        let _tree = TurbineMerkleTree::new_from_leaves(&leaves);
    });
}

#[bench]
fn bench_merkle_create_tree_from_leaves_64(b: &mut Bencher) {
    bench_merkle_create_tree_from_leaves(b, 64);
}

#[bench]
fn bench_merkle_create_tree_from_leaves_96(b: &mut Bencher) {
    bench_merkle_create_tree_from_leaves(b, 64);
}

#[bench]
fn bench_merkle_create_tree_from_leaves_128(b: &mut Bencher) {
    bench_merkle_create_tree_from_leaves(b, 64);
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

fn bench_merkle_create_tree_from_bufs(b: &mut Bencher, pkt_count: usize) {
    let packets = create_random_packets(pkt_count);
    b.iter(|| {
        let _tree = TurbineMerkleTree::new_from_bufs(&packets);
    });
}

fn bench_merkle_create_tree_from_bufs_vec_par(b: &mut Bencher, pkt_count: usize, chunk: usize) {
    let packets = create_random_packets(pkt_count);
    let packets_buf_vec: Vec<_> = packets.iter().map(|p| vec![&p[..100], &p[100..]]).collect();
    b.iter(|| {
        let _tree = TurbineMerkleTree::new_from_bufs_vec_par(&packets_buf_vec, chunk);
    });
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_64(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs(b, 64);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_128(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs(b, 128);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_128_64(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 128, 64);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_128_32(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 128, 32);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_128_16(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 128, 16);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_128_8(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 128, 8);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_64_32(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 64, 32);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_64_16(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 64, 16);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_64_8(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 64, 8);
}
