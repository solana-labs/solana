//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use counter::Counter;
use crdt::Crdt;
use entry::Entry;
use ledger::{Block, LedgerWriter};
use log::Level;
use packet::BlobRecycler;
use result::{Error, Result};
use service::Service;
use signature::Keypair;
use std::cmp;
use std::net::UdpSocket;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::responder;
use timing::{duration_as_ms, duration_as_s};
use vote_stage::send_leader_vote;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WriteStageReturnType {
    LeaderRotation,
    ChannelDisconnected,
}

pub struct WriteStage {
    thread_hdls: Vec<JoinHandle<()>>,
    write_thread: JoinHandle<WriteStageReturnType>,
}

impl WriteStage {
    // Given a vector of potential new entries to write, return as many as we can
    // fit before we hit the entry height for leader rotation. Also return a boolean
    // reflecting whether we actually hit an entry height for leader rotation.
    fn find_leader_rotation_index(
        crdt: &Arc<RwLock<Crdt>>,
        leader_rotation_interval: u64,
        entry_height: u64,
        mut new_entries: Vec<Entry>,
    ) -> (Vec<Entry>, bool) {
        // Find out how many more entries we can squeeze in until the next leader
        // rotation
        let entries_until_leader_rotation =
            leader_rotation_interval - (entry_height % leader_rotation_interval);

        let new_entries_length = new_entries.len();

        let mut i = cmp::min(entries_until_leader_rotation as usize, new_entries_length);

        let mut is_leader_rotation = false;

        loop {
            if (entry_height + i as u64) % leader_rotation_interval == 0 {
                let rcrdt = crdt.read().unwrap();
                let my_id = rcrdt.my_data().id;
                let next_leader = rcrdt.get_scheduled_leader(entry_height + i as u64);
                if next_leader != Some(my_id) {
                    is_leader_rotation = true;
                    break;
                }
            }

            if i == new_entries_length {
                break;
            }

            i += cmp::min(leader_rotation_interval as usize, new_entries_length - i);
        }

        new_entries.truncate(i as usize);

        (new_entries, is_leader_rotation)
    }

    /// Process any Entry items that have been published by the RecordStage.
    /// continuosly send entries out
    pub fn write_and_send_entries(
        crdt: &Arc<RwLock<Crdt>>,
        ledger_writer: &mut LedgerWriter,
        entry_sender: &Sender<Vec<Entry>>,
        entry_receiver: &Receiver<Vec<Entry>>,
        entry_height: &mut u64,
        leader_rotation_interval: u64,
    ) -> Result<()> {
        let mut ventries = Vec::new();
        let mut received_entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        let mut num_new_entries = received_entries.len();
        let mut num_txs = 0;

        loop {
            // Find out how many more entries we can squeeze in until the next leader
            // rotation
            let (new_entries, is_leader_rotation) = Self::find_leader_rotation_index(
                crdt,
                leader_rotation_interval,
                *entry_height + num_new_entries as u64,
                received_entries,
            );

            num_new_entries += new_entries.len();
            ventries.push(new_entries);

            if is_leader_rotation {
                break;
            }

            if let Ok(n) = entry_receiver.try_recv() {
                received_entries = n;
            } else {
                break;
            }
        }

        info!("write_stage entries: {}", num_new_entries);

        let to_blobs_total = 0;
        let mut blob_send_total = 0;
        let mut register_entry_total = 0;
        let mut crdt_votes_total = 0;

        let start = Instant::now();
        for _ in 0..ventries.len() {
            let entries = ventries.pop().unwrap();
            for e in entries.iter() {
                num_txs += e.transactions.len();
            }
            let crdt_votes_start = Instant::now();
            let votes = &entries.votes();
            crdt.write().unwrap().insert_votes(&votes);
            crdt_votes_total += duration_as_ms(&crdt_votes_start.elapsed());

            ledger_writer.write_entries(entries.clone())?;
            // Once the entries have been written to the ledger, then we can
            // safely incement entry height
            *entry_height += entries.len() as u64;

            let register_entry_start = Instant::now();
            register_entry_total += duration_as_ms(&register_entry_start.elapsed());

            inc_new_counter_info!("write_stage-write_entries", entries.len());

            //TODO(anatoly): real stake based voting needs to change this
            //leader simply votes if the current set of validators have voted
            //on a valid last id

            trace!("New entries? {}", entries.len());
            let blob_send_start = Instant::now();
            if !entries.is_empty() {
                inc_new_counter_info!("write_stage-recv_vote", votes.len());
                inc_new_counter_info!("write_stage-broadcast_entries", entries.len());
                trace!("broadcasting {}", entries.len());
                entry_sender.send(entries)?;
            }

            blob_send_total += duration_as_ms(&blob_send_start.elapsed());
        }
        info!("done write_stage txs: {} time {} ms txs/s: {} to_blobs_total: {} register_entry_total: {} blob_send_total: {} crdt_votes_total: {}",
              num_txs, duration_as_ms(&start.elapsed()),
              num_txs as f32 / duration_as_s(&start.elapsed()),
              to_blobs_total,
              register_entry_total,
              blob_send_total,
              crdt_votes_total);

        Ok(())
    }

    /// Create a new WriteStage for writing and broadcasting entries.
    pub fn new(
        keypair: Arc<Keypair>,
        bank: Arc<Bank>,
        crdt: Arc<RwLock<Crdt>>,
        ledger_path: &str,
        entry_receiver: Receiver<Vec<Entry>>,
        entry_height: u64,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder(
            "write_stage_vote_sender",
            Arc::new(send),
            vote_blob_receiver,
        );
        let (entry_sender, entry_receiver_forward) = channel();
        let mut ledger_writer = LedgerWriter::recover(ledger_path).unwrap();

        let write_thread = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || {
                let mut last_vote = 0;
                let mut last_valid_validator_timestamp = 0;
                let id;
                let leader_rotation_interval;
                {
                    let rcrdt = crdt.read().unwrap();
                    id = rcrdt.id;
                    leader_rotation_interval = rcrdt.get_leader_rotation_interval();
                }
                let mut entry_height = entry_height;
                let blob_recycler = BlobRecycler::default();
                loop {
                    info!("write_stage entry height: {}", entry_height);
                    // Note that entry height is not zero indexed, it starts at 1, so the
                    // old leader is in power up to and including entry height
                    // n * leader_rotation_interval for some "n". Once we've forwarded
                    // that last block, check for the next scheduled leader.
                    if entry_height % (leader_rotation_interval as u64) == 0 {
                        let rcrdt = crdt.read().unwrap();
                        let my_id = rcrdt.my_data().id;
                        let scheduled_leader = rcrdt.get_scheduled_leader(entry_height);
                        drop(rcrdt);
                        match scheduled_leader {
                            Some(id) if id == my_id => (),
                            // If the leader stays in power for the next
                            // round as well, then we don't exit. Otherwise, exit.
                            _ => {
                                // When the broadcast stage has received the last blob, it
                                // will signal to close the fetch stage, which will in turn
                                // close down this write stage
                                return WriteStageReturnType::LeaderRotation;
                            }
                        }
                    }

                    if let Err(e) = Self::write_and_send_entries(
                        &crdt,
                        &mut ledger_writer,
                        &entry_sender,
                        &entry_receiver,
                        &mut entry_height,
                        leader_rotation_interval,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                                return WriteStageReturnType::ChannelDisconnected
                            }
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => {
                                inc_new_counter_info!(
                                    "write_stage-write_and_send_entries-error",
                                    1
                                );
                                error!("{:?}", e);
                            }
                        }
                    };
                    if let Err(e) = send_leader_vote(
                        &id,
                        &keypair,
                        &bank,
                        &crdt,
                        &blob_recycler,
                        &vote_blob_sender,
                        &mut last_vote,
                        &mut last_valid_validator_timestamp,
                    ) {
                        inc_new_counter_info!("write_stage-leader_vote-error", 1);
                        error!("{:?}", e);
                    }
                }
            }).unwrap();

        let thread_hdls = vec![t_responder];
        (
            WriteStage {
                write_thread,
                thread_hdls,
            },
            entry_receiver_forward,
        )
    }
}

impl Service for WriteStage {
    type JoinReturnType = WriteStageReturnType;

    fn join(self) -> thread::Result<WriteStageReturnType> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }

        self.write_thread.join()
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::{Crdt, Node};
    use entry::Entry;
    use ledger::{genesis, read_ledger};
    use recorder::Recorder;
    use service::Service;
    use signature::{Keypair, KeypairUtil, Pubkey};
    use std::fs::remove_dir_all;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, RwLock};
    use write_stage::{WriteStage, WriteStageReturnType};

    struct DummyWriteStage {
        my_id: Pubkey,
        write_stage: WriteStage,
        entry_sender: Sender<Vec<Entry>>,
        write_stage_entry_receiver: Receiver<Vec<Entry>>,
        crdt: Arc<RwLock<Crdt>>,
        bank: Arc<Bank>,
        leader_ledger_path: String,
        ledger_tail: Vec<Entry>,
    }

    fn process_ledger(ledger_path: &str, bank: &Bank) -> (u64, Vec<Entry>) {
        let entries = read_ledger(ledger_path, true).expect("opening ledger");

        let entries = entries
            .map(|e| e.unwrap_or_else(|err| panic!("failed to parse entry. error: {}", err)));

        info!("processing ledger...");
        bank.process_ledger(entries).expect("process_ledger")
    }

    fn setup_dummy_write_stage(leader_rotation_interval: u64) -> DummyWriteStage {
        // Setup leader info
        let leader_keypair = Arc::new(Keypair::new());
        let my_id = leader_keypair.pubkey();
        let leader_info = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let mut crdt = Crdt::new(leader_info.info).expect("Crdt::new");
        crdt.set_leader_rotation_interval(leader_rotation_interval);
        let crdt = Arc::new(RwLock::new(crdt));
        let bank = Bank::new_default(true);
        let bank = Arc::new(bank);

        // Make a ledger
        let (_, leader_ledger_path) = genesis("test_leader_rotation_exit", 10_000);

        let (entry_height, ledger_tail) = process_ledger(&leader_ledger_path, &bank);

        // Make a dummy pipe
        let (entry_sender, entry_receiver) = channel();

        // Start up the write stage
        let (write_stage, write_stage_entry_receiver) = WriteStage::new(
            leader_keypair,
            bank.clone(),
            crdt.clone(),
            &leader_ledger_path,
            entry_receiver,
            entry_height,
        );

        DummyWriteStage {
            my_id,
            write_stage,
            entry_sender,
            write_stage_entry_receiver,
            crdt,
            bank,
            leader_ledger_path,
            ledger_tail,
        }
    }

    #[test]
    fn test_write_stage_leader_rotation_exit() {
        let leader_rotation_interval = 10;
        let write_stage_info = setup_dummy_write_stage(leader_rotation_interval);

        {
            let mut wcrdt = write_stage_info.crdt.write().unwrap();
            wcrdt.set_scheduled_leader(leader_rotation_interval, write_stage_info.my_id);
        }

        let last_entry_hash = write_stage_info
            .ledger_tail
            .last()
            .expect("Ledger should not be empty")
            .id;

        let genesis_entry_height = write_stage_info.ledger_tail.len() as u64;

        // Input enough entries to make exactly leader_rotation_interval entries, which will
        // trigger a check for leader rotation. Because the next scheduled leader
        // is ourselves, we won't exit
        let mut recorder = Recorder::new(last_entry_hash);
        for _ in genesis_entry_height..leader_rotation_interval {
            let new_entry = recorder.record(vec![]);
            write_stage_info.entry_sender.send(new_entry).unwrap();
        }

        // Set the scheduled next leader in the crdt to some other node
        let leader2_keypair = Keypair::new();
        let leader2_info = Node::new_localhost_with_pubkey(leader2_keypair.pubkey());

        {
            let mut wcrdt = write_stage_info.crdt.write().unwrap();
            wcrdt.insert(&leader2_info.info);
            wcrdt.set_scheduled_leader(2 * leader_rotation_interval, leader2_keypair.pubkey());
        }

        // Input another leader_rotation_interval dummy entries one at a time,
        // which will take us past the point of the leader rotation.
        // The write_stage will see that it's no longer the leader after
        // checking the schedule, and exit
        for _ in 0..leader_rotation_interval {
            let new_entry = recorder.record(vec![]);
            write_stage_info.entry_sender.send(new_entry).unwrap();
        }

        assert_eq!(
            write_stage_info.write_stage.join().unwrap(),
            WriteStageReturnType::LeaderRotation
        );

        // Make sure the ledger contains exactly 2 * leader_rotation_interval entries
        let (entry_height, _) =
            process_ledger(&write_stage_info.leader_ledger_path, &write_stage_info.bank);
        remove_dir_all(write_stage_info.leader_ledger_path).unwrap();
        assert_eq!(entry_height, 2 * leader_rotation_interval);
    }
}
