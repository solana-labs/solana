#![feature(test)]

extern crate test;

use {
    rayon::{iter::ParallelIterator, prelude::*},
    solana_perf::turbine_merkle::{TurbineMerkleHash, TurbineMerkleTree},
    solana_sdk::signature::{Keypair, Signer},
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
            .map(|p| TurbineMerkleHash::hash_leaf(&[p]))
            .collect();
        let _tree = TurbineMerkleTree::new_from_leaves(&leaves);
    });
}

#[bench]
fn bench_merkle_create_tree_from_leaves_64(b: &mut Bencher) {
    bench_merkle_create_tree_from_leaves(b, 64);
}

#[bench]
fn bench_merkle_create_proofs_96_single(b: &mut Bencher) {
    let packets = create_random_packets(96);

    let mut leaves: Vec<TurbineMerkleHash> = packets
        .iter()
        .map(|p| TurbineMerkleHash::hash_leaf(&[p]))
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
        .map(|p| TurbineMerkleHash::hash_leaf(&[p]))
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
        .map(|p| TurbineMerkleHash::hash_leaf(&[p]))
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
        .map(|p| TurbineMerkleHash::hash_leaf(&[p]))
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

#[ignore]
#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_128_32(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 128, 32);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_128_16(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 128, 16);
}

#[ignore]
#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_64_32(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 64, 32);
}

#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_64_16(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 64, 16);
}

#[ignore]
#[bench]
fn bench_merkle_create_tree_from_bufs_vec_par_64_8(b: &mut Bencher) {
    bench_merkle_create_tree_from_bufs_vec_par(b, 64, 8);
}

// Simulate work required for one batch of outgoing shreds.
#[bench]
fn bench_merkle_fec64_outgoing(b: &mut Bencher) {
    let keypair = Keypair::new();
    let packets = create_random_packets(64);
    b.iter(|| {
        let bufs_vec: Vec<_> = packets.iter().map(|p| vec![&p[..333], &p[333..]]).collect();
        let tree = TurbineMerkleTree::new_from_bufs_vec_par(&bufs_vec, 16);
        let root_hash = tree.root();
        let _signature = keypair.sign_message(root_hash.as_ref());
        let _proofs: Vec<_> = (0..64).map(|i| (i, tree.prove(i))).collect();
    });
}

// Simulate work required for one batch of incoming shreds verifying each
// shred individually.
#[bench]
fn bench_merkle_fec64_incoming_individual(b: &mut Bencher) {
    let keypair = Keypair::new();
    let packets = create_random_packets(64);
    let bufs_vec: Vec<_> = packets.iter().map(|p| vec![&p[..333], &p[333..]]).collect();
    let tree = TurbineMerkleTree::new_from_bufs_vec_par(&bufs_vec, 16);
    let root_hash = tree.root();
    let signature = keypair.sign_message(root_hash.as_ref());
    let proof_data: Vec<_> = bufs_vec
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v, root_hash, signature, tree.prove(i)))
        .collect();
    b.iter(|| {
        proof_data.par_chunks(16).for_each(|slice| {
            slice.iter().for_each(|(i, v, root_hash, sig, proof)| {
                let leaf_hash = TurbineMerkleHash::hash_leaf(v);
                assert!(proof.verify(root_hash, &leaf_hash, *i));
                assert!(sig.verify(keypair.pubkey().as_ref(), root_hash.as_ref()));
            });
        });
    });
}

// Simulate work required for one batch of incoming shreds comparing each
// root/signature pair to a verified value.
#[bench]
fn bench_merkle_fec64_incoming_saved_hash(b: &mut Bencher) {
    let keypair = Keypair::new();
    let packets = create_random_packets(64);
    let bufs_vec: Vec<_> = packets.iter().map(|p| vec![&p[..333], &p[333..]]).collect();
    let tree = TurbineMerkleTree::new_from_bufs_vec_par(&bufs_vec, 16);
    let root_hash = tree.root();
    let signature = keypair.sign_message(root_hash.as_ref());
    let proof_data: Vec<_> = bufs_vec
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v, root_hash, signature, tree.prove(i)))
        .collect();
    b.iter(|| {
        let (fec_set_root_hash, fec_set_sig) = proof_data
            .first()
            .map(|(_, _, root_hash, sig, _)| (root_hash, sig))
            .unwrap();
        assert!(fec_set_sig.verify(keypair.pubkey().as_ref(), root_hash.as_ref()));
        proof_data.par_chunks(16).for_each(|slice| {
            slice.iter().for_each(|(i, v, root_hash, sig, proof)| {
                let leaf_hash = TurbineMerkleHash::hash_leaf(v);
                assert_eq!(sig, fec_set_sig);
                assert_eq!(root_hash, fec_set_root_hash);
                assert!(proof.verify(root_hash, &leaf_hash, *i));
            });
        });
    });
}

#[bench]
fn bench_signature_fec64_outgoing(b: &mut Bencher) {
    let keypair = Keypair::new();
    let packets = create_random_packets(64);
    b.iter(|| {
        packets.par_chunks(16).for_each(|slice| {
            for p in slice {
                keypair.sign_message(p);
            }
        });
    });
}

#[bench]
fn bench_signature_fec64_incoming(b: &mut Bencher) {
    let keypair = Keypair::new();
    let packets = create_random_packets(64);
    let sig_data: Vec<_> = packets
        .iter()
        .map(|p| (p, keypair.sign_message(p)))
        .collect();
    b.iter(|| {
        sig_data.par_chunks(16).for_each(|slice| {
            for (p, s) in slice {
                assert!(s.verify(keypair.pubkey().as_ref(), p));
            }
        });
    });
}
