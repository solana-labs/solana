use {
    crate::{cluster_info_vote_listener::VerifiedLabelVotePacketsReceiver, result::Result},
    itertools::Itertools,
    solana_perf::packet::PacketBatch,
    solana_runtime::bank::Bank,
    solana_sdk::{
        account::from_account,
        clock::{Slot, UnixTimestamp},
        hash::Hash,
        pubkey::Pubkey,
        signature::Signature,
        slot_hashes::SlotHashes,
        sysvar,
    },
    solana_vote::vote_transaction::{VoteTransaction, VoteTransaction::VoteStateUpdate},
    std::{
        collections::{BTreeMap, HashMap, HashSet},
        sync::Arc,
        time::Duration,
    },
};

const MAX_VOTES_PER_VALIDATOR: usize = 1000;

pub struct VerifiedVoteMetadata {
    pub vote_account_key: Pubkey,
    pub vote: VoteTransaction,
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
        // Sort by stake weight so heavier validators' votes are sent first
        let vote_account_keys: Vec<Pubkey> = my_leader_bank
            .vote_accounts()
            .iter()
            .map(|(pubkey, &(stake, _))| (pubkey, stake))
            .sorted_unstable_by_key(|&(_, stake)| std::cmp::Reverse(stake))
            .map(|(&pubkey, _)| pubkey)
            .collect();
        Self {
            my_leader_bank,
            slot_hashes,
            verified_vote_packets,
            vote_account_keys,
            previously_sent_to_bank_votes,
        }
    }

    fn filter_vote(
        &mut self,
        slot: &Slot,
        hash: &Hash,
        packet: &PacketBatch,
        tx_signature: &Signature,
    ) -> Option<PacketBatch> {
        // Don't send the same vote to the same bank multiple times
        if self.previously_sent_to_bank_votes.contains(tx_signature) {
            return None;
        }
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
    }
}

/// Each iteration returns all of the missing votes for a single validator, the votes
/// ordered from smallest to largest.
///
/// Iterator is done after iterating through all vote accounts
impl<'a> Iterator for ValidatorGossipVotesIterator<'a> {
    type Item = Vec<PacketBatch>;

    fn next(&mut self) -> Option<Self::Item> {
        use SingleValidatorVotes::*;
        while let Some(vote_account_key) = self.vote_account_keys.pop() {
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
                                match validator_gossip_votes {
                                    FullTowerVote(GossipVote {
                                        slot,
                                        hash,
                                        packet_batch,
                                        signature,
                                        ..
                                    }) => self
                                        .filter_vote(slot, hash, packet_batch, signature)
                                        .map(|packet| vec![packet])
                                        .unwrap_or_default(),
                                    IncrementalVotes(validator_gossip_votes) => {
                                        validator_gossip_votes
                                            .range((start_vote_slot, Hash::default())..)
                                            .filter_map(|((slot, hash), (packet, tx_signature))| {
                                                self.filter_vote(slot, hash, packet, tx_signature)
                                            })
                                            .collect::<Vec<PacketBatch>>()
                                    }
                                }
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

#[derive(Debug, Default, Clone)]
pub struct GossipVote {
    slot: Slot,
    hash: Hash,
    packet_batch: PacketBatch,
    signature: Signature,
    timestamp: Option<UnixTimestamp>,
}

pub enum SingleValidatorVotes {
    FullTowerVote(GossipVote),
    IncrementalVotes(BTreeMap<(Slot, Hash), (PacketBatch, Signature)>),
}

impl SingleValidatorVotes {
    fn get_latest_gossip_slot(&self) -> Slot {
        match self {
            Self::FullTowerVote(vote) => vote.slot,
            _ => 0,
        }
    }

    fn get_latest_timestamp(&self) -> Option<UnixTimestamp> {
        match self {
            Self::FullTowerVote(vote) => vote.timestamp,
            _ => None,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        match self {
            Self::IncrementalVotes(votes) => votes.len(),
            _ => 1,
        }
    }
}

#[derive(Default)]
pub struct VerifiedVotePackets(HashMap<Pubkey, SingleValidatorVotes>);

impl VerifiedVotePackets {
    pub fn receive_and_process_vote_packets(
        &mut self,
        vote_packets_receiver: &VerifiedLabelVotePacketsReceiver,
        would_be_leader: bool,
    ) -> Result<()> {
        use SingleValidatorVotes::*;
        const RECV_TIMEOUT: Duration = Duration::from_millis(200);
        let vote_packets = vote_packets_receiver.recv_timeout(RECV_TIMEOUT)?;
        let vote_packets = std::iter::once(vote_packets).chain(vote_packets_receiver.try_iter());

        for gossip_votes in vote_packets {
            if would_be_leader {
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
                    let timestamp = vote.timestamp();

                    match vote {
                        VoteStateUpdate(_) => {
                            let (latest_gossip_slot, latest_timestamp) =
                                self.0.get(&vote_account_key).map_or((0, None), |vote| {
                                    (vote.get_latest_gossip_slot(), vote.get_latest_timestamp())
                                });
                            // Since votes are not incremental, we keep only the latest vote
                            // If the vote is for the same slot we will only allow it if
                            // it has a later timestamp (refreshed vote)
                            //
                            // Timestamp can be None if something was wrong with the senders clock.
                            // We directly compare as Options to ensure that votes with proper
                            // timestamps have precedence (Some is > None).
                            if slot > latest_gossip_slot
                                || ((slot == latest_gossip_slot) && (timestamp > latest_timestamp))
                            {
                                self.0.insert(
                                    vote_account_key,
                                    FullTowerVote(GossipVote {
                                        slot,
                                        hash,
                                        packet_batch,
                                        signature,
                                        timestamp,
                                    }),
                                );
                            }
                        }
                        _ => {
                            if let Some(FullTowerVote(gossip_vote)) =
                                self.0.get_mut(&vote_account_key)
                            {
                                if slot > gossip_vote.slot {
                                    warn!(
                                        "Originally {} submitted full tower votes, but now has reverted to incremental votes. Converting back to old format.",
                                        vote_account_key
                                    );
                                    let mut votes = BTreeMap::new();
                                    let GossipVote {
                                        slot,
                                        hash,
                                        packet_batch,
                                        signature,
                                        ..
                                    } = std::mem::take(gossip_vote);
                                    votes.insert((slot, hash), (packet_batch, signature));
                                    self.0.insert(vote_account_key, IncrementalVotes(votes));
                                } else {
                                    continue;
                                }
                            };
                            let validator_votes: &mut BTreeMap<
                                (Slot, Hash),
                                (PacketBatch, Signature),
                            > = match self
                                .0
                                .entry(vote_account_key)
                                .or_insert(IncrementalVotes(BTreeMap::new()))
                            {
                                IncrementalVotes(votes) => votes,
                                FullTowerVote(_) => continue, // Should never happen
                            };
                            validator_votes.insert((slot, hash), (packet_batch, signature));
                            if validator_votes.len() > MAX_VOTES_PER_VALIDATOR {
                                let smallest_key = validator_votes.keys().next().cloned().unwrap();
                                validator_votes.remove(&smallest_key).unwrap();
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{SingleValidatorVotes::*, *},
        crate::{result::Error, vote_simulator::VoteSimulator},
        crossbeam_channel::{unbounded, Receiver, Sender},
        solana_perf::packet::Packet,
        solana_sdk::slot_hashes::MAX_ENTRIES,
        solana_vote_program::vote_state::{Lockout, Vote, VoteStateUpdate},
        std::collections::VecDeque,
    };

    #[test]
    fn test_verified_vote_packets_receive_and_process_vote_packets() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();

        // Construct the buffer
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        // Send a vote from `vote_account_key`, check that it was inserted
        let vote_slot = 0;
        let vote_hash = Hash::new_unique();
        let vote = Vote::new(vec![vote_slot], vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: VoteTransaction::from(vote.clone()),
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
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
            vote: VoteTransaction::from(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
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
            vote: VoteTransaction::from(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
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
            vote: VoteTransaction::from(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::from([2u8; 64]),
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
            Err(Error::RecvTimeout(_))
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
                vote: VoteTransaction::from(vote),
                packet_batch: PacketBatch::default(),
                signature: Signature::from([1u8; 64]),
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
                vote: VoteTransaction::from(vote),
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
        let vote_simulator = VoteSimulator::new(num_validators);
        let mut my_leader_bank = vote_simulator.bank_forks.read().unwrap().root_bank();

        // Create a set of valid ancestor hashes for this fork
        for _ in 0..MAX_ENTRIES {
            let slot = my_leader_bank.slot() + 1;
            my_leader_bank = Arc::new(Bank::new_from_parent(
                my_leader_bank,
                &Pubkey::default(),
                slot,
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
                    vote: VoteTransaction::from(vote),
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

        // One batch of vote packets per validator
        assert_eq!(verified_vote_packets.0.len(), num_validators);
        // Each validator should have one vote per slot
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
        for _ in 0..num_validators {
            let validator_batch: Vec<PacketBatch> = gossip_votes_iterator.next().unwrap();
            assert_eq!(validator_batch.len(), slot_hashes.slot_hashes().len());
            let expected_len = validator_batch[0].len();
            assert!(validator_batch
                .iter()
                .all(|batch| batch.len() == expected_len));
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
        let new_leader_slot = my_leader_bank.slot() + 1;
        let my_leader_bank = Arc::new(Bank::new_from_parent(
            my_leader_bank,
            &Pubkey::default(),
            new_leader_slot,
        ));
        let vote_account_key = vote_simulator.vote_pubkeys[1];
        let vote = VoteTransaction::from(Vote::new(vec![vote_slot], vote_hash));
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote,
            packet_batch: PacketBatch::default(),
            signature: Signature::new_unique(),
        }])
        .unwrap();
        // Ingest the votes into the buffer
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        let mut gossip_votes_iterator = ValidatorGossipVotesIterator::new(
            my_leader_bank,
            &verified_vote_packets,
            &mut previously_sent_to_bank_votes,
        );
        assert!(gossip_votes_iterator.next().is_some());
        assert!(gossip_votes_iterator.next().is_none());
    }

    #[test]
    fn test_only_latest_vote_is_sent_with_feature() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();

        // Send three vote state updates that are out of order
        let first_vote = VoteStateUpdate::from(vec![(2, 4), (4, 3), (6, 2), (7, 1)]);
        let second_vote = VoteStateUpdate::from(vec![(2, 4), (4, 3), (11, 1)]);
        let third_vote = VoteStateUpdate::from(vec![(2, 5), (4, 4), (11, 3), (12, 2), (13, 1)]);

        for vote in [second_vote, first_vote] {
            s.send(vec![VerifiedVoteMetadata {
                vote_account_key,
                vote: VoteTransaction::from(vote),
                packet_batch: PacketBatch::default(),
                signature: Signature::from([1u8; 64]),
            }])
            .unwrap();
        }

        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();

        // second_vote should be kept and first_vote ignored
        let slot = verified_vote_packets
            .0
            .get(&vote_account_key)
            .unwrap()
            .get_latest_gossip_slot();
        assert_eq!(11, slot);

        // Now send the third_vote, it should overwrite second_vote
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: VoteTransaction::from(third_vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
        }])
        .unwrap();

        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        let slot = verified_vote_packets
            .0
            .get(&vote_account_key)
            .unwrap()
            .get_latest_gossip_slot();
        assert_eq!(13, slot);
    }

    fn send_vote_state_update_and_process(
        s: &Sender<Vec<VerifiedVoteMetadata>>,
        r: &Receiver<Vec<VerifiedVoteMetadata>>,
        vote: VoteStateUpdate,
        vote_account_key: Pubkey,
        verified_vote_packets: &mut VerifiedVotePackets,
    ) -> GossipVote {
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote: VoteTransaction::from(vote),
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(r, true)
            .unwrap();
        match verified_vote_packets.0.get(&vote_account_key).unwrap() {
            SingleValidatorVotes::FullTowerVote(gossip_vote) => gossip_vote.clone(),
            _ => panic!("Received incremental vote"),
        }
    }

    #[test]
    fn test_latest_vote_tie_break_with_feature() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();

        // Send identical vote state updates with different timestamps
        let mut vote = VoteStateUpdate::from(vec![(2, 4), (4, 3), (6, 2), (7, 1)]);
        vote.timestamp = Some(5);

        let mut vote_later_ts = vote.clone();
        vote_later_ts.timestamp = Some(6);

        let mut vote_earlier_ts = vote.clone();
        vote_earlier_ts.timestamp = Some(4);

        let mut vote_no_ts = vote.clone();
        vote_no_ts.timestamp = None;

        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        // Original vote
        let GossipVote {
            slot, timestamp, ..
        } = send_vote_state_update_and_process(
            &s,
            &r,
            vote.clone(),
            vote_account_key,
            &mut verified_vote_packets,
        );
        assert_eq!(slot, vote.last_voted_slot().unwrap());
        assert_eq!(timestamp, vote.timestamp);

        // Same vote with later timestamp should override
        let GossipVote {
            slot, timestamp, ..
        } = send_vote_state_update_and_process(
            &s,
            &r,
            vote_later_ts.clone(),
            vote_account_key,
            &mut verified_vote_packets,
        );
        assert_eq!(slot, vote_later_ts.last_voted_slot().unwrap());
        assert_eq!(timestamp, vote_later_ts.timestamp);

        // Same vote with earlier timestamp should not override
        let GossipVote {
            slot, timestamp, ..
        } = send_vote_state_update_and_process(
            &s,
            &r,
            vote_earlier_ts,
            vote_account_key,
            &mut verified_vote_packets,
        );
        assert_eq!(slot, vote_later_ts.last_voted_slot().unwrap());
        assert_eq!(timestamp, vote_later_ts.timestamp);

        // Same vote with no timestamp should not override
        let GossipVote {
            slot, timestamp, ..
        } = send_vote_state_update_and_process(
            &s,
            &r,
            vote_no_ts,
            vote_account_key,
            &mut verified_vote_packets,
        );
        assert_eq!(slot, vote_later_ts.last_voted_slot().unwrap());
        assert_eq!(timestamp, vote_later_ts.timestamp);
    }

    #[test]
    fn test_latest_vote_feature_upgrade() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();

        // Send incremental votes
        for i in 0..100 {
            let vote = VoteTransaction::from(Vote::new(vec![i], Hash::new_unique()));
            s.send(vec![VerifiedVoteMetadata {
                vote_account_key,
                vote,
                packet_batch: PacketBatch::default(),
                signature: Signature::from([1u8; 64]),
            }])
            .unwrap();
        }

        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());
        // Receive votes without the feature active
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            100,
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .len()
        );

        // Now send some new votes
        for i in 101..201 {
            let slots = std::iter::zip((i - 30)..(i + 1), (1..32).rev())
                .map(|(slot, confirmation_count)| {
                    Lockout::new_with_confirmation_count(slot, confirmation_count)
                })
                .collect::<VecDeque<Lockout>>();
            let vote = VoteTransaction::from(VoteStateUpdate::new(
                slots,
                Some(i - 32),
                Hash::new_unique(),
            ));
            s.send(vec![VerifiedVoteMetadata {
                vote_account_key,
                vote,
                packet_batch: PacketBatch::default(),
                signature: Signature::from([1u8; 64]),
            }])
            .unwrap();
        }

        // Receive votes with the feature active
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        if let FullTowerVote(vote) = verified_vote_packets.0.get(&vote_account_key).unwrap() {
            assert_eq!(200, vote.slot);
        } else {
            panic!("Feature active but incremental votes are present");
        }
    }

    #[test]
    fn test_incremental_votes_with_feature_active() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        let hash = Hash::new_unique();
        let vote = VoteTransaction::from(Vote::new(vec![42], hash));
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote,
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
        }])
        .unwrap();

        // Receive incremental votes with the feature active
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();

        // Should still store as incremental votes
        if let IncrementalVotes(votes) = verified_vote_packets.0.get(&vote_account_key).unwrap() {
            assert!(votes.contains_key(&(42, hash)));
        } else {
            panic!("Although feature is active, incremental votes should not be stored as full tower votes");
        }
    }

    #[test]
    fn test_latest_votes_downgrade_full_to_incremental() {
        let (s, r) = unbounded();
        let vote_account_key = solana_sdk::pubkey::new_rand();
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        let vote = VoteTransaction::from(VoteStateUpdate::from(vec![(42, 1)]));
        let hash_42 = vote.hash();
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote,
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
        }])
        .unwrap();

        // Receive full votes
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            42,
            verified_vote_packets
                .0
                .get(&vote_account_key)
                .unwrap()
                .get_latest_gossip_slot()
        );

        // Try to send an old incremental vote from pre feature activation
        let vote = VoteTransaction::from(Vote::new(vec![34], Hash::new_unique()));
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote,
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
        }])
        .unwrap();

        // Try to receive nothing should happen
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        if let FullTowerVote(vote) = verified_vote_packets.0.get(&vote_account_key).unwrap() {
            assert_eq!(42, vote.slot);
        } else {
            panic!("Old vote triggered a downgrade conversion");
        }

        // Now try to send an incremental vote
        let vote = VoteTransaction::from(Vote::new(vec![43], Hash::new_unique()));
        let hash_43 = vote.hash();
        s.send(vec![VerifiedVoteMetadata {
            vote_account_key,
            vote,
            packet_batch: PacketBatch::default(),
            signature: Signature::from([1u8; 64]),
        }])
        .unwrap();

        // Try to receive and vote lands as well as the conversion back to incremental votes
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        if let IncrementalVotes(votes) = verified_vote_packets.0.get(&vote_account_key).unwrap() {
            assert!(votes.contains_key(&(42, hash_42)));
            assert!(votes.contains_key(&(43, hash_43)));
            assert_eq!(2, votes.len());
        } else {
            panic!("Conversion back to incremental votes failed");
        }
    }
}
