//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::blocktree_processor;
use crate::cluster_info::ClusterInfo;
use crate::entry::{Entry, EntryReceiver, EntrySender, EntrySlice};
use crate::leader_schedule_utils;
use crate::packet::BlobError;
use crate::poh_recorder::PohRecorder;
use crate::result;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use solana_metrics::counter::Counter;
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::timing::duration_as_ms;
use solana_vote_api::vote_transaction::VoteTransaction;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;

pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;

// Implement a destructor for the ReplayStage thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct ReplayStage {
    t_replay: JoinHandle<result::Result<()>>,
}

impl ReplayStage {
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new<T>(
        my_id: &Pubkey,
        vote_account: &Pubkey,
        voting_keypair: Option<Arc<T>>,
        blocktree: Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        exit: &Arc<AtomicBool>,
        ledger_signal_receiver: Receiver<bool>,
        subscriptions: &Arc<RpcSubscriptions>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> (Self, Receiver<(u64, Pubkey)>, EntryReceiver)
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        let (forward_entry_sender, forward_entry_receiver) = channel();
        let (slot_full_sender, slot_full_receiver) = channel();
        trace!("replay stage");
        let exit_ = exit.clone();
        let subscriptions = subscriptions.clone();
        let bank_forks = bank_forks.clone();
        let poh_recorder = poh_recorder.clone();
        let my_id = *my_id;
        let vote_account = *vote_account;
        let mut ticks_per_slot = 0;

        // Start the replay stage loop
        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                let mut progress = HashMap::new();
                loop {
                    let now = Instant::now();
                    // Stop getting entries if we get exit signal
                    if exit_.load(Ordering::Relaxed) {
                        break;
                    }
                    Self::generate_new_bank_forks(&blocktree, &mut bank_forks.write().unwrap());
                    let active_banks = bank_forks.read().unwrap().active_banks();
                    trace!("active banks {:?}", active_banks);
                    let mut votable: Vec<Arc<Bank>> = vec![];
                    let mut is_tpu_bank_active = poh_recorder.lock().unwrap().bank().is_some();
                    for bank_slot in &active_banks {
                        let bank = bank_forks.read().unwrap().get(*bank_slot).unwrap().clone();
                        ticks_per_slot = bank.ticks_per_slot();
                        if bank.collector_id() != my_id {
                            Self::replay_blocktree_into_bank(
                                &bank,
                                &blocktree,
                                &mut progress,
                                &forward_entry_sender,
                            )?;
                        }
                        let max_tick_height = (*bank_slot + 1) * bank.ticks_per_slot() - 1;
                        if bank.tick_height() == max_tick_height {
                            Self::process_completed_bank(
                                &my_id,
                                bank,
                                &mut progress,
                                &mut votable,
                                &slot_full_sender,
                            );
                        }
                    }

                    if ticks_per_slot == 0 {
                        let frozen_banks = bank_forks.read().unwrap().frozen_banks();
                        let bank = frozen_banks.values().next().unwrap();
                        ticks_per_slot = bank.ticks_per_slot();
                    }

                    // TODO: fork selection
                    // vote on the latest one for now
                    votable.sort_by(|b1, b2| b1.slot().cmp(&b2.slot()));

                    if let Some(bank) = votable.last() {
                        subscriptions.notify_subscribers(&bank);

                        if let Some(ref voting_keypair) = voting_keypair {
                            let keypair = voting_keypair.as_ref();
                            let vote = VoteTransaction::new_vote(
                                &vote_account,
                                keypair,
                                bank.slot(),
                                bank.last_blockhash(),
                                0,
                            );
                            cluster_info.write().unwrap().push_vote(vote);
                        }
                        let next_leader_slot =
                            leader_schedule_utils::next_leader_slot(&my_id, bank.slot(), &bank);
                        poh_recorder.lock().unwrap().reset(
                            bank.tick_height(),
                            bank.last_blockhash(),
                            bank.slot(),
                            next_leader_slot,
                            ticks_per_slot,
                        );
                        debug!(
                            "{:?} voted and reset poh at {}. next leader slot {:?}",
                            my_id,
                            bank.tick_height(),
                            next_leader_slot
                        );
                        is_tpu_bank_active = false;
                    }

                    let mut reached_leader_tick = false;
                    if !is_tpu_bank_active {
                        let poh = poh_recorder.lock().unwrap();
                        reached_leader_tick = poh.reached_leader_tick();

                        debug!(
                            "{:?} TPU bank inactive. poh tick {}, leader {}",
                            my_id,
                            poh.tick_height(),
                            reached_leader_tick
                        );
                    };

                    if !is_tpu_bank_active {
                        assert!(ticks_per_slot > 0);
                        let poh_tick_height = poh_recorder.lock().unwrap().tick_height();
                        let poh_slot = leader_schedule_utils::tick_height_to_slot(
                            ticks_per_slot,
                            poh_tick_height + 1,
                        );
                        Self::start_leader(
                            &my_id,
                            &bank_forks,
                            &poh_recorder,
                            &cluster_info,
                            &blocktree,
                            poh_slot,
                            reached_leader_tick,
                        );
                    }

                    inc_new_counter_info!(
                        "replicate_stage-duration",
                        duration_as_ms(&now.elapsed()) as usize
                    );
                    let timer = Duration::from_millis(100);
                    let result = ledger_signal_receiver.recv_timeout(timer);
                    match result {
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                        Ok(_) => trace!("blocktree signal"),
                    };
                }
                Ok(())
            })
            .unwrap();
        (
            Self { t_replay },
            slot_full_receiver,
            forward_entry_receiver,
        )
    }
    pub fn start_leader(
        my_id: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blocktree: &Blocktree,
        poh_slot: u64,
        reached_leader_tick: bool,
    ) {
        trace!("{} checking poh slot {}", my_id, poh_slot);
        if blocktree.meta(poh_slot).unwrap().is_some() {
            // We've already broadcasted entries for this slot, skip it
            return;
        }
        if bank_forks.read().unwrap().get(poh_slot).is_none() {
            let frozen = bank_forks.read().unwrap().frozen_banks();
            let parent_slot = poh_recorder.lock().unwrap().start_slot();
            assert!(frozen.contains_key(&parent_slot));
            let parent = &frozen[&parent_slot];

            leader_schedule_utils::slot_leader_at(poh_slot, parent)
                .map(|next_leader| {
                    debug!(
                        "me: {} leader {} at poh slot {}",
                        my_id, next_leader, poh_slot
                    );
                    cluster_info.write().unwrap().set_leader(&next_leader);
                    if next_leader == *my_id && reached_leader_tick {
                        debug!("{} starting tpu for slot {}", my_id, poh_slot);
                        inc_new_counter_info!("replay_stage-new_leader", poh_slot as usize);
                        let tpu_bank = Bank::new_from_parent(parent, my_id, poh_slot);
                        bank_forks.write().unwrap().insert(poh_slot, tpu_bank);
                        if let Some(tpu_bank) = bank_forks.read().unwrap().get(poh_slot).cloned() {
                            assert_eq!(
                                bank_forks.read().unwrap().working_bank().slot(),
                                tpu_bank.slot()
                            );
                            debug!(
                                "poh_recorder new working bank: me: {} next_slot: {} next_leader: {}",
                                my_id,
                                tpu_bank.slot(),
                                next_leader
                            );
                            poh_recorder.lock().unwrap().set_bank(&tpu_bank);
                        }
                    }
                })
                .or_else(|| {
                    error!("{} No next leader found", my_id);
                    None
                });
        }
    }
    pub fn replay_blocktree_into_bank(
        bank: &Bank,
        blocktree: &Blocktree,
        progress: &mut HashMap<u64, (Hash, usize)>,
        forward_entry_sender: &EntrySender,
    ) -> result::Result<()> {
        let (entries, num) = Self::load_blocktree_entries(bank, blocktree, progress)?;
        let len = entries.len();
        let result =
            Self::replay_entries_into_bank(bank, entries, progress, forward_entry_sender, num);
        if result.is_ok() {
            trace!("verified entries {}", len);
            inc_new_counter_info!("replicate-stage_process_entries", len);
        } else {
            info!("debug to verify entries {}", len);
            //TODO: mark this fork as failed
            inc_new_counter_info!("replicate-stage_failed_process_entries", len);
        }
        Ok(())
    }

    pub fn load_blocktree_entries(
        bank: &Bank,
        blocktree: &Blocktree,
        progress: &mut HashMap<u64, (Hash, usize)>,
    ) -> result::Result<(Vec<Entry>, usize)> {
        let bank_slot = bank.slot();
        let bank_progress = &mut progress
            .entry(bank_slot)
            .or_insert((bank.last_blockhash(), 0));
        blocktree.get_slot_entries_with_blob_count(bank_slot, bank_progress.1 as u64, None)
    }

    pub fn replay_entries_into_bank(
        bank: &Bank,
        entries: Vec<Entry>,
        progress: &mut HashMap<u64, (Hash, usize)>,
        forward_entry_sender: &EntrySender,
        num: usize,
    ) -> result::Result<()> {
        let bank_progress = &mut progress
            .entry(bank.slot())
            .or_insert((bank.last_blockhash(), 0));
        let result = Self::verify_and_process_entries(&bank, &entries, &bank_progress.0);
        bank_progress.1 += num;
        if let Some(last_entry) = entries.last() {
            bank_progress.0 = last_entry.hash;
        }
        if result.is_ok() {
            forward_entry_sender.send(entries)?;
        }
        result
    }

    pub fn verify_and_process_entries(
        bank: &Bank,
        entries: &[Entry],
        last_entry: &Hash,
    ) -> result::Result<()> {
        if !entries.verify(last_entry) {
            trace!(
                "entry verification failed {} {} {} {}",
                entries.len(),
                bank.tick_height(),
                last_entry,
                bank.last_blockhash()
            );
            return Err(result::Error::BlobError(BlobError::VerificationFailed));
        }
        blocktree_processor::process_entries(bank, entries)?;

        Ok(())
    }

    fn process_completed_bank(
        my_id: &Pubkey,
        bank: Arc<Bank>,
        progress: &mut HashMap<u64, (Hash, usize)>,
        votable: &mut Vec<Arc<Bank>>,
        slot_full_sender: &Sender<(u64, Pubkey)>,
    ) {
        bank.freeze();
        info!("bank frozen {}", bank.slot());
        progress.remove(&bank.slot());
        if let Err(e) = slot_full_sender.send((bank.slot(), bank.collector_id())) {
            info!("{} slot_full alert failed: {:?}", my_id, e);
        }
        if bank.is_votable() {
            votable.push(bank);
        }
    }

    fn generate_new_bank_forks(blocktree: &Blocktree, forks: &mut BankForks) {
        // Find the next slot that chains to the old slot
        let frozen_banks = forks.frozen_banks();
        let frozen_bank_slots: Vec<u64> = frozen_banks.keys().cloned().collect();
        trace!("frozen_banks {:?}", frozen_bank_slots);
        let next_slots = blocktree
            .get_slots_since(&frozen_bank_slots)
            .expect("Db error");
        trace!("generate new forks {:?}", next_slots);
        for (parent_id, children) in next_slots {
            let parent_bank = frozen_banks
                .get(&parent_id)
                .expect("missing parent in bank forks")
                .clone();
            for child_id in children {
                if frozen_banks.get(&child_id).is_some() {
                    trace!("child already frozen {}", child_id);
                    continue;
                }
                if forks.get(child_id).is_some() {
                    trace!("child already active {}", child_id);
                    continue;
                }
                let leader = leader_schedule_utils::slot_leader_at(child_id, &parent_bank).unwrap();
                info!("new fork:{} parent:{}", child_id, parent_id);
                forks.insert(
                    child_id,
                    Bank::new_from_parent(&parent_bank, &leader, child_id),
                );
            }
        }
    }
}

impl Service for ReplayStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_replay.join().map(|_| ())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::banking_stage::create_test_recorder;
    use crate::blocktree::create_new_tmp_ledger;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::create_ticks;
    use crate::entry::{next_entry_mut, Entry};
    use crate::fullnode::new_banks_from_blocktree;
    use crate::replay_stage::ReplayStage;
    use crate::result::Error;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_vote_error_replay_stage_correctness() {
        solana_logger::setup();
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(&my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();

        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(10_000, &leader_id, 500);

        let (my_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            my_node.info.clone(),
        )));

        // Set up the replay stage
        {
            let voting_keypair = Arc::new(Keypair::new());
            let (bank_forks, _bank_forks_info, blocktree, l_receiver) =
                new_banks_from_blocktree(&my_ledger_path, None);
            let bank = bank_forks.working_bank();

            let blocktree = Arc::new(blocktree);
            let (exit, poh_recorder, poh_service, _entry_receiver) = create_test_recorder(&bank);
            let (replay_stage, _slot_full_receiver, ledger_writer_recv) = ReplayStage::new(
                &my_keypair.pubkey(),
                &voting_keypair.pubkey(),
                Some(voting_keypair.clone()),
                blocktree.clone(),
                &Arc::new(RwLock::new(bank_forks)),
                cluster_info_me.clone(),
                &exit,
                l_receiver,
                &Arc::new(RpcSubscriptions::default()),
                &poh_recorder,
            );

            let keypair = voting_keypair.as_ref();
            let vote =
                VoteTransaction::new_vote(&keypair.pubkey(), keypair, 0, bank.last_blockhash(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

            info!("Send ReplayStage an entry, should see it on the ledger writer receiver");
            let next_tick = create_ticks(1, bank.last_blockhash());
            blocktree
                .write_entries(1, 0, 0, genesis_block.ticks_per_slot, next_tick.clone())
                .unwrap();

            let received_tick = ledger_writer_recv
                .recv()
                .expect("Expected to receive an entry on the ledger writer receiver");

            assert_eq!(next_tick[0], received_tick[0]);

            exit.store(true, Ordering::Relaxed);
            replay_stage.join().unwrap();
            poh_service.join().unwrap();
        }
        let _ignored = remove_dir_all(&my_ledger_path);
    }

    #[test]
    fn test_no_vote_empty_transmission() {
        let genesis_block = GenesisBlock::new(10_000).0;
        let bank = Arc::new(Bank::new(&genesis_block));
        let mut blockhash = bank.last_blockhash();
        let mut entries = Vec::new();
        for _ in 0..genesis_block.ticks_per_slot {
            let entry = next_entry_mut(&mut blockhash, 1, vec![]); //just ticks
            entries.push(entry);
        }
        let (sender, _receiver) = channel();

        let mut progress = HashMap::new();
        let (forward_entry_sender, _forward_entry_receiver) = channel();
        ReplayStage::replay_entries_into_bank(
            &bank,
            entries.clone(),
            &mut progress,
            &forward_entry_sender,
            0,
        )
        .unwrap();

        let mut votable = vec![];
        ReplayStage::process_completed_bank(
            &Pubkey::default(),
            bank,
            &mut progress,
            &mut votable,
            &sender,
        );
        assert!(progress.is_empty());
        // Don't vote on slot that only contained ticks
        assert!(votable.is_empty());
    }

    #[test]
    fn test_replay_stage_poh_ok_entry_receiver() {
        let (forward_entry_sender, forward_entry_receiver) = channel();
        let genesis_block = GenesisBlock::new(10_000).0;
        let bank = Arc::new(Bank::new(&genesis_block));
        let mut blockhash = bank.last_blockhash();
        let mut entries = Vec::new();
        for _ in 0..5 {
            let entry = next_entry_mut(&mut blockhash, 1, vec![]); //just ticks
            entries.push(entry);
        }

        let mut progress = HashMap::new();
        let res = ReplayStage::replay_entries_into_bank(
            &bank,
            entries.clone(),
            &mut progress,
            &forward_entry_sender,
            0,
        );
        assert!(res.is_ok(), "replay failed {:?}", res);
        let res = forward_entry_receiver.try_recv();
        match res {
            Ok(_) => (),
            Err(e) => assert!(false, "Entries were not sent correctly {:?}", e),
        }
    }

    #[test]
    fn test_replay_stage_poh_error_entry_receiver() {
        let (forward_entry_sender, forward_entry_receiver) = channel();
        let mut entries = Vec::new();
        for _ in 0..5 {
            let entry = Entry::new(&mut Hash::default(), 1, vec![]); //just broken entries
            entries.push(entry);
        }

        let genesis_block = GenesisBlock::new(10_000).0;
        let bank = Arc::new(Bank::new(&genesis_block));
        let mut progress = HashMap::new();
        let res = ReplayStage::replay_entries_into_bank(
            &bank,
            entries.clone(),
            &mut progress,
            &forward_entry_sender,
            0,
        );

        match res {
            Ok(_) => assert!(false, "Should have failed because entries are broken"),
            Err(Error::BlobError(BlobError::VerificationFailed)) => (),
            Err(e) => assert!(
                false,
                "Should have failed because with blob error, instead, got {:?}",
                e
            ),
        }
        assert!(forward_entry_receiver.try_recv().is_err());
    }
}
