use crate::cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS};
use crate::packet::Packets;
use crate::poh_recorder::PohRecorder;
use crate::result::{Error, Result};
use crate::{packet, sigverify};
use crossbeam_channel::{
    unbounded, Receiver as CrossbeamReceiver, RecvTimeoutError, Sender as CrossbeamSender,
};
use solana_ledger::bank_forks::BankForks;
use solana_metrics::inc_new_counter_debug;
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::Slot, program_utils::limited_deserialize, pubkey::Pubkey, transaction::Transaction,
};
use solana_vote_program::vote_instruction::VoteInstruction;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct VoteTracker {
    // Don't track votes from people who are not staked, acts as a spam filter
    epoch_validators: HashSet<Arc<Pubkey>>,
    // Map from a slot to a set of validators who have voted for that slot
    votes: HashMap<Slot, HashSet<Arc<Pubkey>>>,
    current_epoch: u64,
}

impl VoteTracker {
    pub fn new(root_bank: &Bank) -> Self {
        let current_epoch = root_bank.epoch_schedule().get_epoch(root_bank.slot());
        let epoch_validators = Self::get_staked_pubkeys(root_bank, current_epoch);
        Self {
            epoch_validators,
            votes: HashMap::new(),
            current_epoch,
        }
    }

    pub fn votes(&self) -> &HashMap<Slot, HashSet<Arc<Pubkey>>> {
        &self.votes
    }

    pub fn get_voter_pubkey(&self, pubkey: &Pubkey) -> Option<&Arc<Pubkey>> {
        self.epoch_validators.get(pubkey)
    }

    pub fn get_staked_pubkeys(bank: &Bank, current_epoch: u64) -> HashSet<Arc<Pubkey>> {
        let mut i = 0;
        let epoch_validators = HashSet::new();
        loop {
            // Get all known vote accounts with nonzero stake in any
            // upcoming epochs
            if let Some(vote_accounts) = bank.epoch_vote_accounts(current_epoch + i) {
                let staked_pubkeys = vote_accounts
                    .into_iter()
                    .filter_map(|(pk, (stake, _))| {
                        if *stake > 0 {
                            Some(Arc::new(*pk))
                        } else {
                            None
                        }
                    })
                    .collect();

                epoch_validators.union(&staked_pubkeys);
            } else {
                break;
            }
            i += 1;
        }

        epoch_validators
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
        let mut old_root = bank_forks.read().unwrap().root();
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let root_bank = bank_forks.read().unwrap().root_bank().clone();

            if root_bank.slot() != old_root {
                Self::process_new_root(&root_bank, &vote_tracker);
                old_root = root_bank.slot();
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
                    tx.message.instructions.first(),
                ) {
                    if let Ok(instruction) = limited_deserialize(&instruction.data) {
                        let (vote, vote_pubkey) = {
                            match instruction {
                                VoteInstruction::Vote(vote) => (vote, Pubkey::default()),
                                _ => {
                                    continue;
                                }
                            }
                        };

                        // Only accept votes from vote pubkeys with non-zero stake
                        if let Some(vote_pubkey) = vote_tracker.get_voter_pubkey(&vote_pubkey) {
                            for slot in vote.slots {
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
            let mut current_validators = vote_tracker.read().unwrap().epoch_validators.clone();
            let new_epoch_validators = VoteTracker::get_staked_pubkeys(root_bank, new_epoch);

            // Remove the old pubkeys
            current_validators.retain(|v| new_epoch_validators.contains(v));

            // Insert any new pubkeys, don't re-insert ones we already have,
            // otherwise memory usage increases from the duplicates being held
            // in VoteTracker.votes
            let new_public_keys: Vec<_> = new_epoch_validators
                .difference(&current_validators)
                .cloned()
                .collect();
            for key in new_public_keys {
                current_validators.insert(key);
            }

            let mut vote_tracker = vote_tracker.write().unwrap();
            std::mem::swap(&mut current_validators, &mut vote_tracker.epoch_validators);
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
