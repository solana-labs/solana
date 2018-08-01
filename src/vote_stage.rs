//! The `vote_stage` votes on the `last_id` of the bank at a regular cadence

use bank::Bank;
use bincode::serialize;
use counter::Counter;
use crdt::Crdt;
use hash::Hash;
use packet::BlobRecycler;
use result::Result;
use service::Service;
use signature::KeyPair;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, spawn, JoinHandle};
use std::time::Duration;
use streamer::BlobSender;
use transaction::Transaction;

const VOTE_TIMEOUT_MS: u64 = 1000;

pub struct VoteStage {
    thread_hdl: JoinHandle<()>,
}

impl VoteStage {
    pub fn new(
        keypair: Arc<KeyPair>,
        bank: Arc<Bank>,
        crdt: Arc<RwLock<Crdt>>,
        blob_recycler: BlobRecycler,
        vote_blob_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = spawn(move || {
            Self::run(
                &keypair,
                &bank,
                &crdt,
                &blob_recycler,
                &vote_blob_sender,
                &exit,
            );
        });
        VoteStage { thread_hdl }
    }

    fn run(
        keypair: &Arc<KeyPair>,
        bank: &Arc<Bank>,
        crdt: &Arc<RwLock<Crdt>>,
        blob_recycler: &BlobRecycler,
        vote_blob_sender: &BlobSender,
        exit: &Arc<AtomicBool>,
    ) {
        while !exit.load(Ordering::Relaxed) {
            let last_id = bank.last_id();

            if let Err(err) = Self::vote(&last_id, keypair, crdt, blob_recycler, vote_blob_sender) {
                info!("Vote failed: {:?}", err);
            }
            sleep(Duration::from_millis(VOTE_TIMEOUT_MS));
        }
    }

    fn vote(
        last_id: &Hash,
        keypair: &Arc<KeyPair>,
        crdt: &Arc<RwLock<Crdt>>,
        blob_recycler: &BlobRecycler,
        vote_blob_sender: &BlobSender,
    ) -> Result<()> {
        let shared_blob = blob_recycler.allocate();
        let (vote, addr) = {
            let mut wcrdt = crdt.write().unwrap();
            //TODO: doesn't seem like there is a synchronous call to get height and id
            info!("voting on {:?}", &last_id[..8]);
            wcrdt.new_vote(*last_id)
        }?;
        {
            let mut blob = shared_blob.write().unwrap();
            let tx = Transaction::new_vote(&keypair, vote, *last_id, 0);
            let bytes = serialize(&tx)?;
            let len = bytes.len();
            blob.data[..len].copy_from_slice(&bytes);
            blob.meta.set_addr(&addr);
            blob.meta.size = len;
        }
        inc_new_counter!("replicate-vote_sent", 1);

        vote_blob_sender.send(VecDeque::from(vec![shared_blob]))?;
        Ok(())
    }
}

impl Service for VoteStage {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        vec![self.thread_hdl]
    }

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()?;
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use bank::Bank;
    use crdt::{Crdt, TestNode};
    use mint::Mint;
    use packet::BlobRecycler;
    use service::Service;
    use signature::{KeyPair, KeyPairUtil};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    /// Ensure the VoteStage issues votes at the expected cadence
    #[test]
    fn test_vote_cadence() {
        let keypair = KeyPair::new();

        let mint = Mint::new(1234);
        let bank = Arc::new(Bank::new(&mint));

        let node = TestNode::new_localhost();
        let mut crdt = Crdt::new(node.data.clone()).expect("Crdt::new");
        crdt.set_leader(node.data.id);
        let blob_recycler = BlobRecycler::default();
        let (sender, receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));

        let vote_stage = VoteStage::new(
            Arc::new(keypair),
            bank.clone(),
            Arc::new(RwLock::new(crdt)),
            blob_recycler.clone(),
            sender,
            exit.clone(),
        );

        receiver.recv().unwrap();

        let timeout = Duration::from_millis(VOTE_TIMEOUT_MS * 2);
        receiver.recv_timeout(timeout).unwrap();
        receiver.recv_timeout(timeout).unwrap();

        exit.store(true, Ordering::Relaxed);
        vote_stage.join().expect("join");
    }
}
