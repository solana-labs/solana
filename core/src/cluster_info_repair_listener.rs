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
                        if Self::should_repair_peer(
                            my_root,
                            peer_epoch_slots.root,
                            &epoch_schedule,
                            NUM_BUFFER_SLOTS,
                        ) {
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
                let mut eligible_repairmen = Self::find_eligible_repairmen(
                    repairee_root,
                    peer_roots,
                    epoch_schedule,
                    NUM_BUFFER_SLOTS,
                );

                eligible_repairmen.push(my_id);

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
        eligible_repairmen: &[&Pubkey],
        socket: &UdpSocket,
        repairee_tvu: &SocketAddr,
    ) -> Result<()> {
        let slot_iter = blocktree
            .slot_meta_iterator(repairee_epoch_slots.root + 1)
            .expect("Couldn't get db iterator");
        let mut total_data_blobs_sent = 0;
        let mut total_coding_blobs_sent = 0;

        for (slot, slot_meta) in slot_iter {
            if slot > my_root {
                break;
            }
            let highest_index = slot_meta.received;
            if !repairee_epoch_slots.slots.contains(&slot) {
                // Calculate the blob indexes this node is responsible for repairing. Note that because we
                // are only repairing slots that are before our root, the slot.received should be equal to
                // the actual total number of blobs in the slot. Optimistically this means that most repairmen should
                // observe the same "total" number of blobs for a particular slot, and thus the calculation in
                // calculate_my_repairman_index_for_slot() will divide responsibility evenly across the cluster
                let num_blobs_in_slot = slot_meta.received as usize;
                if let Some((my_repairman_index, repairman_step)) =
                    Self::calculate_my_repairman_index_for_slot(
                        my_id,
                        &eligible_repairmen,
                        num_blobs_in_slot,
                        REPAIR_REDUNDANCY,
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
        eligible_repairmen: &[&Pubkey],
        num_blobs_in_slot: usize,
        repair_redundancy: usize,
    ) -> Option<(usize, usize)> {
        let total_blobs = num_blobs_in_slot * repair_redundancy;
        let total_repairmen = min(total_blobs, eligible_repairmen.len());

        // Partitions the repairmen into `num_repairman_buckets` different groups.
        // Each repairman within the same group will be responsible for repairing,
        // for some `n`, all the blobs with index equal to `n % num_repairman_buckets`
        // All repairmen within the same group will be sending the same blobs.
        let num_repairman_buckets = total_repairmen / repair_redundancy;

        // Calculate the indexes this node is responsible for
        if let Some(my_position) = eligible_repairmen[..total_repairmen]
            .iter()
            .position(|id| *id == my_id)
        {
            Some((my_position % num_repairman_buckets, num_repairman_buckets))
        } else {
            // If there are more repairmen than `total_blobs`, then some repairmen
            // will not have any responsibility to repair this slot
            None
        }
    }

    fn find_eligible_repairmen<'a>(
        repairee_root: u64,
        repairman_roots: &'a HashMap<Pubkey, (u64, u64)>,
        epoch_schedule: &EpochSchedule,
        num_buffer_slots: usize,
    ) -> Vec<&'a Pubkey> {
        let repairmen: Vec<_> = repairman_roots
            .iter()
            .filter_map(|(repairman_id, (_, repairman_root))| {
                if Self::should_repair_peer(
                    *repairman_root,
                    repairee_root,
                    epoch_schedule,
                    num_buffer_slots,
                ) {
                    Some(repairman_id)
                } else {
                    None
                }
            })
            .collect();

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
        epoch_schedule: &EpochSchedule,
        num_buffer_slots: usize,
    ) -> bool {
        // Check if this potential repairman's confirmed leader schedule is greater
        // than an epoch ahead of the repairee's known schedule
        let repairman_epoch = epoch_schedule.get_stakers_epoch(repairman_root);
        let repairee_epoch =
            epoch_schedule.get_stakers_epoch(repairee_root + num_buffer_slots as u64);

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
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::tests::make_many_slot_entries;
    use crate::streamer;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_serve_repairs_to_repairee() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let blobs_per_slot = 5;
        let num_slots = 10;
        assert_eq!(num_slots % 2, 0);
        let (blobs, _) = make_many_slot_entries(0, num_slots, blobs_per_slot);

        // Write slots in the range [0, num_slots] to blocktree
        blocktree.insert_data_blobs(&blobs).unwrap();

        // Set up my information
        let my_id = Pubkey::new_rand();
        let my_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        // Set up the repairee's EpochSlots, such that they are missing every odd indexed slot
        // in the range (repairee_root, num_slots]
        let repairee_id = Pubkey::new_rand();
        let repairee_root = 0;
        let repairee_slots: HashSet<_> = (0..=num_slots).step_by(2).collect();
        let repairee_epoch_slots = EpochSlots::new(repairee_id, repairee_root, repairee_slots, 1);

        // Mock out some other repairmen such that each repairman is responsible for 1 blob in a slot
        let num_repairmen = blobs_per_slot - 1;
        let mut eligible_repairmen: Vec<_> =
            (0..num_repairmen).map(|_| Pubkey::new_rand()).collect();
        eligible_repairmen.push(my_id);
        let eligible_repairmen_refs: Vec<_> = eligible_repairmen.iter().collect();

        // Set up a blob reciever for the repairee
        let (repairee_sender, repairee_receiver) = channel();
        let repairee_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());
        let repairee_tvu = repairee_socket.local_addr().unwrap();
        let repairee_exit = Arc::new(AtomicBool::new(false));
        let repairee_receiver_thread_hdl =
            streamer::blob_receiver(repairee_socket, &repairee_exit, repairee_sender);

        // Have all the repairman send the repairs
        for repairman_id in &eligible_repairmen {
            ClusterInfoRepairListener::serve_repairs_to_repairee(
                &repairman_id,
                num_slots - 1,
                &blocktree,
                &repairee_epoch_slots,
                &eligible_repairmen_refs,
                &my_socket,
                &repairee_tvu,
            )
            .unwrap();
        }

        let mut received_blobs = vec![];

        // This repairee was missing exactly `num_slots / 2` slots, so we expect to get
        // `(num_slots / 2) * blobs_per_slot` blobs.
        let num_expected_blobs = (num_slots / 2) * blobs_per_slot;
        while (received_blobs.len() as u64) < num_expected_blobs {
            received_blobs.extend(repairee_receiver.recv());
        }

        // Make sure no extra blobs get sent
        sleep(Duration::from_millis(1000));
        assert!(repairee_receiver.try_recv().is_err());
        assert_eq!(received_blobs.len() as u64, num_expected_blobs);

        // Shutdown
        repairee_exit.store(true, Ordering::Relaxed);
        repairee_receiver_thread_hdl.join().unwrap();
        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_shuffle_repairmen() {
        let num_repairmen = 10;
        let eligible_repairmen: Vec<_> = (0..num_repairmen).map(|_| Pubkey::new_rand()).collect();

        let unshuffled_refs: Vec<_> = eligible_repairmen.iter().collect();
        let mut expected_order = unshuffled_refs.clone();

        // Find the expected shuffled order based on a fixed seed
        ClusterInfoRepairListener::shuffle_repairmen(&mut expected_order, unshuffled_refs[0], 0);
        for _ in 0..10 {
            let mut copied = unshuffled_refs.clone();
            ClusterInfoRepairListener::shuffle_repairmen(&mut copied, unshuffled_refs[0], 0);

            // Make sure shuffling repairmen is deterministic every time
            assert_eq!(copied, expected_order);

            // Make sure shuffling actually changes the order of the keys
            assert_ne!(copied, unshuffled_refs);
        }
    }

    #[test]
    fn test_calculate_my_repairman_index_for_slot() {
        // Test when the number of blobs in the slot > number of repairmen
        let num_repairmen = 10;
        let num_blobs_in_slot = 42;
        let repair_redundancy = 3;
        run_calculate_my_repairman_index_for_slot(
            num_repairmen,
            num_blobs_in_slot,
            repair_redundancy,
        );

        // Test when the number of blobs in the slot <= number of repairmen
        let num_repairmen = 10;
        let num_blobs_in_slot = 10;
        let repair_redundancy = 3;
        run_calculate_my_repairman_index_for_slot(
            num_repairmen,
            num_blobs_in_slot,
            repair_redundancy,
        );

        // Test when there are more validators than repair_redundancy * num_blobs_in_slot
        let num_repairmen = 42;
        let num_blobs_in_slot = 10;
        let repair_redundancy = 3;
        run_calculate_my_repairman_index_for_slot(
            num_repairmen,
            num_blobs_in_slot,
            repair_redundancy,
        );
    }

    #[test]
    fn test_should_repair_peer() {
        let epoch_schedule = EpochSchedule::new(32, 16, false);

        // If repairee is ahead of us, we don't repair
        let repairman_root = 0;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            &epoch_schedule,
            0,
        ));

        // If repairee is at the same place as us, we don't repair
        let repairman_root = 5;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            &epoch_schedule,
            0,
        ));

        // If repairee is behind but in the same confirmed epoch, we don't repair
        let repairman_root = 15;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            &epoch_schedule,
            0,
        ));

        // If we have confirmed the next epoch, but the repairee is within the buffer
        // range, we don't repair
        let repairman_root = 16;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            &epoch_schedule,
            11,
        ));

        // If we have confirmed the next epoch, but the repairee is behind a confirmed epoch
        // even with the buffer, then we should repair
        let repairman_root = 16;
        let repairee_root = 5;
        assert!(ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            &epoch_schedule,
            10,
        ));
    }

    fn run_calculate_my_repairman_index_for_slot(
        num_repairmen: usize,
        num_blobs_in_slot: usize,
        repair_redundancy: usize,
    ) {
        let eligible_repairmen: Vec<_> = (0..num_repairmen).map(|_| Pubkey::new_rand()).collect();
        let eligible_repairmen_ref: Vec<_> = eligible_repairmen.iter().collect();
        let mut results = HashMap::new();
        let mut none_results = 0;
        let mut step = None;
        for pk in &eligible_repairmen {
            if let Some((repairman_index, repairman_step)) =
                ClusterInfoRepairListener::calculate_my_repairman_index_for_slot(
                    pk,
                    &eligible_repairmen_ref[..],
                    num_blobs_in_slot,
                    repair_redundancy,
                )
            {
                if let Some(step) = step {
                    assert_eq!(step, repairman_step);
                } else {
                    step = Some(repairman_step);
                }

                results
                    .entry(repairman_index)
                    .and_modify(|e| *e += 1)
                    .or_insert(1);
            } else {
                // This repairman isn't responsible for repairing this slot
                none_results += 1;
            }
        }

        // Analyze the results:

        // 1) Each bucket should have at least min(repair_redundancy, num_repairmen)
        // peers responsible for that bucket
        for b in results.keys() {
            assert!(results[b] >= min(num_repairmen, repair_redundancy));
        }

        // 2) Buckets should be as evenly divided as possible among the repairmen
        let min_repairmen = results.values().min_by(|x, y| x.cmp(y)).unwrap();
        let max_repairmen = results.values().max_by(|x, y| x.cmp(y)).unwrap();
        assert!(*max_repairmen <= *min_repairmen + 1);

        // 3) There should only be repairmen who are not responsible for repairing this slot
        // if we have more repairman than `num_blobs_in_slot * repair_redundancy`. In this case the
        // first `num_blobs_in_slot * repair_redundancy` repairmen woudl send one blob, and the rest
        // would noe be responsible for sending any repairs
        assert_eq!(
            none_results,
            num_repairmen.saturating_sub(num_blobs_in_slot * repair_redundancy)
        );
    }
}
