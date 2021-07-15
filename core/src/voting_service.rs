use solana_gossip::cluster_info::ClusterInfo;
use solana_poh::poh_recorder::PohRecorder;
use solana_sdk::{clock::Slot, transaction::Transaction};
use std::{
    sync::{mpsc::Receiver, Arc, Mutex},
    thread::{self, Builder, JoinHandle},
};

pub enum VoteOp {
    PushVote {
        tx: Transaction,
        tower_slots: Vec<Slot>,
    },
    RefreshVote {
        tx: Transaction,
        last_voted_slot: Slot,
    },
}

impl VoteOp {
    fn tx(&self) -> &Transaction {
        match self {
            VoteOp::PushVote { tx, tower_slots: _ } => tx,
            VoteOp::RefreshVote {
                tx,
                last_voted_slot: _,
            } => tx,
        }
    }
}

pub struct VotingService {
    thread_hdl: JoinHandle<()>,
}

impl VotingService {
    pub fn new(
        vote_receiver: Receiver<VoteOp>,
        cluster_info: Arc<ClusterInfo>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("sol-vote-service".to_string())
            .spawn(move || {
                for vote_op in vote_receiver.iter() {
                    Self::handle_vote(&cluster_info, &poh_recorder, vote_op);
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    pub fn handle_vote(
        cluster_info: &ClusterInfo,
        poh_recorder: &Mutex<PohRecorder>,
        vote_op: VoteOp,
    ) {
        let _ = cluster_info.send_transaction(
            vote_op.tx(),
            crate::banking_stage::next_leader_tpu(cluster_info, poh_recorder),
        );

        match vote_op {
            VoteOp::PushVote { tx, tower_slots } => {
                cluster_info.push_vote(&tower_slots, tx);
            }
            VoteOp::RefreshVote {
                tx,
                last_voted_slot,
            } => {
                cluster_info.refresh_vote(tx, last_voted_slot);
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
