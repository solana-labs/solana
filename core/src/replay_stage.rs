//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::blocktree_processor;
use crate::cluster_info::ClusterInfo;
use crate::consensus::{StakeLockout, Tower};
use crate::entry::{Entry, EntrySlice};
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::leader_schedule_utils;
use crate::packet::BlobError;
use crate::poh_recorder::PohRecorder;
use crate::result::{Error, Result};
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use solana_metrics::{datapoint_warn, inc_new_counter_info};
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::timing::{self, duration_as_ms};
use solana_sdk::transaction::Transaction;
use solana_vote_api::vote_instruction;
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
    t_replay: JoinHandle<Result<()>>,
}

#[derive(Default)]
struct ForkProgress {
    last_entry: Hash,
    num_blobs: usize,
    started_ms: u64,
    is_dead: bool,
}
impl ForkProgress {
    pub fn new(last_entry: Hash) -> Self {
        Self {
            last_entry,
            num_blobs: 0,
            started_ms: timing::timestamp(),
            is_dead: false,
        }
    }
}

impl ReplayStage {
    #[allow(
        clippy::new_ret_no_self,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn new<T>(
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        voting_keypair: Option<&Arc<T>>,
        blocktree: Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        exit: &Arc<AtomicBool>,
        ledger_signal_receiver: Receiver<bool>,
        subscriptions: &Arc<RpcSubscriptions>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) -> (Self, Receiver<(u64, Pubkey)>, Receiver<Vec<Arc<Bank>>>)
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        let (root_bank_sender, root_bank_receiver) = channel();
        let (slot_full_sender, slot_full_receiver) = channel();
        trace!("replay stage");
        let exit_ = exit.clone();
        let subscriptions = subscriptions.clone();
        let bank_forks = bank_forks.clone();
        let poh_recorder = poh_recorder.clone();
        let my_pubkey = *my_pubkey;
        let mut ticks_per_slot = 0;
        let mut tower = Tower::new_from_forks(&bank_forks.read().unwrap(), &my_pubkey);
        // Start the replay stage loop
        let leader_schedule_cache = leader_schedule_cache.clone();
        let vote_account = *vote_account;
        let voting_keypair = voting_keypair.cloned();
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

                    Self::generate_new_bank_forks(
                        &blocktree,
                        &mut bank_forks.write().unwrap(),
                        &leader_schedule_cache,
                    );

                    let mut is_tpu_bank_active = poh_recorder.lock().unwrap().bank().is_some();

                    Self::replay_active_banks(
                        &blocktree,
                        &bank_forks,
                        &my_pubkey,
                        &mut ticks_per_slot,
                        &mut progress,
                        &slot_full_sender,
                    )?;

                    if ticks_per_slot == 0 {
                        let frozen_banks = bank_forks.read().unwrap().frozen_banks();
                        let bank = frozen_banks.values().next().unwrap();
                        ticks_per_slot = bank.ticks_per_slot();
                    }

                    let votable = Self::generate_votable_banks(&bank_forks, &tower, &mut progress);

                    if let Some((_, bank)) = votable.last() {
                        subscriptions.notify_subscribers(bank.slot(), &bank_forks);

                        Self::handle_votable_bank(
                            &bank,
                            &bank_forks,
                            &mut tower,
                            &mut progress,
                            &vote_account,
                            &voting_keypair,
                            &cluster_info,
                            &blocktree,
                            &leader_schedule_cache,
                            &root_bank_sender,
                        )?;

                        Self::reset_poh_recorder(
                            &my_pubkey,
                            &blocktree,
                            &bank,
                            &poh_recorder,
                            ticks_per_slot,
                            &leader_schedule_cache,
                        );

                        is_tpu_bank_active = false;
                    }

                    let (reached_leader_tick, grace_ticks) = if !is_tpu_bank_active {
                        let poh = poh_recorder.lock().unwrap();
                        poh.reached_leader_tick()
                    } else {
                        (false, 0)
                    };

                    if !is_tpu_bank_active {
                        assert!(ticks_per_slot > 0);
                        let poh_tick_height = poh_recorder.lock().unwrap().tick_height();
                        let poh_slot = leader_schedule_utils::tick_height_to_slot(
                            ticks_per_slot,
                            poh_tick_height + 1,
                        );
                        Self::start_leader(
                            &my_pubkey,
                            &bank_forks,
                            &poh_recorder,
                            &cluster_info,
                            poh_slot,
                            reached_leader_tick,
                            grace_ticks,
                            &leader_schedule_cache,
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
        (Self { t_replay }, slot_full_receiver, root_bank_receiver)
    }
    pub fn start_leader(
        my_pubkey: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        poh_slot: u64,
        reached_leader_tick: bool,
        grace_ticks: u64,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) {
        trace!("{} checking poh slot {}", my_pubkey, poh_slot);
        if bank_forks.read().unwrap().get(poh_slot).is_none() {
            let parent_slot = poh_recorder.lock().unwrap().start_slot();
            let parent = {
                let r_bf = bank_forks.read().unwrap();
                r_bf.get(parent_slot)
                    .expect("start slot doesn't exist in bank forks")
                    .clone()
            };
            assert!(parent.is_frozen());

            leader_schedule_cache.slot_leader_at(poh_slot, Some(&parent))
                .map(|next_leader| {
                    debug!(
                        "me: {} leader {} at poh slot {}",
                        my_pubkey, next_leader, poh_slot
                    );
                    cluster_info.write().unwrap().set_leader(&next_leader);
                    if next_leader == *my_pubkey && reached_leader_tick {
                        debug!("{} starting tpu for slot {}", my_pubkey, poh_slot);
                        datapoint_warn!(
                            "replay_stage-new_leader",
                            ("count", poh_slot, i64),
                            ("grace", grace_ticks, i64));
                        let tpu_bank = Bank::new_from_parent(&parent, my_pubkey, poh_slot);
                        bank_forks.write().unwrap().insert(tpu_bank);
                        if let Some(tpu_bank) = bank_forks.read().unwrap().get(poh_slot).cloned() {
                            assert_eq!(
                                bank_forks.read().unwrap().working_bank().slot(),
                                tpu_bank.slot()
                            );
                            debug!(
                                "poh_recorder new working bank: me: {} next_slot: {} next_leader: {}",
                                my_pubkey,
                                tpu_bank.slot(),
                                next_leader
                            );
                            poh_recorder.lock().unwrap().set_bank(&tpu_bank);
                        }
                    }
                })
                .or_else(|| {
                    warn!("{} No next leader found", my_pubkey);
                    None
                });
        }
    }

    // Returns Some(result) if the `result` is a fatal error, which is an error that will cause a
    // bank to be marked as dead/corrupted
    fn is_replay_result_fatal(result: &Result<()>) -> bool {
        match result {
            Err(Error::TransactionError(e)) => {
                // Transactions withand transaction errors mean this fork is bogus
                let tx_error = Err(e.clone());
                !Bank::can_commit(&tx_error)
            }
            Err(Error::BlobError(BlobError::VerificationFailed)) => true,
            _ => false,
        }
    }

    fn replay_blocktree_into_bank(
        bank: &Bank,
        blocktree: &Blocktree,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> Result<()> {
        let (entries, num) = Self::load_blocktree_entries(bank, blocktree, progress)?;
        let result = Self::replay_entries_into_bank(bank, entries, progress, num);

        if Self::is_replay_result_fatal(&result) {
            Self::mark_dead_slot(bank.slot(), blocktree, progress);
        }

        result
    }

    fn mark_dead_slot(slot: u64, blocktree: &Blocktree, progress: &mut HashMap<u64, ForkProgress>) {
        // Remove from progress map so we no longer try to replay this bank
        let mut progress_entry = progress
            .get_mut(&slot)
            .expect("Progress entry must exist after call to replay_entries_into_bank()");
        progress_entry.is_dead = true;
        blocktree
            .set_dead_slot(slot)
            .expect("Failed to mark slot as dead in blocktree");
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_votable_bank<T>(
        bank: &Arc<Bank>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &mut Tower,
        progress: &mut HashMap<u64, ForkProgress>,
        vote_account: &Pubkey,
        voting_keypair: &Option<Arc<T>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blocktree: &Arc<Blocktree>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        root_bank_sender: &Sender<Vec<Arc<Bank>>>,
    ) -> Result<()>
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        if let Some(new_root) = tower.record_vote(bank.slot(), bank.hash()) {
            // get the root bank before squash
            let root_bank = bank_forks
                .read()
                .unwrap()
                .get(new_root)
                .expect("Root bank doesn't exist")
                .clone();
            let mut rooted_banks = root_bank.parents();
            rooted_banks.push(root_bank);
            let rooted_slots: Vec<_> = rooted_banks.iter().map(|bank| bank.slot()).collect();
            blocktree
                .set_roots(&rooted_slots)
                .expect("Ledger set roots failed");
            // Set root first in leader schedule_cache before bank_forks because bank_forks.root
            // is consumed by repair_service to update gossip, so we don't want to get blobs for
            // repair on gossip before we update leader schedule, otherwise they may get dropped.
            leader_schedule_cache.set_root(rooted_banks.last().unwrap());
            bank_forks.write().unwrap().set_root(new_root);
            Self::handle_new_root(&bank_forks, progress);
            root_bank_sender.send(rooted_banks)?;
        }
        tower.update_epoch(&bank);
        if let Some(ref voting_keypair) = voting_keypair {
            let node_keypair = cluster_info.read().unwrap().keypair.clone();

            // Send our last few votes along with the new one
            let vote_ix = vote_instruction::vote(
                &vote_account,
                &voting_keypair.pubkey(),
                tower.recent_votes(),
            );

            let mut vote_tx =
                Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));;

            let blockhash = bank.last_blockhash();
            vote_tx.partial_sign(&[node_keypair.as_ref()], blockhash);
            vote_tx.partial_sign(&[voting_keypair.as_ref()], blockhash);
            cluster_info.write().unwrap().push_vote(vote_tx);
        }
        Ok(())
    }

    fn reset_poh_recorder(
        my_pubkey: &Pubkey,
        blocktree: &Blocktree,
        bank: &Arc<Bank>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        ticks_per_slot: u64,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) {
        let next_leader_slot =
            leader_schedule_cache.next_leader_slot(&my_pubkey, bank.slot(), &bank, Some(blocktree));
        poh_recorder.lock().unwrap().reset(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.slot(),
            next_leader_slot,
            ticks_per_slot,
        );
        debug!(
            "{:?} voted and reset poh at {}. next leader slot {:?}",
            my_pubkey,
            bank.tick_height(),
            next_leader_slot
        );
    }

    fn replay_active_banks(
        blocktree: &Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        my_pubkey: &Pubkey,
        ticks_per_slot: &mut u64,
        progress: &mut HashMap<u64, ForkProgress>,
        slot_full_sender: &Sender<(u64, Pubkey)>,
    ) -> Result<()> {
        let active_banks = bank_forks.read().unwrap().active_banks();
        trace!("active banks {:?}", active_banks);

        for bank_slot in &active_banks {
            // If the fork was marked as dead, don't replay it
            if progress.get(bank_slot).map(|p| p.is_dead).unwrap_or(false) {
                continue;
            }

            let bank = bank_forks.read().unwrap().get(*bank_slot).unwrap().clone();
            *ticks_per_slot = bank.ticks_per_slot();
            if bank.collector_id() != my_pubkey
                && Self::is_replay_result_fatal(&Self::replay_blocktree_into_bank(
                    &bank, &blocktree, progress,
                ))
            {
                // If the bank was corrupted, don't try to run the below logic to check if the
                // bank is completed
                continue;
            }
            let max_tick_height = (*bank_slot + 1) * bank.ticks_per_slot() - 1;
            if bank.tick_height() == max_tick_height {
                Self::process_completed_bank(my_pubkey, bank, slot_full_sender);
            }
        }

        Ok(())
    }

    fn generate_votable_banks(
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &Tower,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> Vec<(u128, Arc<Bank>)> {
        let tower_start = Instant::now();
        // Tower voting
        let descendants = bank_forks.read().unwrap().descendants();
        let ancestors = bank_forks.read().unwrap().ancestors();
        let frozen_banks = bank_forks.read().unwrap().frozen_banks();

        trace!("frozen_banks {}", frozen_banks.len());
        let mut votable: Vec<(u128, Arc<Bank>)> = frozen_banks
            .values()
            .filter(|b| {
                let is_votable = b.is_votable();
                trace!("bank is votable: {} {}", b.slot(), is_votable);
                is_votable
            })
            .filter(|b| {
                let is_recent_epoch = tower.is_recent_epoch(b);
                trace!("bank is is_recent_epoch: {} {}", b.slot(), is_recent_epoch);
                is_recent_epoch
            })
            .filter(|b| {
                let has_voted = tower.has_voted(b.slot());
                trace!("bank is has_voted: {} {}", b.slot(), has_voted);
                !has_voted
            })
            .filter(|b| {
                let is_locked_out = tower.is_locked_out(b.slot(), &descendants);
                trace!("bank is is_locked_out: {} {}", b.slot(), is_locked_out);
                !is_locked_out
            })
            .map(|bank| {
                (
                    bank,
                    tower.collect_vote_lockouts(
                        bank.slot(),
                        bank.vote_accounts().into_iter(),
                        &ancestors,
                    ),
                )
            })
            .filter(|(b, stake_lockouts)| {
                let vote_threshold = tower.check_vote_stake_threshold(b.slot(), &stake_lockouts);
                Self::confirm_forks(tower, stake_lockouts, progress, bank_forks);
                debug!("bank vote_threshold: {} {}", b.slot(), vote_threshold);
                vote_threshold
            })
            .map(|(b, stake_lockouts)| (tower.calculate_weight(&stake_lockouts), b.clone()))
            .collect();

        votable.sort_by_key(|b| b.0);
        let ms = timing::duration_as_ms(&tower_start.elapsed());

        trace!("votable_banks {}", votable.len());
        if !votable.is_empty() {
            let weights: Vec<u128> = votable.iter().map(|x| x.0).collect();
            info!(
                "@{:?} tower duration: {:?} len: {} weights: {:?}",
                timing::timestamp(),
                ms,
                votable.len(),
                weights
            );
        }
        inc_new_counter_info!("replay_stage-tower_duration", ms as usize);

        votable
    }

    fn confirm_forks(
        tower: &Tower,
        stake_lockouts: &HashMap<u64, StakeLockout>,
        progress: &mut HashMap<u64, ForkProgress>,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        progress.retain(|slot, prog| {
            let duration = timing::timestamp() - prog.started_ms;
            if tower.is_slot_confirmed(*slot, stake_lockouts)
                && bank_forks
                    .read()
                    .unwrap()
                    .get(*slot)
                    .map(|s| s.is_frozen())
                    .unwrap_or(true)
            {
                info!("validator fork confirmed {} {}", *slot, duration);
                datapoint_warn!("validator-confirmation", ("duration_ms", duration, i64));
                false
            } else {
                debug!(
                    "validator fork not confirmed {} {} {:?}",
                    *slot,
                    duration,
                    stake_lockouts.get(slot)
                );
                true
            }
        });
    }

    fn load_blocktree_entries(
        bank: &Bank,
        blocktree: &Blocktree,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> Result<(Vec<Entry>, usize)> {
        let bank_slot = bank.slot();
        let bank_progress = &mut progress
            .entry(bank_slot)
            .or_insert_with(|| ForkProgress::new(bank.last_blockhash()));
        blocktree.get_slot_entries_with_blob_count(bank_slot, bank_progress.num_blobs as u64, None)
    }

    fn replay_entries_into_bank(
        bank: &Bank,
        entries: Vec<Entry>,
        progress: &mut HashMap<u64, ForkProgress>,
        num: usize,
    ) -> Result<()> {
        let bank_progress = &mut progress
            .entry(bank.slot())
            .or_insert_with(|| ForkProgress::new(bank.last_blockhash()));
        let result = Self::verify_and_process_entries(&bank, &entries, &bank_progress.last_entry);
        bank_progress.num_blobs += num;
        if let Some(last_entry) = entries.last() {
            bank_progress.last_entry = last_entry.hash;
        }

        result
    }

    pub fn verify_and_process_entries(
        bank: &Bank,
        entries: &[Entry],
        last_entry: &Hash,
    ) -> Result<()> {
        if !entries.verify(last_entry) {
            trace!(
                "entry verification failed {} {} {} {}",
                entries.len(),
                bank.tick_height(),
                last_entry,
                bank.last_blockhash()
            );

            datapoint_error!(
                "replay-stage-entry_verification_failure",
                ("slot", bank.slot(), i64),
                ("last_entry", last_entry.to_string(), String),
            );
            return Err(Error::BlobError(BlobError::VerificationFailed));
        }
        blocktree_processor::process_entries(bank, entries)?;

        Ok(())
    }

    fn handle_new_root(
        bank_forks: &Arc<RwLock<BankForks>>,
        progress: &mut HashMap<u64, ForkProgress>,
    ) {
        let r_bank_forks = bank_forks.read().unwrap();
        progress.retain(|k, _| r_bank_forks.get(*k).is_some());
    }

    fn process_completed_bank(
        my_pubkey: &Pubkey,
        bank: Arc<Bank>,
        slot_full_sender: &Sender<(u64, Pubkey)>,
    ) {
        bank.freeze();
        bank.commit_credits();
        info!("bank frozen {}", bank.slot());
        if let Err(e) = slot_full_sender.send((bank.slot(), *bank.collector_id())) {
            trace!("{} slot_full alert failed: {:?}", my_pubkey, e);
        }
    }

    fn generate_new_bank_forks(
        blocktree: &Blocktree,
        forks: &mut BankForks,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) {
        // Find the next slot that chains to the old slot
        let frozen_banks = forks.frozen_banks();
        let frozen_bank_slots: Vec<u64> = frozen_banks.keys().cloned().collect();
        trace!("frozen_banks {:?}", frozen_bank_slots);
        let next_slots = blocktree
            .get_slots_since(&frozen_bank_slots)
            .expect("Db error");
        // Filter out what we've already seen
        trace!("generate new forks {:?}", next_slots);
        for (parent_id, children) in next_slots {
            let parent_bank = frozen_banks
                .get(&parent_id)
                .expect("missing parent in bank forks")
                .clone();
            for child_id in children {
                if forks.get(child_id).is_some() {
                    trace!("child already active or frozen {}", child_id);
                    continue;
                }
                let leader = leader_schedule_cache
                    .slot_leader_at(child_id, Some(&parent_bank))
                    .unwrap();
                info!("new fork:{} parent:{}", child_id, parent_id);
                forks.insert(Bank::new_from_parent(&parent_bank, &leader, child_id));
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
    use crate::blocktree::get_tmp_ledger_path;
    use crate::entry;
    use crate::genesis_utils::create_genesis_block;
    use crate::packet::Blob;
    use crate::replay_stage::ReplayStage;
    use solana_runtime::genesis_utils::GenesisBlockInfo;
    use solana_sdk::hash::hash;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use solana_sdk::transaction::TransactionError;
    use std::fs::remove_dir_all;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_child_slots_of_same_parent() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );

            let genesis_block = create_genesis_block(10_000).genesis_block;
            let bank0 = Bank::new(&genesis_block);
            let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank0));
            let mut bank_forks = BankForks::new(0, bank0);
            bank_forks.working_bank().freeze();

            // Insert blob for slot 1, generate new forks, check result
            let mut blob_slot_1 = Blob::default();
            blob_slot_1.set_slot(1);
            blob_slot_1.set_parent(0);
            blocktree.insert_data_blobs(&vec![blob_slot_1]).unwrap();
            assert!(bank_forks.get(1).is_none());
            ReplayStage::generate_new_bank_forks(
                &blocktree,
                &mut bank_forks,
                &leader_schedule_cache,
            );
            assert!(bank_forks.get(1).is_some());

            // Insert blob for slot 3, generate new forks, check result
            let mut blob_slot_2 = Blob::default();
            blob_slot_2.set_slot(2);
            blob_slot_2.set_parent(0);
            blocktree.insert_data_blobs(&vec![blob_slot_2]).unwrap();
            assert!(bank_forks.get(2).is_none());
            ReplayStage::generate_new_bank_forks(
                &blocktree,
                &mut bank_forks,
                &leader_schedule_cache,
            );
            assert!(bank_forks.get(1).is_some());
            assert!(bank_forks.get(2).is_some());
        }

        let _ignored = remove_dir_all(&ledger_path);
    }

    #[test]
    fn test_handle_new_root() {
        let genesis_block = create_genesis_block(10_000).genesis_block;
        let bank0 = Bank::new(&genesis_block);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank0)));
        let mut progress = HashMap::new();
        progress.insert(5, ForkProgress::new(Hash::default()));
        ReplayStage::handle_new_root(&bank_forks, &mut progress);
        assert!(progress.is_empty());
    }

    #[test]
    fn test_dead_forks() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );
            let GenesisBlockInfo {
                genesis_block,
                mint_keypair,
                ..
            } = create_genesis_block(1000);
            let bank0 = Arc::new(Bank::new(&genesis_block));
            let mut progress = HashMap::new();
            progress.insert(bank0.slot(), ForkProgress::new(bank0.last_blockhash()));

            let keypair1 = Keypair::new();
            let keypair2 = Keypair::new();
            let missing_keypair = Keypair::new();

            // Insert entry with TransactionError::AccountNotFound error
            let account_not_found_blob = entry::next_entry(
                &bank0.last_blockhash(),
                1,
                vec![
                    system_transaction::create_user_account(
                        &keypair1,
                        &keypair2.pubkey(),
                        2,
                        bank0.last_blockhash(),
                    ), // should be fine,
                    system_transaction::transfer(
                        &missing_keypair,
                        &mint_keypair.pubkey(),
                        2,
                        bank0.last_blockhash(),
                    ), // should cause AccountNotFound error
                ],
            )
            .to_blob();

            blocktree
                .insert_data_blobs(&[account_not_found_blob])
                .unwrap();
            assert_matches!(
                ReplayStage::replay_blocktree_into_bank(&bank0, &blocktree, &mut progress),
                Err(Error::TransactionError(TransactionError::AccountNotFound))
            );

            // Check that the erroring bank was marked as dead in the progress map
            assert!(progress
                .get(&bank0.slot())
                .map(|b| b.is_dead)
                .unwrap_or(false));

            // Check that the erroring bank was marked as dead in blocktree
            assert!(blocktree.is_dead(bank0.slot()));

            // Create new bank
            let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), bank0.slot() + 1);
            progress.insert(bank1.slot(), ForkProgress::new(bank0.last_blockhash()));
            let bad_hash = hash(&[2; 30]);

            // Insert entry that causes verification failure
            let mut verifcation_failure_blob = entry::next_entry(
                // use wrong blockhash
                &bad_hash,
                1,
                vec![system_transaction::create_user_account(
                    &keypair1,
                    &keypair2.pubkey(),
                    2,
                    bank1.last_blockhash(),
                )],
            )
            .to_blob();
            verifcation_failure_blob.set_slot(1);
            verifcation_failure_blob.set_index(0);
            verifcation_failure_blob.set_parent(bank0.slot());

            blocktree
                .insert_data_blobs(&[verifcation_failure_blob])
                .unwrap();
            assert_matches!(
                ReplayStage::replay_blocktree_into_bank(&bank1, &blocktree, &mut progress),
                Err(Error::BlobError(BlobError::VerificationFailed))
            );
            // Check that the erroring bank was marked as dead in the progress map
            assert!(progress
                .get(&bank1.slot())
                .map(|b| b.is_dead)
                .unwrap_or(false));

            // Check that the erroring bank was marked as dead in blocktree
            assert!(blocktree.is_dead(bank1.slot()));
        }

        let _ignored = remove_dir_all(&ledger_path);
    }
}
