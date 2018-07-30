//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use bincode::serialize;
use counter::Counter;
use crdt::Crdt;
use entry::Entry;
use entry_writer::EntryWriter;
use ledger::Block;
use packet::BlobRecycler;
use result::{Error, Result};
use service::Service;
use signature::KeyPair;
use std::collections::VecDeque;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::{responder, BlobReceiver, BlobSender};
use timing;
use transaction::Transaction;
use vote_stage::VOTE_TIMEOUT_MS;
use voting::entries_to_votes;

pub struct WriteStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl WriteStage {
    /// Process any Entry items that have been published by the RecordStage.
    /// continuosly broadcast blobs of entries out
    pub fn write_and_send_entries<W: Write>(
        crdt: &Arc<RwLock<Crdt>>,
        entry_writer: &mut EntryWriter<W>,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
        entry_receiver: &Receiver<Vec<Entry>>,
    ) -> Result<()> {
        let entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        let votes = entries_to_votes(&entries);
        crdt.write().unwrap().insert_votes(&votes);

        //TODO(anatoly): real stake based voting needs to change this
        //leader simply votes if the current set of validators have voted
        //on a valid last id
        entry_writer.write_and_register_entries(&entries)?;
        trace!("New blobs? {}", entries.len());
        let mut blobs = VecDeque::new();
        entries.to_blobs(blob_recycler, &mut blobs);

        if !blobs.is_empty() {
            inc_new_counter!("write_stage-recv_vote", votes.len());
            inc_new_counter!("write_stage-broadcast_blobs", blobs.len());
            trace!("broadcasting {}", blobs.len());
            blob_sender.send(blobs)?;
        }
        Ok(())
    }
    pub fn leader_vote(
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
                let last_id = bank.last_id();
                let shared_blob = blob_recycler.allocate();
                let (vote, addr) = crdt.write().unwrap().new_vote(last_id)?;
                let tx = Transaction::new_vote(&keypair, vote, last_id, 0);
                let bytes = serialize(&tx)?;
                let len = bytes.len();
                {
                    let mut blob = shared_blob.write().unwrap();
                    blob.data[..len].copy_from_slice(&bytes);
                    blob.meta.set_addr(&addr);
                    blob.meta.size = len;
                }
                bank.process_transaction(&tx)?;
                vote_blob_sender.send(VecDeque::from(vec![shared_blob]))?;
                info!("{:x} leader_sent_vote", debug_id);
                inc_new_counter!("write_stage-leader_sent_vote", 1);
            } else {
            }
        }
        Ok(())
    }
    /// Create a new WriteStage for writing and broadcasting entries.
    pub fn new<W: Write + Send + 'static>(
        keypair: KeyPair,
        bank: Arc<Bank>,
        crdt: Arc<RwLock<Crdt>>,
        blob_recycler: BlobRecycler,
        writer: W,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> (Self, BlobReceiver) {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder(
            "write_stage_vote_sender",
            send,
            blob_recycler.clone(),
            vote_blob_receiver,
        );
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || {
                let mut entry_writer = EntryWriter::new(&bank, writer);
                let mut last_vote = 0;
                let debug_id = crdt.read().unwrap().debug_id();
                loop {
                    if let Err(e) = Self::write_and_send_entries(
                        &crdt,
                        &mut entry_writer,
                        &blob_sender,
                        &blob_recycler,
                        &entry_receiver,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => {
                                inc_new_counter!("write_stage-write_and_send_entries-error", 1);
                                error!("{:?}", e);
                            }
                        }
                    };
                    if let Err(e) = Self::leader_vote(
                        debug_id,
                        &keypair,
                        &bank,
                        &crdt,
                        &blob_recycler,
                        &vote_blob_sender,
                        &mut last_vote,
                    ) {
                        inc_new_counter!("write_stage-leader_vote-error", 1);
                        error!("{:?}", e);
                    }
                }
            })
            .unwrap();

        let thread_hdls = vec![t_responder, thread_hdl];
        (WriteStage { thread_hdls }, blob_receiver)
    }
}

impl Service for WriteStage {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        self.thread_hdls
    }
    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls() {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::{Crdt, NodeInfo};
    use entry::next_entry;
    use hash::Hash;
    use logger;
    use mint::Mint;
    use packet::BlobRecycler;
    use signature::{KeyPair, KeyPairUtil, PublicKey};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use timing;
    use transaction::{Transaction, Vote};
    use vote_stage::VOTE_TIMEOUT_MS;
    use write_stage::WriteStage;

    #[test]
    fn test_leader_vote() {
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

        // Insert some validators and votes for new last_id
        for i in 0..10 {
            let mut validator =
                NodeInfo::new_leader(&format!("127.0.0.1:234{}", i).parse().unwrap());

            let vote = Vote {
                version: validator.version + 1,
                contact_info_version: 1,
            };

            if i < 6 {
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
        let res = WriteStage::leader_vote(
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

        for i in 0..10 {
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
        let res = WriteStage::leader_vote(
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
