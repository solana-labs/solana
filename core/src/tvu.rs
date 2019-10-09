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
use crate::blockstream_service::BlockstreamService;
use crate::blocktree::{Blocktree, CompletedSlotsReceiver};
use crate::cluster_info::ClusterInfo;
use crate::confidence::ForkConfidenceCache;
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::ledger_cleanup_service::LedgerCleanupService;
use crate::poh_recorder::PohRecorder;
use crate::replay_stage::ReplayStage;
use crate::retransmit_stage::RetransmitStage;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::shred_fetch_stage::ShredFetchStage;
use crate::snapshot_package::SnapshotPackagerService;
use crate::storage_stage::{StorageStage, StorageState};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

pub struct Tvu {
    fetch_stage: ShredFetchStage,
    retransmit_stage: RetransmitStage,
    replay_stage: ReplayStage,
    blockstream_service: Option<BlockstreamService>,
    ledger_cleanup_service: Option<LedgerCleanupService>,
    storage_stage: StorageStage,
    snapshot_packager_service: Option<SnapshotPackagerService>,
}

pub struct Sockets {
    pub fetch: Vec<UdpSocket>,
    pub repair: UdpSocket,
    pub retransmit: UdpSocket,
    pub forwards: Vec<UdpSocket>,
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
        voting_keypair: Option<&Arc<T>>,
        storage_keypair: &Arc<Keypair>,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sockets: Sockets,
        blocktree: Arc<Blocktree>,
        storage_state: &StorageState,
        blockstream_unix_socket: Option<&PathBuf>,
        max_ledger_slots: Option<u64>,
        ledger_signal_receiver: Receiver<bool>,
        subscriptions: &Arc<RpcSubscriptions>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        exit: &Arc<AtomicBool>,
        completed_slots_receiver: CompletedSlotsReceiver,
        fork_confidence_cache: Arc<RwLock<ForkConfidenceCache>>,
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
            forwards: tvu_forward_sockets,
        } = sockets;

        let (fetch_sender, fetch_receiver) = channel();

        let repair_socket = Arc::new(repair_socket);
        let mut fetch_sockets: Vec<Arc<UdpSocket>> =
            fetch_sockets.into_iter().map(Arc::new).collect();
        fetch_sockets.push(repair_socket.clone());
        let forward_sockets: Vec<Arc<UdpSocket>> =
            tvu_forward_sockets.into_iter().map(Arc::new).collect();
        let fetch_stage =
            ShredFetchStage::new(fetch_sockets, forward_sockets, &fetch_sender, &exit);

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
            fetch_receiver,
            &exit,
            completed_slots_receiver,
            *bank_forks.read().unwrap().working_bank().epoch_schedule(),
        );

        let (blockstream_slot_sender, blockstream_slot_receiver) = channel();
        let (ledger_cleanup_slot_sender, ledger_cleanup_slot_receiver) = channel();
        let (snapshot_packager_service, snapshot_package_sender) = {
            let snapshot_config = { bank_forks.read().unwrap().snapshot_config().clone() };
            if snapshot_config.is_some() {
                // Start a snapshot packaging service
                let (sender, receiver) = channel();
                let snapshot_packager_service = SnapshotPackagerService::new(receiver, exit);
                (Some(snapshot_packager_service), Some(sender))
            } else {
                (None, None)
            }
        };

        let (replay_stage, root_bank_receiver) = ReplayStage::new(
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
            leader_schedule_cache,
            vec![blockstream_slot_sender, ledger_cleanup_slot_sender],
            snapshot_package_sender,
            fork_confidence_cache,
        );

        let blockstream_service = if let Some(blockstream_unix_socket) = blockstream_unix_socket {
            let blockstream_service = BlockstreamService::new(
                blockstream_slot_receiver,
                blocktree.clone(),
                blockstream_unix_socket,
                &exit,
            );
            Some(blockstream_service)
        } else {
            None
        };

        let ledger_cleanup_service = max_ledger_slots.map(|max_ledger_slots| {
            LedgerCleanupService::new(
                ledger_cleanup_slot_receiver,
                blocktree.clone(),
                max_ledger_slots,
                &exit,
            )
        });

        let storage_stage = StorageStage::new(
            storage_state,
            root_bank_receiver,
            Some(blocktree),
            &keypair,
            storage_keypair,
            &exit,
            &bank_forks,
            &cluster_info,
        );

        Tvu {
            fetch_stage,
            retransmit_stage,
            replay_stage,
            blockstream_service,
            ledger_cleanup_service,
            storage_stage,
            snapshot_packager_service,
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
        if self.ledger_cleanup_service.is_some() {
            self.ledger_cleanup_service.unwrap().join()?;
        }
        self.replay_stage.join()?;
        if let Some(s) = self.snapshot_packager_service {
            s.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::banking_stage::create_test_recorder;
    use crate::blocktree::create_new_tmp_ledger;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use solana_runtime::bank::Bank;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_tvu_exit() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(&target1_keypair.pubkey());

        let starting_balance = 10_000;
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(starting_balance);

        let bank_forks = BankForks::new(0, Bank::new(&genesis_block));

        //start cluster_info1
        let mut cluster_info1 = ClusterInfo::new_with_invalid_keypair(target1.info.clone());
        cluster_info1.insert_info(leader.info.clone());
        let cref1 = Arc::new(RwLock::new(cluster_info1));

        let (blocktree_path, _) = create_new_tmp_ledger!(&genesis_block);
        let (blocktree, l_receiver, completed_slots_receiver) =
            Blocktree::open_with_signal(&blocktree_path)
                .expect("Expected to successfully open ledger");
        let blocktree = Arc::new(blocktree);
        let bank = bank_forks.working_bank();
        let (exit, poh_recorder, poh_service, _entry_receiver) =
            create_test_recorder(&bank, &blocktree);
        let voting_keypair = Keypair::new();
        let storage_keypair = Arc::new(Keypair::new());
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let fork_confidence_cache = Arc::new(RwLock::new(ForkConfidenceCache::default()));
        let tvu = Tvu::new(
            &voting_keypair.pubkey(),
            Some(&Arc::new(voting_keypair)),
            &storage_keypair,
            &Arc::new(RwLock::new(bank_forks)),
            &cref1,
            {
                Sockets {
                    repair: target1.sockets.repair,
                    retransmit: target1.sockets.retransmit,
                    fetch: target1.sockets.tvu,
                    forwards: target1.sockets.tvu_forwards,
                }
            },
            blocktree,
            &StorageState::default(),
            None,
            None,
            l_receiver,
            &Arc::new(RpcSubscriptions::default()),
            &poh_recorder,
            &leader_schedule_cache,
            &exit,
            completed_slots_receiver,
            fork_confidence_cache,
        );
        exit.store(true, Ordering::Relaxed);
        tvu.join().unwrap();
        poh_service.join().unwrap();
    }
}
