//! A stage to broadcast data from a leader node to validators
use self::broadcast_bad_blob_sizes::BroadcastBadBlobSizes;
use self::broadcast_fake_blobs_run::BroadcastFakeBlobsRun;
use self::fail_entry_verification_broadcast_run::FailEntryVerificationBroadcastRun;
use self::standard_broadcast_run::StandardBroadcastRun;
use crate::blocktree::Blocktree;
use crate::cluster_info::{ClusterInfo, ClusterInfoError};
use crate::erasure::CodingGenerator;
use crate::poh_recorder::WorkingBankEntries;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::staking_utils;
use rayon::ThreadPool;
use solana_metrics::{
    datapoint, inc_new_counter_debug, inc_new_counter_error, inc_new_counter_info,
};
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Instant;

mod broadcast_bad_blob_sizes;
mod broadcast_fake_blobs_run;
mod broadcast_utils;
mod fail_entry_verification_broadcast_run;
mod standard_broadcast_run;

pub const NUM_THREADS: u32 = 10;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    ChannelDisconnected,
}

#[derive(PartialEq, Clone, Debug)]
pub enum BroadcastStageType {
    Standard,
    FailEntryVerification,
    BroadcastFakeBlobs,
    BroadcastBadBlobSizes,
}

impl BroadcastStageType {
    pub fn new_broadcast_stage(
        &self,
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        receiver: Receiver<WorkingBankEntries>,
        exit_sender: &Arc<AtomicBool>,
        blocktree: &Arc<Blocktree>,
    ) -> BroadcastStage {
        match self {
            BroadcastStageType::Standard => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                exit_sender,
                blocktree,
                StandardBroadcastRun::new(),
            ),

            BroadcastStageType::FailEntryVerification => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                exit_sender,
                blocktree,
                FailEntryVerificationBroadcastRun::new(),
            ),

            BroadcastStageType::BroadcastFakeBlobs => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                exit_sender,
                blocktree,
                BroadcastFakeBlobsRun::new(0),
            ),

            BroadcastStageType::BroadcastBadBlobSizes => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                exit_sender,
                blocktree,
                BroadcastBadBlobSizes::new(),
            ),
        }
    }
}

trait BroadcastRun {
    fn run(
        &mut self,
        broadcast: &mut Broadcast,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()>;
}

struct Broadcast {
    coding_generator: CodingGenerator,
    thread_pool: ThreadPool,
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
        mut broadcast_stage_run: impl BroadcastRun,
    ) -> BroadcastStageReturnType {
        let coding_generator = CodingGenerator::default();

        let mut broadcast = Broadcast {
            coding_generator,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(sys_info::cpu_num().unwrap_or(NUM_THREADS) as usize)
                .build()
                .unwrap(),
        };

        loop {
            if let Err(e) =
                broadcast_stage_run.run(&mut broadcast, &cluster_info, receiver, sock, blocktree)
            {
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
    fn new(
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        receiver: Receiver<WorkingBankEntries>,
        exit_sender: &Arc<AtomicBool>,
        blocktree: &Arc<Blocktree>,
        broadcast_stage_run: impl BroadcastRun + Send + 'static,
    ) -> Self {
        let blocktree = blocktree.clone();
        let exit_sender = exit_sender.clone();
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _finalizer = Finalizer::new(exit_sender);
                Self::run(
                    &sock,
                    &cluster_info,
                    &receiver,
                    &blocktree,
                    broadcast_stage_run,
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
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::service::Service;
    use solana_runtime::bank::Bank;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
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
            StandardBroadcastRun::new(),
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
