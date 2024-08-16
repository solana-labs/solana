use {
    itertools::Itertools,
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{clock::FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET, pubkey::Pubkey},
    std::{net::SocketAddr, sync::RwLock},
};

/// Returns a list of tpu vote sockets for the leaders of the next N fanout
/// slots. Leaders and sockets are deduped.
pub(crate) fn upcoming_leader_tpu_vote_sockets(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
    fanout_slots: u64,
) -> Vec<SocketAddr> {
    let upcoming_leaders = {
        let poh_recorder = poh_recorder.read().unwrap();
        (1..=fanout_slots)
            .filter_map(|n_slots| poh_recorder.leader_after_n_slots(n_slots))
            .collect_vec()
    };

    upcoming_leaders
        .into_iter()
        .dedup()
        .filter_map(|leader_pubkey| {
            cluster_info
                .lookup_contact_info(&leader_pubkey, ContactInfo::tpu_vote)?
                .ok()
        })
        // dedup again since leaders could potentially share the same tpu vote socket
        .dedup()
        .collect()
}

pub(crate) fn next_leader_tpu_vote(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, SocketAddr)> {
    next_leader(cluster_info, poh_recorder, ContactInfo::tpu_vote)
}

pub(crate) fn next_leader<F, E>(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
    port_selector: F,
) -> Option<(Pubkey, SocketAddr)>
where
    F: FnOnce(&ContactInfo) -> Result<SocketAddr, E>,
{
    let leader_pubkey = poh_recorder
        .read()
        .unwrap()
        .leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET)?;
    cluster_info
        .lookup_contact_info(&leader_pubkey, port_selector)?
        .map(|addr| (leader_pubkey, addr))
        .ok()
}
