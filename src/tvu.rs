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
use crate::blockstream_service::BlockstreamService;
use crate::blocktree::Blocktree;
use crate::blocktree_processor::BankForksInfo;
use crate::cluster_info::ClusterInfo;
use crate::leader_scheduler::LeaderScheduler;
use crate::replay_stage::ReplayStage;
use crate::retransmit_stage::RetransmitStage;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::storage_stage::{StorageStage, StorageState};
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;

pub struct TvuRotationInfo {
    pub bank: Arc<Bank>,        // Frozen bank representing the last voted on state
    pub last_entry_id: Hash,    // last_entry_id of the current working_bank()
    pub next_slot: u64,         // next slot
    pub next_leader_id: Pubkey, // leader for the next slot
}
pub type TvuRotationSender = Sender<TvuRotationInfo>;
pub type TvuRotationReceiver = Receiver<TvuRotationInfo>;

pub struct Tvu {
    fetch_stage: BlobFetchStage,
    retransmit_stage: RetransmitStage,
    replay_stage: ReplayStage,
    blockstream_service: Option<BlockstreamService>,
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
    /// * `cluster_info` - The cluster_info state.
    /// * `sockets` - fetch, repair, and retransmit sockets
    /// * `blocktree` - the ledger itself
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new<T>(
        voting_keypair: Option<Arc<T>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        bank_forks_info: &[BankForksInfo],
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sockets: Sockets,
        blocktree: Arc<Blocktree>,
        storage_rotate_count: u64,
        to_leader_sender: &TvuRotationSender,
        storage_state: &StorageState,
        blockstream: Option<&String>,
        ledger_signal_receiver: Receiver<bool>,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        subscriptions: &Arc<RpcSubscriptions>,
    ) -> Self
    where
        T: 'static + KeypairUtil + Sync + Send,
    {
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

        let (replay_stage, mut previous_receiver) = ReplayStage::new(
            keypair.pubkey(),
            voting_keypair,
            blocktree.clone(),
            &bank_forks,
            &bank_forks_info,
            cluster_info.clone(),
            exit.clone(),
            to_leader_sender,
            ledger_signal_receiver,
            &leader_scheduler,
            subscriptions,
        );

        let blockstream_service = if blockstream.is_some() {
            let (blockstream_service, blockstream_receiver) = BlockstreamService::new(
                previous_receiver,
                blockstream.unwrap().to_string(),
                bank_forks.read().unwrap().working_bank().tick_height(), // TODO: BlockstreamService needs to deal with BankForks somehow still
                leader_scheduler,
                exit.clone(),
            );
            previous_receiver = blockstream_receiver;
            Some(blockstream_service)
        } else {
            None
        };

        let storage_stage = StorageStage::new(
            storage_state,
            previous_receiver,
            Some(blocktree),
            &keypair,
            &exit.clone(),
            bank_forks_info[0].entry_height, // TODO: StorageStage needs to deal with BankForks somehow still
            storage_rotate_count,
            &cluster_info,
        );

        Tvu {
            fetch_stage,
            retransmit_stage,
            replay_stage,
            blockstream_service,
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
        if self.blockstream_service.is_some() {
            self.blockstream_service.unwrap().join()?;
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
    use solana_sdk::hash::Hash;

    #[test]
    fn test_tvu_exit() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(target1_keypair.pubkey());

        let starting_balance = 10_000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new(starting_balance);

        let bank_forks = BankForks::new(0, Bank::new(&genesis_block));
        let bank_forks_info = vec![BankForksInfo {
            bank_id: 0,
            entry_height: 0,
            last_entry_id: Hash::default(),
        }];
        let leader_scheduler_config = LeaderSchedulerConfig::default();
        let leader_scheduler =
            LeaderScheduler::new_with_bank(&leader_scheduler_config, &bank_forks.working_bank());
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));

        //start cluster_info1
        let mut cluster_info1 = ClusterInfo::new(target1.info.clone());
        cluster_info1.insert_info(leader.info.clone());
        cluster_info1.set_leader(leader.info.id);
        let cref1 = Arc::new(RwLock::new(cluster_info1));

        let blocktree_path = get_tmp_ledger_path("test_tvu_exit");
        let (blocktree, l_receiver) = Blocktree::open_with_signal(&blocktree_path)
            .expect("Expected to successfully open ledger");
        let (sender, _receiver) = channel();
        let tvu = Tvu::new(
            Some(Arc::new(Keypair::new())),
            &Arc::new(RwLock::new(bank_forks)),
            &bank_forks_info,
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
