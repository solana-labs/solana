//! A stage to broadcast data from a leader node to validators
//!
use crate::blocktree::Blocktree;
use crate::cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo, DATA_PLANE_FANOUT};
use crate::entry::Entry;
use crate::entry::EntrySlice;
#[cfg(feature = "erasure")]
use crate::erasure::CodingGenerator;
use crate::packet::index_blobs;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::staking_utils;
use rayon::prelude::*;
use solana_metrics::counter::Counter;
use solana_metrics::{influxdb, submit};
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    LeaderRotation,
    ChannelDisconnected,
    ExitSignal,
}

struct Broadcast {
    id: Pubkey,
    blob_index: u64,

    #[cfg(feature = "erasure")]
    coding_generator: CodingGenerator,
}

impl Broadcast {
    fn run(
        &mut self,
        slot_height: u64,
        max_tick_height: u64,
        broadcast_table: &[NodeInfo],
        receiver: &Receiver<Vec<(Entry, u64)>>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let entries = receiver.recv_timeout(timer)?;
        let now = Instant::now();
        let mut num_entries = entries.len();
        let mut ventries = Vec::new();
        let mut last_tick = entries.last().map(|v| v.1).unwrap_or(0);
        ventries.push(entries);

        while let Ok(entries) = receiver.try_recv() {
            num_entries += entries.len();
            last_tick = entries.last().map(|v| v.1).unwrap_or(0);
            ventries.push(entries);
        }

        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        let to_blobs_start = Instant::now();

        let blobs: Vec<_> = ventries
            .into_par_iter()
            .flat_map(|p| {
                let entries: Vec<_> = p.into_iter().map(|e| e.0).collect();
                entries.to_shared_blobs()
            })
            .collect();

        // TODO: blob_index should be slot-relative...
        index_blobs(&blobs, &self.id, &mut self.blob_index, slot_height);
        let parent = {
            if slot_height == 0 {
                0
            } else {
                slot_height - 1
            }
        };
        for b in blobs.iter() {
            b.write().unwrap().set_parent(parent);
        }

        let to_blobs_elapsed = duration_as_ms(&to_blobs_start.elapsed());

        let broadcast_start = Instant::now();

        inc_new_counter_info!("streamer-broadcast-sent", blobs.len());

        assert!(last_tick <= max_tick_height);
        let contains_last_tick = last_tick == max_tick_height;

        if contains_last_tick {
            blobs.last().unwrap().write().unwrap().set_is_last_in_slot();
        }

        blocktree.write_shared_blobs(&blobs)?;

        // Send out data
        ClusterInfo::broadcast(&self.id, contains_last_tick, &broadcast_table, sock, &blobs)?;

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
    #[allow(clippy::too_many_arguments)]
    fn run(
        slot_height: u64,
        bank: &Arc<Bank>,
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blob_index: u64,
        receiver: &Receiver<Vec<(Entry, u64)>>,
        exit_signal: &Arc<AtomicBool>,
        blocktree: &Arc<Blocktree>,
    ) -> BroadcastStageReturnType {
        let me = cluster_info.read().unwrap().my_data().clone();

        let mut broadcast = Broadcast {
            id: me.id,
            blob_index,
            #[cfg(feature = "erasure")]
            coding_generator: CodingGenerator::new(),
        };

        let max_tick_height = (slot_height + 1) * bank.ticks_per_slot() - 1;

        loop {
            if exit_signal.load(Ordering::Relaxed) {
                return BroadcastStageReturnType::ExitSignal;
            }
            let mut broadcast_table = cluster_info
                .read()
                .unwrap()
                .sorted_tvu_peers(&staking_utils::node_stakes(&bank));
            // Layer 1, leader nodes are limited to the fanout size.
            broadcast_table.truncate(DATA_PLANE_FANOUT);
            inc_new_counter_info!("broadcast_service-num_peers", broadcast_table.len() + 1);
            if let Err(e) = broadcast.run(
                slot_height,
                max_tick_height,
                &broadcast_table,
                receiver,
                sock,
                blocktree,
            ) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) | Error::SendError => {
                        return BroadcastStageReturnType::ChannelDisconnected;
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slot_height: u64,
        bank: &Arc<Bank>,
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        blob_index: u64,
        receiver: Receiver<Vec<(Entry, u64)>>,
        exit_sender: Arc<AtomicBool>,
        blocktree: &Arc<Blocktree>,
    ) -> Self {
        let exit_signal = Arc::new(AtomicBool::new(false));
        let blocktree = blocktree.clone();
        let bank = bank.clone();
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_sender);
                Self::run(
                    slot_height,
                    &bank,
                    &sock,
                    &cluster_info,
                    blob_index,
                    &receiver,
                    &exit_signal,
                    &blocktree,
                )
            })
            .unwrap();

        Self { thread_hdl }
    }
}

impl Service for BroadcastStage {
    type JoinReturnType = BroadcastStageReturnType;

    fn join(self) -> thread::Result<BroadcastStageReturnType> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::blocktree::{get_tmp_ledger_path, Blocktree};
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::create_ticks;
    use crate::service::Service;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    struct MockBroadcastStage {
        blocktree: Arc<Blocktree>,
        broadcast_service: BroadcastStage,
    }

    fn setup_dummy_broadcast_service(
        slot_height: u64,
        leader_pubkey: Pubkey,
        ledger_path: &str,
        entry_receiver: Receiver<Vec<(Entry, u64)>>,
        blob_index: u64,
    ) -> MockBroadcastStage {
        // Make the database ledger
        let blocktree = Arc::new(Blocktree::open(ledger_path).unwrap());

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
        let broadcast_service = BroadcastStage::new(
            slot_height,
            &bank,
            leader_info.sockets.broadcast,
            cluster_info,
            blob_index,
            entry_receiver,
            exit_sender,
            &blocktree,
        );

        MockBroadcastStage {
            blocktree,
            broadcast_service,
        }
    }

    #[test]
    #[ignore]
    //TODO this test won't work since broadcast stage no longer edits the ledger
    fn test_broadcast_ledger() {
        let ledger_path = get_tmp_ledger_path("test_broadcast_ledger");
        {
            // Create the leader scheduler
            let leader_keypair = Keypair::new();
            let start_tick_height = 0;
            let max_tick_height = start_tick_height + DEFAULT_TICKS_PER_SLOT;

            let (entry_sender, entry_receiver) = channel();
            let broadcast_service = setup_dummy_broadcast_service(
                0,
                leader_keypair.pubkey(),
                &ledger_path,
                entry_receiver,
                0,
            );

            let ticks = create_ticks(max_tick_height - start_tick_height, Hash::default());
            for (i, tick) in ticks.into_iter().enumerate() {
                entry_sender
                    .send(vec![(tick, i as u64 + 1)])
                    .expect("Expect successful send to broadcast service");
            }

            sleep(Duration::from_millis(2000));
            let blocktree = broadcast_service.blocktree;
            let mut blob_index = 0;
            for i in 0..max_tick_height - start_tick_height {
                let slot = (start_tick_height + i + 1) / DEFAULT_TICKS_PER_SLOT;

                let result = blocktree.get_data_blob(slot, blob_index).unwrap();

                blob_index += 1;
                assert!(result.is_some());
            }

            drop(entry_sender);
            broadcast_service
                .broadcast_service
                .join()
                .expect("Expect successful join of broadcast service");
        }

        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
