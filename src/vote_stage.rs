//! The `vote_stage` votes on the `last_id` of the bank at a regular cadence

use bank::Bank;
use bincode::serialize;
use counter::Counter;
use crdt::Crdt;
use packet::{BlobRecycler, SharedBlob};
use result::Result;
use service::Service;
use signature::KeyPair;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, spawn, JoinHandle};
use std::time::Duration;
use streamer::BlobSender;
use timing;
use transaction::Transaction;

pub const VOTE_TIMEOUT_MS: u64 = 1000;

pub struct VoteStage {
    thread_hdl: JoinHandle<()>,
}

pub fn create_vote_tx_and_blob(
    bank: &Arc<Bank>,
    keypair: &KeyPair,
    crdt: &Arc<RwLock<Crdt>>,
    blob_recycler: &BlobRecycler,
) -> Result<(Transaction, SharedBlob)> {
    let last_id = bank.last_id();
    let shared_blob = blob_recycler.allocate();
    let (vote, addr) = {
        let mut wcrdt = crdt.write().unwrap();
        //TODO: doesn't seem like there is a synchronous call to get height and id
        info!("voting on {:?}", &last_id[..8]);
        wcrdt.new_vote(last_id)
    }?;
    let tx = Transaction::new_vote(&keypair, vote, last_id, 0);
    {
        let mut blob = shared_blob.write().unwrap();
        let bytes = serialize(&tx)?;
        let len = bytes.len();
        blob.data[..len].copy_from_slice(&bytes);
        blob.meta.set_addr(&addr);
        blob.meta.size = len;
    }
    Ok((tx, shared_blob))
}

pub fn send_leader_vote(
    debug_id: u64,
    keypair: &KeyPair,
    bank: &Arc<Bank>,
    crdt: &Arc<RwLock<Crdt>>,
    blob_recycler: &BlobRecycler,
    vote_blob_sender: &BlobSender,
    last_vote: &mut u64,
) -> Result<()> {
    let now = timing::timestamp();
    if now - *last_vote > VOTE_TIMEOUT_MS {
        //TODO(anatoly): vote if the last id set is mostly valid
        let ids: Vec<_> = crdt.read()
            .unwrap()
            .table
            .values()
            .map(|x| x.ledger_state.last_id)
            .collect();
        let total = bank.count_valid_ids(&ids);
        //TODO(anatoly): this isn't stake based voting
        info!(
            "{:x}: valid_ids {}/{} {}",
            debug_id,
            total,
            ids.len(),
            (2 * ids.len()) / 3
        );
        if total > (2 * ids.len()) / 3 {
            *last_vote = now;

            if let Ok((tx, shared_blob)) =
                create_vote_tx_and_blob(bank, keypair, crdt, blob_recycler)
            {
                bank.process_transaction(&tx)?;
                vote_blob_sender.send(VecDeque::from(vec![shared_blob]))?;
                info!("{:x} leader_sent_vote", debug_id);
                inc_new_counter!("write_stage-leader_sent_vote", 1);
            }
        }
    }
    Ok(())
}

fn send_validator_vote(
    bank: &Arc<Bank>,
    keypair: &Arc<KeyPair>,
    crdt: &Arc<RwLock<Crdt>>,
    blob_recycler: &BlobRecycler,
    vote_blob_sender: &BlobSender,
) -> Result<()> {
    if let Ok((_, shared_blob)) = create_vote_tx_and_blob(bank, keypair, crdt, blob_recycler) {
        inc_new_counter!("replicate-vote_sent", 1);

        vote_blob_sender.send(VecDeque::from(vec![shared_blob]))?;
    }
    Ok(())
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
            if let Err(err) =
                send_validator_vote(bank, keypair, crdt, blob_recycler, vote_blob_sender)
            {
                info!("Vote failed: {:?}", err);
            }
            sleep(Duration::from_millis(VOTE_TIMEOUT_MS));
        }
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
    use crdt::{Crdt, NodeInfo, TestNode};
    use entry::next_entry;
    use hash::Hash;
    use logger;
    use mint::Mint;
    use packet::BlobRecycler;
    use service::Service;
    use signature::{KeyPair, KeyPairUtil};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use transaction::{Transaction, Vote};

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

    #[test]
    fn test_send_leader_vote() {
        logger::setup();

        // create a mint/bank
        let mint = Mint::new(1000);
        let bank = Arc::new(Bank::new(&mint));
        let hash0 = Hash::default();

        // get a non-default hash last_id
        let entry = next_entry(&hash0, 1, vec![]);
        bank.register_entry_id(&entry.id);

        // Create a leader
        let leader_data = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let leader_pubkey = leader_data.id.clone();
        let mut leader_crdt = Crdt::new(leader_data).unwrap();

        // give the leader some tokens
        let give_leader_tokens_tx =
            Transaction::new(&mint.keypair(), leader_pubkey.clone(), 100, entry.id);
        bank.process_transaction(&give_leader_tokens_tx).unwrap();

        leader_crdt.set_leader(leader_pubkey);

        // Insert 7 agreeing validators / 3 disagreeing
        // and votes for new last_id
        for i in 0..10 {
            let mut validator =
                NodeInfo::new_leader(&format!("127.0.0.1:234{}", i).parse().unwrap());

            let vote = Vote {
                version: validator.version + 1,
                contact_info_version: 1,
            };

            if i < 7 {
                validator.ledger_state.last_id = entry.id;
            }

            leader_crdt.insert(&validator);
            trace!("validator id: {:?}", validator.id);

            leader_crdt.insert_vote(&validator.id, &vote, entry.id);
        }
        let leader = Arc::new(RwLock::new(leader_crdt));
        let blob_recycler = BlobRecycler::default();
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let mut last_vote: u64 = timing::timestamp() - VOTE_TIMEOUT_MS - 1;
        let res = send_leader_vote(
            1234,
            &mint.keypair(),
            &bank,
            &leader,
            &blob_recycler,
            &vote_blob_sender,
            &mut last_vote,
        );
        trace!("vote result: {:?}", res);
        assert!(res.is_ok());
        let vote_blob = vote_blob_receiver.recv_timeout(Duration::from_millis(500));
        trace!("vote_blob: {:?}", vote_blob);

        // leader shouldn't vote yet, not enough votes
        assert!(vote_blob.is_err());

        // add two more nodes and see that it succeeds
        for i in 0..2 {
            let mut validator =
                NodeInfo::new_leader(&format!("127.0.0.1:234{}", i).parse().unwrap());

            let vote = Vote {
                version: validator.version + 1,
                contact_info_version: 1,
            };

            validator.ledger_state.last_id = entry.id;

            leader.write().unwrap().insert(&validator);
            trace!("validator id: {:?}", validator.id);

            leader
                .write()
                .unwrap()
                .insert_vote(&validator.id, &vote, entry.id);
        }

        last_vote = timing::timestamp() - VOTE_TIMEOUT_MS - 1;
        let res = send_leader_vote(
            2345,
            &mint.keypair(),
            &bank,
            &leader,
            &blob_recycler,
            &vote_blob_sender,
            &mut last_vote,
        );
        trace!("vote result: {:?}", res);
        assert!(res.is_ok());
        let vote_blob = vote_blob_receiver.recv_timeout(Duration::from_millis(500));
        trace!("vote_blob: {:?}", vote_blob);

        // leader should vote now
        assert!(vote_blob.is_ok());
    }

}
