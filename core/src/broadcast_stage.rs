//! A stage to broadcast data from a leader node to validators
use self::broadcast_fake_blobs_run::BroadcastFakeBlobsRun;
use self::fail_entry_verification_broadcast_run::FailEntryVerificationBroadcastRun;
use self::standard_broadcast_run::StandardBroadcastRun;
use crate::cluster_info::{ClusterInfo, ClusterInfoError};
use crate::poh_recorder::WorkingBankEntry;
use crate::result::{Error, Result};
use crate::service::Service;
use solana_ledger::blocktree::Blocktree;
use solana_ledger::staking_utils;
use solana_metrics::{inc_new_counter_error, inc_new_counter_info};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Instant;

mod broadcast_fake_blobs_run;
pub(crate) mod broadcast_utils;
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
}

impl BroadcastStageType {
    pub fn new_broadcast_stage(
        &self,
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        receiver: Receiver<WorkingBankEntry>,
        exit_sender: &Arc<AtomicBool>,
        blocktree: &Arc<Blocktree>,
    ) -> BroadcastStage {
        match self {
            BroadcastStageType::Standard => {
                let keypair = cluster_info.read().unwrap().keypair.clone();
                BroadcastStage::new(
                    sock,
                    cluster_info,
                    receiver,
                    exit_sender,
                    blocktree,
                    StandardBroadcastRun::new(keypair),
                )
            }

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
        }
    }
}

trait BroadcastRun {
    fn run(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntry>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()>;
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
        receiver: &Receiver<WorkingBankEntry>,
        blocktree: &Arc<Blocktree>,
        mut broadcast_stage_run: impl BroadcastRun,
    ) -> BroadcastStageReturnType {
        loop {
            if let Err(e) = broadcast_stage_run.run(&cluster_info, receiver, sock, blocktree) {
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
        receiver: Receiver<WorkingBankEntry>,
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
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::service::Service;
    use solana_ledger::blocktree::{get_tmp_ledger_path, Blocktree};
    use solana_ledger::entry::create_ticks;
    use solana_runtime::bank::Bank;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::path::Path;
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
        ledger_path: &Path,
        entry_receiver: Receiver<WorkingBankEntry>,
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

        let leader_keypair = cluster_info.read().unwrap().keypair.clone();
        // Start up the broadcast stage
        let broadcast_service = BroadcastStage::new(
            leader_info.sockets.broadcast,
            cluster_info,
            entry_receiver,
            &exit_sender,
            &blocktree,
            StandardBroadcastRun::new(leader_keypair),
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
            let start_tick_height;
            let max_tick_height;
            let ticks_per_slot;
            let slot;
            {
                let bank = broadcast_service.bank.clone();
                start_tick_height = bank.tick_height();
                max_tick_height = bank.max_tick_height();
                ticks_per_slot = bank.ticks_per_slot();
                slot = bank.slot();
                let ticks = create_ticks(max_tick_height - start_tick_height, 0, Hash::default());
                for (i, tick) in ticks.into_iter().enumerate() {
                    entry_sender
                        .send((bank.clone(), (tick, i as u64 + 1)))
                        .expect("Expect successful send to broadcast service");
                }
            }
            sleep(Duration::from_millis(2000));

            trace!(
                "[broadcast_ledger] max_tick_height: {}, start_tick_height: {}, ticks_per_slot: {}",
                max_tick_height,
                start_tick_height,
                ticks_per_slot,
            );

            let blocktree = broadcast_service.blocktree;
            let (entries, _) = blocktree
                .get_slot_entries_with_shred_count(slot, 0)
                .expect("Expect entries to be present");
            assert_eq!(entries.len(), max_tick_height as usize);

            drop(entry_sender);
            broadcast_service
                .broadcast_service
                .join()
                .expect("Expect successful join of broadcast service");
        }

        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
