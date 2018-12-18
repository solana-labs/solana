//! The `broadcast_service` broadcasts data from a leader node to validators
//!
use crate::cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo};
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::entry::Entry;
#[cfg(feature = "erasure")]
use crate::erasure;
use crate::leader_scheduler::LeaderScheduler;
use crate::ledger::Block;
use crate::packet::{index_blobs, SharedBlob};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::window::{SharedWindow, WindowIndex, WindowUtil};
use log::Level;
use rayon::prelude::*;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastServiceReturnType {
    LeaderRotation,
    ChannelDisconnected,
    ExitSignal,
}

#[allow(clippy::too_many_arguments)]
fn broadcast(
    db_ledger: &Arc<DbLedger>,
    max_tick_height: Option<u64>,
    leader_id: Pubkey,
    node_info: &NodeInfo,
    broadcast_table: &[NodeInfo],
    window: &SharedWindow,
    receiver: &Receiver<Vec<Entry>>,
    sock: &UdpSocket,
    transmit_index: &mut WindowIndex,
    receive_index: &mut u64,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<()> {
    let id = node_info.id;
    let timer = Duration::new(1, 0);
    let entries = receiver.recv_timeout(timer)?;
    let now = Instant::now();
    let mut num_entries = entries.len();
    let mut ventries = Vec::new();
    ventries.push(entries);

    let mut contains_last_tick = false;
    while let Ok(entries) = receiver.try_recv() {
        num_entries += entries.len();
        ventries.push(entries);
    }

    if let Some(Some(last)) = ventries.last().map(|entries| entries.last()) {
        contains_last_tick |= Some(last.tick_height) == max_tick_height;
    }

    inc_new_counter_info!("broadcast_service-entries_received", num_entries);

    let to_blobs_start = Instant::now();

    // Generate the slot heights for all the entries inside ventries
    let slot_heights = generate_slots(&ventries, leader_scheduler);

    let blobs: Vec<_> = ventries
        .into_par_iter()
        .flat_map(|p| p.to_blobs())
        .collect();

    let blobs_slot_heights: Vec<(SharedBlob, u64)> = blobs.into_iter().zip(slot_heights).collect();

    let to_blobs_elapsed = duration_as_ms(&to_blobs_start.elapsed());

    let blobs_chunking = Instant::now();
    // We could receive more blobs than window slots so
    // break them up into window-sized chunks to process
    let window_size = window.read().unwrap().window_size();
    let blobs_chunked = blobs_slot_heights
        .chunks(window_size as usize)
        .map(|x| x.to_vec());
    let chunking_elapsed = duration_as_ms(&blobs_chunking.elapsed());

    let broadcast_start = Instant::now();
    for blobs in blobs_chunked {
        let blobs_len = blobs.len();
        trace!("{}: broadcast blobs.len: {}", id, blobs_len);

        index_blobs(blobs.iter(), &node_info.id, *receive_index);

        // keep the cache of blobs that are broadcast
        inc_new_counter_info!("streamer-broadcast-sent", blobs.len());
        {
            let mut win = window.write().unwrap();
            assert!(blobs.len() <= win.len());
            let blobs: Vec<_> = blobs.into_iter().map(|(b, _)| b).collect();
            for b in &blobs {
                let ix = b.read().unwrap().index().expect("blob index");
                let pos = (ix % window_size) as usize;
                if let Some(x) = win[pos].data.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().index().unwrap(),
                        pos
                    );
                }
                if let Some(x) = win[pos].coding.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().index().unwrap(),
                        pos
                    );
                }

                trace!("{} null {}", id, pos);
            }
            for b in &blobs {
                {
                    let ix = b.read().unwrap().index().expect("blob index");
                    let pos = (ix % window_size) as usize;
                    trace!("{} caching {} at {}", id, ix, pos);
                    assert!(win[pos].data.is_none());
                    win[pos].data = Some(b.clone());
                }
            }

            db_ledger
                .write_shared_blobs(&blobs)
                .expect("Unrecoverable failure to write to database");
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
            contains_last_tick,
            leader_id,
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
        "broadcast_service-time_ms",
        duration_as_ms(&now.elapsed()) as usize
    );
    info!(
        "broadcast: {} entries, blob time {} chunking time {} broadcast time {}",
        num_entries, to_blobs_elapsed, chunking_elapsed, broadcast_elapsed
    );

    submit(
        influxdb::Point::new("broadcast-service")
            .add_field(
                "transmit-index",
                influxdb::Value::Integer(transmit_index.data as i64),
            )
            .to_owned(),
    );

    Ok(())
}

fn generate_slots(
    ventries: &[Vec<Entry>],
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Vec<u64> {
    // Generate the slot heights for all the entries inside ventries
    let r_leader_scheduler = leader_scheduler.read().unwrap();
    ventries
        .iter()
        .flat_map(|p| {
            let slot_heights: Vec<u64> = p
                .iter()
                .map(|e| {
                    let tick_height = if e.is_tick() {
                        e.tick_height
                    } else {
                        e.tick_height + 1
                    };
                    let (_, slot) = r_leader_scheduler
                        .get_scheduled_leader(tick_height)
                        .expect("Leader schedule should never be unknown while indexing blobs");
                    slot
                })
                .collect();

            slot_heights
        })
        .collect()
}

// Implement a destructor for the BroadcastService3 thread to signal it exited
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

pub struct BroadcastService {
    thread_hdl: JoinHandle<BroadcastServiceReturnType>,
}

impl BroadcastService {
    fn run(
        db_ledger: &Arc<DbLedger>,
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window: &SharedWindow,
        entry_height: u64,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
        receiver: &Receiver<Vec<Entry>>,
        max_tick_height: Option<u64>,
        exit_signal: &Arc<AtomicBool>,
    ) -> BroadcastServiceReturnType {
        let mut transmit_index = WindowIndex {
            data: entry_height,
            coding: entry_height,
        };
        let mut receive_index = entry_height;
        let me = cluster_info.read().unwrap().my_data().clone();
        loop {
            if exit_signal.load(Ordering::Relaxed) {
                return BroadcastServiceReturnType::ExitSignal;
            }
            let broadcast_table = cluster_info.read().unwrap().tvu_peers();
            inc_new_counter_info!("broadcast_service-num_peers", broadcast_table.len() + 1);
            let leader_id = cluster_info.read().unwrap().leader_id();
            if let Err(e) = broadcast(
                db_ledger,
                max_tick_height,
                leader_id,
                &me,
                &broadcast_table,
                &window,
                &receiver,
                &sock,
                &mut transmit_index,
                &mut receive_index,
                leader_scheduler,
            ) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                        return BroadcastServiceReturnType::ChannelDisconnected
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
    /// * `exit_sender` - Set to true when this service exits, allows rest of Tpu to exit cleanly.
    /// Otherwise, when a Tpu closes, it only closes the stages that come after it. The stages
    /// that come before could be blocked on a receive, and never notice that they need to
    /// exit. Now, if any stage of the Tpu closes, it will lead to closing the WriteStage (b/c
    /// WriteStage is the last stage in the pipeline), which will then close Broadcast service,
    /// which will then close FetchStage in the Tpu, and then the rest of the Tpu,
    /// completing the cycle.
    #[allow(clippy::too_many_arguments, clippy::new_ret_no_self)]
    pub fn new(
        db_ledger: Arc<DbLedger>,
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window: SharedWindow,
        entry_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        receiver: Receiver<Vec<Entry>>,
        max_tick_height: Option<u64>,
        exit_sender: Arc<AtomicBool>,
    ) -> (Self, Arc<AtomicBool>) {
        let exit_signal = Arc::new(AtomicBool::new(false));
        let exit_signal_ = exit_signal.clone();
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_sender);
                Self::run(
                    &db_ledger,
                    &sock,
                    &cluster_info,
                    &window,
                    entry_height,
                    &leader_scheduler,
                    &receiver,
                    max_tick_height,
                    &exit_signal_,
                )
            })
            .unwrap();

        (Self { thread_hdl }, exit_signal)
    }
}

impl Service for BroadcastService {
    type JoinReturnType = BroadcastServiceReturnType;

    fn join(self) -> thread::Result<BroadcastServiceReturnType> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::db_ledger::DbLedger;
    use crate::ledger::create_ticks;
    use crate::ledger::get_tmp_ledger_path;
    use crate::service::Service;
    use crate::window::new_window;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    struct DummyBroadcastService {
        db_ledger: Arc<DbLedger>,
        broadcast_service: BroadcastService,
        entry_sender: Sender<Vec<Entry>>,
        exit_signal: Arc<AtomicBool>,
    }

    fn setup_dummy_broadcast_service(
        leader_pubkey: Pubkey,
        ledger_path: &str,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        entry_height: u64,
        max_tick_height: u64,
    ) -> DummyBroadcastService {
        // Make the database ledger
        let db_ledger = Arc::new(DbLedger::open(ledger_path).unwrap());

        // Make the leader node and scheduler
        let leader_info = Node::new_localhost_with_pubkey(leader_pubkey);

        // Make a node to broadcast to
        let buddy_keypair = Keypair::new();
        let broadcast_buddy = Node::new_localhost_with_pubkey(buddy_keypair.pubkey());

        // Fill the cluster_info with the buddy's info
        let mut cluster_info = ClusterInfo::new(leader_info.info.clone());
        cluster_info.insert_info(broadcast_buddy.info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let window = new_window(32 * 1024);
        let shared_window = Arc::new(RwLock::new(window));
        let (entry_sender, entry_receiver) = channel();
        let exit_sender = Arc::new(AtomicBool::new(false));

        // Start up the broadcast stage
        let (broadcast_service, exit_signal) = BroadcastService::new(
            db_ledger.clone(),
            leader_info.sockets.broadcast,
            cluster_info,
            shared_window,
            entry_height,
            leader_scheduler,
            entry_receiver,
            Some(max_tick_height),
            exit_sender,
        );

        DummyBroadcastService {
            db_ledger,
            broadcast_service,
            entry_sender,
            exit_signal,
        }
    }

    #[test]
    fn test_broadcast_ledger() {
        let ledger_path = get_tmp_ledger_path("test_broadcast");
        {
            // Create the leader scheduler
            let leader_keypair = Keypair::new();
            let mut leader_scheduler =
                LeaderScheduler::from_bootstrap_leader(leader_keypair.pubkey());

            // Mock the tick height to look like the tick height right after a leader transition
            leader_scheduler.last_seed_height = Some(leader_scheduler.bootstrap_height);
            leader_scheduler.set_leader_schedule(vec![leader_keypair.pubkey()]);
            leader_scheduler.use_only_bootstrap_leader = false;
            let start_tick_height = leader_scheduler.bootstrap_height;
            let max_tick_height = start_tick_height + leader_scheduler.last_seed_height.unwrap();
            let entry_height = 2 * start_tick_height;

            let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));
            let broadcast_service = setup_dummy_broadcast_service(
                leader_keypair.pubkey(),
                &ledger_path,
                leader_scheduler.clone(),
                entry_height,
                max_tick_height,
            );

            let ticks = create_ticks(
                (max_tick_height - start_tick_height) as usize,
                Hash::default(),
            );
            for (i, mut tick) in ticks.into_iter().enumerate() {
                // Simulate the tick heights generated in poh.rs
                tick.tick_height = start_tick_height + i as u64 + 1;
                broadcast_service
                    .entry_sender
                    .send(vec![tick])
                    .expect("Expect successful send to broadcast service");
            }

            sleep(Duration::from_millis(2000));
            let db_ledger = broadcast_service.db_ledger;
            for i in 0..max_tick_height - start_tick_height {
                let (_, slot) = leader_scheduler
                    .read()
                    .unwrap()
                    .get_scheduled_leader(start_tick_height + i + 1)
                    .expect("Leader should exist");
                let result = db_ledger
                    .data_cf
                    .get_by_slot_index(&db_ledger.db, slot, entry_height + i)
                    .unwrap();

                assert!(result.is_some());
            }

            broadcast_service.exit_signal.store(true, Ordering::Relaxed);
            broadcast_service
                .broadcast_service
                .join()
                .expect("Expect successful join of broadcast service");
        }

        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
