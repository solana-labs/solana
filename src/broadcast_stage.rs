//! The `broadcast_stage` broadcasts data from a leader node to validators
//!
use cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo};
use counter::Counter;
use entry::Entry;
#[cfg(feature = "erasure")]
use erasure;
use leader_scheduler::LeaderScheduler;
use ledger::Block;
use log::Level;
use packet::SharedBlobs;
use rayon::prelude::*;
use result::{Error, Result};
use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};
use timing::duration_as_ms;
use window::{self, SharedWindow, WindowIndex, WindowUtil, WINDOW_SIZE};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    LeaderRotation,
    ChannelDisconnected,
}

#[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
fn broadcast(
    bootstrap_height_option: &Option<u64>,
    leader_rotation_interval_option: &Option<u64>,
    leader_scheduler_option: &Option<Arc<RwLock<LeaderScheduler>>>,
    node_info: &NodeInfo,
    broadcast_table: &[NodeInfo],
    window: &SharedWindow,
    receiver: &Receiver<Vec<Entry>>,
    sock: &UdpSocket,
    transmit_index: &mut WindowIndex,
    receive_index: &mut u64,
) -> Result<()> {
    let id = node_info.id;
    let timer = Duration::new(1, 0);
    let entries = receiver.recv_timeout(timer)?;
    let now = Instant::now();
    let mut num_entries = entries.len();
    let mut ventries = Vec::new();
    ventries.push(entries);
    while let Ok(entries) = receiver.try_recv() {
        num_entries += entries.len();
        ventries.push(entries);
    }
    inc_new_counter_info!("broadcast_stage-entries_received", num_entries);

    let to_blobs_start = Instant::now();
    let dq: SharedBlobs = ventries
        .into_par_iter()
        .flat_map(|p| p.to_blobs())
        .collect();

    let to_blobs_elapsed = duration_as_ms(&to_blobs_start.elapsed());

    // flatten deque to vec
    let blobs_vec: SharedBlobs = dq.into_iter().collect();

    let blobs_chunking = Instant::now();
    // We could receive more blobs than window slots so
    // break them up into window-sized chunks to process
    let blobs_chunked = blobs_vec.chunks(WINDOW_SIZE as usize).map(|x| x.to_vec());
    let chunking_elapsed = duration_as_ms(&blobs_chunking.elapsed());

    trace!("{}", window.read().unwrap().print(&id, *receive_index));

    let broadcast_start = Instant::now();
    for mut blobs in blobs_chunked {
        let blobs_len = blobs.len();
        trace!("{}: broadcast blobs.len: {}", id, blobs_len);

        // Index the blobs
        window::index_blobs(node_info, &blobs, receive_index)
            .expect("index blobs for initial window");

        // keep the cache of blobs that are broadcast
        inc_new_counter_info!("streamer-broadcast-sent", blobs.len());
        {
            let mut win = window.write().unwrap();
            assert!(blobs.len() <= win.len());
            for b in &blobs {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                if let Some(x) = win[pos].data.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                }
                if let Some(x) = win[pos].coding.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                }

                trace!("{} null {}", id, pos);
            }
            for b in &blobs {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                trace!("{} caching {} at {}", id, ix, pos);
                assert!(win[pos].data.is_none());
                win[pos].data = Some(b.clone());
            }
        }

        // Fill in the coding blob data from the window data blobs
        #[cfg(feature = "erasure")]
        {
            erasure::generate_coding(
                &id,
                &mut window.write().unwrap(),
                *receive_index,
                blobs_len,
                &mut transmit_index.coding,
            )?;
        }

        *receive_index += blobs_len as u64;

        // Send blobs out from the window
        ClusterInfo::broadcast(
            bootstrap_height_option,
            leader_rotation_interval_option,
            leader_scheduler_option,
            &node_info,
            &broadcast_table,
            &window,
            &sock,
            transmit_index,
            *receive_index,
        )?;
    }
    let broadcast_elapsed = duration_as_ms(&broadcast_start.elapsed());

    inc_new_counter_info!(
        "broadcast_stage-time_ms",
        duration_as_ms(&now.elapsed()) as usize
    );
    info!(
        "broadcast: {} entries, blob time {} chunking time {} broadcast time {}",
        num_entries, to_blobs_elapsed, chunking_elapsed, broadcast_elapsed
    );

    Ok(())
}

// Implement a destructor for the BroadcastStage thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct BroadcastStage {
    thread_hdl: JoinHandle<BroadcastStageReturnType>,
}

impl BroadcastStage {
    fn run(
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window: &SharedWindow,
        entry_height: u64,
        receiver: &Receiver<Vec<Entry>>,
        leader_scheduler_option: Option<Arc<RwLock<LeaderScheduler>>>,
    ) -> BroadcastStageReturnType {
        let mut transmit_index = WindowIndex {
            data: entry_height,
            coding: entry_height,
        };
        let mut receive_index = entry_height;
        let me = cluster_info.read().unwrap().my_data().clone();
        let mut leader_rotation_interval_option = None;
        let mut bootstrap_height_option = None;
        if let Some(leader_scheduler) = leader_scheduler_option {
            // Keep track of the leader_rotation_interval, so we don't have to
            // grab the leader_scheduler lock every iteration of the loop.
            let ls_lock = leader_scheduler.read().unwrap();
            leader_rotation_interval_option = Some(ls_lock.leader_rotation_interval);
            bootstrap_height_option = Some(ls_lock.bootstrap_height);
        }
        loop {
            if let Some(leader_rotation_interval) = leader_rotation_interval_option {
                if transmit_index.data % (leader_rotation_interval as u64) == 0 {
                    let my_id = me.id;
                    let leader_scheduler_lock = leader_scheduler_option.as_ref().expect(
                        "leader_scheduler_option cannot be None if 
                        leader_rotation_interval_option is not None",
                    );

                    let rlock = leader_scheduler_lock.read().unwrap();

                    match rlock.get_scheduled_leader(transmit_index.data) {
                        Some(leader_id) if leader_id == my_id => (),
                        // In this case, the write_stage moved the schedule too far
                        // ahead and we no longer are in the known window. In that case,
                        // just continue broadcasting until we catch up.
                        None => (),
                        // If the leader stays in power for the next
                        // round as well, then we don't exit. Otherwise, exit.
                        _ => {
                            return BroadcastStageReturnType::LeaderRotation;
                        }
                    }
                }
            }

            let broadcast_table = cluster_info.read().unwrap().compute_broadcast_table();
            if let Err(e) = broadcast(
                &bootstrap_height_option,
                &leader_rotation_interval_option,
                &leader_scheduler_option,
                &me,
                &broadcast_table,
                &window,
                &receiver,
                &sock,
                &mut transmit_index,
                &mut receive_index,
            ) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                        return BroadcastStageReturnType::ChannelDisconnected
                    }
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    Error::ClusterInfoError(ClusterInfoError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                    _ => {
                        inc_new_counter_info!("streamer-broadcaster-error", 1, 1);
                        error!("broadcaster error: {:?}", e);
                    }
                }
            }
        }
    }

    /// Service to broadcast messages from the leader to layer 1 nodes.
    /// See `cluster_info` for network layer definitions.
    /// # Arguments
    /// * `sock` - Socket to send from.
    /// * `exit` - Boolean to signal system exit.
    /// * `cluster_info` - ClusterInfo structure
    /// * `window` - Cache of blobs that we have broadcast
    /// * `receiver` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
    /// * `exit_sender` - Set to true when this stage exits, allows rest of Tpu to exit cleanly. Otherwise,
    /// when a Tpu stage closes, it only closes the stages that come after it. The stages
    /// that come before could be blocked on a receive, and never notice that they need to
    /// exit. Now, if any stage of the Tpu closes, it will lead to closing the WriteStage (b/c
    /// WriteStage is the last stage in the pipeline), which will then close Broadcast stage,
    /// which will then close FetchStage in the Tpu, and then the rest of the Tpu,
    /// completing the cycle.
    pub fn new(
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window: SharedWindow,
        entry_height: u64,
        receiver: Receiver<Vec<Entry>>,
        leader_scheduler_option: Option<Arc<RwLock<LeaderScheduler>>>,
        exit_sender: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_sender);
                Self::run(
                    &sock,
                    &cluster_info,
                    &window,
                    entry_height,
                    &receiver,
                    leader_scheduler_option,
                )
            }).unwrap();

        BroadcastStage { thread_hdl }
    }
}

impl Service for BroadcastStage {
    type JoinReturnType = BroadcastStageReturnType;

    fn join(self) -> thread::Result<BroadcastStageReturnType> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use broadcast_stage::{BroadcastStage, BroadcastStageReturnType};
    use cluster_info::{ClusterInfo, Node};
    use entry::Entry;
    use leader_scheduler::{set_new_leader, LeaderScheduler, LeaderSchedulerConfig};
    use ledger::next_entries_mut;
    use mint::Mint;
    use service::Service;
    use signature::{Keypair, KeypairUtil};
    use std::cmp;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Arc, RwLock};
    use window::{new_window_from_entries, SharedWindow};

    struct DummyBroadcastStage {
        bank: Bank,
        broadcast_stage: BroadcastStage,
        shared_window: SharedWindow,
        entry_sender: Sender<Vec<Entry>>,
        entries: Vec<Entry>,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    }

    fn setup_dummy_broadcast_stage(
        leader_keypair: Keypair,
        leader_scheduler_config: &LeaderSchedulerConfig,
    ) -> DummyBroadcastStage {
        // Setup dummy leader info
        let leader_info = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        // Give the leader somebody to broadcast to so he isn't lonely
        let buddy_keypair = Keypair::new();
        let broadcast_buddy = Node::new_localhost_with_pubkey(buddy_keypair.pubkey());

        // Fill the cluster_info with the buddy's info
        let mut cluster_info =
            ClusterInfo::new(leader_info.info.clone()).expect("ClusterInfo::new");
        cluster_info.insert(&broadcast_buddy.info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        // Make dummy initial entries
        let mint = Mint::new(10000);
        let entries = mint.create_entries();
        let entry_height = entries.len() as u64;

        // Setup a window
        let window = new_window_from_entries(&entries, entry_height, &leader_info.info);

        let shared_window = Arc::new(RwLock::new(window));

        let (entry_sender, entry_receiver) = channel();
        let exit_sender = Arc::new(AtomicBool::new(false));

        // Make a leader scheduler to manipulate in later tests
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::new(leader_scheduler_config)));

        // Make a bank to manipulate the stakes for leader selection
        let bank = Bank::new_default(true);

        // Start up the broadcast stage
        let broadcast_stage = BroadcastStage::new(
            leader_info.sockets.broadcast,
            cluster_info.clone(),
            shared_window.clone(),
            entry_height,
            entry_receiver,
            Some(leader_scheduler.clone()),
            exit_sender,
        );

        DummyBroadcastStage {
            broadcast_stage,
            shared_window,
            entry_sender,
            bank,
            entries,
            leader_scheduler,
        }
    }

    fn find_highest_window_index(shared_window: &SharedWindow) -> u64 {
        let window = shared_window.read().unwrap();
        window.iter().fold(0, |m, w_slot| {
            if let Some(ref blob) = w_slot.data {
                cmp::max(m, blob.read().unwrap().get_index().unwrap())
            } else {
                m
            }
        })
    }

    #[test]
    fn test_broadcast_stage_leader_rotation_exit() {
        let leader_keypair = Keypair::new();
        let leader_rotation_interval = 10;
        let bootstrap_height = 20;
        let seed_rotation_interval = 3 * leader_rotation_interval;
        let active_window = bootstrap_height + 3 * seed_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_keypair.pubkey(),
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            // The active window needs to >= the cumulative entry_height reached by
            // any of the below test cases
            Some(active_window),
        );

        let broadcast_info = setup_dummy_broadcast_stage(leader_keypair, &leader_scheduler_config);
        let genesis_len = broadcast_info.entries.len() as u64;
        assert!(genesis_len < bootstrap_height);
        let mut last_id = broadcast_info
            .entries
            .last()
            .expect("Ledger should not be empty")
            .id;
        let mut num_hashes = 0;

        // Input enough entries to make exactly leader_rotation_interval entries, which will
        // trigger a check for leader rotation. Because the next scheduled leader
        // is ourselves, we won't exit
        for entry_height in (genesis_len + 1)..(bootstrap_height + 1) {
            broadcast_info
                .leader_scheduler
                .write()
                .unwrap()
                .update_height(entry_height, &broadcast_info.bank);
            let new_entry = next_entries_mut(&mut last_id, &mut num_hashes, vec![]);
            broadcast_info.entry_sender.send(new_entry).unwrap();
        }

        // Set the scheduled leader for the next seed_rotation_interval to somebody else
        let next_leader_height = bootstrap_height + seed_rotation_interval;
        set_new_leader(
            &broadcast_info.bank,
            &mut (*broadcast_info.leader_scheduler.write().unwrap()),
            next_leader_height,
        );

        // Input another leader_rotation_interval dummy entries, which will take us
        // past the point of the leader rotation. The broadcast_stage will see that
        // it's no longer the leader after checking the schedule, and exit
        for entry_height in (bootstrap_height + 1)..(next_leader_height + 1) {
            broadcast_info
                .leader_scheduler
                .write()
                .unwrap()
                .update_height(entry_height, &broadcast_info.bank);
            let new_entry = next_entries_mut(&mut last_id, &mut num_hashes, vec![]);
            match broadcast_info.entry_sender.send(new_entry) {
                // We disconnected, break out of loop and check the results
                Err(_) => break,
                _ => (),
            };
        }

        // Make sure the threads closed cleanly
        assert_eq!(
            broadcast_info.broadcast_stage.join().unwrap(),
            BroadcastStageReturnType::LeaderRotation
        );

        let highest_index = find_highest_window_index(&broadcast_info.shared_window);
        // The blob index is zero indexed, so it will always be one behind the entry height
        // which starts at one.
        assert_eq!(highest_index, bootstrap_height + seed_rotation_interval - 1);
    }
}
