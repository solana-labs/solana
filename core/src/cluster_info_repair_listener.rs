use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::crds_value::EpochSlots;
use crate::result::Result;
use crate::service::Service;
use byteorder::{ByteOrder, LittleEndian};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_metrics::datapoint;
use solana_runtime::epoch_schedule::EpochSchedule;
use solana_sdk::pubkey::Pubkey;
use std::cmp::min;
use std::collections::HashMap;
use std::mem;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub const REPAIRMEN_SLEEP_MILLIS: usize = 1000;
pub const REPAIR_REDUNDANCY: usize = 3;
pub const BLOB_SEND_SLEEP_MILLIS: usize = 2;
pub const NUM_BUFFER_SLOTS: usize = 100;

pub struct ClusterInfoRepairListener {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ClusterInfoRepairListener {
    pub fn new(
        blocktree: &Arc<Blocktree>,
        exit: &Arc<AtomicBool>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        epoch_schedule: EpochSchedule,
    ) -> Self {
        let exit = exit.clone();
        let blocktree = blocktree.clone();
        let thread = Builder::new()
            .name("solana-cluster_info_repair_listener".to_string())
            .spawn(move || {
                // Maps a peer to the last timestamp and root they gossiped
                let mut peer_roots: HashMap<Pubkey, (u64, u64)> = HashMap::new();
                let _ = Self::recv_loop(
                    &blocktree,
                    &mut peer_roots,
                    exit,
                    &cluster_info,
                    epoch_schedule,
                );
            })
            .unwrap();
        Self {
            thread_hdls: vec![thread],
        }
    }

    fn recv_loop(
        blocktree: &Blocktree,
        peer_roots: &mut HashMap<Pubkey, (u64, u64)>,
        exit: Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        epoch_schedule: EpochSchedule,
    ) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let my_id = cluster_info.read().unwrap().id();
        let mut my_gossiped_root = 0;

        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let peers = cluster_info.read().unwrap().gossip_peers();
            let mut peers_needing_repairs: HashMap<Pubkey, EpochSlots> = HashMap::new();

            // Iterate through all the known nodes in the network, looking for ones that
            // need repairs
            for peer in peers {
                let last_update_ts = Self::get_last_ts(peer.id, peer_roots);
                let my_root =
                    Self::update_my_gossiped_root(&my_id, cluster_info, &mut my_gossiped_root);
                {
                    let r_cluster_info = cluster_info.read().unwrap();

                    // Update our local map with the updated peers' information
                    if let Some((peer_epoch_slots, ts)) =
                        r_cluster_info.get_epoch_state_for_node(&peer.id, last_update_ts)
                    {
                        // Following logic needs to be fast because it holds the lock
                        // preventing updates on gossip
                        peer_roots.insert(peer.id, (ts, peer_epoch_slots.root));
                        if Self::should_repair_peer(my_root, peer_epoch_slots.root, epoch_schedule)
                        {
                            // Clone out EpochSlots structure to avoid holding lock on gossip
                            peers_needing_repairs.insert(peer.id, peer_epoch_slots.clone());
                        }
                    }
                }
            }

            // After updating all the peers, send out repairs to those that need it
            let _ = Self::serve_repairs(
                &my_id,
                blocktree,
                peer_roots,
                &peers_needing_repairs,
                &socket,
                cluster_info,
                &epoch_schedule,
                &mut my_gossiped_root,
            );

            sleep(Duration::from_millis(REPAIRMEN_SLEEP_MILLIS as u64));
        }
    }

    fn serve_repairs(
        my_id: &Pubkey,
        blocktree: &Blocktree,
        peer_roots: &HashMap<Pubkey, (u64, u64)>,
        repairees: &HashMap<Pubkey, EpochSlots>,
        socket: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        epoch_schedule: &EpochSchedule,
        my_gossiped_root: &mut u64,
    ) -> Result<()> {
        for (repairee_id, repairee_epoch_slots) in repairees {
            let repairee_root = repairee_epoch_slots.root;

            let repairee_tvu = {
                let r_cluster_info = cluster_info.read().unwrap();
                let contact_info = r_cluster_info.get_contact_info_for_node(repairee_id);
                contact_info.map(|c| c.tvu)
            };

            if let Some(repairee_tvu) = repairee_tvu {
                // For every repairee, get the set of repairmen who are responsible for
                let mut eligible_repairmen =
                    Self::find_eligible_repairmen(my_id, repairee_root, peer_roots, epoch_schedule);

                Self::shuffle_repairmen(
                    &mut eligible_repairmen,
                    repairee_id,
                    repairee_epoch_slots.root,
                );

                let my_root = Self::update_my_gossiped_root(my_id, cluster_info, my_gossiped_root);

                let _ = Self::serve_repairs_to_repairee(
                    my_id,
                    my_root,
                    blocktree,
                    &repairee_epoch_slots,
                    &eligible_repairmen,
                    socket,
                    &repairee_tvu,
                );
            }
        }

        Ok(())
    }

    fn serve_repairs_to_repairee(
        my_id: &Pubkey,
        my_root: u64,
        blocktree: &Blocktree,
        repairee_epoch_slots: &EpochSlots,
        eligible_repairmen: &Vec<&Pubkey>,
        socket: &UdpSocket,
        repairee_tvu: &SocketAddr,
    ) -> Result<()> {
        let slot_iter = blocktree
            .slot_meta_iterator(repairee_epoch_slots.root + 1)
            .expect("Couldn't get db iterator");
        let mut total_data_blobs_sent = 0;
        let mut total_coding_blobs_sent = 0;

        while slot_iter.valid() && slot_iter.key().unwrap() <= my_root {
            let slot = slot_iter.key().unwrap();
            let highest_index = slot_iter.value().unwrap().received;
            if !repairee_epoch_slots.slots.contains(&slot) {
                // Calculate the blob indexes this node is responsible for repairing. Note that because we
                // are only repairing slots that are before our root, the slot.received should be equal to
                // the actual total number of blobs in the slot. Optimistically this means that most repairmen should
                // observe the same "total" number of blobs for a particular slot, and thus the calculation in
                // calculate_my_repairman_index_for_slot() will divide responsibility evenly across the cluster
                let num_blobs_in_slot = slot_iter.value().unwrap().received as usize;
                if let Some((my_repairman_index, repairman_step)) =
                    Self::calculate_my_repairman_index_for_slot(
                        my_id,
                        &eligible_repairmen,
                        num_blobs_in_slot,
                    )
                {
                    // Repairee is missing this slot, send them the blobs for this slot
                    for i in (0..highest_index)
                        .skip(my_repairman_index)
                        .step_by(repairman_step)
                    {
                        let mut should_sleep = false;
                        // Loop over the blob indexes and query the database for these blob that
                        // this node is reponsible for repairing. This should be faster than using
                        // a database iterator over the slots because by the time this node is
                        // sending the blobs in this slot for repair, we expect these slots
                        // to be full.
                        if let Some(blob_data) = blocktree
                            .get_data_blob_bytes(slot, i)
                            .expect("Failed to read data blob from blocktree")
                        {
                            socket.send_to(&blob_data[..], repairee_tvu)?;
                            total_data_blobs_sent += 1;
                            should_sleep = true;
                        }

                        if let Some(coding_bytes) = blocktree
                            .get_coding_blob_bytes(slot, i)
                            .expect("Failed to read coding blob from blocktree")
                        {
                            socket.send_to(&coding_bytes[..], repairee_tvu)?;
                            total_coding_blobs_sent += 1;
                            should_sleep = true;
                        }

                        if should_sleep {
                            sleep(Duration::from_millis(BLOB_SEND_SLEEP_MILLIS as u64));
                        }
                    }
                }
            }
        }

        Self::report_repair_metrics(total_data_blobs_sent, total_coding_blobs_sent);
        Ok(())
    }

    fn report_repair_metrics(total_data_blobs_sent: u64, total_coding_blobs_sent: u64) {
        if total_data_blobs_sent > 0 || total_coding_blobs_sent > 0 {
            datapoint!(
                "repairman_activity",
                ("data_sent", total_data_blobs_sent, i64),
                ("coding_sent", total_coding_blobs_sent, i64)
            );
        }
    }

    fn shuffle_repairmen(
        eligible_repairmen: &mut Vec<&Pubkey>,
        repairee_id: &Pubkey,
        repairee_root: u64,
    ) {
        // Make a seed from pubkey + repairee root
        let mut seed = [0u8; mem::size_of::<Pubkey>()];
        let repairee_id_bytes = repairee_id.as_ref();
        seed[..repairee_id_bytes.len()].copy_from_slice(repairee_id_bytes);
        LittleEndian::write_u64(&mut seed[0..], repairee_root);

        // Deterministically shuffle the eligible repairmen based on the seed
        let mut rng = ChaChaRng::from_seed(seed);
        eligible_repairmen.shuffle(&mut rng);
    }

    fn calculate_my_repairman_index_for_slot(
        my_id: &Pubkey,
        eligible_repairmen: &Vec<&Pubkey>,
        num_blobs_in_slot: usize,
    ) -> Option<(usize, usize)> {
        let total_blobs = num_blobs_in_slot * REPAIR_REDUNDANCY;
        let total_repairmen = min(total_blobs, eligible_repairmen.len());
        let repairmen = &eligible_repairmen[..total_repairmen];

        // The total number of blobs sent by each repairman
        let blobs_per_repairman = total_blobs / total_repairmen;

        // Partitions the repairmen into `num_repairman_buckets` different groups.
        // Each repairman within the same group will be responsible for repairing,
        // for some `n`, all the blobs with index equal to `n % num_repairman_buckets`
        // All repairmen within the same group will be sending the same blobs.
        let num_repairman_buckets = total_blobs / blobs_per_repairman;

        // Calculate the indexes this node is responsible for
        if let Some(my_position) = repairmen.iter().position(|id| *id == my_id) {
            Some((my_position % num_repairman_buckets, num_repairman_buckets))
        } else {
            // If there are more repairmen than `total_blobs`, then some repairmen
            // will not have any responsibility to repair this slot
            None
        }
    }

    fn find_eligible_repairmen<'a>(
        my_id: &'a Pubkey,
        repairee_root: u64,
        repairman_roots: &'a HashMap<Pubkey, (u64, u64)>,
        epoch_schedule: &EpochSchedule,
    ) -> Vec<&'a Pubkey> {
        let mut repairmen: Vec<_> = repairman_roots
            .iter()
            .filter_map(|(repairman_id, (_, repairman_root))| {
                if Self::should_repair_peer(*repairman_root, repairee_root, *epoch_schedule) {
                    Some(repairman_id)
                } else {
                    None
                }
            })
            .collect();

        repairmen.push(my_id);
        repairmen
    }

    fn update_my_gossiped_root(
        my_id: &Pubkey,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        old_root: &mut u64,
    ) -> u64 {
        let new_root = cluster_info
            .read()
            .unwrap()
            .get_gossiped_root_for_node(&my_id, None);

        if let Some(new_root) = new_root {
            *old_root = new_root;
            new_root
        } else {
            *old_root
        }
    }

    // Decide if a repairman with root == `repairman_root` should send repairs to a
    // potential repairee with root == `repairee_root`
    fn should_repair_peer(
        repairman_root: u64,
        repairee_root: u64,
        epoch_schedule: EpochSchedule,
    ) -> bool {
        // Check if this potential repairman's confirmed leader schedule is greater
        // than an epoch ahead of the repairee's known schedule
        let repairman_epoch = epoch_schedule.get_stakers_epoch(repairman_root);
        let repairee_epoch =
            epoch_schedule.get_stakers_epoch(repairee_root + NUM_BUFFER_SLOTS as u64);
        repairman_epoch > repairee_epoch
    }

    fn get_root(pubkey: Pubkey, peer_roots: &mut HashMap<Pubkey, (u64, u64)>) -> Option<u64> {
        peer_roots.get(&pubkey).map(|(_, last_root)| *last_root)
    }

    fn get_last_ts(pubkey: Pubkey, peer_roots: &mut HashMap<Pubkey, (u64, u64)>) -> Option<u64> {
        peer_roots.get(&pubkey).map(|(last_ts, _)| *last_ts)
    }
}

impl Service for ClusterInfoRepairListener {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serve_repairs() {}

    #[test]
    fn test_shuffle_repairmen() {}

    #[test]
    fn calculate_my_repairman_index_for_slot() {}

    #[test]
    fn test_find_eligible_repairmen() {}

    #[test]
    fn test_update_my_gossiped_root() {}

    #[test]
    fn test_should_repair_peer() {}
}
