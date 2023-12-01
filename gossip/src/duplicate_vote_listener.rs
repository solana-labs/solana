use {
    crate::{
        cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS},
        crds::Cursor,
        crds_value::DuplicateVote,
    },
    crossbeam_channel::Sender,
    solana_sdk::{hash::Hash, pubkey::Pubkey, slot_history::Slot},
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct DuplicateVoteListener {
    thread_hdl: JoinHandle<()>,
}

impl DuplicateVoteListener {
    pub fn new(
        exit: Arc<AtomicBool>,
        cluster_info: Arc<ClusterInfo>,
        sender: Sender<(Pubkey, VoteTransaction, Slot, Hash)>,
    ) -> Self {
        let listen_thread = Builder::new()
            .name("solDupVoteLstnr".to_string())
            .spawn(move || {
                Self::recv_loop(exit, &cluster_info, sender);
            })
            .unwrap();

        Self {
            thread_hdl: listen_thread,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }

    fn recv_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &ClusterInfo,
        sender: Sender<(Pubkey, VoteTransaction, Slot, Hash)>,
    ) {
        let mut cursor = Cursor::default();
        while !exit.load(Ordering::Relaxed) {
            let votes: Vec<DuplicateVote> = cluster_info.get_duplicate_vote(&mut cursor);
            for vote in votes {
                let DuplicateVote {
                    from,
                    vote_tx,
                    slot,
                    hash,
                    ..
                } = vote;
                sender.send((from, vote_tx, slot, hash)).unwrap();
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }
}
