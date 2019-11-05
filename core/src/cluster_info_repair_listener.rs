use crate::cluster_info::ClusterInfo;
use crate::crds_value::EpochSlots;
use crate::result::Result;
use crate::service::Service;
use byteorder::{ByteOrder, LittleEndian};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use solana_ledger::blocktree::Blocktree;
use solana_ledger::rooted_slot_iterator::RootedSlotIterator;
use solana_metrics::datapoint;
use solana_sdk::{epoch_schedule::EpochSchedule, pubkey::Pubkey};
use std::{
    cmp,
    collections::HashMap,
    mem,
    net::SocketAddr,
    net::UdpSocket,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::{self, sleep, Builder, JoinHandle},
    time::Duration,
};

pub const REPAIRMEN_SLEEP_MILLIS: usize = 100;
pub const REPAIR_REDUNDANCY: usize = 1;
pub const NUM_BUFFER_SLOTS: usize = 50;
pub const GOSSIP_DELAY_SLOTS: usize = 2;
pub const NUM_SLOTS_PER_UPDATE: usize = 2;

// Represents the blobs that a repairman is responsible for repairing in specific slot. More
// specifically, a repairman is responsible for every blob in this slot with index
// `(start_index + step_size * i) % num_blobs_in_slot`, for all `0 <= i <= num_blobs_to_send - 1`
// in this slot.
struct BlobIndexesToRepairIterator {
    start_index: usize,
    num_blobs_to_send: usize,
    step_size: usize,
    num_blobs_in_slot: usize,
    blobs_sent: usize,
}

impl BlobIndexesToRepairIterator {
    fn new(
        start_index: usize,
        num_blobs_to_send: usize,
        step_size: usize,
        num_blobs_in_slot: usize,
    ) -> Self {
        Self {
            start_index,
            num_blobs_to_send,
            step_size,
            num_blobs_in_slot,
            blobs_sent: 0,
        }
    }
}

impl Iterator for BlobIndexesToRepairIterator {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.blobs_sent == self.num_blobs_to_send {
            None
        } else {
            let blob_index = Some(
                (self.start_index + self.step_size * self.blobs_sent) % self.num_blobs_in_slot,
            );
            self.blobs_sent += 1;
            blob_index
        }
    }
}

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
                // Maps a peer to
                // 1) The latest timestamp of the EpochSlots gossip message at which a repair was
                // sent to this peer
                // 2) The latest root the peer gossiped
                let mut peer_roots: HashMap<Pubkey, (u64, u64)> = HashMap::new();
                let _ = Self::recv_loop(
                    &blocktree,
                    &mut peer_roots,
                    &exit,
                    &cluster_info,
                    &epoch_schedule,
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
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        epoch_schedule: &EpochSchedule,
    ) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let my_pubkey = cluster_info.read().unwrap().id();
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
                if let Some(repairee_epoch_slots) = Self::process_potential_repairee(
                    &my_pubkey,
                    &peer.id,
                    cluster_info,
                    peer_roots,
                    &mut my_gossiped_root,
                ) {
                    peers_needing_repairs.insert(peer.id, repairee_epoch_slots);
                }
            }

            // After updating all the peers, send out repairs to those that need it
            let _ = Self::serve_repairs(
                &my_pubkey,
                blocktree,
                peer_roots,
                &peers_needing_repairs,
                &socket,
                cluster_info,
                &mut my_gossiped_root,
                epoch_schedule,
            );

            sleep(Duration::from_millis(REPAIRMEN_SLEEP_MILLIS as u64));
        }
    }

    fn process_potential_repairee(
        my_pubkey: &Pubkey,
        peer_pubkey: &Pubkey,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        peer_roots: &mut HashMap<Pubkey, (u64, u64)>,
        my_gossiped_root: &mut u64,
    ) -> Option<EpochSlots> {
        let last_cached_repair_ts = Self::get_last_ts(peer_pubkey, peer_roots);
        let my_root = Self::read_my_gossiped_root(&my_pubkey, cluster_info, my_gossiped_root);
        {
            let r_cluster_info = cluster_info.read().unwrap();

            // Update our local map with the updated peers' information
            if let Some((peer_epoch_slots, updated_ts)) =
                r_cluster_info.get_epoch_state_for_node(&peer_pubkey, last_cached_repair_ts)
            {
                let peer_entry = peer_roots.entry(*peer_pubkey).or_default();
                let peer_root = cmp::max(peer_epoch_slots.root, peer_entry.1);
                let mut result = None;
                let last_repair_ts = {
                    // Following logic needs to be fast because it holds the lock
                    // preventing updates on gossip
                    if Self::should_repair_peer(my_root, peer_epoch_slots.root, NUM_BUFFER_SLOTS) {
                        // Clone out EpochSlots structure to avoid holding lock on gossip
                        result = Some(peer_epoch_slots.clone());
                        updated_ts
                    } else {
                        // No repairs were sent, don't need to update the timestamp
                        peer_entry.0
                    }
                };

                *peer_entry = (last_repair_ts, peer_root);
                result
            } else {
                None
            }
        }
    }

    fn serve_repairs(
        my_pubkey: &Pubkey,
        blocktree: &Blocktree,
        peer_roots: &HashMap<Pubkey, (u64, u64)>,
        repairees: &HashMap<Pubkey, EpochSlots>,
        socket: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        my_gossiped_root: &mut u64,
        epoch_schedule: &EpochSchedule,
    ) -> Result<()> {
        for (repairee_pubkey, repairee_epoch_slots) in repairees {
            let repairee_root = repairee_epoch_slots.root;

            let repairee_repair_addr = {
                let r_cluster_info = cluster_info.read().unwrap();
                let contact_info = r_cluster_info.get_contact_info_for_node(repairee_pubkey);
                contact_info.map(|c| c.repair)
            };

            if let Some(repairee_addr) = repairee_repair_addr {
                // For every repairee, get the set of repairmen who are responsible for
                let mut eligible_repairmen = Self::find_eligible_repairmen(
                    my_pubkey,
                    repairee_root,
                    peer_roots,
                    NUM_BUFFER_SLOTS,
                );

                Self::shuffle_repairmen(
                    &mut eligible_repairmen,
                    repairee_pubkey,
                    repairee_epoch_slots.root,
                );

                let my_root =
                    Self::read_my_gossiped_root(my_pubkey, cluster_info, my_gossiped_root);

                let _ = Self::serve_repairs_to_repairee(
                    my_pubkey,
                    repairee_pubkey,
                    my_root,
                    blocktree,
                    &repairee_epoch_slots,
                    &eligible_repairmen,
                    socket,
                    &repairee_addr,
                    NUM_SLOTS_PER_UPDATE,
                    epoch_schedule,
                );
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn serve_repairs_to_repairee(
        my_pubkey: &Pubkey,
        repairee_pubkey: &Pubkey,
        my_root: u64,
        blocktree: &Blocktree,
        repairee_epoch_slots: &EpochSlots,
        eligible_repairmen: &[&Pubkey],
        socket: &UdpSocket,
        repairee_addr: &SocketAddr,
        num_slots_to_repair: usize,
        epoch_schedule: &EpochSchedule,
    ) -> Result<()> {
        let slot_iter = RootedSlotIterator::new(repairee_epoch_slots.root, &blocktree);
        if slot_iter.is_err() {
            info!(
                "Root for repairee is on different fork. My root: {}, repairee_root: {} repairee_pubkey: {:?}",
                my_root, repairee_epoch_slots.root, repairee_pubkey,
            );
            return Ok(());
        }

        let mut slot_iter = slot_iter?;

        let mut total_data_blobs_sent = 0;
        let mut total_coding_blobs_sent = 0;
        let mut num_slots_repaired = 0;
        let max_confirmed_repairee_epoch =
            epoch_schedule.get_leader_schedule_epoch(repairee_epoch_slots.root);
        let max_confirmed_repairee_slot =
            epoch_schedule.get_last_slot_in_epoch(max_confirmed_repairee_epoch);

        // Skip the first slot in the iterator because we know it's the root slot which the repairee
        // already has
        slot_iter.next();
        for (slot, slot_meta) in slot_iter {
            if slot > my_root
                || num_slots_repaired >= num_slots_to_repair
                || slot > max_confirmed_repairee_slot
            {
                break;
            }
            if !repairee_epoch_slots.slots.contains(&slot) {
                // Calculate the blob indexes this node is responsible for repairing. Note that
                // because we are only repairing slots that are before our root, the slot.received
                // should be equal to the actual total number of blobs in the slot. Optimistically
                // this means that most repairmen should observe the same "total" number of blobs
                // for a particular slot, and thus the calculation in
                // calculate_my_repairman_index_for_slot() will divide responsibility evenly across
                // the cluster
                let num_blobs_in_slot = slot_meta.received as usize;
                if let Some(my_repair_indexes) = Self::calculate_my_repairman_index_for_slot(
                    my_pubkey,
                    &eligible_repairmen,
                    num_blobs_in_slot,
                    REPAIR_REDUNDANCY,
                ) {
                    // Repairee is missing this slot, send them the blobs for this slot
                    for blob_index in my_repair_indexes {
                        // Loop over the sblob indexes and query the database for these blob that
                        // this node is reponsible for repairing. This should be faster than using
                        // a database iterator over the slots because by the time this node is
                        // sending the blobs in this slot for repair, we expect these slots
                        // to be full.
                        if let Some(blob_data) = blocktree
                            .get_data_shred(slot, blob_index as u64)
                            .expect("Failed to read data blob from blocktree")
                        {
                            socket.send_to(&blob_data[..], repairee_addr)?;
                            total_data_blobs_sent += 1;
                        }

                        if let Some(coding_bytes) = blocktree
                            .get_coding_shred(slot, blob_index as u64)
                            .expect("Failed to read coding blob from blocktree")
                        {
                            socket.send_to(&coding_bytes[..], repairee_addr)?;
                            total_coding_blobs_sent += 1;
                        }
                    }

                    num_slots_repaired += 1;
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
        repairee_pubkey: &Pubkey,
        repairee_root: u64,
    ) {
        // Make a seed from pubkey + repairee root
        let mut seed = [0u8; mem::size_of::<Pubkey>()];
        let repairee_pubkey_bytes = repairee_pubkey.as_ref();
        seed[..repairee_pubkey_bytes.len()].copy_from_slice(repairee_pubkey_bytes);
        LittleEndian::write_u64(&mut seed[0..], repairee_root);

        // Deterministically shuffle the eligible repairmen based on the seed
        let mut rng = ChaChaRng::from_seed(seed);
        eligible_repairmen.shuffle(&mut rng);
    }

    // The calculation should partition the blobs in the slot across the repairmen in the cluster
    // such that each blob in the slot is the responsibility of `repair_redundancy` or
    // `repair_redundancy + 1` number of repairmen in the cluster.
    fn calculate_my_repairman_index_for_slot(
        my_pubkey: &Pubkey,
        eligible_repairmen: &[&Pubkey],
        num_blobs_in_slot: usize,
        repair_redundancy: usize,
    ) -> Option<BlobIndexesToRepairIterator> {
        let total_blobs = num_blobs_in_slot * repair_redundancy;
        let total_repairmen_for_slot = cmp::min(total_blobs, eligible_repairmen.len());

        let blobs_per_repairman = cmp::min(
            (total_blobs + total_repairmen_for_slot - 1) / total_repairmen_for_slot,
            num_blobs_in_slot,
        );

        // Calculate the indexes this node is responsible for
        if let Some(my_position) = eligible_repairmen[..total_repairmen_for_slot]
            .iter()
            .position(|id| *id == my_pubkey)
        {
            let start_index = my_position % num_blobs_in_slot;
            Some(BlobIndexesToRepairIterator::new(
                start_index,
                blobs_per_repairman,
                total_repairmen_for_slot,
                num_blobs_in_slot,
            ))
        } else {
            // If there are more repairmen than `total_blobs`, then some repairmen
            // will not have any responsibility to repair this slot
            None
        }
    }

    fn find_eligible_repairmen<'a>(
        my_pubkey: &'a Pubkey,
        repairee_root: u64,
        repairman_roots: &'a HashMap<Pubkey, (u64, u64)>,
        num_buffer_slots: usize,
    ) -> Vec<&'a Pubkey> {
        let mut repairmen: Vec<_> = repairman_roots
            .iter()
            .filter_map(|(repairman_pubkey, (_, repairman_root))| {
                if Self::should_repair_peer(
                    *repairman_root,
                    repairee_root,
                    num_buffer_slots - GOSSIP_DELAY_SLOTS,
                ) {
                    Some(repairman_pubkey)
                } else {
                    None
                }
            })
            .collect();

        repairmen.push(my_pubkey);
        repairmen.sort();
        repairmen
    }

    // Read my root out of gossip, and update the cached `old_root`
    fn read_my_gossiped_root(
        my_pubkey: &Pubkey,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        old_root: &mut u64,
    ) -> u64 {
        let new_root = cluster_info
            .read()
            .unwrap()
            .get_gossiped_root_for_node(&my_pubkey, None);

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
        num_buffer_slots: usize,
    ) -> bool {
        // Check if this potential repairman's root is greater than the repairee root +
        // num_buffer_slots
        repairman_root > repairee_root + num_buffer_slots as u64
    }

    fn get_last_ts(pubkey: &Pubkey, peer_roots: &mut HashMap<Pubkey, (u64, u64)>) -> Option<u64> {
        peer_roots.get(pubkey).map(|(last_ts, _)| *last_ts)
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
    use crate::blob::{Blob, SharedBlob};
    use crate::cluster_info::Node;
    use crate::streamer;
    use solana_ledger::blocktree::get_tmp_ledger_path;
    use solana_ledger::blocktree::make_many_slot_entries;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    struct MockRepairee {
        id: Pubkey,
        receiver: Receiver<Vec<SharedBlob>>,
        tvu_address: SocketAddr,
        repairee_exit: Arc<AtomicBool>,
        repairee_receiver_thread_hdl: JoinHandle<()>,
    }

    impl MockRepairee {
        pub fn new(
            id: Pubkey,
            receiver: Receiver<Vec<SharedBlob>>,
            tvu_address: SocketAddr,
            repairee_exit: Arc<AtomicBool>,
            repairee_receiver_thread_hdl: JoinHandle<()>,
        ) -> Self {
            Self {
                id,
                receiver,
                tvu_address,
                repairee_exit,
                repairee_receiver_thread_hdl,
            }
        }

        pub fn make_mock_repairee() -> Self {
            let id = Pubkey::new_rand();
            let (repairee_sender, repairee_receiver) = channel();
            let repairee_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());
            let repairee_tvu_addr = repairee_socket.local_addr().unwrap();
            let repairee_exit = Arc::new(AtomicBool::new(false));
            let repairee_receiver_thread_hdl =
                streamer::blob_receiver(repairee_socket, &repairee_exit, repairee_sender);

            Self::new(
                id,
                repairee_receiver,
                repairee_tvu_addr,
                repairee_exit,
                repairee_receiver_thread_hdl,
            )
        }

        pub fn close(self) -> Result<()> {
            self.repairee_exit.store(true, Ordering::Relaxed);
            self.repairee_receiver_thread_hdl.join()?;
            Ok(())
        }
    }

    #[test]
    fn test_process_potential_repairee() {
        // Set up node ids
        let my_pubkey = Pubkey::new_rand();
        let peer_pubkey = Pubkey::new_rand();

        // Set up cluster_info
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            Node::new_localhost().info,
        )));

        // Push a repairee's epoch slots into cluster info
        let repairee_root = 0;
        let repairee_slots = BTreeSet::new();
        cluster_info.write().unwrap().push_epoch_slots(
            peer_pubkey,
            repairee_root,
            repairee_slots.clone(),
        );

        // Set up locally cached information
        let mut peer_roots = HashMap::new();
        let mut my_gossiped_root = repairee_root;

        // Root is not sufficiently far ahead, we shouldn't repair
        assert!(ClusterInfoRepairListener::process_potential_repairee(
            &my_pubkey,
            &peer_pubkey,
            &cluster_info,
            &mut peer_roots,
            &mut my_gossiped_root,
        )
        .is_none());

        // Update the root to be sufficiently far ahead. A repair should now occur even if the
        // object in gossip is not updated
        my_gossiped_root = repairee_root + NUM_BUFFER_SLOTS as u64 + 1;
        assert!(ClusterInfoRepairListener::process_potential_repairee(
            &my_pubkey,
            &peer_pubkey,
            &cluster_info,
            &mut peer_roots,
            &mut my_gossiped_root,
        )
        .is_some());

        // An repair was already sent, so if gossip is not updated, no repair should be sent again,
        // even if our root moves forward
        my_gossiped_root += 4;
        assert!(ClusterInfoRepairListener::process_potential_repairee(
            &my_pubkey,
            &peer_pubkey,
            &cluster_info,
            &mut peer_roots,
            &mut my_gossiped_root,
        )
        .is_none());

        // Sleep to make sure the timestamp is updated in gossip. Update the gossiped EpochSlots.
        // Now a repair should be sent again
        sleep(Duration::from_millis(10));
        cluster_info
            .write()
            .unwrap()
            .push_epoch_slots(peer_pubkey, repairee_root, repairee_slots);
        assert!(ClusterInfoRepairListener::process_potential_repairee(
            &my_pubkey,
            &peer_pubkey,
            &cluster_info,
            &mut peer_roots,
            &mut my_gossiped_root,
        )
        .is_some());
    }

    #[test]
    fn test_serve_repairs_to_repairee() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let entries_per_slot = 5;
        let num_slots = 10;
        assert_eq!(num_slots % 2, 0);
        let (shreds, _) = make_many_slot_entries(0, num_slots, entries_per_slot);
        let num_shreds_per_slot = shreds.len() as u64 / num_slots;

        // Write slots in the range [0, num_slots] to blocktree
        blocktree.insert_shreds(shreds, None).unwrap();

        // Write roots so that these slots will qualify to be sent by the repairman
        let roots: Vec<_> = (0..=num_slots - 1).collect();
        blocktree.set_roots(&roots).unwrap();

        // Set up my information
        let my_pubkey = Pubkey::new_rand();
        let my_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        // Set up a mock repairee with a socket listening for incoming repairs
        let mock_repairee = MockRepairee::make_mock_repairee();

        // Set up the repairee's EpochSlots, such that they are missing every odd indexed slot
        // in the range (repairee_root, num_slots]
        let repairee_root = 0;
        let repairee_slots: BTreeSet<_> = (0..=num_slots).step_by(2).collect();
        let repairee_epoch_slots =
            EpochSlots::new(mock_repairee.id, repairee_root, repairee_slots, 1);

        // Mock out some other repairmen such that each repairman is responsible for 1 shred in a slot
        let num_repairmen = entries_per_slot - 1;
        let mut eligible_repairmen: Vec<_> =
            (0..num_repairmen).map(|_| Pubkey::new_rand()).collect();
        eligible_repairmen.push(my_pubkey);
        let eligible_repairmen_refs: Vec<_> = eligible_repairmen.iter().collect();

        // Have all the repairman send the repairs
        let epoch_schedule = EpochSchedule::custom(32, 16, false);
        let num_missing_slots = num_slots / 2;
        for repairman_pubkey in &eligible_repairmen {
            ClusterInfoRepairListener::serve_repairs_to_repairee(
                &repairman_pubkey,
                &mock_repairee.id,
                num_slots - 1,
                &blocktree,
                &repairee_epoch_slots,
                &eligible_repairmen_refs,
                &my_socket,
                &mock_repairee.tvu_address,
                num_missing_slots as usize,
                &epoch_schedule,
            )
            .unwrap();
        }

        let mut received_shreds: Vec<Arc<RwLock<Blob>>> = vec![];

        // This repairee was missing exactly `num_slots / 2` slots, so we expect to get
        // `(num_slots / 2) * num_shreds_per_slot * REPAIR_REDUNDANCY` blobs.
        let num_expected_shreds = (num_slots / 2) * num_shreds_per_slot * REPAIR_REDUNDANCY as u64;
        while (received_shreds.len() as u64) < num_expected_shreds {
            received_shreds.extend(mock_repairee.receiver.recv().unwrap());
        }

        // Make sure no extra blobs get sent
        sleep(Duration::from_millis(1000));
        assert!(mock_repairee.receiver.try_recv().is_err());
        assert_eq!(received_shreds.len() as u64, num_expected_shreds);

        // Shutdown
        mock_repairee.close().unwrap();
        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_no_repair_past_confirmed_epoch() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let stakers_slot_offset = 16;
        let slots_per_epoch = stakers_slot_offset * 2;
        let epoch_schedule = EpochSchedule::custom(slots_per_epoch, stakers_slot_offset, false);

        // Create blobs for first two epochs and write them to blocktree
        let total_slots = slots_per_epoch * 2;
        let (shreds, _) = make_many_slot_entries(0, total_slots, 1);
        blocktree.insert_shreds(shreds, None).unwrap();

        // Write roots so that these slots will qualify to be sent by the repairman
        let roots: Vec<_> = (0..=slots_per_epoch * 2 - 1).collect();
        blocktree.set_roots(&roots).unwrap();

        // Set up my information
        let my_pubkey = Pubkey::new_rand();
        let my_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        // Set up a mock repairee with a socket listening for incoming repairs
        let mock_repairee = MockRepairee::make_mock_repairee();

        // Set up the repairee's EpochSlots, such that:
        // 1) They are missing all of the second epoch, but have all of the first epoch.
        // 2) The root only confirms epoch 1, so the leader for epoch 2 is unconfirmed.
        //
        // Thus, no repairmen should send any blobs to this repairee b/c this repairee
        // already has all the slots for which they have a confirmed leader schedule
        let repairee_root = 0;
        let repairee_slots: BTreeSet<_> = (0..=slots_per_epoch).collect();
        let repairee_epoch_slots =
            EpochSlots::new(mock_repairee.id, repairee_root, repairee_slots.clone(), 1);

        ClusterInfoRepairListener::serve_repairs_to_repairee(
            &my_pubkey,
            &mock_repairee.id,
            total_slots - 1,
            &blocktree,
            &repairee_epoch_slots,
            &vec![&my_pubkey],
            &my_socket,
            &mock_repairee.tvu_address,
            1 as usize,
            &epoch_schedule,
        )
        .unwrap();

        // Make sure no shreds get sent
        sleep(Duration::from_millis(1000));
        assert!(mock_repairee.receiver.try_recv().is_err());

        // Set the root to stakers_slot_offset, now epoch 2 should be confirmed, so the repairee
        // is now eligible to get slots from epoch 2:
        let repairee_epoch_slots =
            EpochSlots::new(mock_repairee.id, stakers_slot_offset, repairee_slots, 1);
        ClusterInfoRepairListener::serve_repairs_to_repairee(
            &my_pubkey,
            &mock_repairee.id,
            total_slots - 1,
            &blocktree,
            &repairee_epoch_slots,
            &vec![&my_pubkey],
            &my_socket,
            &mock_repairee.tvu_address,
            1 as usize,
            &epoch_schedule,
        )
        .unwrap();

        // Make sure some blobs get sent this time
        sleep(Duration::from_millis(1000));
        assert!(mock_repairee.receiver.try_recv().is_ok());

        // Shutdown
        mock_repairee.close().unwrap();
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

        // Test when num_blobs_in_slot is a multiple of num_repairmen
        let num_repairmen = 12;
        let num_blobs_in_slot = 48;
        let repair_redundancy = 3;

        run_calculate_my_repairman_index_for_slot(
            num_repairmen,
            num_blobs_in_slot,
            repair_redundancy,
        );

        // Test when num_repairmen and num_blobs_in_slot are relatively prime
        let num_repairmen = 12;
        let num_blobs_in_slot = 47;
        let repair_redundancy = 12;

        run_calculate_my_repairman_index_for_slot(
            num_repairmen,
            num_blobs_in_slot,
            repair_redundancy,
        );

        // Test 1 repairman
        let num_repairmen = 1;
        let num_blobs_in_slot = 30;
        let repair_redundancy = 3;

        run_calculate_my_repairman_index_for_slot(
            num_repairmen,
            num_blobs_in_slot,
            repair_redundancy,
        );

        // Test when repair_redundancy is 1, and num_blobs_in_slot does not evenly
        // divide num_repairmen
        let num_repairmen = 12;
        let num_blobs_in_slot = 47;
        let repair_redundancy = 1;

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

        // Test when there are more repairmen than repair_redundancy * num_blobs_in_slot
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
        // If repairee is ahead of us, we don't repair
        let repairman_root = 0;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            0,
        ));

        // If repairee is at the same place as us, we don't repair
        let repairman_root = 5;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            0,
        ));

        // If repairee is behind with no buffer, we repair
        let repairman_root = 15;
        let repairee_root = 5;
        assert!(ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            0,
        ));

        // If repairee is behind, but within the buffer, we don't repair
        let repairman_root = 16;
        let repairee_root = 5;
        assert!(!ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
            11,
        ));

        // If repairee is behind, but outside the buffer, we repair
        let repairman_root = 16;
        let repairee_root = 5;
        assert!(ClusterInfoRepairListener::should_repair_peer(
            repairman_root,
            repairee_root,
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
        for pk in &eligible_repairmen {
            if let Some(my_repair_indexes) =
                ClusterInfoRepairListener::calculate_my_repairman_index_for_slot(
                    pk,
                    &eligible_repairmen_ref[..],
                    num_blobs_in_slot,
                    repair_redundancy,
                )
            {
                for blob_index in my_repair_indexes {
                    results
                        .entry(blob_index)
                        .and_modify(|e| *e += 1)
                        .or_insert(1);
                }
            } else {
                // This repairman isn't responsible for repairing this slot
                none_results += 1;
            }
        }

        // Analyze the results:

        // 1) If there are a sufficient number of repairmen, then each blob should be sent
        // `repair_redundancy` OR `repair_redundancy + 1` times.
        let num_expected_redundancy = cmp::min(num_repairmen, repair_redundancy);
        for b in results.keys() {
            assert!(
                results[b] == num_expected_redundancy || results[b] == num_expected_redundancy + 1
            );
        }

        // 2) The number of times each blob is sent should be evenly distributed
        let max_times_blob_sent = results.values().min_by(|x, y| x.cmp(y)).unwrap();
        let min_times_blob_sent = results.values().max_by(|x, y| x.cmp(y)).unwrap();
        assert!(*max_times_blob_sent <= *min_times_blob_sent + 1);

        // 3) There should only be repairmen who are not responsible for repairing this slot
        // if we have more repairman than `num_blobs_in_slot * repair_redundancy`. In this case the
        // first `num_blobs_in_slot * repair_redundancy` repairmen would send one blob, and the rest
        // would not be responsible for sending any repairs
        assert_eq!(
            none_results,
            num_repairmen.saturating_sub(num_blobs_in_slot * repair_redundancy)
        );
    }
}
