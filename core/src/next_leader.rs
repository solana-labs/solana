use {
    solana_gossip::{
        cluster_info::ClusterInfo, legacy_contact_info::LegacyContactInfo as ContactInfo,
    },
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{clock::FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET, pubkey::Pubkey},
    std::{net::SocketAddr, sync::RwLock},
};

pub(crate) fn next_leader_tpu(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, SocketAddr)> {
    next_leader_x(cluster_info, poh_recorder, |leader| leader.tpu)
}

pub(crate) fn next_leader_tpu_forwards(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, SocketAddr)> {
    next_leader_x(cluster_info, poh_recorder, |leader| leader.tpu_forwards)
}

pub(crate) fn next_leader_tpu_vote(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, SocketAddr)> {
    next_leader_x(cluster_info, poh_recorder, |leader| leader.tpu_vote)
}

fn next_leader_x<F>(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
    port_selector: F,
) -> Option<(Pubkey, SocketAddr)>
where
    F: FnOnce(&ContactInfo) -> SocketAddr,
{
    let leader_pubkey = poh_recorder
        .read()
        .unwrap()
        .leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET);
    if let Some(leader_pubkey) = leader_pubkey {
        cluster_info
            .lookup_contact_info(&leader_pubkey, port_selector)
            .map(|addr| (leader_pubkey, addr))
    } else {
        None
    }
}
