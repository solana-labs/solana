//! The `broadcast_service` broadcasts data from a leader node to validators
//!
use crate::bank::Bank;
use crate::cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo, DATA_PLANE_FANOUT};
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::entry::Entry;
use crate::entry::EntrySlice;
#[cfg(feature = "erasure")]
use crate::erasure::CodingGenerator;
use crate::leader_scheduler::LeaderScheduler;
use crate::packet::index_blobs;
use crate::result::{Error, Result};
use crate::service::Service;
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

struct Broadcast {
    id: Pubkey,
    max_tick_height: Option<u64>,
    blob_index: u64,

    #[cfg(feature = "erasure")]
    coding_generator: CodingGenerator,
}

impl Broadcast {
    fn run(
        &mut self,
        broadcast_table: &[NodeInfo],
        receiver: &Receiver<Vec<Entry>>,
        sock: &UdpSocket,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
        db_ledger: &Arc<DbLedger>,
    ) -> Result<()> {
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

        let last_tick = match self.max_tick_height {
            Some(max_tick_height) => {
                if let Some(Some(last)) = ventries.last().map(|entries| entries.last()) {
                    last.tick_height == max_tick_height
                } else {
                    false
                }
            }
            None => false,
        };

        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        let to_blobs_start = Instant::now();

        // Generate the slot heights for all the entries inside ventries
        //  this may span slots if this leader broadcasts for consecutive slots...
        let slots = generate_slots(&ventries, leader_scheduler);

        let blobs: Vec<_> = ventries
            .into_par_iter()
            .flat_map(|p| p.to_shared_blobs())
            .collect();

        // TODO: blob_index should be slot-relative...
        index_blobs(&blobs, &self.id, self.blob_index, &slots);

        let to_blobs_elapsed = duration_as_ms(&to_blobs_start.elapsed());

        let broadcast_start = Instant::now();

        inc_new_counter_info!("streamer-broadcast-sent", blobs.len());

        db_ledger
            .write_consecutive_blobs(&blobs)
            .expect("Unrecoverable failure to write to database");

        // don't count coding blobs in the blob indexes
        self.blob_index += blobs.len() as u64;

        // Send out data
        ClusterInfo::broadcast(&self.id, last_tick, &broadcast_table, sock, &blobs)?;

        // Fill in the coding blob data from the window data blobs
        #[cfg(feature = "erasure")]
        {
            let coding = self.coding_generator.next(&blobs)?;

            // send out erasures
            ClusterInfo::broadcast(&self.id, false, &broadcast_table, sock, &coding)?;
        }

        let broadcast_elapsed = duration_as_ms(&broadcast_start.elapsed());

        inc_new_counter_info!(
            "broadcast_service-time_ms",
            duration_as_ms(&now.elapsed()) as usize
        );
        info!(
            "broadcast: {} entries, blob time {} broadcast time {}",
            num_entries, to_blobs_elapsed, broadcast_elapsed
        );

        submit(
            influxdb::Point::new("broadcast-service")
                .add_field(
                    "transmit-index",
                    influxdb::Value::Integer(self.blob_index as i64),
                )
                .to_owned(),
        );

        Ok(())
    }
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
        bank: &Arc<Bank>,
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        entry_height: u64,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
        receiver: &Receiver<Vec<Entry>>,
        max_tick_height: Option<u64>,
        exit_signal: &Arc<AtomicBool>,
    ) -> BroadcastServiceReturnType {
        let me = cluster_info.read().unwrap().my_data().clone();

        let mut broadcast = Broadcast {
            id: me.id,
            max_tick_height,
            blob_index: entry_height,
            #[cfg(feature = "erasure")]
            coding_generator: CodingGenerator::new(),
        };

        loop {
            if exit_signal.load(Ordering::Relaxed) {
                return BroadcastServiceReturnType::ExitSignal;
            }
            let mut broadcast_table = cluster_info.read().unwrap().sorted_tvu_peers(&bank);
            // Layer 1, leader nodes are limited to the fanout size.
            broadcast_table.truncate(DATA_PLANE_FANOUT);
            inc_new_counter_info!("broadcast_service-num_peers", broadcast_table.len() + 1);
            if let Err(e) = broadcast.run(
                &broadcast_table,
                receiver,
                sock,
                leader_scheduler,
                db_ledger,
            ) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                        return BroadcastServiceReturnType::ChannelDisconnected;
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
    pub fn new(
        db_ledger: Arc<DbLedger>,
        bank: Arc<Bank>,
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        entry_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        receiver: Receiver<Vec<Entry>>,
        max_tick_height: Option<u64>,
        exit_sender: Arc<AtomicBool>,
    ) -> Self {
        let exit_signal = Arc::new(AtomicBool::new(false));
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_sender);
                Self::run(
                    &db_ledger,
                    &bank,
                    &sock,
                    &cluster_info,
                    entry_height,
                    &leader_scheduler,
                    &receiver,
                    max_tick_height,
                    &exit_signal,
                )
            })
            .unwrap();

        Self { thread_hdl }
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
    use crate::db_ledger::get_tmp_ledger_path;
    use crate::db_ledger::DbLedger;
    use crate::entry::create_ticks;
    use crate::service::Service;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    struct MockBroadcastService {
        db_ledger: Arc<DbLedger>,
        broadcast_service: BroadcastService,
    }

    fn setup_dummy_broadcast_service(
        leader_pubkey: Pubkey,
        ledger_path: &str,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        entry_receiver: Receiver<Vec<Entry>>,
        entry_height: u64,
        max_tick_height: u64,
    ) -> MockBroadcastService {
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

        let exit_sender = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(Bank::default());

        // Start up the broadcast stage
        let broadcast_service = BroadcastService::new(
            db_ledger.clone(),
            bank.clone(),
            leader_info.sockets.broadcast,
            cluster_info,
            entry_height,
            leader_scheduler,
            entry_receiver,
            Some(max_tick_height),
            exit_sender,
        );

        MockBroadcastService {
            db_ledger,
            broadcast_service,
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
            let (entry_sender, entry_receiver) = channel();
            let broadcast_service = setup_dummy_broadcast_service(
                leader_keypair.pubkey(),
                &ledger_path,
                leader_scheduler.clone(),
                entry_receiver,
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
                entry_sender
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
                let result = db_ledger.get_data_blob(slot, entry_height + i).unwrap();

                assert!(result.is_some());
            }

            drop(entry_sender);
            broadcast_service
                .broadcast_service
                .join()
                .expect("Expect successful join of broadcast service");
        }

        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
