use crate::cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS};
use crate::packet::Packets;
use crate::poh_recorder::PohRecorder;
use crate::result::Result;
use crate::{packet, sigverify};
use crossbeam_channel::Sender as CrossbeamSender;
use solana_metrics::inc_new_counter_debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

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
    ) -> Self {
        let exit = exit.clone();
        let poh_recorder = poh_recorder.clone();
        let thread = Builder::new()
            .name("solana-cluster_info_vote_listener".to_string())
            .spawn(move || {
                let _ = Self::recv_loop(
                    exit,
                    &cluster_info,
                    sigverify_disabled,
                    &sender,
                    poh_recorder,
                );
            })
            .unwrap();
        Self {
            thread_hdls: vec![thread],
        }
    }
    fn recv_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sigverify_disabled: bool,
<<<<<<< HEAD
        sender: &CrossbeamSender<Vec<Packets>>,
=======
        verified_vote_packets_sender: VerifiedVotePacketsSender,
        verified_vote_transactions_sender: VerifiedVoteTransactionsSender,
    ) -> Result<()> {
        let mut last_ts = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            let (labels, votes, new_ts) = cluster_info.get_votes(last_ts);
            inc_new_counter_debug!("cluster_info_vote_listener-recv_count", votes.len());

            last_ts = new_ts;
            if !votes.is_empty() {
                let (vote_txs, packets) = Self::verify_votes(votes, labels, sigverify_disabled);
                verified_vote_transactions_sender.send(vote_txs)?;
                verified_vote_packets_sender.send(packets)?;
            }

            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }

    fn verify_votes(
        votes: Vec<Transaction>,
        labels: Vec<CrdsValueLabel>,
        sigverify_disabled: bool,
    ) -> (Vec<Transaction>, Vec<(CrdsValueLabel, Packets)>) {
        let msgs = packet::to_packets_chunked(&votes, 1);
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

        let (vote_txs, packets) = izip!(
            labels.into_iter(),
            votes.into_iter(),
            r.iter().flatten(),
            msgs,
        )
        .filter_map(|(label, vote, verify_result, packet)| {
            if *verify_result != 0 {
                Some((vote, (label, packet)))
            } else {
                None
            }
        })
        .unzip();
        (vote_txs, packets)
    }

    fn bank_send_loop(
        exit: Arc<AtomicBool>,
        verified_vote_packets_receiver: VerifiedVotePacketsReceiver,
>>>>>>> 79829c98d... Fix vote listener passing bad transactions (#9694)
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
                    sigverify::mark_disabled(&mut msgs, &r);
                    sender.send(msgs)?;
                }
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
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
    use solana_sdk::signature::Signature;
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
<<<<<<< HEAD
=======

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
    fn test_update_new_root() {
        let (vote_tracker, bank, _) = setup();

        // Check outdated slots are purged with new root
        let new_voter = Arc::new(Pubkey::new_rand());
        // Make separate copy so the original doesn't count toward
        // the ref count, which would prevent cleanup
        let new_voter_ = Arc::new(*new_voter);
        vote_tracker.insert_vote(bank.slot(), new_voter_);
        assert!(vote_tracker
            .slot_vote_trackers
            .read()
            .unwrap()
            .contains_key(&bank.slot()));
        let bank1 = Bank::new_from_parent(&bank, &Pubkey::default(), bank.slot() + 1);
        vote_tracker.process_new_root_bank(&bank1);
        assert!(!vote_tracker
            .slot_vote_trackers
            .read()
            .unwrap()
            .contains_key(&bank.slot()));

        // Check `keys` and `epoch_authorized_voters` are purged when new
        // root bank moves to the next epoch
        assert!(vote_tracker.keys.read().unwrap().contains(&new_voter));
        let current_epoch = bank.epoch();
        let new_epoch_bank = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.epoch_schedule()
                .get_first_slot_in_epoch(current_epoch + 1),
        );
        vote_tracker.process_new_root_bank(&new_epoch_bank);
        assert!(!vote_tracker.keys.read().unwrap().contains(&new_voter));
        assert_eq!(
            *vote_tracker.current_epoch.read().unwrap(),
            current_epoch + 1
        );
    }

    #[test]
    fn test_update_new_leader_schedule_epoch() {
        let (vote_tracker, bank, _) = setup();

        // Check outdated slots are purged with new root
        let leader_schedule_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let next_leader_schedule_epoch = leader_schedule_epoch + 1;
        let mut next_leader_schedule_computed = bank.slot();
        loop {
            next_leader_schedule_computed += 1;
            if bank.get_leader_schedule_epoch(next_leader_schedule_computed)
                == next_leader_schedule_epoch
            {
                break;
            }
        }
        assert_eq!(
            bank.get_leader_schedule_epoch(next_leader_schedule_computed),
            next_leader_schedule_epoch
        );
        let next_leader_schedule_bank =
            Bank::new_from_parent(&bank, &Pubkey::default(), next_leader_schedule_computed);
        vote_tracker.update_leader_schedule_epoch(&next_leader_schedule_bank);
        assert_eq!(
            *vote_tracker.leader_schedule_epoch.read().unwrap(),
            next_leader_schedule_epoch
        );
        assert_eq!(
            vote_tracker
                .epoch_authorized_voters
                .read()
                .unwrap()
                .get(&next_leader_schedule_epoch)
                .unwrap(),
            next_leader_schedule_bank
                .epoch_stakes(next_leader_schedule_epoch)
                .unwrap()
                .epoch_authorized_voters()
        );
    }

    #[test]
    fn test_process_votes() {
        // Create some voters at genesis
        let (vote_tracker, _, validator_voting_keypairs) = setup();
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
        let (vote_tracker, _, validator_voting_keypairs) = setup();
        // Send some votes to process
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
        let (vote_tracker, bank, validator_voting_keypairs) = setup();
        let last_known_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let last_known_slot = bank
            .epoch_schedule()
            .get_last_slot_in_epoch(last_known_epoch);

        // Check we can get the authorized voters
        for keypairs in &validator_voting_keypairs {
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
        let new_epoch_authorized_voters: HashMap<_, _> = new_keypairs
            .iter()
            .chain(validator_voting_keypairs[0..5].iter())
            .map(|keypair| (keypair.vote_keypair.pubkey(), keypair.vote_keypair.pubkey()))
            .collect();

        vote_tracker
            .epoch_authorized_voters
            .write()
            .unwrap()
            .insert(new_epoch, Arc::new(new_epoch_authorized_voters));

        // These keypairs made it into the new epoch
        for keypairs in new_keypairs
            .iter()
            .chain(validator_voting_keypairs[0..5].iter())
        {
            assert!(vote_tracker
                .get_authorized_voter(&keypairs.vote_keypair.pubkey(), first_slot_in_new_epoch)
                .is_some());
        }

        // These keypairs were not refreshed in new epoch
        for keypairs in validator_voting_keypairs[5..10].iter() {
            assert!(vote_tracker
                .get_authorized_voter(&keypairs.vote_keypair.pubkey(), first_slot_in_new_epoch)
                .is_none());
        }
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
                100,
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

        ClusterInfoVoteListener::process_votes(&vote_tracker, vote_tx, 0);
        let ref_count = Arc::strong_count(
            &vote_tracker
                .keys
                .read()
                .unwrap()
                .get(&validator0_keypairs.vote_keypair.pubkey())
                .unwrap(),
        );

        // This pubkey voted for a slot, so ref count is `ref_count_per_vote + 1`,
        // +1 in `vote_tracker.keys` and +ref_count_per_vote for the one vote
        let mut current_ref_count = ref_count_per_vote + 1;
        assert_eq!(ref_count, current_ref_count);

        // Setup next epoch
        let old_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let new_epoch = old_epoch + 1;
        let new_epoch_vote_accounts: HashMap<_, _> = vec![(
            validator0_keypairs.vote_keypair.pubkey(),
            validator0_keypairs.vote_keypair.pubkey(),
        )]
        .into_iter()
        .collect();
        vote_tracker
            .epoch_authorized_voters
            .write()
            .unwrap()
            .insert(new_epoch, Arc::new(new_epoch_vote_accounts));

        // Test with votes across two epochs
        let first_slot_in_new_epoch = bank.epoch_schedule().get_first_slot_in_epoch(new_epoch);

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
                .keys
                .read()
                .unwrap()
                .get(&validator0_keypairs.vote_keypair.pubkey())
                .unwrap(),
        );
        current_ref_count += 2 * ref_count_per_vote;
        assert_eq!(ref_count, current_ref_count);
    }

    fn setup() -> (Arc<VoteTracker>, Arc<Bank>, Vec<ValidatorVoteKeypairs>) {
        let validator_voting_keypairs: Vec<_> = (0..10)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
                100,
            );
        let bank = Bank::new(&genesis_config);
        let vote_tracker = VoteTracker::new(&bank);

        // Integrity Checks
        let current_epoch = bank.epoch();
        let leader_schedule_epoch = bank.get_leader_schedule_epoch(bank.slot());

        // Check the vote tracker has all the known epoch state on construction
        for epoch in current_epoch..=leader_schedule_epoch {
            assert_eq!(
                vote_tracker
                    .epoch_authorized_voters
                    .read()
                    .unwrap()
                    .get(&epoch)
                    .unwrap(),
                bank.epoch_stakes(epoch).unwrap().epoch_authorized_voters()
            );
        }

        // Check the epoch state is correct
        assert_eq!(
            *vote_tracker.leader_schedule_epoch.read().unwrap(),
            leader_schedule_epoch,
        );
        assert_eq!(*vote_tracker.current_epoch.read().unwrap(), current_epoch);
        (
            Arc::new(vote_tracker),
            Arc::new(bank),
            validator_voting_keypairs,
        )
    }

    #[test]
    fn test_verify_votes_empty() {
        solana_logger::setup();
        let votes = vec![];
        let labels = vec![];
        let (vote_txs, packets) = ClusterInfoVoteListener::verify_votes(votes, labels, false);
        assert!(vote_txs.is_empty());
        assert!(packets.is_empty());
    }

    fn verify_packets_len(packets: &Vec<(CrdsValueLabel, Packets)>, ref_value: usize) {
        let num_packets: usize = packets.iter().map(|p| p.1.packets.len()).sum();
        assert_eq!(num_packets, ref_value);
    }

    fn test_vote_tx() -> Transaction {
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let auth_voter_keypair = Keypair::new();
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            Hash::default(),
            Hash::default(),
            &node_keypair,
            &vote_keypair,
            &auth_voter_keypair,
        );

        vote_tx
    }

    #[test]
    fn test_verify_votes_1_pass() {
        let vote_tx = test_vote_tx();
        let votes = vec![vote_tx.clone()];
        let labels = vec![CrdsValueLabel::Vote(0, Pubkey::new_rand())];
        let (vote_txs, packets) = ClusterInfoVoteListener::verify_votes(votes, labels, false);
        assert_eq!(vote_txs.len(), 1);
        verify_packets_len(&packets, 1);
    }

    #[test]
    fn test_bad_vote() {
        let vote_tx = test_vote_tx();
        let mut bad_vote = vote_tx.clone();
        bad_vote.signatures[0] = Signature::default();
        let votes = vec![vote_tx.clone(), bad_vote, vote_tx];
        let label = CrdsValueLabel::Vote(0, Pubkey::new_rand());
        let labels: Vec<_> = (0..votes.len())
            .into_iter()
            .map(|_| label.clone())
            .collect();
        let (vote_txs, packets) = ClusterInfoVoteListener::verify_votes(votes, labels, false);
        assert_eq!(vote_txs.len(), 2);
        verify_packets_len(&packets, 2);
    }
>>>>>>> 79829c98d... Fix vote listener passing bad transactions (#9694)
}
