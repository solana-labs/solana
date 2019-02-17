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

use crate::bank::Bank;
use crate::blob_fetch_stage::BlobFetchStage;
use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::entry_stream_stage::EntryStreamStage;
use crate::leader_scheduler::LeaderScheduler;
use crate::replay_stage::ReplayStage;
use crate::retransmit_stage::RetransmitStage;
use crate::service::Service;
use crate::storage_stage::{StorageStage, StorageState};
use crate::tpu::{TpuReturnType, TpuRotationReceiver, TpuRotationSender};
use crate::voting_keypair::VotingKeypair;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, RwLock};
use std::thread;

pub type TvuReturnType = TpuReturnType;
pub type TvuRotationSender = TpuRotationSender;
pub type TvuRotationReceiver = TpuRotationReceiver;

pub struct Tvu {
    fetch_stage: BlobFetchStage,
    retransmit_stage: RetransmitStage,
    replay_stage: ReplayStage,
    entry_stream_stage: Option<EntryStreamStage>,
    storage_stage: StorageStage,
    exit: Arc<AtomicBool>,
    last_entry_id: Arc<RwLock<Hash>>,
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
    /// * `blob_index` - Index of last processed blob
    /// * `last_entry_id` - Hash of the last entry
    /// * `cluster_info` - The cluster_info state.
    /// * `sockets` - My fetch, repair, and restransmit sockets
    /// * `blocktree` - the ledger itself
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new(
        voting_keypair: Option<Arc<VotingKeypair>>,
        bank: &Arc<Bank>,
        blob_index: u64,
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
            bank,
            blocktree.clone(),
            &cluster_info,
            Arc::new(retransmit_socket),
            repair_socket,
            blob_fetch_receiver,
            leader_scheduler.clone(),
            exit.clone(),
        );

        let l_last_entry_id = Arc::new(RwLock::new(last_entry_id));

        let (replay_stage, mut previous_receiver) = ReplayStage::new(
            keypair.pubkey(),
            voting_keypair,
            blocktree.clone(),
            bank.clone(),
            cluster_info.clone(),
            exit.clone(),
            blob_index,
            l_last_entry_id.clone(),
            to_leader_sender,
            ledger_signal_receiver,
            &leader_scheduler,
        );

        let entry_stream_stage = if entry_stream.is_some() {
            let (entry_stream_stage, entry_stream_receiver) = EntryStreamStage::new(
                previous_receiver,
                entry_stream.unwrap().to_string(),
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
            last_entry_id: l_last_entry_id,
        }
    }

    #[cfg(test)]
    pub fn get_pause(&self) -> Arc<AtomicBool> {
        self.replay_stage.get_pause()
    }

    pub fn get_state(&self) -> Hash {
        *self.last_entry_id.read().unwrap()
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
    use crate::bank::Bank;
    use crate::blocktree::{get_tmp_ledger_path, BlocktreeConfig};
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::next_entry_mut;
    use crate::entry::EntrySlice;
    use crate::genesis_block::GenesisBlock;
    use crate::gossip_service::GossipService;
    use crate::leader_scheduler::LeaderSchedulerConfig;
    use crate::packet::index_blobs;
    use crate::storage_stage::STORAGE_ROTATE_TEST_COUNT;
    use crate::streamer;
    use solana_sdk::system_transaction::SystemTransaction;
    use std::fs::remove_dir_all;
    use std::time::Duration;

    fn new_gossip(
        cluster_info: Arc<RwLock<ClusterInfo>>,
        gossip: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> GossipService {
        GossipService::new(&cluster_info, None, gossip, exit)
    }

    #[test]
    fn test_tvu_exit() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(target1_keypair.pubkey());

        let starting_balance = 10_000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new(starting_balance);
        let bank = Arc::new(Bank::new(&genesis_block));
        let leader_scheduler_config = LeaderSchedulerConfig::default();
        let leader_scheduler = LeaderScheduler::new_with_bank(&leader_scheduler_config, &bank);
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
            &bank,
            0,
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
        );
        tvu.close().expect("close");
    }

    /// Test that message sent from leader to target1 and replayed to target2
    #[test]
    fn test_replay() {
        solana_logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(target1_keypair.pubkey());
        let target2 = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));

        // start cluster_info_l
        let mut cluster_info_l = ClusterInfo::new(leader.info.clone());
        cluster_info_l.set_leader(leader.info.id);

        let cref_l = Arc::new(RwLock::new(cluster_info_l));
        let dr_l = new_gossip(cref_l, leader.sockets.gossip, exit.clone());

        // start cluster_info2
        let mut cluster_info2 = ClusterInfo::new(target2.info.clone());
        cluster_info2.insert_info(leader.info.clone());
        cluster_info2.set_leader(leader.info.id);
        let cref2 = Arc::new(RwLock::new(cluster_info2));
        let dr_2 = new_gossip(cref2, target2.sockets.gossip, exit.clone());

        // setup some blob services to send blobs into the socket
        // to simulate the source peer and get blobs out of the socket to
        // simulate target peer
        let (s_reader, r_reader) = channel();
        let blob_sockets: Vec<Arc<UdpSocket>> =
            target2.sockets.tvu.into_iter().map(Arc::new).collect();

        let t_receiver = streamer::blob_receiver(blob_sockets[0].clone(), exit.clone(), s_reader);

        // simulate leader sending messages
        let (s_responder, r_responder) = channel();
        let t_responder = streamer::responder(
            "test_replay",
            Arc::new(leader.sockets.retransmit),
            r_responder,
        );

        // TODO: Fix this test so it always works with the default
        // LeaderSchedulerConfig/BlocktreeConfig configuration
        let mut leader_scheduler_config = LeaderSchedulerConfig::default();
        leader_scheduler_config.ticks_per_slot = 64;
        let blocktree_config = BlocktreeConfig::new(leader_scheduler_config.ticks_per_slot);

        let starting_balance = 10_000;
        let (genesis_block, mint_keypair) = GenesisBlock::new(starting_balance);
        let tvu_addr = target1.info.tvu;
        let bank = Arc::new(Bank::new(&genesis_block));
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::new_with_bank(
            &leader_scheduler_config,
            &bank,
        )));
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), starting_balance);

        // start cluster_info1
        let mut cluster_info1 = ClusterInfo::new(target1.info.clone());
        cluster_info1.insert_info(leader.info.clone());
        cluster_info1.set_leader(leader.info.id);
        let cref1 = Arc::new(RwLock::new(cluster_info1));
        let dr_1 = new_gossip(cref1.clone(), target1.sockets.gossip, exit.clone());

        let mut cur_hash = Hash::default();
        let blocktree_path = get_tmp_ledger_path("test_replay");

        let (blocktree, l_receiver) =
            Blocktree::open_with_config_signal(&blocktree_path, &blocktree_config)
                .expect("Expected to successfully open ledger");
        let vote_account_keypair = Arc::new(Keypair::new());
        let voting_keypair = VotingKeypair::new_local(&vote_account_keypair);
        let (sender, _) = channel();
        let tvu = Tvu::new(
            Some(Arc::new(voting_keypair)),
            &bank,
            0,
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
        );

        let mut alice_ref_balance = starting_balance;
        let mut msgs = Vec::new();
        let mut blob_idx = 0;
        let num_transfers = 10;
        let transfer_amount = 501;
        let bob_keypair = Keypair::new();
        for i in 0..num_transfers {
            let entry0 = next_entry_mut(&mut cur_hash, i, vec![]);
            let entry_tick0 = next_entry_mut(&mut cur_hash, i + 1, vec![]);

            let tx0 = SystemTransaction::new_account(
                &mint_keypair,
                bob_keypair.pubkey(),
                transfer_amount,
                cur_hash,
                0,
            );
            let entry_tick1 = next_entry_mut(&mut cur_hash, i + 1, vec![]);
            let entry1 = next_entry_mut(&mut cur_hash, i + num_transfers, vec![tx0]);
            let entry_tick2 = next_entry_mut(&mut cur_hash, i + 1, vec![]);

            alice_ref_balance -= transfer_amount;

            let entries = vec![entry0, entry_tick0, entry_tick1, entry1, entry_tick2];
            let blobs = entries.to_shared_blobs();
            index_blobs(&blobs, &mut blob_idx, &vec![0; blobs.len()]);
            blobs
                .iter()
                .for_each(|b| b.write().unwrap().meta.set_addr(&tvu_addr));
            msgs.extend(blobs.into_iter());
        }

        // send the blobs into the socket
        s_responder.send(msgs).expect("send");
        drop(s_responder);

        // receive retransmitted messages
        let timer = Duration::new(1, 0);
        while let Ok(_msg) = r_reader.recv_timeout(timer) {
            trace!("got msg");
        }

        let alice_balance = bank.get_balance(&mint_keypair.pubkey());
        assert_eq!(alice_balance, alice_ref_balance);

        let bob_balance = bank.get_balance(&bob_keypair.pubkey());
        assert_eq!(bob_balance, starting_balance - alice_ref_balance);

        tvu.close().expect("close");
        exit.store(true, Ordering::Relaxed);
        dr_l.join().expect("join");
        dr_2.join().expect("join");
        dr_1.join().expect("join");
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&blocktree_path);
    }
}
