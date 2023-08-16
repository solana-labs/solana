#![feature(test)]

extern crate test;

use {
    rand::{seq::SliceRandom, Rng},
    solana_gossip::legacy_contact_info::LegacyContactInfo as ContactInfo,
    solana_ledger::shred::{Shred, ShredFlags},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    solana_turbine::{
        cluster_nodes::{make_test_cluster, new_cluster_nodes, ClusterNodes},
        retransmit_stage::RetransmitStage,
    },
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
    slot: Slot,
    slot_leader: &Pubkey,
    num_simulated_shreds: usize,
) {
    let parent_offset = u16::from(slot != 0);
    for i in 0..num_simulated_shreds {
        let index = i as u32;
        let shred = Shred::new_from_data(
            slot,
            index,
            parent_offset,
            &[],
            ShredFlags::empty(),
            0,
            0,
            0,
        );
        let _retransmit_peers =
            cluster_nodes.get_retransmit_peers(slot_leader, &shred.id(), /*fanout:*/ 200);
    }
}

fn get_retransmit_peers_deterministic_wrapper(b: &mut Bencher, unstaked_ratio: Option<(u32, u32)>) {
    let mut rng = rand::thread_rng();
    let (nodes, cluster_nodes) = make_cluster_nodes(&mut rng, unstaked_ratio);
    let slot_leader = *nodes[1..].choose(&mut rng).unwrap().pubkey();
    let slot = rand::random::<u64>();
    b.iter(|| {
        get_retransmit_peers_deterministic(&cluster_nodes, slot, &slot_leader, NUM_SIMULATED_SHREDS)
    });
}

#[bench]
fn bench_get_retransmit_peers_deterministic_unstaked_ratio_1_2(b: &mut Bencher) {
    get_retransmit_peers_deterministic_wrapper(b, Some((1, 2)));
}

#[bench]
fn bench_get_retransmit_peers_deterministic_unstaked_ratio_1_32(b: &mut Bencher) {
    get_retransmit_peers_deterministic_wrapper(b, Some((1, 32)));
}
