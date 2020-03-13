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
    account::Account,
    clock::{Epoch, Slot},
    epoch_schedule::EpochSchedule,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    transaction::Transaction,
};
use solana_vote_program::{vote_instruction::VoteInstruction, vote_state::VoteState};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

// Map from a vote account to the authorized voter for an epoch
pub type EpochAuthorizedVoters = HashMap<Arc<Pubkey>, Arc<Pubkey>>;
pub type NodeIdToVoteAccounts = HashMap<Pubkey, Vec<Arc<Pubkey>>>;

pub struct SlotVoteTracker {
    voted: HashSet<Arc<Pubkey>>,
    updates: Option<Vec<Arc<Pubkey>>>,
}

impl SlotVoteTracker {
    #[allow(dead_code)]
    pub fn get_updates(&mut self) -> Option<Vec<Arc<Pubkey>>> {
        self.updates.take()
    }
}

pub struct VoteTracker {
    // Map from a slot to a set of validators who have voted for that slot
    slot_vote_trackers: RwLock<HashMap<Slot, Arc<RwLock<SlotVoteTracker>>>>,
    // Don't track votes from people who are not staked, acts as a spam filter
    epoch_authorized_voters: RwLock<HashMap<Epoch, EpochAuthorizedVoters>>,
    // Map from node id to the set of associated vote accounts
    node_id_to_vote_accounts: RwLock<HashMap<Epoch, NodeIdToVoteAccounts>>,
    all_pubkeys: RwLock<HashSet<Arc<Pubkey>>>,
    epoch_schedule: EpochSchedule,
}

impl VoteTracker {
    pub fn new(root_bank: &Bank) -> Self {
        let current_epoch = root_bank.epoch();
        let leader_schedule_epoch = root_bank
            .epoch_schedule()
            .get_leader_schedule_epoch(root_bank.slot());

        let vote_tracker = Self {
            epoch_authorized_voters: RwLock::new(HashMap::new()),
            slot_vote_trackers: RwLock::new(HashMap::new()),
            node_id_to_vote_accounts: RwLock::new(HashMap::new()),
            all_pubkeys: RwLock::new(HashSet::new()),
            epoch_schedule: *root_bank.epoch_schedule(),
        };

        // Parse voter information about all the known epochs
        for epoch in current_epoch..=leader_schedule_epoch {
            let (new_epoch_authorized_voters, new_node_id_to_vote_accounts, new_pubkeys) =
                VoteTracker::parse_epoch_state(
                    epoch,
                    root_bank
                        .epoch_vote_accounts(epoch)
                        .expect("Epoch vote accounts must exist"),
                    &vote_tracker.all_pubkeys.read().unwrap(),
                );
            vote_tracker.process_new_leader_schedule_epoch_state(
                epoch,
                new_epoch_authorized_voters,
                new_node_id_to_vote_accounts,
                new_pubkeys,
            );
        }

        vote_tracker
    }

    pub fn get_slot_vote_tracker(&self, slot: Slot) -> Option<Arc<RwLock<SlotVoteTracker>>> {
        self.slot_vote_trackers.read().unwrap().get(&slot).cloned()
    }

    // Returns Some if the given pubkey is a staked voter for the epoch at the given
    // slot. Note this decision uses bank.EpochStakes not live stakes.
    pub fn get_voter_pubkey(&self, pubkey: &Pubkey, slot: Slot) -> Option<Arc<Pubkey>> {
        let epoch = self.epoch_schedule.get_epoch(slot);
        self.epoch_authorized_voters
            .read()
            .unwrap()
            .get(&epoch)
            .map(|epoch_authorized_voters| {
                epoch_authorized_voters
                    .get_key_value(pubkey)
                    .map(|(key, _)| key)
            })
            .unwrap_or(None)
            .cloned()
    }

    pub fn get_authorized_voter(&self, pubkey: &Pubkey, slot: Slot) -> Option<Arc<Pubkey>> {
        let epoch = self.epoch_schedule.get_epoch(slot);
        self.epoch_authorized_voters
            .read()
            .unwrap()
            .get(&epoch)
            .map(|epoch_authorized_voters| epoch_authorized_voters.get(pubkey))
            .unwrap_or(None)
            .cloned()
    }

    pub fn parse_epoch_state(
        epoch: Epoch,
        epoch_vote_acounts: &HashMap<Pubkey, (u64, Account)>,
        all_pubkeys: &HashSet<Arc<Pubkey>>,
    ) -> (
        EpochAuthorizedVoters,
        NodeIdToVoteAccounts,
        HashSet<Arc<Pubkey>>,
    ) {
        let mut new_pubkeys = HashSet::new();
        let mut node_id_to_vote_accounts: NodeIdToVoteAccounts = HashMap::new();
        // Get all known vote accounts with nonzero stake and read out their
        // authorized voters
        let epoch_authorized_voters = epoch_vote_acounts
            .iter()
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
                    // Read out the authorized voters
                    let mut authorized_voters = vote_state.authorized_voters().clone();
                    authorized_voters.get_and_cache_authorized_voter_for_epoch(epoch);
                    let authorized_voter = authorized_voters
                        .get_authorized_voter(epoch)
                        .expect("Authorized voter for current epoch must be known");

                    // Get Arcs for all the needed keys
                    let unduplicated_authorized_voter_key = all_pubkeys
                        .get(&authorized_voter)
                        .cloned()
                        .unwrap_or_else(|| {
                            new_pubkeys
                                .get(&authorized_voter)
                                .cloned()
                                .unwrap_or_else(|| {
                                    let new_key = Arc::new(authorized_voter);
                                    new_pubkeys.insert(new_key.clone());
                                    new_key
                                })
                        });

                    let unduplicated_key = all_pubkeys.get(key).cloned().unwrap_or_else(|| {
                        new_pubkeys.get(key).cloned().unwrap_or_else(|| {
                            let new_key = Arc::new(*key);
                            new_pubkeys.insert(new_key.clone());
                            new_key
                        })
                    });

                    node_id_to_vote_accounts
                        .entry(vote_state.node_pubkey)
                        .or_default()
                        .push(unduplicated_key.clone());

                    Some((unduplicated_key, unduplicated_authorized_voter_key))
                } else {
                    None
                }
            })
            .collect();

        (
            epoch_authorized_voters,
            node_id_to_vote_accounts,
            new_pubkeys,
        )
    }

    pub fn vote_contains_authorized_voter(
        vote_tx: &Transaction,
        authorized_voter: &Pubkey,
    ) -> bool {
        let message = &vote_tx.message;
        for (i, key) in message.account_keys.iter().enumerate() {
            if message.is_signer(i) && key == authorized_voter {
                return true;
            }
        }

        false
    }

    // Given a set of validator node ids `N` and vote accounts `V`, removes the vote accounts
    // from `V` that belong to `N`
    pub fn node_id_to_vote_accounts(&self, node_ids: &[Pubkey], slot: Slot) -> Vec<Arc<Pubkey>> {
        let epoch = self.epoch_schedule.get_epoch(slot);
        if let Some(node_id_to_vote_accounts) =
            self.node_id_to_vote_accounts.read().unwrap().get(&epoch)
        {
            node_ids
                .iter()
                .flat_map(|node_id| {
                    node_id_to_vote_accounts
                        .get(node_id)
                        .cloned()
                        .unwrap_or_else(|| vec![])
                        .into_iter()
                })
                .collect()
        } else {
            vec![]
        }
    }

    fn process_new_leader_schedule_epoch_state(
        &self,
        new_leader_schedule_epoch: Epoch,
        new_epoch_authorized_voters: EpochAuthorizedVoters,
        new_node_id_to_vote_accounts: NodeIdToVoteAccounts,
        new_pubkeys: HashSet<Arc<Pubkey>>,
    ) {
        self.epoch_authorized_voters
            .write()
            .unwrap()
            .insert(new_leader_schedule_epoch, new_epoch_authorized_voters);
        self.node_id_to_vote_accounts
            .write()
            .unwrap()
            .insert(new_leader_schedule_epoch, new_node_id_to_vote_accounts);
        for key in new_pubkeys {
            self.all_pubkeys.write().unwrap().insert(key);
        }

        self.all_pubkeys
            .write()
            .unwrap()
            .retain(|pubkey| Arc::strong_count(pubkey) > 1);
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
        vote_tracker: Arc<VoteTracker>,
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

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
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
            let poh_bank = poh_recorder.lock().unwrap().bank();
            if let Some(bank) = poh_bank {
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
        vote_tracker: Arc<VoteTracker>,
        bank_forks: &RwLock<BankForks>,
    ) -> Result<()> {
        let (mut old_leader_schedule_epoch, mut last_root) = {
            let root_bank = bank_forks.read().unwrap().root_bank().clone();
            (
                root_bank.get_leader_schedule_epoch(root_bank.slot()),
                root_bank.slot(),
            )
        };

        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let root_bank = bank_forks.read().unwrap().root_bank().clone();
            if root_bank.slot() != last_root {
                Self::process_new_root(&vote_tracker, root_bank.slot());
                last_root = root_bank.slot();
            }

            let new_leader_schedule_epoch =
                { root_bank.get_leader_schedule_epoch(root_bank.slot()) };

            if old_leader_schedule_epoch != new_leader_schedule_epoch {
                assert!(vote_tracker
                    .epoch_authorized_voters
                    .read()
                    .unwrap()
                    .get(&new_leader_schedule_epoch)
                    .is_none());
                Self::process_new_leader_schedule_epoch(
                    &root_bank,
                    &vote_tracker,
                    new_leader_schedule_epoch,
                );
                old_leader_schedule_epoch = new_leader_schedule_epoch;
            }

            if let Err(e) =
                Self::get_and_process_votes(&vote_txs_receiver, &vote_tracker, last_root)
            {
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

    fn get_and_process_votes(
        vote_txs_receiver: &CrossbeamReceiver<Vec<Transaction>>,
        vote_tracker: &Arc<VoteTracker>,
        last_root: Slot,
    ) -> Result<()> {
        let timer = Duration::from_millis(200);
        let mut vote_txs = vote_txs_receiver.recv_timeout(timer)?;
        while let Ok(new_txs) = vote_txs_receiver.try_recv() {
            vote_txs.extend(new_txs);
        }

        Self::process_votes(vote_tracker, vote_txs, last_root);
        Ok(())
    }

    fn process_votes(vote_tracker: &VoteTracker, vote_txs: Vec<Transaction>, root: Slot) {
        let mut diff: HashMap<Slot, HashSet<Arc<Pubkey>>> = HashMap::new();
        {
            let all_slot_trackers = &vote_tracker.slot_vote_trackers;
            for tx in vote_txs {
                if let (Some(vote_pubkey), Some(vote_instruction)) = tx
                    .message
                    .instructions
                    .first()
                    .and_then(|first_instruction| {
                        first_instruction.accounts.first().map(|offset| {
                            (
                                tx.message.account_keys.get(*offset as usize),
                                limited_deserialize(&first_instruction.data).ok(),
                            )
                        })
                    })
                    .unwrap_or((None, None))
                {
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
                    if !VoteTracker::vote_contains_authorized_voter(
                        &tx,
                        &actual_authorized_voter.unwrap(),
                    ) {
                        continue;
                    }

                    for slot in vote.slots {
                        if slot <= root {
                            continue;
                        }

                        // Only accept votes from authorized vote pubkeys with non-zero stake
                        // that we determined at leader_schedule_epoch boundaries
                        if let Some(vote_pubkey) = vote_tracker.get_voter_pubkey(&vote_pubkey, slot)
                        {
                            // Don't insert if we already have marked down this pubkey
                            // voting for this slot
                            if let Some(slot_tracker) = all_slot_trackers.read().unwrap().get(&slot)
                            {
                                if slot_tracker.read().unwrap().voted.contains(&vote_pubkey) {
                                    continue;
                                }
                            }

                            diff.entry(slot).or_default().insert(vote_pubkey.clone());
                        }
                    }
                }
            }
        }

        for (slot, slot_diff) in diff {
            let slot_tracker = vote_tracker
                .slot_vote_trackers
                .read()
                .unwrap()
                .get(&slot)
                .cloned();
            if let Some(slot_tracker) = slot_tracker {
                let mut w_slot_tracker = slot_tracker.write().unwrap();
                let mut updates = w_slot_tracker.updates.take().unwrap_or_else(|| vec![]);
                for pk in slot_diff {
                    w_slot_tracker.voted.insert(pk.clone());
                    updates.push(pk);
                }
                w_slot_tracker.updates = Some(updates);
            } else {
                let voted: HashSet<_> = slot_diff.into_iter().collect();
                let new_slot_tracker = SlotVoteTracker {
                    voted: voted.clone(),
                    updates: Some(voted.into_iter().collect()),
                };
                vote_tracker
                    .slot_vote_trackers
                    .write()
                    .unwrap()
                    .insert(slot, Arc::new(RwLock::new(new_slot_tracker)));
            }
        }
    }

    fn process_new_root(vote_tracker: &VoteTracker, new_root: Slot) {
        let root_epoch = vote_tracker.epoch_schedule.get_epoch(new_root);
        vote_tracker
            .slot_vote_trackers
            .write()
            .unwrap()
            .retain(|slot, _| *slot >= new_root);
        vote_tracker
            .node_id_to_vote_accounts
            .write()
            .unwrap()
            .retain(|epoch, _| epoch >= &root_epoch);
        vote_tracker
            .epoch_authorized_voters
            .write()
            .unwrap()
            .retain(|epoch, _| epoch >= &root_epoch);
    }

    fn process_new_leader_schedule_epoch(
        root_bank: &Bank,
        vote_tracker: &VoteTracker,
        new_leader_schedule_epoch: Epoch,
    ) {
        let (new_epoch_authorized_voters, new_node_id_to_vote_accounts, new_pubkeys) =
            VoteTracker::parse_epoch_state(
                new_leader_schedule_epoch,
                root_bank
                    .epoch_vote_accounts(new_leader_schedule_epoch)
                    .expect("Epoch vote accounts must exist"),
                &vote_tracker.all_pubkeys.read().unwrap(),
            );

        vote_tracker.process_new_leader_schedule_epoch_state(
            new_leader_schedule_epoch,
            new_epoch_authorized_voters,
            new_node_id_to_vote_accounts,
            new_pubkeys,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet;
    use solana_runtime::{
        bank::Bank,
        genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
    };
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, Signer};
    use solana_vote_program::{vote_state::create_account, vote_transaction};

    #[test]
    fn test_max_vote_tx_fits() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let slots: Vec<_> = (0..31).into_iter().collect();

        let vote_tx = vote_transaction::new_vote_transaction(
            slots,
            Hash::default(),
            Hash::default(),
            &node_keypair,
            &vote_keypair,
            &vote_keypair,
        );

        use bincode::serialized_size;
        info!("max vote size {}", serialized_size(&vote_tx).unwrap());

        let msgs = packet::to_packets(&[vote_tx]); // panics if won't fit

        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn vote_contains_authorized_voter() {
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let authorized_voter = Keypair::new();

        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            Hash::default(),
            Hash::default(),
            &node_keypair,
            &vote_keypair,
            &authorized_voter,
        );

        // Check that the two signing keys pass the check
        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            &node_keypair.pubkey()
        ));

        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            &authorized_voter.pubkey()
        ));

        // Non signing key shouldn't pass the check
        assert!(!VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            &vote_keypair.pubkey()
        ));

        // Set the authorized voter == vote keypair
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            Hash::default(),
            Hash::default(),
            &node_keypair,
            &vote_keypair,
            &vote_keypair,
        );

        // Check that the node_keypair and vote keypair pass the authorized voter check
        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            &node_keypair.pubkey()
        ));

        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            &vote_keypair.pubkey()
        ));

        // The other keypair should not pss the cchecck
        assert!(!VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            &authorized_voter.pubkey()
        ));
    }

    #[test]
    fn test_process_votes() {
        // Create some voters at genesis
        let validator_voting_keypairs: Vec<_> = (0..10)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
            );
        let bank = Bank::new(&genesis_config);

        // Send some votes to process
        let vote_tracker = Arc::new(VoteTracker::new(&bank));
        let (votes_sender, votes_receiver) = unbounded();

        let vote_slots = vec![1, 2];
        validator_voting_keypairs.iter().for_each(|keypairs| {
            let node_keypair = &keypairs.node_keypair;
            let vote_keypair = &keypairs.vote_keypair;
            let vote_tx = vote_transaction::new_vote_transaction(
                vote_slots.clone(),
                Hash::default(),
                Hash::default(),
                node_keypair,
                vote_keypair,
                vote_keypair,
            );
            votes_sender.send(vec![vote_tx]).unwrap();
        });

        // Check that all the votes were registered for each validator correctly
        ClusterInfoVoteListener::get_and_process_votes(&votes_receiver, &vote_tracker, 0).unwrap();
        for vote_slot in vote_slots {
            let slot_vote_tracker = vote_tracker.get_slot_vote_tracker(vote_slot).unwrap();
            let r_slot_vote_tracker = slot_vote_tracker.read().unwrap();
            for voting_keypairs in &validator_voting_keypairs {
                let pubkey = voting_keypairs.vote_keypair.pubkey();
                assert!(r_slot_vote_tracker.voted.contains(&pubkey));
                assert!(r_slot_vote_tracker
                    .updates
                    .as_ref()
                    .unwrap()
                    .contains(&Arc::new(pubkey)));
            }
        }
    }

    #[test]
    fn test_process_votes2() {
        // Create some voters at genesis
        let num_voters = 10;
        let validator_voting_keypairs: Vec<_> = (0..num_voters)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
            );
        let bank = Bank::new(&genesis_config);

        // Send some votes to process
        let vote_tracker = Arc::new(VoteTracker::new(&bank));
        let (votes_sender, votes_receiver) = unbounded();

        for (i, keyset) in validator_voting_keypairs.chunks(2).enumerate() {
            let validator_votes: Vec<_> = keyset
                .iter()
                .map(|keypairs| {
                    let node_keypair = &keypairs.node_keypair;
                    let vote_keypair = &keypairs.vote_keypair;
                    let vote_tx = vote_transaction::new_vote_transaction(
                        vec![i as u64 + 1],
                        Hash::default(),
                        Hash::default(),
                        node_keypair,
                        vote_keypair,
                        vote_keypair,
                    );
                    vote_tx
                })
                .collect();
            votes_sender.send(validator_votes).unwrap();
        }

        // Check that all the votes were registered for each validator correctly
        ClusterInfoVoteListener::get_and_process_votes(&votes_receiver, &vote_tracker, 0).unwrap();
        for (i, keyset) in validator_voting_keypairs.chunks(2).enumerate() {
            let slot_vote_tracker = vote_tracker.get_slot_vote_tracker(i as u64 + 1).unwrap();
            let r_slot_vote_tracker = &slot_vote_tracker.read().unwrap();
            for voting_keypairs in keyset {
                let pubkey = voting_keypairs.vote_keypair.pubkey();
                assert!(r_slot_vote_tracker.voted.contains(&pubkey));
                assert!(r_slot_vote_tracker
                    .updates
                    .as_ref()
                    .unwrap()
                    .contains(&Arc::new(pubkey)));
            }
        }
    }

    #[test]
    fn test_get_voters_by_epoch() {
        // Create some voters at genesis
        let validator_voting_keypairs: Vec<_> = (0..10)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
            );
        let bank = Bank::new(&genesis_config);

        let vote_tracker = VoteTracker::new(&bank);
        let last_known_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let last_known_slot = bank
            .epoch_schedule()
            .get_last_slot_in_epoch(last_known_epoch);

        // Check we can get the voters and authorized voters
        for keypairs in &validator_voting_keypairs {
            assert!(vote_tracker
                .get_voter_pubkey(&keypairs.vote_keypair.pubkey(), last_known_slot)
                .is_some());
            assert!(vote_tracker
                .get_voter_pubkey(&keypairs.vote_keypair.pubkey(), last_known_slot + 1)
                .is_none());
            assert!(vote_tracker
                .get_authorized_voter(&keypairs.vote_keypair.pubkey(), last_known_slot)
                .is_some());
            assert!(vote_tracker
                .get_authorized_voter(&keypairs.vote_keypair.pubkey(), last_known_slot + 1)
                .is_none());
        }

        // Create the set of relevant voters for the next epoch
        let new_epoch = last_known_epoch + 1;
        let first_slot_in_new_epoch = bank.epoch_schedule().get_first_slot_in_epoch(new_epoch);
        let new_keypairs: Vec<_> = (0..10)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();
        let new_epoch_vote_accounts: HashMap<_, _> = new_keypairs
            .iter()
            .chain(validator_voting_keypairs[0..5].iter())
            .map(|keypair| {
                (
                    keypair.vote_keypair.pubkey(),
                    (
                        1,
                        bank.get_account(&keypair.vote_keypair.pubkey())
                            .unwrap_or(create_account(
                                &keypair.vote_keypair.pubkey(),
                                &keypair.vote_keypair.pubkey(),
                                0,
                                100,
                            )),
                    ),
                )
            })
            .collect();

        let (new_epoch_authorized_voters, new_node_id_to_vote_accounts, new_pubkeys) =
            VoteTracker::parse_epoch_state(
                new_epoch,
                &new_epoch_vote_accounts,
                &vote_tracker.all_pubkeys.read().unwrap(),
            );

        vote_tracker.process_new_leader_schedule_epoch_state(
            new_epoch,
            new_epoch_authorized_voters,
            new_node_id_to_vote_accounts,
            new_pubkeys,
        );

        // These keypairs made it into the new epoch
        for keypairs in new_keypairs
            .iter()
            .chain(validator_voting_keypairs[0..5].iter())
        {
            assert!(vote_tracker
                .get_voter_pubkey(&keypairs.vote_keypair.pubkey(), first_slot_in_new_epoch)
                .is_some());
            assert!(vote_tracker
                .get_authorized_voter(&keypairs.vote_keypair.pubkey(), first_slot_in_new_epoch)
                .is_some());
        }

        // These keypairs were not refreshed in new epoch
        for keypairs in validator_voting_keypairs[5..10].iter() {
            assert!(vote_tracker
                .get_voter_pubkey(&keypairs.vote_keypair.pubkey(), first_slot_in_new_epoch)
                .is_none());
            assert!(vote_tracker
                .get_authorized_voter(&keypairs.vote_keypair.pubkey(), first_slot_in_new_epoch)
                .is_none());
        }
    }

    #[test]
    fn test_node_id_to_vote_accounts() {
        // Create some voters at genesis
        let validator_voting_keypairs: Vec<_> = (0..10)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
            );
        let bank = Bank::new(&genesis_config);

        // Send some votes to process
        let vote_tracker = VoteTracker::new(&bank);

        // Given all the node id's, should diff out all the vote accounts
        let node_ids: Vec<_> = validator_voting_keypairs
            .iter()
            .map(|v| v.node_keypair.pubkey())
            .collect();
        let vote_accounts: Vec<_> = validator_voting_keypairs
            .iter()
            .map(|v| Arc::new(v.vote_keypair.pubkey()))
            .collect();
        assert_eq!(
            vote_tracker.node_id_to_vote_accounts(&node_ids, bank.slot()),
            vote_accounts
        );
    }

    #[test]
    fn test_vote_tracker_references() {
        // The number of references that get stored for a pubkey every time
        // a vote is made. One stored in the SlotVoteTracker.voted, one in
        // SlotVoteTracker.updates
        let ref_count_per_vote = 2;

        // Create some voters at genesis
        let validator_voting_keypairs: Vec<_> = (0..2)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();

        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
            );
        let bank = Bank::new(&genesis_config);

        // Send a vote to process, should add a reference to the pubkey for that voter
        // in the tracker
        let validator0_keypairs = &validator_voting_keypairs[0];
        let vote_tracker = VoteTracker::new(&bank);
        let vote_tx = vec![vote_transaction::new_vote_transaction(
            // Must vote > root to be processed
            vec![bank.slot() + 1],
            Hash::default(),
            Hash::default(),
            &validator0_keypairs.node_keypair,
            &validator0_keypairs.vote_keypair,
            &validator0_keypairs.vote_keypair,
        )];

        let mut current_ref_count = Arc::strong_count(
            &vote_tracker
                .get_voter_pubkey(&validator0_keypairs.vote_keypair.pubkey(), bank.slot())
                .unwrap(),
        );

        {
            ClusterInfoVoteListener::process_votes(&vote_tracker, vote_tx, 0);
            let ref_count = Arc::strong_count(
                &vote_tracker
                    .get_voter_pubkey(&validator0_keypairs.vote_keypair.pubkey(), bank.slot())
                    .unwrap(),
            );

            // This pubkey voted for a slot, so ref count goes up
            current_ref_count += ref_count_per_vote;
            assert_eq!(ref_count, current_ref_count);
        }

        // Move into the next epoch, a new set of voters is introduced, with some
        // old voters also still present
        let new_pubkey = Pubkey::new_rand();

        // Pubkey of a vote account that will stick around for the next epoch
        let old_refreshed_pubkey = validator0_keypairs.vote_keypair.pubkey();
        let old_refreshed_account = bank.get_account(&old_refreshed_pubkey).unwrap();

        // Pubkey of a vote account that will be removed in the next epoch
        let old_outdated_pubkey = validator_voting_keypairs[1].vote_keypair.pubkey();
        let new_epoch = bank.get_leader_schedule_epoch(bank.slot()) + 1;
        let first_slot_in_new_epoch = bank.epoch_schedule().get_first_slot_in_epoch(new_epoch);

        // Create the set of relevant voters for the next epoch
        let new_epoch_vote_accounts: HashMap<_, _> = vec![
            ((old_refreshed_pubkey.clone(), (1, old_refreshed_account))),
            (
                new_pubkey.clone(),
                (1, create_account(&new_pubkey, &new_pubkey, 0, 100)),
            ),
        ]
        .into_iter()
        .collect();

        let (new_epoch_authorized_voters, new_node_id_to_vote_accounts, new_pubkeys) =
            VoteTracker::parse_epoch_state(
                new_epoch,
                &new_epoch_vote_accounts,
                &vote_tracker.all_pubkeys.read().unwrap(),
            );

        assert_eq!(
            new_pubkeys,
            vec![Arc::new(new_pubkey)].into_iter().collect()
        );

        // Should add 3 new references to `old_refreshed_pubkey`, two in `new_epoch_authorized_voters`,
        // (one for the voter, one for the authorized voter b/c both are the same key) and
        // one in `new_node_id_to_vote_accounts`s
        vote_tracker.process_new_leader_schedule_epoch_state(
            new_epoch,
            new_epoch_authorized_voters,
            new_node_id_to_vote_accounts,
            new_pubkeys,
        );

        assert!(vote_tracker
            .get_voter_pubkey(&new_pubkey, first_slot_in_new_epoch)
            .is_some());
        assert!(vote_tracker
            .get_voter_pubkey(&old_outdated_pubkey, first_slot_in_new_epoch)
            .is_none());

        // Make sure new copies of the same pubkeys aren't constantly being
        // introduced when the same voter is in both the old and new epoch
        // Instead, only the ref count should go up.
        let ref_count = Arc::strong_count(
            &vote_tracker
                .get_voter_pubkey(&old_refreshed_pubkey, first_slot_in_new_epoch)
                .unwrap(),
        );

        // Ref count goes up by 3 (see above comments)
        current_ref_count += 3;
        assert_eq!(ref_count, current_ref_count);

        // Make 2 new votes in two different epochs, ref count should go up
        // by 2 * ref_count_per_vote
        let vote_txs: Vec<_> = [bank.slot() + 2, first_slot_in_new_epoch]
            .iter()
            .map(|slot| {
                vote_transaction::new_vote_transaction(
                    // Must vote > root to be processed
                    vec![*slot],
                    Hash::default(),
                    Hash::default(),
                    &validator0_keypairs.node_keypair,
                    &validator0_keypairs.vote_keypair,
                    &validator0_keypairs.vote_keypair,
                )
            })
            .collect();

        ClusterInfoVoteListener::process_votes(&vote_tracker, vote_txs, 0);

        let ref_count = Arc::strong_count(
            &vote_tracker
                .get_voter_pubkey(&old_refreshed_pubkey, first_slot_in_new_epoch)
                .unwrap(),
        );

        // Ref count goes up by 2 (see above comments)
        current_ref_count += 2 * ref_count_per_vote;
        assert_eq!(ref_count, current_ref_count);
    }
}
