//! A stage to broadcast data from a leader node to validators
//!
use crate::blocktree::Blocktree;
use crate::cluster_info::{ClusterInfo, ClusterInfoError};
use crate::entry::EntrySlice;
use crate::erasure::CodingGenerator;
use crate::packet::index_blobs;
use crate::poh_recorder::WorkingBankEntries;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::staking_utils;
use rayon::prelude::*;
use rayon::ThreadPool;
use solana_metrics::{
    datapoint, inc_new_counter_debug, inc_new_counter_error, inc_new_counter_info,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signable;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

pub const NUM_THREADS: u32 = 10;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    ChannelDisconnected,
}

#[derive(Default)]
struct BroadcastStats {
    num_entries: Vec<usize>,
    run_elapsed: Vec<u64>,
    to_blobs_elapsed: Vec<u64>,
}

struct Broadcast {
    id: Pubkey,
    coding_generator: CodingGenerator,
    stats: BroadcastStats,
    thread_pool: ThreadPool,
}

impl Broadcast {
    fn run(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let (mut bank, entries) = receiver.recv_timeout(timer)?;
        let mut max_tick_height = bank.max_tick_height();

        let run_start = Instant::now();
        let mut num_entries = entries.len();
        let mut ventries = Vec::new();
        let mut last_tick = entries.last().map(|v| v.1).unwrap_or(0);
        ventries.push(entries);

        assert!(last_tick <= max_tick_height);
        if last_tick != max_tick_height {
            while let Ok((same_bank, entries)) = receiver.try_recv() {
                // If the bank changed, that implies the previous slot was interrupted and we do not have to
                // broadcast its entries.
                if same_bank.slot() != bank.slot() {
                    num_entries = 0;
                    ventries.clear();
                    bank = same_bank.clone();
                    max_tick_height = bank.max_tick_height();
                }
                num_entries += entries.len();
                last_tick = entries.last().map(|v| v.1).unwrap_or(0);
                ventries.push(entries);
                assert!(last_tick <= max_tick_height,);
                if last_tick == max_tick_height {
                    break;
                }
            }
        }

        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        let to_blobs_start = Instant::now();

        let blobs: Vec<_> = self.thread_pool.install(|| {
            ventries
                .into_par_iter()
                .map(|p| {
                    let entries: Vec<_> = p.into_iter().map(|e| e.0).collect();
                    entries.to_shared_blobs()
                })
                .flatten()
                .collect()
        });

        let blob_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0);

        index_blobs(
            &blobs,
            &self.id,
            blob_index,
            bank.slot(),
            bank.parent().map_or(0, |parent| parent.slot()),
        );

        if last_tick == max_tick_height {
            blobs.last().unwrap().write().unwrap().set_is_last_in_slot();
        }

        // Make sure not to modify the blob header or data after signing it here
        self.thread_pool.install(|| {
            blobs.par_iter().for_each(|b| {
                b.write()
                    .unwrap()
                    .sign(&cluster_info.read().unwrap().keypair);
            })
        });

        blocktree.write_shared_blobs(&blobs)?;

        let coding = self.coding_generator.next(&blobs);

        self.thread_pool.install(|| {
            coding.par_iter().for_each(|c| {
                c.write()
                    .unwrap()
                    .sign(&cluster_info.read().unwrap().keypair);
            })
        });

        let to_blobs_elapsed = duration_as_ms(&to_blobs_start.elapsed());

        let broadcast_start = Instant::now();

        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        // Send out data
        cluster_info
            .read()
            .unwrap()
            .broadcast(sock, &blobs, stakes.as_ref())?;

        inc_new_counter_debug!("streamer-broadcast-sent", blobs.len());

        // send out erasures
        cluster_info
            .read()
            .unwrap()
            .broadcast(sock, &coding, stakes.as_ref())?;

        self.update_broadcast_stats(
            duration_as_ms(&broadcast_start.elapsed()),
            duration_as_ms(&run_start.elapsed()),
            num_entries,
            to_blobs_elapsed,
            blob_index,
        );

        Ok(())
    }

    fn update_broadcast_stats(
        &mut self,
        broadcast_elapsed: u64,
        run_elapsed: u64,
        num_entries: usize,
        to_blobs_elapsed: u64,
        blob_index: u64,
    ) {
        inc_new_counter_info!("broadcast_service-time_ms", broadcast_elapsed as usize);

        self.stats.num_entries.push(num_entries);
        self.stats.to_blobs_elapsed.push(to_blobs_elapsed);
        self.stats.run_elapsed.push(run_elapsed);
        if self.stats.num_entries.len() >= 16 {
            info!(
                "broadcast: entries: {:?} blob times ms: {:?} broadcast times ms: {:?}",
                self.stats.num_entries, self.stats.to_blobs_elapsed, self.stats.run_elapsed
            );
            self.stats.num_entries.clear();
            self.stats.to_blobs_elapsed.clear();
            self.stats.run_elapsed.clear();
        }

        datapoint!("broadcast-service", ("transmit-index", blob_index, i64));
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
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        blocktree: &Arc<Blocktree>,
    ) -> BroadcastStageReturnType {
        let me = cluster_info.read().unwrap().my_data().clone();
        let coding_generator = CodingGenerator::default();

        let mut broadcast = Broadcast {
            id: me.id,
            coding_generator,
            stats: BroadcastStats::default(),
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(sys_info::cpu_num().unwrap_or(NUM_THREADS) as usize)
                .build()
                .unwrap(),
        };

        loop {
            if let Err(e) = broadcast.run(&cluster_info, receiver, sock, blocktree) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) | Error::SendError => {
                        return BroadcastStageReturnType::ChannelDisconnected;
                    }
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    Error::ClusterInfoError(ClusterInfoError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                    _ => {
                        inc_new_counter_error!("streamer-broadcaster-error", 1, 1);
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
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        receiver: Receiver<WorkingBankEntries>,
        exit_sender: &Arc<AtomicBool>,
        blocktree: &Arc<Blocktree>,
    ) -> Self {
        let blocktree = blocktree.clone();
        let exit_sender = exit_sender.clone();
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _finalizer = Finalizer::new(exit_sender);
                Self::run(&sock, &cluster_info, &receiver, &blocktree)
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
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::service::Service;
    use solana_runtime::bank::Bank;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    struct MockBroadcastStage {
        blocktree: Arc<Blocktree>,
        broadcast_service: BroadcastStage,
        bank: Arc<Bank>,
    }

    fn setup_dummy_broadcast_service(
        leader_pubkey: &Pubkey,
        ledger_path: &str,
        entry_receiver: Receiver<WorkingBankEntries>,
    ) -> MockBroadcastStage {
        // Make the database ledger
        let blocktree = Arc::new(Blocktree::open(ledger_path).unwrap());

        // Make the leader node and scheduler
        let leader_info = Node::new_localhost_with_pubkey(leader_pubkey);

        // Make a node to broadcast to
        let buddy_keypair = Keypair::new();
        let broadcast_buddy = Node::new_localhost_with_pubkey(&buddy_keypair.pubkey());

        // Fill the cluster_info with the buddy's info
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(leader_info.info.clone());
        cluster_info.insert_info(broadcast_buddy.info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let exit_sender = Arc::new(AtomicBool::new(false));

        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));

        // Start up the broadcast stage
        let broadcast_service = BroadcastStage::new(
            leader_info.sockets.broadcast,
            cluster_info,
            entry_receiver,
            &exit_sender,
            &blocktree,
        );

        MockBroadcastStage {
            blocktree,
            broadcast_service,
            bank,
        }
    }

    #[test]
    fn test_broadcast_ledger() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path("test_broadcast_ledger");

        {
            // Create the leader scheduler
            let leader_keypair = Keypair::new();

            let (entry_sender, entry_receiver) = channel();
            let broadcast_service = setup_dummy_broadcast_service(
                &leader_keypair.pubkey(),
                &ledger_path,
                entry_receiver,
            );
            let bank = broadcast_service.bank.clone();
            let start_tick_height = bank.tick_height();
            let max_tick_height = bank.max_tick_height();
            let ticks_per_slot = bank.ticks_per_slot();

            let ticks = create_ticks(max_tick_height - start_tick_height, Hash::default());
            for (i, tick) in ticks.into_iter().enumerate() {
                entry_sender
                    .send((bank.clone(), vec![(tick, i as u64 + 1)]))
                    .expect("Expect successful send to broadcast service");
            }

            sleep(Duration::from_millis(2000));

            trace!(
                "[broadcast_ledger] max_tick_height: {}, start_tick_height: {}, ticks_per_slot: {}",
                max_tick_height,
                start_tick_height,
                ticks_per_slot,
            );

            let blocktree = broadcast_service.blocktree;
            let mut blob_index = 0;
            for i in 0..max_tick_height - start_tick_height {
                let slot = (start_tick_height + i + 1) / ticks_per_slot;

                let result = blocktree.get_data_blob(slot, blob_index).unwrap();

                blob_index += 1;
                result.expect("expect blob presence");
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
