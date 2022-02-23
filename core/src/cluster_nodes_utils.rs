use {
    rand::{seq::SliceRandom, Rng},
    solana_gossip::{
        cluster_info::ClusterInfo,
        contact_info::ContactInfo,
        crds::GossipRoute,
        crds_value::{CrdsData, CrdsValue},
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair, timing::timestamp},
    solana_streamer::socket::SocketAddrSpace,
    std::{collections::HashMap, iter::repeat_with, sync::Arc},
};

pub fn make_cluster<R: Rng>(
    rng: &mut R,
    num_nodes: usize,
) -> (
    Vec<ContactInfo>,
    HashMap<Pubkey, u64>, // stakes
    ClusterInfo,
) {
    let mut nodes: Vec<_> = repeat_with(|| ContactInfo::new_rand(rng, None))
        .take(num_nodes)
        .collect();
    nodes.shuffle(rng);
    let this_node = nodes[0].clone();
    let mut stakes: HashMap<Pubkey, u64> = nodes
        .iter()
        .filter_map(|node| {
            if rng.gen_ratio(1, 7) {
                None // No stake for some of the nodes.
            } else {
                Some((node.id, rng.gen_range(0, 20)))
            }
        })
        .collect();
    // Add some staked nodes with no contact-info.
    stakes.extend(repeat_with(|| (Pubkey::new_unique(), rng.gen_range(0, 20))).take(100));
    let cluster_info = ClusterInfo::new(
        this_node,
        Arc::new(Keypair::new()),
        SocketAddrSpace::Unspecified,
    );
    {
        let now = timestamp();
        let mut gossip_crds = cluster_info.gossip.crds.write().unwrap();
        // First node is pushed to crds table by ClusterInfo constructor.
        for node in nodes.iter().skip(1) {
            let node = CrdsData::ContactInfo(node.clone());
            let node = CrdsValue::new_unsigned(node);
            assert_eq!(
                gossip_crds.insert(node, now, GossipRoute::LocalMessage),
                Ok(())
            );
        }
    }
    (nodes, stakes, cluster_info)
}
