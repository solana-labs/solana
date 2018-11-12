//! The `tvu` module implements the Transaction Validation Unit, a
//! 3-stage transaction validation pipeline in software.
//!
//! 1. Fetch Stage
//! - Incoming blobs are picked up from the replicate socket and repair socket.
//! 2. SharedWindow Stage
//! - Blobs are windowed until a contiguous chunk is available.  This stage also repairs and
//! retransmits blobs that are in the queue.
//! 3. Replicate Stage
//! - Transactions in blobs are processed and applied to the bank.
//! - TODO We need to verify the signatures in the blobs.

use bank::Bank;
use blob_fetch_stage::BlobFetchStage;
use cluster_info::ClusterInfo;
use hash::Hash;
use ledger_write_stage::LedgerWriteStage;
use replicate_stage::{ReplicateStage, ReplicateStageReturnType};
use retransmit_stage::RetransmitStage;
use service::Service;
use signature::Keypair;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use storage_stage::{StorageStage, StorageState};
use window::SharedWindow;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TvuReturnType {
    LeaderRotation(u64, u64, Hash),
}

pub struct Tvu {
    replicate_stage: ReplicateStage,
    fetch_stage: BlobFetchStage,
    retransmit_stage: RetransmitStage,
    ledger_write_stage: LedgerWriteStage,
    storage_stage: StorageStage,
    exit: Arc<AtomicBool>,
}

impl Tvu {
    /// This service receives messages from a leader in the network and processes the transactions
    /// on the bank state.
    /// # Arguments
    /// * `bank` - The bank state.
    /// * `keypair` - Node's key pair for signing
    /// * `vote_account_keypair` - Vote key pair
    /// * `entry_height` - Initial ledger height
    /// * `cluster_info` - The cluster_info state.
    /// * `window` - The window state.
    /// * `replicate_socket` - my replicate socket
    /// * `repair_socket` - my repair socket
    /// * `retransmit_socket` - my retransmit socket
    /// * `ledger_path` - path to the ledger file
    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
    pub fn new(
        keypair: Arc<Keypair>,
        vote_account_keypair: Arc<Keypair>,
        bank: &Arc<Bank>,
        entry_height: u64,
        last_entry_id: Hash,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window: SharedWindow,
        replicate_sockets: Vec<UdpSocket>,
        repair_socket: UdpSocket,
        retransmit_socket: UdpSocket,
        ledger_path: Option<&str>,
    ) -> Self {
        let exit = Arc::new(AtomicBool::new(false));

        let repair_socket = Arc::new(repair_socket);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            replicate_sockets.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new_multi_socket(blob_sockets, exit.clone());
        //TODO
        //the packets coming out of blob_receiver need to be sent to the GPU and verified
        //then sent to the window, which does the erasure coding reconstruction
        let (retransmit_stage, blob_window_receiver) = RetransmitStage::new(
            &cluster_info,
            window,
            bank.tick_height(),
            entry_height,
            Arc::new(retransmit_socket),
            repair_socket,
            blob_fetch_receiver,
            bank.leader_scheduler.clone(),
        );

        let (replicate_stage, ledger_entry_receiver) = ReplicateStage::new(
            keypair.clone(),
            vote_account_keypair,
            bank.clone(),
            cluster_info,
            blob_window_receiver,
            exit.clone(),
            entry_height,
            last_entry_id,
        );

        let (ledger_write_stage, storage_entry_receiver) =
            LedgerWriteStage::new(ledger_path, ledger_entry_receiver);

        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            ledger_path,
            keypair,
            exit.clone(),
            entry_height,
        );

        Tvu {
            replicate_stage,
            fetch_stage,
            retransmit_stage,
            ledger_write_stage,
            storage_stage,
            exit,
        }
    }

    pub fn is_exited(&self) -> bool {
        self.exit.load(Ordering::Relaxed)
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<Option<TvuReturnType>> {
        self.fetch_stage.close();
        self.join()
    }
}

impl Service for Tvu {
    type JoinReturnType = Option<TvuReturnType>;

    fn join(self) -> thread::Result<Option<TvuReturnType>> {
        self.retransmit_stage.join()?;
        self.fetch_stage.join()?;
        self.ledger_write_stage.join()?;
        self.storage_stage.join()?;
        match self.replicate_stage.join()? {
            Some(ReplicateStageReturnType::LeaderRotation(
                tick_height,
                entry_height,
                last_entry_id,
            )) => Ok(Some(TvuReturnType::LeaderRotation(
                tick_height,
                entry_height,
                last_entry_id,
            ))),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use bank::Bank;
    use bincode::serialize;
    use cluster_info::{ClusterInfo, Node};
    use entry::Entry;
    use hash::Hash;
    use leader_scheduler::LeaderScheduler;
    use logger;
    use mint::Mint;
    use ncp::Ncp;
    use packet::SharedBlob;
    use service::Service;
    use signature::{Keypair, KeypairUtil};
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer;
    use system_transaction::SystemTransaction;
    use transaction::Transaction;
    use tvu::Tvu;
    use window::{self, SharedWindow};

    fn new_ncp(
        cluster_info: Arc<RwLock<ClusterInfo>>,
        gossip: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> (Ncp, SharedWindow) {
        let window = Arc::new(RwLock::new(window::default_window()));
        let ncp = Ncp::new(&cluster_info, window.clone(), None, gossip, exit);
        (ncp, window)
    }

    /// Test that message sent from leader to target1 and replicated to target2
    #[test]
    fn test_replicate() {
        logger::setup();
        let leader = Node::new_localhost();
        let target1_keypair = Keypair::new();
        let target1 = Node::new_localhost_with_pubkey(target1_keypair.pubkey());
        let target2 = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));

        //start cluster_info_l
        let mut cluster_info_l = ClusterInfo::new(leader.info.clone()).expect("ClusterInfo::new");
        cluster_info_l.set_leader(leader.info.id);

        let cref_l = Arc::new(RwLock::new(cluster_info_l));
        let dr_l = new_ncp(cref_l, leader.sockets.gossip, exit.clone());

        //start cluster_info2
        let mut cluster_info2 = ClusterInfo::new(target2.info.clone()).expect("ClusterInfo::new");
        cluster_info2.insert(&leader.info);
        cluster_info2.set_leader(leader.info.id);
        let leader_id = leader.info.id;
        let cref2 = Arc::new(RwLock::new(cluster_info2));
        let dr_2 = new_ncp(cref2, target2.sockets.gossip, exit.clone());

        // setup some blob services to send blobs into the socket
        // to simulate the source peer and get blobs out of the socket to
        // simulate target peer
        let (s_reader, r_reader) = channel();
        let blob_sockets: Vec<Arc<UdpSocket>> = target2
            .sockets
            .replicate
            .into_iter()
            .map(Arc::new)
            .collect();

        let t_receiver = streamer::blob_receiver(blob_sockets[0].clone(), exit.clone(), s_reader);

        // simulate leader sending messages
        let (s_responder, r_responder) = channel();
        let t_responder = streamer::responder(
            "test_replicate",
            Arc::new(leader.sockets.retransmit),
            r_responder,
        );

        let starting_balance = 10_000;
        let mint = Mint::new(starting_balance);
        let replicate_addr = target1.info.contact_info.tvu;
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_id,
        )));
        let mut bank = Bank::new(&mint);
        bank.leader_scheduler = leader_scheduler;
        let bank = Arc::new(bank);

        //start cluster_info1
        let mut cluster_info1 = ClusterInfo::new(target1.info.clone()).expect("ClusterInfo::new");
        cluster_info1.insert(&leader.info);
        cluster_info1.set_leader(leader.info.id);
        let cref1 = Arc::new(RwLock::new(cluster_info1));
        let dr_1 = new_ncp(cref1.clone(), target1.sockets.gossip, exit.clone());

        let vote_account_keypair = Arc::new(Keypair::new());
        let mut cur_hash = Hash::default();
        let tvu = Tvu::new(
            Arc::new(target1_keypair),
            vote_account_keypair,
            &bank,
            0,
            cur_hash,
            cref1,
            dr_1.1,
            target1.sockets.replicate,
            target1.sockets.repair,
            target1.sockets.retransmit,
            None,
        );

        let mut alice_ref_balance = starting_balance;
        let mut msgs = Vec::new();
        let mut blob_idx = 0;
        let num_transfers = 10;
        let transfer_amount = 501;
        let bob_keypair = Keypair::new();
        for i in 0..num_transfers {
            let entry0 = Entry::new(&cur_hash, i, vec![]);
            cur_hash = entry0.id;
            bank.register_tick(&cur_hash);
            let entry_tick0 = Entry::new(&cur_hash, i + 1, vec![]);
            cur_hash = entry_tick0.id;

            let tx0 = Transaction::system_new(
                &mint.keypair(),
                bob_keypair.pubkey(),
                transfer_amount,
                cur_hash,
            );
            bank.register_tick(&cur_hash);
            let entry_tick1 = Entry::new(&cur_hash, i + 1, vec![]);
            cur_hash = entry_tick1.id;
            let entry1 = Entry::new(&cur_hash, i + num_transfers, vec![tx0]);
            bank.register_tick(&entry1.id);
            let entry_tick2 = Entry::new(&entry1.id, i + 1, vec![]);
            cur_hash = entry_tick2.id;

            alice_ref_balance -= transfer_amount;

            for entry in vec![entry0, entry_tick0, entry_tick1, entry1, entry_tick2] {
                let mut b = SharedBlob::default();
                {
                    let mut w = b.write().unwrap();
                    w.set_index(blob_idx).unwrap();
                    blob_idx += 1;
                    w.set_id(&leader_id).unwrap();

                    let serialized_entry = serialize(&entry).unwrap();

                    w.data_mut()[..serialized_entry.len()].copy_from_slice(&serialized_entry);
                    w.set_size(serialized_entry.len());
                    w.meta.set_addr(&replicate_addr);
                }
                msgs.push(b);
            }
        }

        // send the blobs into the socket
        s_responder.send(msgs).expect("send");
        drop(s_responder);

        // receive retransmitted messages
        let timer = Duration::new(1, 0);
        while let Ok(_msg) = r_reader.recv_timeout(timer) {
            trace!("got msg");
        }

        let alice_balance = bank.get_balance(&mint.keypair().pubkey());
        assert_eq!(alice_balance, alice_ref_balance);

        let bob_balance = bank.get_balance(&bob_keypair.pubkey());
        assert_eq!(bob_balance, starting_balance - alice_ref_balance);

        tvu.close().expect("close");
        exit.store(true, Ordering::Relaxed);
        dr_l.0.join().expect("join");
        dr_2.0.join().expect("join");
        dr_1.0.join().expect("join");
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
}
