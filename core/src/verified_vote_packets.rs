use {
    crate::{cluster_info_vote_listener::VerifiedLabelVotePacketsReceiver, result::Result},
<<<<<<< HEAD
    solana_gossip::crds_value::CrdsValueLabel,
    solana_perf::packet::Packets,
    solana_sdk::clock::Slot,
=======
    crossbeam_channel::Select,
    solana_perf::packet::PacketBatch,
    solana_runtime::bank::Bank,
    solana_sdk::{
        account::from_account, clock::Slot, hash::Hash, pubkey::Pubkey, signature::Signature,
        slot_hashes::SlotHashes, sysvar,
    },
    solana_vote_program::vote_state::VoteTransaction,
>>>>>>> 254ef3e7b (Rename Packets to PacketBatch (#21794))
    std::{
        collections::{hash_map::Entry, HashMap},
        time::Duration,
    },
};

<<<<<<< HEAD
=======
const MAX_VOTES_PER_VALIDATOR: usize = 1000;

pub struct VerifiedVoteMetadata {
    pub vote_account_key: Pubkey,
    pub vote: Box<dyn VoteTransaction>,
    pub packet_batch: PacketBatch,
    pub signature: Signature,
}

pub struct ValidatorGossipVotesIterator<'a> {
    my_leader_bank: Arc<Bank>,
    slot_hashes: SlotHashes,
    verified_vote_packets: &'a VerifiedVotePackets,
    vote_account_keys: Vec<Pubkey>,
    previously_sent_to_bank_votes: &'a mut HashSet<Signature>,
}

impl<'a> ValidatorGossipVotesIterator<'a> {
    pub fn new(
        my_leader_bank: Arc<Bank>,
        verified_vote_packets: &'a VerifiedVotePackets,
        previously_sent_to_bank_votes: &'a mut HashSet<Signature>,
    ) -> Self {
        let slot_hashes_account = my_leader_bank.get_account(&sysvar::slot_hashes::id());

        if slot_hashes_account.is_none() {
            warn!(
                "Slot hashes sysvar doesn't exist on bank {}",
                my_leader_bank.slot()
            );
        }

        let slot_hashes_account = slot_hashes_account.unwrap_or_default();
        let slot_hashes = from_account::<SlotHashes, _>(&slot_hashes_account).unwrap_or_default();

        // TODO: my_leader_bank.vote_accounts() may not contain zero-staked validators
        // in this epoch, but those validators may have stake warming up in the next epoch
        let vote_account_keys: Vec<Pubkey> =
            my_leader_bank.vote_accounts().keys().copied().collect();

        Self {
            my_leader_bank,
            slot_hashes,
            verified_vote_packets,
            vote_account_keys,
            previously_sent_to_bank_votes,
        }
    }
}

/// Each iteration returns all of the missing votes for a single validator, the votes
/// ordered from smallest to largest.
///
/// Iterator is done after iterating through all vote accounts
impl<'a> Iterator for ValidatorGossipVotesIterator<'a> {
    type Item = Vec<PacketBatch>;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: Maybe prioritize by stake weight
        while !self.vote_account_keys.is_empty() {
            let vote_account_key = self.vote_account_keys.pop().unwrap();
            // Get all the gossip votes we've queued up for this validator
            // that are:
            // 1) missing from the current leader bank
            // 2) on the same fork
            let validator_votes = self
                .verified_vote_packets
                .0
                .get(&vote_account_key)
                .and_then(|validator_gossip_votes| {
                    // Fetch the validator's vote state from the bank
                    self.my_leader_bank
                        .vote_accounts()
                        .get(&vote_account_key)
                        .and_then(|(_stake, vote_account)| {
                            vote_account.vote_state().as_ref().ok().map(|vote_state| {
                                let start_vote_slot =
                                    vote_state.last_voted_slot().map(|x| x + 1).unwrap_or(0);
                                // Filter out the votes that are outdated
                                validator_gossip_votes
                                    .range((start_vote_slot, Hash::default())..)
                                    .filter_map(|((slot, hash), (packet, tx_signature))| {
                                        if self.previously_sent_to_bank_votes.contains(tx_signature)
                                        {
                                            return None;
                                        }
                                        // Don't send the same vote to the same bank multiple times
                                        self.previously_sent_to_bank_votes.insert(*tx_signature);
                                        // Filter out votes on the wrong fork (or too old to be)
                                        // on this fork
                                        if self
                                            .slot_hashes
                                            .get(slot)
                                            .map(|found_hash| found_hash == hash)
                                            .unwrap_or(false)
                                        {
                                            Some(packet.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<PacketBatch>>()
                            })
                        })
                });
            if let Some(validator_votes) = validator_votes {
                if !validator_votes.is_empty() {
                    return Some(validator_votes);
                }
            }
        }
        None
    }
}

pub type SingleValidatorVotes = BTreeMap<(Slot, Hash), (PacketBatch, Signature)>;

>>>>>>> 254ef3e7b (Rename Packets to PacketBatch (#21794))
#[derive(Default)]
pub struct VerifiedVotePackets(HashMap<CrdsValueLabel, (u64, Slot, Packets)>);

impl VerifiedVotePackets {
    pub fn receive_and_process_vote_packets(
        &mut self,
        vote_packets_receiver: &VerifiedLabelVotePacketsReceiver,
        last_update_version: &mut u64,
        would_be_leader: bool,
    ) -> Result<()> {
        let timer = Duration::from_millis(200);
        let vote_packets = vote_packets_receiver.recv_timeout(timer)?;
        *last_update_version += 1;
        if would_be_leader {
            for (label, slot, packet) in vote_packets {
                self.0.insert(label, (*last_update_version, slot, packet));
            }
        } else {
            self.0.clear();
            self.0.shrink_to_fit();
        }
        while let Ok(vote_packets) = vote_packets_receiver.try_recv() {
            if would_be_leader {
<<<<<<< HEAD
                for (label, slot, packet) in vote_packets {
                    self.0.insert(label, (*last_update_version, slot, packet));
=======
                for verfied_vote_metadata in gossip_votes {
                    let VerifiedVoteMetadata {
                        vote_account_key,
                        vote,
                        packet_batch,
                        signature,
                    } = verfied_vote_metadata;
                    if vote.is_empty() {
                        error!("Empty votes should have been filtered out earlier in the pipeline");
                        continue;
                    }
                    let slot = vote.last_voted_slot().unwrap();
                    let hash = vote.hash();

                    let validator_votes = self.0.entry(vote_account_key).or_default();
                    validator_votes.insert((slot, hash), (packet_batch, signature));

                    if validator_votes.len() > MAX_VOTES_PER_VALIDATOR {
                        let smallest_key = validator_votes.keys().next().cloned().unwrap();
                        validator_votes.remove(&smallest_key).unwrap();
                    }
>>>>>>> 254ef3e7b (Rename Packets to PacketBatch (#21794))
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn get_vote_packets(&self, key: &CrdsValueLabel) -> Option<&(u64, Slot, Packets)> {
        self.0.get(key)
    }

    pub fn get_latest_votes(&self, last_update_version: u64) -> (u64, Packets) {
        let mut new_update_version = last_update_version;
        let mut votes = HashMap::new();
        for (label, (version, slot, packets)) in &self.0 {
            new_update_version = std::cmp::max(*version, new_update_version);
            if *version <= last_update_version {
                continue;
            }
            match votes.entry(label.pubkey()) {
                Entry::Vacant(entry) => {
                    entry.insert((slot, packets));
                }
                Entry::Occupied(mut entry) => {
                    let (entry_slot, _) = entry.get();
                    if *entry_slot < slot {
                        *entry.get_mut() = (slot, packets);
                    }
                }
            }
        }
        let packets = votes
            .into_iter()
            .flat_map(|(_, (_, packets))| &packets.packets)
            .cloned()
            .collect();
        (new_update_version, Packets::new(packets))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::result::Error,
        crossbeam_channel::{unbounded, RecvTimeoutError},
        solana_perf::packet::{Meta, Packet},
    };

    #[test]
    fn test_get_latest_votes() {
        let pubkey = solana_sdk::pubkey::new_rand();
        let label1 = CrdsValueLabel::Vote(0, pubkey);
        let label2 = CrdsValueLabel::Vote(1, pubkey);
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

<<<<<<< HEAD
        let data = Packet {
            meta: Meta {
                repair: true,
                ..Meta::default()
            },
            ..Packet::default()
        };

        let none_empty_packets = Packets::new(vec![data, Packet::default()]);

        verified_vote_packets
            .0
            .insert(label1, (2, 42, none_empty_packets));
=======
        // Send a vote from `vote_account_key`, check that it was inserted
        let vote_slot = 0;
        let vote_hash = Hash::new_unique();
        let vote = Vote::new(vec![vote_slot], vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: Box::new(vote.clone()),
            packet_batch: PacketBatch::default(),
            signature: Signature::new(&[1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .len(),
            1
        );

        // Same slot, same hash, should not be inserted
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: Box::new(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::new(&[1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .len(),
            1
        );

        // Same slot, different hash, should still be inserted
        let new_vote_hash = Hash::new_unique();
        let vote = Vote::new(vec![vote_slot], new_vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: Box::new(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::new(&[1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .len(),
            2
        );

        // Different vote slot, should be inserted
        let vote_slot = 1;
        let vote_hash = Hash::new_unique();
        let vote = Vote::new(vec![vote_slot], vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: Box::new(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::new(&[2u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .len(),
            3
        );

        // No new messages, should time out
        assert_matches!(
            verified_vote_packets.receive_and_process_vote_packets(&r, true),
            Err(Error::ReadyTimeout)
        );
    }

    #[test]
    fn test_verified_vote_packets_receive_and_process_vote_packets_max_len() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();

        // Construct the buffer
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        // Send many more votes than the upper limit per validator
        for _ in 0..2 * MAX_VOTES_PER_VALIDATOR {
            let vote_slot = 0;
            let vote_hash = Hash::new_unique();
            let vote = Vote::new(vec![vote_slot], vote_hash);
            s.send(vec![VerifiedVoteMetadata {
                vote_account_key,
                vote: Box::new(vote),
                packet_batch: PacketBatch::default(),
                signature: Signature::new(&[1u8; 64]),
            }])
            .unwrap();
        }

        // At most `MAX_VOTES_PER_VALIDATOR` should be stored per validator
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .len(),
            MAX_VOTES_PER_VALIDATOR
        );
    }

    #[test]
    fn test_verified_vote_packets_validator_gossip_votes_iterator_wrong_fork() {
        let (s, r) = unbounded();
        let vote_simulator = VoteSimulator::new(1);
        let my_leader_bank = vote_simulator.bank_forks.read().unwrap().root_bank();
        let vote_account_key = vote_simulator.vote_pubkeys[0];

        // Create a bunch of votes with random vote hashes, which should all be ignored
        // since they are not on the same fork as `my_leader_bank`, i.e. their hashes do
        // not exist in the SlotHashes sysvar for `my_leader_bank`
        for _ in 0..MAX_VOTES_PER_VALIDATOR {
            let vote_slot = 0;
            let vote_hash = Hash::new_unique();
            let vote = Vote::new(vec![vote_slot], vote_hash);
            s.send(vec![VerifiedVoteMetadata {
                vote_account_key,
                vote: Box::new(vote),
                packet_batch: PacketBatch::default(),
                signature: Signature::new_unique(),
            }])
            .unwrap();
        }

        // Ingest the votes into the buffer
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();

        // Create tracker for previously sent bank votes
        let mut previously_sent_to_bank_votes = HashSet::new();
        let mut gossip_votes_iterator = ValidatorGossipVotesIterator::new(
            my_leader_bank,
            &verified_vote_packets,
            &mut previously_sent_to_bank_votes,
        );

        // Wrong fork, we should get no hashes
        assert!(gossip_votes_iterator.next().is_none());
    }

    #[test]
    fn test_verified_vote_packets_validator_gossip_votes_iterator_correct_fork() {
        let (s, r) = unbounded();
        let num_validators = 2;
        let vote_simulator = VoteSimulator::new(2);
        let mut my_leader_bank = vote_simulator.bank_forks.read().unwrap().root_bank();

        // Create a set of valid ancestor hashes for this fork
        for _ in 0..MAX_ENTRIES {
            my_leader_bank = Arc::new(Bank::new_from_parent(
                &my_leader_bank,
                &Pubkey::default(),
                my_leader_bank.slot() + 1,
            ));
        }
        let slot_hashes_account = my_leader_bank
            .get_account(&sysvar::slot_hashes::id())
            .expect("Slot hashes sysvar must exist");
        let slot_hashes = from_account::<SlotHashes, _>(&slot_hashes_account).unwrap();

        // Create valid votes
        for i in 0..num_validators {
            let vote_account_key = vote_simulator.vote_pubkeys[i];
            // Used to uniquely identify the packets for each validator
            let num_packets = i + 1;
            for (vote_slot, vote_hash) in slot_hashes.slot_hashes().iter() {
                let vote = Vote::new(vec![*vote_slot], *vote_hash);
                s.send(vec![VerifiedVoteMetadata {
                    vote_account_key,
                    vote: Box::new(vote),
                    packet_batch: PacketBatch::new(vec![Packet::default(); num_packets]),
                    signature: Signature::new_unique(),
                }])
                .unwrap();
            }
        }

        // Ingest the votes into the buffer
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();

        // Check we get two batches, one for each validator. Each batch
        // should only contain a packets structure with the specific number
        // of packets associated with that batch
        assert_eq!(verified_vote_packets.0.len(), 2);
        // Every validator should have `slot_hashes.slot_hashes().len()` votes
        assert!(verified_vote_packets
            .0
            .values()
            .all(|validator_votes| validator_votes.len() == slot_hashes.slot_hashes().len()));

        let mut previously_sent_to_bank_votes = HashSet::new();
        let mut gossip_votes_iterator = ValidatorGossipVotesIterator::new(
            my_leader_bank.clone(),
            &verified_vote_packets,
            &mut previously_sent_to_bank_votes,
        );

        // Get and verify batches
        let num_expected_batches = 2;
        for _ in 0..num_expected_batches {
            let validator_batch: Vec<PacketBatch> = gossip_votes_iterator.next().unwrap();
            assert_eq!(validator_batch.len(), slot_hashes.slot_hashes().len());
            let expected_len = validator_batch[0].packets.len();
            assert!(validator_batch
                .iter()
                .all(|batch| batch.packets.len() == expected_len));
        }

        // Should be empty now
        assert!(gossip_votes_iterator.next().is_none());

        // If we construct another iterator, should return nothing because `previously_sent_to_bank_votes`
        // should filter out everything
        let mut gossip_votes_iterator = ValidatorGossipVotesIterator::new(
            my_leader_bank.clone(),
            &verified_vote_packets,
            &mut previously_sent_to_bank_votes,
        );
        assert!(gossip_votes_iterator.next().is_none());

        // If we add a new vote, we should return it
        my_leader_bank.freeze();
        let vote_slot = my_leader_bank.slot();
        let vote_hash = my_leader_bank.hash();
        let my_leader_bank = Arc::new(Bank::new_from_parent(
            &my_leader_bank,
            &Pubkey::default(),
            my_leader_bank.slot() + 1,
        ));
        let vote_account_key = vote_simulator.vote_pubkeys[1];
        let vote = Box::new(Vote::new(vec![vote_slot], vote_hash));
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote,
            packet_batch: PacketBatch::default(),
            signature: Signature::new_unique(),
        }])
        .unwrap();
        // Ingest the votes into the buffer
>>>>>>> 254ef3e7b (Rename Packets to PacketBatch (#21794))
        verified_vote_packets
            .0
            .insert(label2, (1, 23, Packets::default()));

        // Both updates have timestamps greater than 0, so both should be returned
        let (new_update_version, updates) = verified_vote_packets.get_latest_votes(0);
        assert_eq!(new_update_version, 2);
        assert_eq!(updates.packets.len(), 2);

        // Only the nonempty packet had a timestamp greater than 1
        let (new_update_version, updates) = verified_vote_packets.get_latest_votes(1);
        assert_eq!(new_update_version, 2);
        assert!(!updates.packets.is_empty());

        // If the given timestamp is greater than all timestamps in any update,
        // returned timestamp should be the same as the given timestamp, and
        // no updates should be returned
        let (new_update_version, updates) = verified_vote_packets.get_latest_votes(3);
        assert_eq!(new_update_version, 3);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_get_and_process_vote_packets() {
        let (s, r) = unbounded();
        let pubkey = solana_sdk::pubkey::new_rand();
        let label1 = CrdsValueLabel::Vote(0, pubkey);
        let label2 = CrdsValueLabel::Vote(1, pubkey);
        let mut update_version = 0;
        s.send(vec![(label1.clone(), 17, Packets::default())])
            .unwrap();
        s.send(vec![(label2.clone(), 23, Packets::default())])
            .unwrap();

        let data = Packet {
            meta: Meta {
                repair: true,
                ..Meta::default()
            },
            ..Packet::default()
        };

        let later_packets = Packets::new(vec![data, Packet::default()]);
        s.send(vec![(label1.clone(), 42, later_packets)]).unwrap();
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());
        verified_vote_packets
            .receive_and_process_vote_packets(&r, &mut update_version, true)
            .unwrap();

        // Test timestamps for same batch are the same
        let update_version1 = verified_vote_packets.get_vote_packets(&label1).unwrap().0;
        assert_eq!(
            update_version1,
            verified_vote_packets.get_vote_packets(&label2).unwrap().0
        );

        // Test the later value overwrote the earlier one for this label
        assert!(
            verified_vote_packets
                .get_vote_packets(&label1)
                .unwrap()
                .2
                .packets
                .len()
                > 1
        );
        assert_eq!(
            verified_vote_packets
                .get_vote_packets(&label2)
                .unwrap()
                .2
                .packets
                .len(),
            0
        );

        // Test timestamp for next batch overwrites the original
        s.send(vec![(label2.clone(), 51, Packets::default())])
            .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, &mut update_version, true)
            .unwrap();
        let update_version2 = verified_vote_packets.get_vote_packets(&label2).unwrap().0;
        assert!(update_version2 > update_version1);

        // Test empty doesn't bump the version
        let before = update_version;
        assert_matches!(
            verified_vote_packets.receive_and_process_vote_packets(&r, &mut update_version, true),
            Err(Error::CrossbeamRecvTimeout(RecvTimeoutError::Timeout))
        );
        assert_eq!(before, update_version);
    }
}
