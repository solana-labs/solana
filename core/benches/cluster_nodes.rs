#![feature(test)]

extern crate test;

use {
    rand::{seq::SliceRandom, Rng},
    solana_core::{
        cluster_nodes::{make_test_cluster, new_cluster_nodes, ClusterNodes},
        retransmit_stage::RetransmitStage,
    },
    solana_gossip::contact_info::ContactInfo,
    solana_sdk::{clock::Slot, hash::hashv, pubkey::Pubkey, signature::Signature},
    test::Bencher,
};

const NUM_SIMULATED_SHREDS: usize = 4;

fn make_cluster_nodes<R: Rng>(
    rng: &mut R,
    unstaked_ratio: Option<(u32, u32)>,
) -> (Vec<ContactInfo>, ClusterNodes<RetransmitStage>) {
    let (nodes, stakes, cluster_info) = make_test_cluster(rng, 5_000, unstaked_ratio);
    let cluster_nodes = new_cluster_nodes::<RetransmitStage>(&cluster_info, &stakes);
    (nodes, cluster_nodes)
}

fn get_retransmit_peers_deterministic(
    cluster_nodes: &ClusterNodes<RetransmitStage>,
    slot: &Slot,
    slot_leader: &Pubkey,
    num_simulated_shreds: usize,
) {
    for i in 0..num_simulated_shreds {
        // see Shred::seed
        let shred_seed = hashv(&[
            &slot.to_le_bytes(),
            &(i as u32).to_le_bytes(),
            &slot_leader.to_bytes(),
        ])
        .to_bytes();

        let (_neighbors, _children) = cluster_nodes.get_retransmit_peers_deterministic(
            shred_seed,
            solana_gossip::cluster_info::DATA_PLANE_FANOUT,
            *slot_leader,
        );
    }
}

fn get_retransmit_peers_compat(
    cluster_nodes: &ClusterNodes<RetransmitStage>,
    slot_leader: &Pubkey,
    signatures: &[Signature],
) {
    for signature in signatures.iter() {
        // see Shred::seed
        let signature = signature.as_ref();
        let offset = signature.len().checked_sub(32).unwrap();
        let shred_seed = signature[offset..].try_into().unwrap();

        let (_neighbors, _children) = cluster_nodes.get_retransmit_peers_compat(
            shred_seed,
            solana_gossip::cluster_info::DATA_PLANE_FANOUT,
            *slot_leader,
        );
    }
}

fn get_retransmit_peers_deterministic_wrapper(b: &mut Bencher, unstaked_ratio: Option<(u32, u32)>) {
    let mut rng = rand::thread_rng();
    let (nodes, cluster_nodes) = make_cluster_nodes(&mut rng, unstaked_ratio);
    let slot_leader = nodes[1..].choose(&mut rng).unwrap().id;
    let slot = rand::random::<u64>();
    b.iter(|| {
        get_retransmit_peers_deterministic(
            &cluster_nodes,
            &slot,
            &slot_leader,
            NUM_SIMULATED_SHREDS,
        )
    });
}

fn get_retransmit_peers_compat_wrapper(b: &mut Bencher, unstaked_ratio: Option<(u32, u32)>) {
    let mut rng = rand::thread_rng();
    let (nodes, cluster_nodes) = make_cluster_nodes(&mut rng, unstaked_ratio);
    let slot_leader = nodes[1..].choose(&mut rng).unwrap().id;
    let signatures: Vec<_> = std::iter::repeat_with(Signature::new_unique)
        .take(NUM_SIMULATED_SHREDS)
        .collect();
    b.iter(|| get_retransmit_peers_compat(&cluster_nodes, &slot_leader, &signatures));
}

#[bench]
fn bench_get_retransmit_peers_deterministic_unstaked_ratio_1_2(b: &mut Bencher) {
    get_retransmit_peers_deterministic_wrapper(b, Some((1, 2)));
}

#[bench]
fn bench_get_retransmit_peers_compat_unstaked_ratio_1_2(b: &mut Bencher) {
    get_retransmit_peers_compat_wrapper(b, Some((1, 2)));
}

#[bench]
fn bench_get_retransmit_peers_deterministic_unstaked_ratio_1_32(b: &mut Bencher) {
    get_retransmit_peers_deterministic_wrapper(b, Some((1, 32)));
}

#[bench]
fn bench_get_retransmit_peers_compat_unstaked_ratio_1_32(b: &mut Bencher) {
    get_retransmit_peers_compat_wrapper(b, Some((1, 32)));
}
