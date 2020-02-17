use crate::cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS};
use crate::packet::Packets;
use crate::poh_recorder::PohRecorder;
use crate::result::{Error, Result};
use crate::{packet, sigverify};
use crossbeam_channel::{
    unbounded, Receiver as CrossbeamReceiver, RecvTimeoutError, Sender as CrossbeamSender,
};
use log::*;
use solana_ledger::bank_forks::BankForks;
use solana_metrics::inc_new_counter_debug;
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::Slot, epoch_schedule::EpochSchedule, program_utils::limited_deserialize, pubkey::Pubkey,
    transaction::Transaction,
};
use solana_vote_program::vote_instruction::VoteInstruction;
use solana_vote_program::vote_state::{AuthorizedVoters, VoteState};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct VoteTracker {
    // Don't track votes from people who are not staked, acts as a spam filter
    epoch_authorized_voters: HashMap<Arc<Pubkey>, AuthorizedVoters>,
    // Map from a slot to a set of validators who have voted for that slot
    votes: HashMap<Slot, HashSet<Arc<Pubkey>>>,
    current_epoch: u64,
    epoch_schedule: EpochSchedule,
}

impl VoteTracker {
    pub fn new(root_bank: &Bank) -> Self {
        let current_epoch = root_bank.epoch_schedule().get_epoch(root_bank.slot());
        let epoch_authorized_voters = Self::get_staked_authorized_voters(root_bank, current_epoch);
        Self {
            epoch_authorized_voters,
            votes: HashMap::new(),
            current_epoch,
            epoch_schedule: root_bank.epoch_schedule().clone(),
        }
    }

    pub fn votes(&self) -> &HashMap<Slot, HashSet<Arc<Pubkey>>> {
        &self.votes
    }

    pub fn get_voter_pubkey(&self, pubkey: &Pubkey) -> Option<&Arc<Pubkey>> {
        self.epoch_authorized_voters
            .get_key_value(pubkey)
            .map(|(key, _)| key)
    }

    pub fn get_authorized_voter(&self, pubkey: &Pubkey, slot: u64) -> Option<Pubkey> {
        let epoch = self.epoch_schedule.get_epoch(slot);
        self.epoch_authorized_voters
            .get(pubkey)
            .map(|authorized_voters| authorized_voters.get_authorized_voter(epoch))
            .unwrap_or(None)
    }

    pub fn get_staked_authorized_voters(
        bank: &Bank,
        current_epoch: u64,
    ) -> HashMap<Arc<Pubkey>, AuthorizedVoters> {
        // Get all known vote accounts with nonzero stake and read out their
        // authorized voters
        bank.epoch_vote_accounts(current_epoch)
            .expect("Epoch vote accounts must exist")
            .into_iter()
            .filter_map(|(key, (stake, account))| {
                let vote_state = VoteState::from(&account);
                if vote_state.is_none() {
                    datapoint_warn!(
                        "cluster_info_vote_listener",
                        (
                            "warn",
                            format!("Unable to get vote_state from account {}", key),
                            String
                        ),
                    );
                    return None;
                }
                let vote_state = vote_state.unwrap();
                if *stake > 0 {
                    let mut authorized_voters = vote_state.authorized_voter().clone();
                    authorized_voters.get_and_cache_authorized_voter_for_epoch(current_epoch);
                    authorized_voters.get_and_cache_authorized_voter_for_epoch(current_epoch + 1);
                    Some((Arc::new(*key), authorized_voters))
                } else {
                    None
                }
            })
            .collect()
    }
}

pub struct ClusterInfoVoteListener {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ClusterInfoVoteListener {
    pub fn new(
        exit: &Arc<AtomicBool>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        sigverify_disabled: bool,
        sender: CrossbeamSender<Vec<Packets>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        vote_tracker: Arc<RwLock<VoteTracker>>,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        let exit_ = exit.clone();
        let poh_recorder = poh_recorder.clone();
        let (vote_txs_sender, vote_txs_receiver) = unbounded();
        let listen_thread = Builder::new()
            .name("solana-cluster_info_vote_listener".to_string())
            .spawn(move || {
                let _ = Self::recv_loop(
                    exit_,
                    &cluster_info,
                    sigverify_disabled,
                    &sender,
                    vote_txs_sender,
                    poh_recorder,
                );
            })
            .unwrap();

        let exit_ = exit.clone();
        let send_thread = Builder::new()
            .name("solana-cluster_info_process_votes".to_string())
            .spawn(move || {
                let _ =
                    Self::process_votes_loop(exit_, vote_txs_receiver, vote_tracker, &bank_forks);
            })
            .unwrap();

        Self {
            thread_hdls: vec![listen_thread, send_thread],
        }
    }

    fn recv_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sigverify_disabled: bool,
        packets_sender: &CrossbeamSender<Vec<Packets>>,
        vote_txs_sender: CrossbeamSender<Vec<Transaction>>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
    ) -> Result<()> {
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Some(bank) = poh_recorder.lock().unwrap().bank() {
                let last_ts = bank.last_vote_sync.load(Ordering::Relaxed);
                let (votes, new_ts) = cluster_info.read().unwrap().get_votes(last_ts);
                bank.last_vote_sync
                    .compare_and_swap(last_ts, new_ts, Ordering::Relaxed);
                inc_new_counter_debug!("cluster_info_vote_listener-recv_count", votes.len());
                let mut msgs = packet::to_packets(&votes);
                if !msgs.is_empty() {
                    let r = if sigverify_disabled {
                        sigverify::ed25519_verify_disabled(&msgs)
                    } else {
                        sigverify::ed25519_verify_cpu(&msgs)
                    };
                    assert_eq!(
                        r.iter()
                            .map(|packets_results| packets_results.len())
                            .sum::<usize>(),
                        votes.len()
                    );
                    let valid_votes: Vec<_> = votes
                        .into_iter()
                        .zip(r.iter().flatten())
                        .filter_map(|(vote, verify_result)| {
                            if *verify_result != 0 {
                                Some(vote)
                            } else {
                                None
                            }
                        })
                        .collect();
                    vote_txs_sender.send(valid_votes)?;
                    sigverify::mark_disabled(&mut msgs, &r);
                    packets_sender.send(msgs)?;
                }
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }

    fn process_votes_loop(
        exit: Arc<AtomicBool>,
        vote_txs_receiver: CrossbeamReceiver<Vec<Transaction>>,
        vote_tracker: Arc<RwLock<VoteTracker>>,
        bank_forks: &RwLock<BankForks>,
    ) -> Result<()> {
        let mut old_leader_schedule_epoch = {
            let root_bank = bank_forks.read().unwrap().root_bank().clone();
            root_bank.get_leader_schedule_epoch(root_bank.slot())
        };

        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let root_bank = bank_forks.read().unwrap().root_bank().clone();
            let new_leader_schedule_epoch =
                { root_bank.get_leader_schedule_epoch(root_bank.slot()) };

            if old_leader_schedule_epoch != new_leader_schedule_epoch {
                Self::process_new_root(&root_bank, &vote_tracker);
                old_leader_schedule_epoch = new_leader_schedule_epoch;
            }

            if let Err(e) = Self::process_votes(&vote_txs_receiver, &vote_tracker) {
                match e {
                    Error::CrossbeamRecvTimeoutError(RecvTimeoutError::Disconnected) => {
                        return Ok(());
                    }
                    Error::CrossbeamRecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    _ => {
                        error!("thread {:?} error {:?}", thread::current().name(), e);
                    }
                }
            }
        }
    }

    fn vote_contains_authorized_voter(vote_tx: &Transaction, authorized_voter: Pubkey) -> bool {
        let message = &vote_tx.message;
        for (i, key) in message.account_keys[1..].iter().enumerate() {
            if message.is_signer(i + 1) && *key == authorized_voter {
                return true;
            }
        }

        false
    }

    fn process_votes(
        vote_txs_receiver: &CrossbeamReceiver<Vec<Transaction>>,
        vote_tracker: &Arc<RwLock<VoteTracker>>,
    ) -> Result<()> {
        let timer = Duration::from_millis(200);
        let mut vote_txs = vote_txs_receiver.recv_timeout(timer)?;
        let mut diff: HashMap<Slot, HashSet<Arc<Pubkey>>> = HashMap::new();
        {
            let vote_tracker = vote_tracker.read().unwrap();
            let slot_pubkeys = &vote_tracker.votes;
            while let Ok(new_txs) = vote_txs_receiver.try_recv() {
                vote_txs.extend(new_txs);
            }

            for tx in vote_txs {
                if let (Some(instruction), Some(vote_pubkey)) = (
                    tx.message.instructions.first(),
                    tx.message.account_keys.first(),
                ) {
                    if let Ok(vote_instruction) = limited_deserialize(&instruction.data) {
                        let vote = {
                            match vote_instruction {
                                VoteInstruction::Vote(vote) => vote,
                                _ => {
                                    continue;
                                }
                            }
                        };

                        if vote.slots.is_empty() {
                            continue;
                        }

                        let last_vote_slot = vote.slots.last().unwrap();

                        // Determine the authorized voter based on the last vote slot. This will
                        // drop votes from authorized voters trying to make votes for slots
                        // earlier than the epoch for which they are authorized
                        let actual_authorized_voter =
                            vote_tracker.get_authorized_voter(&vote_pubkey, *last_vote_slot);

                        if actual_authorized_voter.is_none() {
                            continue;
                        }

                        // Voting without the correct authorized pubkey, dump the vote
                        if !Self::vote_contains_authorized_voter(
                            &tx,
                            actual_authorized_voter.unwrap(),
                        ) {
                            continue;
                        }

                        // Only accept votes from authorized vote pubkeys with non-zero stake
                        // that we determined at leader_schedule_epoch boundaries
                        if let Some(vote_pubkey) = vote_tracker.get_voter_pubkey(&vote_pubkey) {
                            for slot in vote.slots {
                                // Don't insert if we already have marked down this pubkey
                                // voting for this slot
                                if slot_pubkeys
                                    .get(&slot)
                                    .map(|slot_vote_pubkeys| {
                                        slot_vote_pubkeys.contains(vote_pubkey)
                                    })
                                    .unwrap_or(false)
                                {
                                    diff.entry(slot).or_default().insert(vote_pubkey.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut vote_tracker = vote_tracker.write().unwrap();
        let all_votes = &mut vote_tracker.votes;
        for (slot, slot_diff) in diff {
            let slot_pubkeys = all_votes.entry(slot).or_default();
            for pk in slot_diff {
                slot_pubkeys.insert(pk);
            }
        }

        Ok(())
    }

    pub fn process_new_root(root_bank: &Bank, vote_tracker: &RwLock<VoteTracker>) {
        let new_epoch = root_bank.epoch_schedule().get_epoch(root_bank.slot());
        if new_epoch != vote_tracker.read().unwrap().current_epoch {
            let mut current_validators =
                vote_tracker.read().unwrap().epoch_authorized_voters.clone();
            let new_epoch_authorized_voters =
                VoteTracker::get_staked_authorized_voters(root_bank, new_epoch);

            // Remove the old pubkeys
            current_validators.retain(|pubkey, _| new_epoch_authorized_voters.contains_key(pubkey));

            // Insert any new pubkeys, don't re-insert ones we already have,
            // otherwise memory usage increases from the duplicates being held
            // in Arc references to those duplicates in VoteTracker.votes
            current_validators.extend(new_epoch_authorized_voters);

            let mut vote_tracker = vote_tracker.write().unwrap();
            std::mem::swap(
                &mut current_validators,
                &mut vote_tracker.epoch_authorized_voters,
            );
            vote_tracker.current_epoch = new_epoch;
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::packet;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, Signer};
    use solana_sdk::transaction::Transaction;
    use solana_vote_program::vote_instruction;
    use solana_vote_program::vote_state::Vote;

    #[test]
    fn test_max_vote_tx_fits() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let slots: Vec<_> = (0..31).into_iter().collect();
        let votes = Vote::new(slots, Hash::default());
        let vote_ix = vote_instruction::vote(&vote_keypair.pubkey(), &vote_keypair.pubkey(), votes);

        let mut vote_tx = Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));

        vote_tx.partial_sign(&[&node_keypair], Hash::default());
        vote_tx.partial_sign(&[&vote_keypair], Hash::default());

        use bincode::serialized_size;
        info!("max vote size {}", serialized_size(&vote_tx).unwrap());

        let msgs = packet::to_packets(&[vote_tx]); // panics if won't fit

        assert_eq!(msgs.len(), 1);
    }
}
