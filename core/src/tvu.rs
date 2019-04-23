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
use crate::entry::{EntryReceiver, EntrySender};
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::poh_recorder::PohRecorder;
use crate::replay_stage::ReplayStage;
use crate::retransmit_stage::RetransmitStage;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::storage_stage::{StorageStage, StorageState};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

pub struct Tvu {
    fetch_stage: BlobFetchStage,
    retransmit_stage: RetransmitStage,
    replay_stage: ReplayStage,
    blockstream_service: Option<BlockstreamService>,
    storage_stage: StorageStage,
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
        vote_account: &Pubkey,
        voting_keypair: Option<Arc<T>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        bank_forks_info: &[BankForksInfo],
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sockets: Sockets,
        blocktree: Arc<Blocktree>,
        storage_rotate_count: u64,
        storage_state: &StorageState,
        blockstream: Option<&String>,
        ledger_signal_receiver: Receiver<bool>,
        subscriptions: &Arc<RpcSubscriptions>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        storage_entry_sender: EntrySender,
        storage_entry_receiver: EntryReceiver,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        exit: &Arc<AtomicBool>,
    ) -> Self
    where
        T: 'static + KeypairUtil + Sync + Send,
    {
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
        let fetch_stage = BlobFetchStage::new_multi_socket(blob_sockets, &blob_fetch_sender, &exit);

        //TODO
        //the packets coming out of blob_receiver need to be sent to the GPU and verified
        //then sent to the window, which does the erasure coding reconstruction
        let retransmit_stage = RetransmitStage::new(
            bank_forks.clone(),
            leader_schedule_cache,
            blocktree.clone(),
            &cluster_info,
            Arc::new(retransmit_socket),
            repair_socket,
            blob_fetch_receiver,
            &exit,
        );

        let (replay_stage, slot_full_receiver) = ReplayStage::new(
            &keypair.pubkey(),
            vote_account,
            voting_keypair,
            blocktree.clone(),
            &bank_forks,
            cluster_info.clone(),
            &exit,
            ledger_signal_receiver,
            subscriptions,
            poh_recorder,
            storage_entry_sender,
            leader_schedule_cache,
        );

        let blockstream_service = if blockstream.is_some() {
            let blockstream_service = BlockstreamService::new(
                slot_full_receiver,
                blocktree.clone(),
                blockstream.unwrap().to_string(),
                &exit,
            );
            Some(blockstream_service)
        } else {
            None
        };

        let storage_keypair = Arc::new(Keypair::new());
        let storage_stage = StorageStage::new(
            storage_state,
            storage_entry_receiver,
            Some(blocktree),
            &keypair,
            &storage_keypair,
            &exit,
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
        }
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
    use crate::banking_stage::create_test_recorder;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::storage_stage::STORAGE_ROTATE_TEST_COUNT;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_tvu_exit() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(&target1_keypair.pubkey());

        let starting_balance = 10_000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new(starting_balance);

        let bank_forks = BankForks::new(0, Bank::new(&genesis_block));
        let bank_forks_info = vec![BankForksInfo {
            bank_slot: 0,
            entry_height: 0,
        }];

        //start cluster_info1
        let mut cluster_info1 = ClusterInfo::new_with_invalid_keypair(target1.info.clone());
        cluster_info1.insert_info(leader.info.clone());
        let cref1 = Arc::new(RwLock::new(cluster_info1));

        let blocktree_path = get_tmp_ledger_path!();
        let (blocktree, l_receiver) = Blocktree::open_with_signal(&blocktree_path)
            .expect("Expected to successfully open ledger");
        let blocktree = Arc::new(blocktree);
        let bank = bank_forks.working_bank();
        let (exit, poh_recorder, poh_service, _entry_receiver) =
            create_test_recorder(&bank, &blocktree);
        let voting_keypair = Keypair::new();
        let (storage_entry_sender, storage_entry_receiver) = channel();
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let tvu = Tvu::new(
            &voting_keypair.pubkey(),
            Some(Arc::new(voting_keypair)),
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
            blocktree,
            STORAGE_ROTATE_TEST_COUNT,
            &StorageState::default(),
            None,
            l_receiver,
            &Arc::new(RpcSubscriptions::default()),
            &poh_recorder,
            storage_entry_sender,
            storage_entry_receiver,
            &leader_schedule_cache,
            &exit,
        );
        exit.store(true, Ordering::Relaxed);
        tvu.join().unwrap();
        poh_service.join().unwrap();
    }
}
