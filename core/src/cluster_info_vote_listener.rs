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
    clock::{Epoch, Slot},
    epoch_schedule::EpochSchedule,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    transaction::Transaction,
};
use solana_vote_program::vote_instruction::VoteInstruction;
use solana_vote_program::vote_state::{AuthorizedVoters, VoteState};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub type EpochAuthorizedVoters = HashMap<Arc<Pubkey>, AuthorizedVoters>;
pub type NodeIdToVoteAccounts = HashMap<Pubkey, Vec<Pubkey>>;

pub struct VoteTracker {
    // Don't track votes from people who are not staked, acts as a spam filter
    epoch_authorized_voters: EpochAuthorizedVoters,
    // Map from a slot to a set of validators who have voted for that slot
    votes: HashMap<Slot, HashSet<Arc<Pubkey>>>,
    // Map from node id to the set of associated vote accounts
    node_id_to_vote_accounts: NodeIdToVoteAccounts,
    epoch_schedule: EpochSchedule,
}

impl VoteTracker {
    pub fn new(root_bank: &Bank) -> Self {
        let leader_schedule_epoch = root_bank
            .epoch_schedule()
            .get_leader_schedule_epoch(root_bank.slot());
        let (epoch_authorized_voters, node_id_to_vote_accounts) =
            Self::parse_epoch_state(root_bank, leader_schedule_epoch);
        Self {
            epoch_authorized_voters,
            votes: HashMap::new(),
            node_id_to_vote_accounts,
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

    pub fn get_authorized_voter(&self, pubkey: &Pubkey, slot: Slot) -> Option<Pubkey> {
        let epoch = self.epoch_schedule.get_epoch(slot);
        self.epoch_authorized_voters
            .get(pubkey)
            .map(|authorized_voters| {
                println!("authorized voters: {:?}", authorized_voters);
                authorized_voters.get_authorized_voter(epoch)
            })
            .unwrap_or(None)
    }

    pub fn parse_epoch_state(
        bank: &Bank,
        epoch: Epoch,
    ) -> (EpochAuthorizedVoters, NodeIdToVoteAccounts) {
        let mut node_id_to_vote_accounts: NodeIdToVoteAccounts = HashMap::new();
        // Get all known vote accounts with nonzero stake and read out their
        // authorized voters
        let epoch_authorized_voters = bank
            .epoch_vote_accounts(epoch)
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
                println!("Found vote state");
                if *stake > 0 {
                    println!("Found vote state with stake > 0, key: {:?}", key);
                    let mut authorized_voters = vote_state.authorized_voters().clone();
                    authorized_voters.get_and_cache_authorized_voter_for_epoch(epoch);
                    authorized_voters.get_and_cache_authorized_voter_for_epoch(epoch + 1);
                    let key = Arc::new(*key);
                    node_id_to_vote_accounts
                        .entry(vote_state.node_pubkey)
                        .or_default()
                        .push(*key.clone());
                    Some((key, authorized_voters))
                } else {
                    None
                }
            })
            .collect();

        (epoch_authorized_voters, node_id_to_vote_accounts)
    }

    pub fn vote_contains_authorized_voter(vote_tx: &Transaction, authorized_voter: Pubkey) -> bool {
        let message = &vote_tx.message;
        for (i, key) in message.account_keys.iter().enumerate() {
            if message.is_signer(i) && *key == authorized_voter {
                return true;
            }
        }

        false
    }

    // Given a set of validator node ids `N` and vote accounts `V`, removes the vote accounts
    // from `V` that belong to `N`
    pub fn diff_vote_accounts(&self, node_ids: &[Pubkey], vote_accounts: &mut HashSet<Pubkey>) {
        for node_id in node_ids {
            self.node_id_to_vote_accounts
                .get(node_id)
                .map(|node_vote_accounts| {
                    for node_vote_account in node_vote_accounts {
                        vote_accounts.remove(node_vote_account);
                    }
                });
        }
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
                Self::process_new_leader_schedule_epoch(
                    &root_bank,
                    &vote_tracker,
                    new_leader_schedule_epoch,
                );
                old_leader_schedule_epoch = new_leader_schedule_epoch;
            }

            if let Err(e) = Self::get_and_process_votes(&vote_txs_receiver, &vote_tracker) {
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
        vote_tracker: &Arc<RwLock<VoteTracker>>,
    ) -> Result<()> {
        let timer = Duration::from_millis(200);
        let mut vote_txs = vote_txs_receiver.recv_timeout(timer)?;
        while let Ok(new_txs) = vote_txs_receiver.try_recv() {
            vote_txs.extend(new_txs);
        }

        Self::process_votes(vote_tracker, vote_txs);
        Ok(())
    }

    fn process_votes(vote_tracker: &Arc<RwLock<VoteTracker>>, vote_txs: Vec<Transaction>) {
        let mut diff: HashMap<Slot, HashSet<Arc<Pubkey>>> = HashMap::new();
        {
            let vote_tracker = vote_tracker.read().unwrap();
            let slot_pubkeys = &vote_tracker.votes;
            for tx in vote_txs {
                if let (Some(vote_pubkey), Some(vote_instruction)) = tx
                    .message
                    .instructions
                    .first()
                    .and_then(|first_instruction| {
                        first_instruction.accounts.first().and_then(|offset| {
                            Some((
                                tx.message.account_keys.get(*offset as usize),
                                limited_deserialize(&first_instruction.data).ok(),
                            ))
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
                            if let Some(slot_vote_pubkeys) = slot_pubkeys.get(&slot) {
                                if slot_vote_pubkeys.contains(vote_pubkey) {
                                    continue;
                                }
                            }

                            diff.entry(slot).or_default().insert(vote_pubkey.clone());
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
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }

    fn process_new_leader_schedule_epoch(
        root_bank: &Bank,
        vote_tracker: &RwLock<VoteTracker>,
        new_leader_schedule_epoch: Epoch,
    ) {
        let (new_epoch_authorized_voters, new_node_id_to_vote_accounts) =
            VoteTracker::parse_epoch_state(root_bank, new_leader_schedule_epoch);

        Self::process_new_leader_schedule_epoch_state(
            vote_tracker,
            new_epoch_authorized_voters,
            new_node_id_to_vote_accounts,
        );
    }

    fn process_new_leader_schedule_epoch_state(
        vote_tracker: &RwLock<VoteTracker>,
        new_epoch_authorized_voters: EpochAuthorizedVoters,
        mut new_node_id_to_vote_accounts: NodeIdToVoteAccounts,
    ) {
        let mut current_validators = vote_tracker.read().unwrap().epoch_authorized_voters.clone();

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

        std::mem::swap(
            &mut new_node_id_to_vote_accounts,
            &mut vote_tracker.node_id_to_vote_accounts,
        )
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
    use solana_sdk::transaction::Transaction;
    use solana_vote_program::{vote_instruction, vote_state::Vote};

    #[test]
    fn test_max_vote_tx_fits() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let slots: Vec<_> = (0..31).into_iter().collect();

        let vote_tx = new_vote_tx(slots, Hash::default(), &node_keypair, &vote_keypair);

        use bincode::serialized_size;
        info!("max vote size {}", serialized_size(&vote_tx).unwrap());

        let msgs = packet::to_packets(&[vote_tx]); // panics if won't fit

        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn vote_contains_authorized_voter() {
        let node_keypair = Keypair::new();
        let authorized_voter = Keypair::new();

        let vote_tx = new_vote_tx(vec![0], Hash::default(), &node_keypair, &authorized_voter);

        // Check that the two signing keys pass the check
        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            node_keypair.pubkey()
        ));

        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            authorized_voter.pubkey()
        ));

        // Set the authorized voter == node keypair
        let vote_tx = new_vote_tx(vec![0], Hash::default(), &node_keypair, &node_keypair);

        // Check that the node_keypair itself still passes the authorized voter check
        assert!(VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            node_keypair.pubkey()
        ));

        // The other keypair should not pass
        assert!(!VoteTracker::vote_contains_authorized_voter(
            &vote_tx,
            authorized_voter.pubkey()
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
        let vote_tracker = Arc::new(RwLock::new(VoteTracker::new(&bank)));
        let (votes_sender, votes_receiver) = unbounded();

        let vote_slots = vec![1, 2];
        validator_voting_keypairs.iter().for_each(|keypairs| {
            let node_keypair = &keypairs.node_keypair;
            let vote_keypair = &keypairs.vote_keypair;
            let vote_tx = new_vote_tx(
                vote_slots.clone(),
                Hash::default(),
                node_keypair,
                vote_keypair,
            );
            votes_sender.send(vec![vote_tx]).unwrap();
        });

        // Check that all the votes were registered for each validator correctly
        ClusterInfoVoteListener::get_and_process_votes(&votes_receiver, &vote_tracker).unwrap();
        for vote_slot in vote_slots {
            let r_vote_tracker = vote_tracker.read().unwrap();
            let votes = r_vote_tracker.votes();
            let votes_for_slot = votes.get(&vote_slot).unwrap();
            for voting_keypairs in &validator_voting_keypairs {
                assert!(votes_for_slot.contains(&voting_keypairs.vote_keypair.pubkey()));
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
        let vote_tracker = Arc::new(RwLock::new(VoteTracker::new(&bank)));
        let (votes_sender, votes_receiver) = unbounded();

        for (i, keyset) in validator_voting_keypairs.chunks(2).enumerate() {
            let validator_votes: Vec<_> = keyset
                .iter()
                .map(|keypairs| {
                    let node_keypair = &keypairs.node_keypair;
                    let vote_keypair = &keypairs.vote_keypair;
                    let vote_tx =
                        new_vote_tx(vec![i as u64], Hash::default(), node_keypair, vote_keypair);
                    vote_tx
                })
                .collect();
            votes_sender.send(validator_votes).unwrap();
        }

        // Check that all the votes were registered for each validator correctly
        ClusterInfoVoteListener::get_and_process_votes(&votes_receiver, &vote_tracker).unwrap();
        let r_vote_tracker = vote_tracker.read().unwrap();
        let votes = r_vote_tracker.votes();
        for (i, keyset) in validator_voting_keypairs.chunks(2).enumerate() {
            let votes_for_slot = votes.get(&(i as u64)).unwrap();
            for voting_keypairs in keyset {
                assert!(votes_for_slot.contains(&voting_keypairs.vote_keypair.pubkey()));
            }
        }
    }

    #[test]
    fn test_diff_vote_accounts() {
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
        let mut vote_accounts = validator_voting_keypairs
            .iter()
            .map(|v| v.vote_keypair.pubkey())
            .collect();
        vote_tracker.diff_vote_accounts(&node_ids, &mut vote_accounts);
        assert!(vote_accounts.is_empty());

        // Given the later half of the node id's, should diff out
        // the later half of the vote accounts
        let node_ids: Vec<_> = validator_voting_keypairs[5..]
            .iter()
            .map(|v| v.node_keypair.pubkey())
            .collect();
        let mut vote_accounts = validator_voting_keypairs
            .iter()
            .map(|v| v.vote_keypair.pubkey())
            .collect::<HashSet<_>>();
        vote_tracker.diff_vote_accounts(&node_ids, &mut vote_accounts);
        let expected = validator_voting_keypairs[0..5]
            .iter()
            .map(|v| v.vote_keypair.pubkey())
            .collect::<HashSet<_>>();
        assert_eq!(vote_accounts, expected);
    }

    #[test]
    fn test_vote_tracker_references() {
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
        let vote_tracker = Arc::new(RwLock::new(VoteTracker::new(&bank)));
        let vote_tx = vec![new_vote_tx(
            vec![0],
            Hash::default(),
            &validator0_keypairs.node_keypair,
            &validator0_keypairs.vote_keypair,
        )];

        {
            let r_vote_tracker = vote_tracker.read().unwrap();
            let pubkey_ref = r_vote_tracker
                .get_voter_pubkey(&validator0_keypairs.vote_keypair.pubkey())
                .unwrap();

            // Refcount is 1 because no votes have referenced this pubkey yet
            assert_eq!(Arc::strong_count(&pubkey_ref), 1);

            ClusterInfoVoteListener::process_votes(&vote_tracker, vote_tx);
            let pubkey_ref = r_vote_tracker
                .get_voter_pubkey(&validator0_keypairs.vote_keypair.pubkey())
                .unwrap();

            // This pubkey voted for a slot, so the refcount is now 2
            assert_eq!(Arc::strong_count(&pubkey_ref), 2);
        }

        // Move into the next epoch, a new set of voters is introduced, with some
        // old voters also still present
        let new_pubkey = Arc::new(Pubkey::new_rand());
        let old_refreshed_pubkey = Arc::new(validator0_keypairs.vote_keypair.pubkey());
        let old_outdated_pubkey = validator_voting_keypairs[1].vote_keypair.pubkey();
        let mut new_epoch_authorized_voters = HashMap::new();
        let new_node_id_to_vote_accounts = HashMap::new();

        // Create the set of relevant voters for the next epoch
        new_epoch_authorized_voters
            .insert(old_refreshed_pubkey.clone(), AuthorizedVoters::default());
        new_epoch_authorized_voters.insert(new_pubkey.clone(), AuthorizedVoters::default());

        ClusterInfoVoteListener::process_new_leader_schedule_epoch_state(
            &vote_tracker,
            new_epoch_authorized_voters,
            new_node_id_to_vote_accounts,
        );
        let r_vote_tracker = vote_tracker.read().unwrap();

        assert!(r_vote_tracker
            .epoch_authorized_voters
            .get(&new_pubkey)
            .is_some());
        assert!(r_vote_tracker
            .epoch_authorized_voters
            .get(&old_outdated_pubkey)
            .is_none());

        // Make sure new copies of the same pubkeys aren't constantly being
        // introduced when the same voter is in both the old and new set
        let pubkey_ref = r_vote_tracker
            .get_voter_pubkey(&old_refreshed_pubkey)
            .unwrap();

        // Ref count remains unchanged from earlier
        assert_eq!(Arc::strong_count(&pubkey_ref), 2);
    }

    fn new_vote_tx(
        slots: Vec<Slot>,
        blockhash: Hash,
        node_keypair: &Keypair,
        authorized_voter_keypair: &Keypair,
    ) -> Transaction {
        let votes = Vote::new(slots, blockhash);
        let vote_ix = vote_instruction::vote(
            &node_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            votes,
        );

        let mut vote_tx = Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));

        vote_tx.partial_sign(&[node_keypair], blockhash);
        vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
        vote_tx
    }
}
