//! The `tvu` module implements the Transaction Validation Unit, a
//! multi-stage transaction validation pipeline in software.
//!
//! 1. BlobFetchStage
//! - Incoming blobs are picked up from the TVU sockets and repair socket.
//! 2. RetransmitStage
//! - Blobs are windowed until a contiguous chunk is available.  This stage also repairs and
//! retransmits blobs that are in the queue.
//! 3. ReplayStage
//! - Transactions in blobs are processed and applied to the bank.
//! - TODO We need to verify the signatures in the blobs.
//! 4. StorageStage
//! - Generating the keys used to encrypt the ledger and sample it for storage mining.

use crate::bank_forks::BankForks;
use crate::blob_fetch_stage::BlobFetchStage;
use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::entry_stream_stage::EntryStreamStage;
use crate::leader_scheduler::LeaderScheduler;
use crate::replay_stage::ReplayStage;
use crate::retransmit_stage::RetransmitStage;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::storage_stage::{StorageStage, StorageState};
use crate::voting_keypair::VotingKeypair;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;

pub type TvuReturnType = (
    u64,    // bank_id,
    u64,    // slot height to initiate a rotation
    Pubkey, // leader upon rotation
);
pub type TvuRotationSender = Sender<TvuReturnType>;
pub type TvuRotationReceiver = Receiver<TvuReturnType>;

pub struct Tvu {
    fetch_stage: BlobFetchStage,
    retransmit_stage: RetransmitStage,
    replay_stage: ReplayStage,
    entry_stream_stage: Option<EntryStreamStage>,
    storage_stage: StorageStage,
    exit: Arc<AtomicBool>,
}

pub struct Sockets {
    pub fetch: Vec<UdpSocket>,
    pub repair: UdpSocket,
    pub retransmit: UdpSocket,
}

impl Tvu {
    /// This service receives messages from a leader in the network and processes the transactions
    /// on the bank state.
    /// # Arguments
    /// * `bank` - The bank state.
    /// * `entry_height` - Initial ledger height
    /// * `last_entry_id` - Hash of the last entry
    /// * `cluster_info` - The cluster_info state.
    /// * `sockets` - My fetch, repair, and restransmit sockets
    /// * `blocktree` - the ledger itself
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new(
        voting_keypair: Option<Arc<VotingKeypair>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        entry_height: u64,
        last_entry_id: Hash,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sockets: Sockets,
        blocktree: Arc<Blocktree>,
        storage_rotate_count: u64,
        to_leader_sender: &TvuRotationSender,
        storage_state: &StorageState,
        entry_stream: Option<&String>,
        ledger_signal_receiver: Receiver<bool>,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        subscriptions: &Arc<RpcSubscriptions>,
    ) -> Self {
        let exit = Arc::new(AtomicBool::new(false));
        let keypair: Arc<Keypair> = cluster_info
            .read()
            .expect("Unable to read from cluster_info during Tvu creation")
            .keypair
            .clone();

        let Sockets {
            repair: repair_socket,
            fetch: fetch_sockets,
            retransmit: retransmit_socket,
        } = sockets;

        let (blob_fetch_sender, blob_fetch_receiver) = channel();

        let repair_socket = Arc::new(repair_socket);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            fetch_sockets.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let fetch_stage =
            BlobFetchStage::new_multi_socket(blob_sockets, &blob_fetch_sender, exit.clone());

        //TODO
        //the packets coming out of blob_receiver need to be sent to the GPU and verified
        //then sent to the window, which does the erasure coding reconstruction
        let retransmit_stage = RetransmitStage::new(
            &bank_forks,
            blocktree.clone(),
            &cluster_info,
            Arc::new(retransmit_socket),
            repair_socket,
            blob_fetch_receiver,
            leader_scheduler.clone(),
            exit.clone(),
        );

        let bank = bank_forks.read().unwrap().working_bank();
        let (replay_stage, mut previous_receiver) = ReplayStage::new(
            keypair.pubkey(),
            voting_keypair,
            blocktree.clone(),
            bank.clone(),
            cluster_info.clone(),
            exit.clone(),
            last_entry_id,
            to_leader_sender,
            ledger_signal_receiver,
            &leader_scheduler,
            subscriptions,
        );

        let entry_stream_stage = if entry_stream.is_some() {
            let (entry_stream_stage, entry_stream_receiver) = EntryStreamStage::new(
                previous_receiver,
                entry_stream.unwrap().to_string(),
                bank.tick_height(),
                leader_scheduler,
                exit.clone(),
            );
            previous_receiver = entry_stream_receiver;
            Some(entry_stream_stage)
        } else {
            None
        };

        let storage_stage = StorageStage::new(
            storage_state,
            previous_receiver,
            Some(blocktree),
            &keypair,
            &exit.clone(),
            entry_height,
            storage_rotate_count,
            &cluster_info,
        );

        Tvu {
            fetch_stage,
            retransmit_stage,
            replay_stage,
            entry_stream_stage,
            storage_stage,
            exit,
        }
    }

    pub fn is_exited(&self) -> bool {
        self.exit.load(Ordering::Relaxed)
    }

    pub fn exit(&self) {
        // Call exit to make sure replay stage is unblocked from a channel it may be blocked on.
        // Then replay stage will set the self.exit variable and cause the rest of the
        // pipeline to exit
        self.replay_stage.exit();
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }
}

impl Service for Tvu {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.retransmit_stage.join()?;
        self.fetch_stage.join()?;
        self.storage_stage.join()?;
        if self.entry_stream_stage.is_some() {
            self.entry_stream_stage.unwrap().join()?;
        }
        self.replay_stage.join()?;
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::leader_scheduler::LeaderSchedulerConfig;
    use crate::storage_stage::STORAGE_ROTATE_TEST_COUNT;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;

    #[test]
    fn test_tvu_exit() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(target1_keypair.pubkey());

        let starting_balance = 10_000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new(starting_balance);

        let bank_forks = BankForks::new(0, Bank::new(&genesis_block));
        let leader_scheduler_config = LeaderSchedulerConfig::default();
        let leader_scheduler =
            LeaderScheduler::new_with_bank(&leader_scheduler_config, &bank_forks.working_bank());
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));

        //start cluster_info1
        let mut cluster_info1 = ClusterInfo::new(target1.info.clone());
        cluster_info1.insert_info(leader.info.clone());
        cluster_info1.set_leader(leader.info.id);
        let cref1 = Arc::new(RwLock::new(cluster_info1));

        let cur_hash = Hash::default();
        let blocktree_path = get_tmp_ledger_path("test_tvu_exit");
        let (blocktree, l_receiver) = Blocktree::open_with_signal(&blocktree_path)
            .expect("Expected to successfully open ledger");
        let vote_account_keypair = Arc::new(Keypair::new());
        let voting_keypair = VotingKeypair::new_local(&vote_account_keypair);
        let (sender, _receiver) = channel();
        let tvu = Tvu::new(
            Some(Arc::new(voting_keypair)),
            &Arc::new(RwLock::new(bank_forks)),
            0,
            cur_hash,
            &cref1,
            {
                Sockets {
                    repair: target1.sockets.repair,
                    retransmit: target1.sockets.retransmit,
                    fetch: target1.sockets.tvu,
                }
            },
            Arc::new(blocktree),
            STORAGE_ROTATE_TEST_COUNT,
            &sender,
            &StorageState::default(),
            None,
            l_receiver,
            leader_scheduler,
            &Arc::new(RpcSubscriptions::default()),
        );
        tvu.close().expect("close");
    }
}
