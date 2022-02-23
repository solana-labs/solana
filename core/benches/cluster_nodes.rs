#![feature(test)]

extern crate test;

use {
    rand::{seq::SliceRandom, Rng},
    solana_core::{
        cluster_nodes::{new_cluster_nodes, ClusterNodes},
        cluster_nodes_utils::make_cluster,
        retransmit_stage::RetransmitStage,
    },
    solana_gossip::contact_info::ContactInfo,
    test::Bencher,
};

fn make_cluster_nodes<R: Rng>(rng: &mut R) -> (Vec<ContactInfo>, ClusterNodes<RetransmitStage>) {
    let (nodes, stakes, cluster_info) = make_cluster(rng, 5_000);
    let cluster_nodes = new_cluster_nodes::<RetransmitStage>(&cluster_info, &stakes);
    (nodes, cluster_nodes)
}

#[bench]
fn bench_get_retransmit_peers_deterministic(b: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let (nodes, cluster_nodes) = make_cluster_nodes(&mut rng);

    let mut shred_seed = [0u8; 32];
    rng.fill(&mut shred_seed[..]);

    let slot_leader = nodes[1..].choose(&mut rng).unwrap().id;

    b.iter(|| {
        let (_neighbors, _children) = cluster_nodes.get_retransmit_peers_deterministic(
            shred_seed,
            solana_gossip::cluster_info::DATA_PLANE_FANOUT,
            slot_leader,
        );
    });
}

#[bench]
fn bench_get_retransmit_peers_compat(b: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let (nodes, cluster_nodes) = make_cluster_nodes(&mut rng);

    let mut shred_seed = [0u8; 32];
    rng.fill(&mut shred_seed[..]);

    let slot_leader = nodes[1..].choose(&mut rng).unwrap().id;

    b.iter(|| {
        let (_neighbors, _children) = cluster_nodes.get_retransmit_peers_compat(
            shred_seed,
            solana_gossip::cluster_info::DATA_PLANE_FANOUT,
            slot_leader,
        );
    });
}
