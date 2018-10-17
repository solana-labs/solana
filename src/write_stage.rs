//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use budget_transaction::BudgetTransaction;
use cluster_info::ClusterInfo;
use counter::Counter;
use entry::Entry;
use leader_scheduler::LeaderScheduler;
use ledger::{Block, LedgerWriter};
use log::Level;
use result::{Error, Result};
use service::Service;
use signature::Keypair;
use solana_program_interface::pubkey::Pubkey;
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
        bank: &Arc<Bank>,
        my_id: &Pubkey,
        leader_scheduler: &mut LeaderScheduler,
        entry_height: u64,
        mut new_entries: Vec<Entry>,
    ) -> (Vec<Entry>, bool) {
        // In the case we're using the default implemntation of the leader scheduler,
        // there won't ever be leader rotations at particular entry heights, so just
        // return the entire input vector of entries
        if leader_scheduler.use_only_bootstrap_leader {
            return (new_entries, false);
        }

        let new_entries_length = new_entries.len();

        // i is the number of entries to take
        let mut i = 0;
        let mut is_leader_rotation = false;

        loop {
            let next_leader = leader_scheduler.get_scheduled_leader(entry_height + i as u64);

            if next_leader != Some(*my_id) {
                is_leader_rotation = true;
                break;
            }

            if i == new_entries_length {
                break;
            }

            // Find out how many more entries we can squeeze in until the next leader
            // rotation
            let entries_until_leader_rotation = leader_scheduler.entries_until_next_leader_rotation(
                entry_height + (i as u64)
            ).expect("Leader rotation should exist when not using default implementation of LeaderScheduler");

            // Check the next leader rotation height entries in new_entries, or
            // if the new_entries doesnt have that many entries remaining,
            // just check the rest of the new_entries_vector
            let step = cmp::min(
                entries_until_leader_rotation as usize,
                new_entries_length - i,
            );

            // 1) Since "i" is the current number/count of items from the new_entries vector we have
            // have already checked, then "i" is also the INDEX into the new_entries vector of the
            // next unprocessed entry. Hence this loop checks all entries between indexes:
            // [entry_height + i, entry_height + i + step - 1], which is equivalent to the
            // entry heights: [entry_height + i + 1, entry_height + i + step]
            for (entry, new_entries_index) in new_entries[i..(i + step)].iter().zip(i..(i + step)) {
                let votes = entry
                    .transactions
                    .iter()
                    .filter_map(BudgetTransaction::vote);
                for (voter_id, _, _) in votes {
                    leader_scheduler
                        .push_vote(voter_id, entry_height + new_entries_index as u64 + 1);
                }
                // TODO: There's an issue here that the bank state may have updated
                // while this entry was in flight from the BankingStage, which could cause
                // the leader schedule, which is based on stake (tied to the bank account balances)
                // right now, to be inconsistent with the rest of the network. Fix once
                // we can pin PoH height to bank state
                leader_scheduler.update_height(entry_height + new_entries_index as u64 + 1, bank);
            }

            i += step
        }

        new_entries.truncate(i as usize);

        (new_entries, is_leader_rotation)
    }

    /// Process any Entry items that have been published by the RecordStage.
    /// continuosly send entries out
    pub fn write_and_send_entries(
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        ledger_writer: &mut LedgerWriter,
        entry_sender: &Sender<Vec<Entry>>,
        entry_receiver: &Receiver<Vec<Entry>>,
        entry_height: &mut u64,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
    ) -> Result<()> {
        let mut ventries = Vec::new();
        let mut received_entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        let now = Instant::now();
        let mut num_new_entries = 0;
        let mut num_txs = 0;

        loop {
            // Find out how many more entries we can squeeze in until the next leader
            // rotation
            let (new_entries, is_leader_rotation) = Self::find_leader_rotation_index(
                bank,
                &cluster_info.read().unwrap().my_data().id,
                &mut *leader_scheduler.write().unwrap(),
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
        inc_new_counter_info!("write_stage-entries_received", num_new_entries);

        debug!("write_stage entries: {}", num_new_entries);

        let mut entries_send_total = 0;
        let mut cluster_info_votes_total = 0;

        let start = Instant::now();
        for entries in ventries {
            let cluster_info_votes_start = Instant::now();
            let votes = &entries.votes();
            cluster_info.write().unwrap().insert_votes(&votes);
            cluster_info_votes_total += duration_as_ms(&cluster_info_votes_start.elapsed());

            for e in &entries {
                num_txs += e.transactions.len();
                ledger_writer.write_entry_noflush(&e)?;
            }
            // Once the entries have been written to the ledger, then we can
            // safely incement entry height
            *entry_height += entries.len() as u64;

            inc_new_counter_info!("write_stage-write_entries", entries.len());

            //TODO(anatoly): real stake based voting needs to change this
            //leader simply votes if the current set of validators have voted
            //on a valid last id

            trace!("New entries? {}", entries.len());
            let entries_send_start = Instant::now();
            if !entries.is_empty() {
                inc_new_counter_info!("write_stage-recv_vote", votes.len());
                inc_new_counter_info!("write_stage-entries_sent", entries.len());
                trace!("broadcasting {}", entries.len());
                entry_sender.send(entries)?;
            }

            entries_send_total += duration_as_ms(&entries_send_start.elapsed());
        }
        ledger_writer.flush()?;
        inc_new_counter_info!(
            "write_stage-time_ms",
            duration_as_ms(&now.elapsed()) as usize
        );
        debug!("done write_stage txs: {} time {} ms txs/s: {} entries_send_total: {} cluster_info_votes_total: {}",
              num_txs, duration_as_ms(&start.elapsed()),
              num_txs as f32 / duration_as_s(&start.elapsed()),
              entries_send_total,
              cluster_info_votes_total);

        Ok(())
    }

    /// Create a new WriteStage for writing and broadcasting entries.
    pub fn new(
        keypair: Arc<Keypair>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        ledger_path: &str,
        entry_receiver: Receiver<Vec<Entry>>,
        entry_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
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
                let id = cluster_info.read().unwrap().id;
                let mut entry_height_ = entry_height;
                loop {
                    // Note that entry height is not zero indexed, it starts at 1, so the
                    // old leader is in power up to and including entry height
                    // n * leader_rotation_interval for some "n". Once we've forwarded
                    // that last block, check for the next scheduled leader.
                    match leader_scheduler
                        .read()
                        .unwrap()
                        .get_scheduled_leader(entry_height_)
                    {
                        Some(leader_id) if leader_id == id => (),
                        None => panic!(
                            "Scheduled leader id should never be unknown while processing entries"
                        ),
                        _ => {
                            // If the leader is no longer in power, exit.
                            // When the broadcast stage has received the last blob, it
                            // will signal to close the fetch stage, which will in turn
                            // close down this write stage
                            return WriteStageReturnType::LeaderRotation;
                        }
                    }

                    if let Err(e) = Self::write_and_send_entries(
                        &bank,
                        &cluster_info,
                        &mut ledger_writer,
                        &entry_sender,
                        &entry_receiver,
                        &mut entry_height_,
                        &leader_scheduler,
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
                        &cluster_info,
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
    use cluster_info::{ClusterInfo, Node};
    use entry::Entry;
    use hash::Hash;
    use leader_scheduler::{set_new_leader, LeaderScheduler, LeaderSchedulerConfig};
    use ledger::{create_tmp_genesis, next_entries_mut, read_ledger};
    use service::Service;
    use signature::{Keypair, KeypairUtil};
    use solana_program_interface::account::Account;
    use solana_program_interface::pubkey::Pubkey;
    use std::fs::remove_dir_all;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, RwLock};
    use write_stage::{WriteStage, WriteStageReturnType};

    struct DummyWriteStage {
        write_stage: WriteStage,
        entry_sender: Sender<Vec<Entry>>,
        _write_stage_entry_receiver: Receiver<Vec<Entry>>,
        bank: Arc<Bank>,
        leader_ledger_path: String,
        ledger_tail: Vec<Entry>,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    }

    fn process_ledger(ledger_path: &str, bank: &Bank) -> (u64, Vec<Entry>) {
        let entries = read_ledger(ledger_path, true).expect("opening ledger");

        let entries = entries
            .map(|e| e.unwrap_or_else(|err| panic!("failed to parse entry. error: {}", err)));

        info!("processing ledger...");
        bank.process_ledger(entries, &mut LeaderScheduler::default())
            .expect("process_ledger")
    }

    fn setup_dummy_write_stage(
        leader_keypair: Arc<Keypair>,
        leader_scheduler_config: &LeaderSchedulerConfig,
        test_name: &str,
    ) -> DummyWriteStage {
        // Setup leader info
        let leader_info = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let cluster_info = Arc::new(RwLock::new(
            ClusterInfo::new(leader_info.info).expect("ClusterInfo::new"),
        ));
        let bank = Arc::new(Bank::default());

        // Make a ledger
        let (_, leader_ledger_path) = create_tmp_genesis(test_name, 10_000);

        let (entry_height, ledger_tail) = process_ledger(&leader_ledger_path, &bank);

        // Make a dummy pipe
        let (entry_sender, entry_receiver) = channel();

        // Make a dummy leader scheduler we can manipulate
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::new(leader_scheduler_config)));

        // Start up the write stage
        let (write_stage, _write_stage_entry_receiver) = WriteStage::new(
            leader_keypair,
            bank.clone(),
            cluster_info.clone(),
            &leader_ledger_path,
            entry_receiver,
            entry_height,
            leader_scheduler.clone(),
        );

        DummyWriteStage {
            write_stage,
            entry_sender,
            // Need to keep this alive, otherwise the write_stage will detect ChannelClosed
            // and shut down
            _write_stage_entry_receiver,
            bank,
            leader_ledger_path,
            ledger_tail,
            leader_scheduler,
        }
    }

    #[test]
    fn test_write_stage_leader_rotation_exit() {
        let leader_keypair = Keypair::new();
        let leader_id = leader_keypair.pubkey();

        // Make a dummy leader scheduler we can manipulate
        let bootstrap_height = 20;
        let leader_rotation_interval = 10;
        let seed_rotation_interval = 2 * leader_rotation_interval;
        let active_window = bootstrap_height + 3 * seed_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_keypair.pubkey(),
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window),
        );

        let write_stage_info = setup_dummy_write_stage(
            Arc::new(leader_keypair),
            &leader_scheduler_config,
            "test_write_stage_leader_rotation_exit",
        );

        let mut last_id = write_stage_info
            .ledger_tail
            .last()
            .expect("Ledger should not be empty")
            .id;
        let mut num_hashes = 0;

        let genesis_entry_height = write_stage_info.ledger_tail.len() as u64;

        // Insert a nonzero balance and vote into the bank to make this node eligible
        // for leader selection
        write_stage_info
            .leader_scheduler
            .write()
            .unwrap()
            .push_vote(leader_id, 1);
        let dummy_id = Keypair::new().pubkey();
        let accounts = write_stage_info.bank.accounts();
        let new_account = Account::new(1, 10, dummy_id.clone());
        accounts
            .write()
            .unwrap()
            .insert(leader_id, new_account.clone());

        // Input enough entries to make exactly leader_rotation_interval entries, which will
        // trigger a check for leader rotation. Because the next scheduled leader
        // is ourselves, we won't exit
        for _ in genesis_entry_height..bootstrap_height {
            let new_entry = next_entries_mut(&mut last_id, &mut num_hashes, vec![]);
            write_stage_info.entry_sender.send(new_entry).unwrap();
        }

        // Set the next scheduled next leader to some other node
        {
            let mut leader_scheduler = write_stage_info.leader_scheduler.write().unwrap();
            set_new_leader(&write_stage_info.bank, &mut (*leader_scheduler), 1);
        }

        // Input enough dummy entries until the next seed rotation_interval,
        // The write_stage will see that it's no longer the leader after
        // checking the schedule, and exit
        for _ in 0..seed_rotation_interval {
            let new_entry = next_entries_mut(&mut last_id, &mut num_hashes, vec![]);
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

    fn make_leader_scheduler(
        my_id: Pubkey,
        bootstrap_height: u64,
        leader_rotation_interval: u64,
        seed_rotation_interval: u64,
        active_window: u64,
    ) -> LeaderScheduler {
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            my_id,
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.push_vote(my_id, 1);
        leader_scheduler
    }

    #[test]
    // Tests for when the leader across slots and epochs are the same
    fn test_same_leader_index_calculation() {
        // Set up a dummy node
        let my_keypair = Arc::new(Keypair::new());
        let my_id = my_keypair.pubkey();

        // Set up a dummy bank
        let bank = Arc::new(Bank::default());
        let accounts = bank.accounts();
        let dummy_id = Keypair::new().pubkey();
        let new_account = Account::new(1, 10, dummy_id.clone());
        accounts.write().unwrap().insert(my_id, new_account.clone());

        let entry = Entry::new(&Hash::default(), 0, vec![]);

        // Note: An slot is the period of leader_rotation_interval entries
        // time during which a leader is in power

        let leader_rotation_interval = 10;
        let bootstrap_height = 17;
        let seed_rotation_interval = 3 * leader_rotation_interval;
        let active_window = bootstrap_height + 3 * seed_rotation_interval;

        let mut leader_scheduler = make_leader_scheduler(
            my_id,
            bootstrap_height,
            leader_rotation_interval,
            seed_rotation_interval,
            active_window,
        );

        // Case 1: A vector that is completely within a certain slot should return that
        // entire vector
        let mut len = (leader_scheduler.bootstrap_height - 1) as usize;
        let mut input = vec![entry.clone(); len];
        let mut result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            0,
            input.clone(),
        );

        assert_eq!(result, (input, false));

        // Case 2: A vector of new entries that spans multiple slots should return the
        // entire vector, assuming that the same leader is in power for all the slots.
        len = 2 * seed_rotation_interval as usize;
        input = vec![entry.clone(); len];
        result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            bootstrap_height - 1,
            input.clone(),
        );

        assert_eq!(result, (input, false));

        // Case 3: A vector that triggers a check for leader rotation should return
        // the entire vector and signal leader_rotation == false, if the
        // same leader is in power for the next slot as well.
        let mut leader_scheduler = make_leader_scheduler(
            my_id,
            bootstrap_height,
            leader_rotation_interval,
            seed_rotation_interval,
            active_window,
        );

        len = 1;
        input = vec![entry.clone(); len];
        result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            bootstrap_height - 1,
            input.clone(),
        );

        assert_eq!(result, (input.clone(), false));

        result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            bootstrap_height + seed_rotation_interval - 1,
            input.clone(),
        );

        assert_eq!(result, (input, false));
    }

    // Tests for when the leader across slots / epochs are different
    #[test]
    fn test_different_leader_index_calculation() {
        // Set up a dummy node
        let my_keypair = Arc::new(Keypair::new());
        let my_id = my_keypair.pubkey();

        // Set up a dummy bank
        let bank = Arc::new(Bank::default());
        let entry = Entry::new(&Hash::default(), 0, vec![]);

        // Note: An slot is the period of leader_rotation_interval entries
        // time during which a leader is in power

        let leader_rotation_interval = 10;
        let bootstrap_height = 17;
        let seed_rotation_interval = 3 * leader_rotation_interval;
        let active_window = 1;
        let swap_entry_height = bootstrap_height + 2 * seed_rotation_interval;

        // Case 1: A vector that spans different epochs for different leaders
        // should get truncated

        // Set the leader scheduler
        let mut leader_scheduler = make_leader_scheduler(
            my_id,
            bootstrap_height,
            leader_rotation_interval,
            seed_rotation_interval,
            active_window,
        );

        set_new_leader(&bank, &mut leader_scheduler, swap_entry_height);

        // Start test
        let mut start_height = bootstrap_height - 1;
        let extra_len = leader_rotation_interval;
        let expected_len = swap_entry_height - start_height;
        let mut len = expected_len + extra_len;
        let mut input = vec![entry.clone(); len as usize];
        let mut result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            start_height,
            input.clone(),
        );
        input.truncate(expected_len as usize);
        assert_eq!(result, (input, true));

        // Case 2: Start at entry height == the height where the next leader is elected, should
        // return no entries
        len = 1;
        input = vec![entry.clone(); len as usize];
        result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            swap_entry_height,
            input.clone(),
        );

        assert_eq!(result, (vec![], true));

        // Case 3: A vector that lands one before leader rotation happens should not be
        // truncated, and should signal leader rotation == false

        // Reset the leader scheduler
        leader_scheduler = make_leader_scheduler(
            my_id,
            bootstrap_height,
            leader_rotation_interval,
            seed_rotation_interval,
            active_window,
        );

        set_new_leader(&bank, &mut leader_scheduler, swap_entry_height);

        // Start test
        start_height = bootstrap_height - 1;
        let len_remaining = swap_entry_height - start_height;
        len = len_remaining - 1;
        input = vec![entry.clone(); len as usize];
        result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            start_height,
            input.clone(),
        );
        assert_eq!(result, (input, false));

        // Case 4: A vector that lands exactly where leader rotation happens should not be
        // truncated, but should return leader rotation == true

        // Reset the leader scheduler
        leader_scheduler = make_leader_scheduler(
            my_id,
            bootstrap_height,
            leader_rotation_interval,
            seed_rotation_interval,
            active_window,
        );
        set_new_leader(&bank, &mut leader_scheduler, swap_entry_height);

        // Generate the schedule
        leader_scheduler.update_height(bootstrap_height, &bank);

        // Start test
        start_height = bootstrap_height + leader_rotation_interval - 1;
        len = swap_entry_height - start_height;
        input = vec![entry.clone(); len as usize];
        result = WriteStage::find_leader_rotation_index(
            &bank,
            &my_id,
            &mut leader_scheduler,
            start_height,
            input.clone(),
        );

        assert_eq!(result, (input, true));
    }
}
