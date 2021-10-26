use crate::{cluster_info_vote_listener::VerifiedLabelVotePacketsReceiver, result::Result};
use crossbeam_channel::Select;
use solana_gossip::crds_value::CrdsValueLabel;
use solana_perf::packet::Packets;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::from_account, clock::Slot, hash::Hash, pubkey::Pubkey, signature::Signature,
    slot_hashes::SlotHashes, sysvar,
};
use solana_vote_program::vote_state::Vote;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

const MAX_VOTES_PER_VALIDATOR: usize = 1000;

pub struct VerifiedVoteMetadata {
    pub label: CrdsValueLabel,
    pub vote: Vote,
    pub packet: Packets,
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
        let slot_hashes_account = my_leader_bank
            .get_account(&sysvar::slot_hashes::id())
            .expect("Slot hashes sysvar must exist");
        let slot_hashes = from_account::<SlotHashes, _>(&slot_hashes_account).unwrap();
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
    type Item = Vec<Packets>;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: Maybe prioritize by stake weight
        self.vote_account_keys.pop().and_then(|vote_account_key| {
            // Get all the gossip votes we've queued up for this validator
            self.verified_vote_packets
                .0
                .get(&vote_account_key)
                .and_then(|validator_gossip_votes| {
                    // Fetch the validator's vote state from the bank
                    self.my_leader_bank
                        .vote_accounts()
                        .get(&vote_account_key)
                        .and_then(|(_stake, vote_account)| {
                            vote_account.vote_state().as_ref().ok().map(|vote_state| {
                                let latest_vote = vote_state.last_voted_slot().unwrap_or(0);
                                // Filter out the votes that are outdated
                                validator_gossip_votes
                                    .range((latest_vote + 1, Hash::default())..)
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
                                    .collect::<Vec<Packets>>()
                            })
                        })
                })
        })
    }
}

pub type SingleValidatorVotes = BTreeMap<(Slot, Hash), (Packets, Signature)>;

#[derive(Default)]
pub struct VerifiedVotePackets(HashMap<Pubkey, SingleValidatorVotes>);

impl VerifiedVotePackets {
    pub fn receive_and_process_vote_packets(
        &mut self,
        vote_packets_receiver: &VerifiedLabelVotePacketsReceiver,
        would_be_leader: bool,
    ) -> Result<()> {
        let mut sel = Select::new();
        sel.recv(vote_packets_receiver);
        let _ = sel.ready_timeout(Duration::from_millis(200))?;
        for gossip_votes in vote_packets_receiver.try_iter() {
            if would_be_leader {
                for verfied_vote_metadata in gossip_votes {
                    let VerifiedVoteMetadata {
                        label,
                        vote,
                        packet,
                        signature,
                    } = verfied_vote_metadata;
                    let validator_key = label.pubkey();
                    if vote.slots.is_empty() {
                        error!("Empty votes should have been filtered out earlier in the pipeline");
                        continue;
                    }
                    let slot = vote.slots.last().unwrap();
                    let hash = vote.hash;

                    let validator_votes = self.0.entry(validator_key).or_default();

                    validator_votes.insert((*slot, hash), (packet, signature));

                    if validator_votes.len() > MAX_VOTES_PER_VALIDATOR {
                        let smallest_key = validator_votes.keys().next().cloned().unwrap();
                        validator_votes.remove(&smallest_key).unwrap();
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Error;
    use crossbeam_channel::unbounded;
    use solana_perf::packet::Packet;
    use solana_sdk::slot_hashes::MAX_ENTRIES;

    #[test]
    fn test_verified_vote_packets_receive_and_process_vote_packets() {
        let (s, r) = unbounded();
        let pubkey = solana_sdk::pubkey::new_rand();

        // Construct the buffer
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        // Send a vote from `pubkey`, check that it was inserted
        let vote_slot = 0;
        let vote_hash = Hash::new_unique();
        let vote_index = 0;
        let label = CrdsValueLabel::Vote(vote_index, pubkey);
        let vote = Vote::new(vec![vote_slot], vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            label: label.clone(),
            vote: vote.clone(),
            packet: Packets::default(),
            signature: Signature::new(&[1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(verified_vote_packets.0.get(&pubkey).unwrap().len(), 1);

        // Same slot, same hash, should not be inserted
        s.send(vec![VerifiedVoteMetadata {
            label: label.clone(),
            vote,
            packet: Packets::default(),
            signature: Signature::new(&[1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(verified_vote_packets.0.get(&pubkey).unwrap().len(), 1);

        // Same slot, different hash, should still be inserted
        let new_vote_hash = Hash::new_unique();
        let vote = Vote::new(vec![vote_slot], new_vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            label,
            vote,
            packet: Packets::default(),
            signature: Signature::new(&[1u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(verified_vote_packets.0.get(&pubkey).unwrap().len(), 2);

        // Different vote slot, should be inserted
        let vote_slot = 1;
        let vote_hash = Hash::new_unique();
        let vote_index = 0;
        let label = CrdsValueLabel::Vote(vote_index, pubkey);
        let vote = Vote::new(vec![vote_slot], vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            label,
            vote,
            packet: Packets::default(),
            signature: Signature::new(&[2u8; 64]),
        }])
        .unwrap();
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(verified_vote_packets.0.get(&pubkey).unwrap().len(), 3);

        // No new messages, should time out
        assert_matches!(
            verified_vote_packets.receive_and_process_vote_packets(&r, true),
            Err(Error::ReadyTimeout)
        );
    }

    #[test]
    fn test_verified_vote_packets_receive_and_process_vote_packets_max_len() {
        let (s, r) = unbounded();
        let pubkey = solana_sdk::pubkey::new_rand();

        // Construct the buffer
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

        // Send many more votes than the upper limit per validator
        for _ in 0..2 * MAX_VOTES_PER_VALIDATOR {
            let vote_slot = 0;
            let vote_hash = Hash::new_unique();
            let vote_index = 0;
            let label = CrdsValueLabel::Vote(vote_index, pubkey);
            let vote = Vote::new(vec![vote_slot], vote_hash);
            s.send(vec![VerifiedVoteMetadata {
                label,
                vote,
                packet: Packets::default(),
                signature: Signature::new(&[1u8; 64]),
            }])
            .unwrap();
        }

        // At most `MAX_VOTES_PER_VALIDATOR` should be stored per validator
        verified_vote_packets
            .receive_and_process_vote_packets(&r, true)
            .unwrap();
        assert_eq!(
            verified_vote_packets.0.get(&pubkey).unwrap().len(),
            MAX_VOTES_PER_VALIDATOR
        );
    }

    #[test]
    fn test_verified_vote_packets_validator_gossip_votes_iterator_wrong_fork() {
        let (s, r) = unbounded();
        let pubkey = Pubkey::new_unique();
        let my_leader_bank = Arc::new(Bank::default_for_tests());

        // Create a bunch of votes with random vote hashes, which should all be ignored
        // since they are not on the same fork as `my_leader_bank`, i.e. their hashes do
        // not exist in the SlotHashes sysvar for `my_leader_bank`
        for _ in 0..MAX_VOTES_PER_VALIDATOR {
            let vote_slot = 0;
            let vote_hash = Hash::new_unique();
            let vote_index = 0;
            let label = CrdsValueLabel::Vote(vote_index, pubkey);
            let vote = Vote::new(vec![vote_slot], vote_hash);
            s.send(vec![VerifiedVoteMetadata {
                label,
                vote,
                packet: Packets::default(),
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
        let pubkey = Pubkey::new_unique();
        let mut my_leader_bank = Arc::new(Bank::default_for_tests());

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

        // Create valid votes now
        let num_validators = 2;
        let vote_index = 0;
        for i in 0..num_validators {
            let pubkey = Pubkey::new_unique();
            // Used to uniquely identify the packets for each validator
            let num_packets = i + 1;
            for (vote_slot, vote_hash) in slot_hashes.slot_hashes().iter() {
                let label = CrdsValueLabel::Vote(vote_index, pubkey);
                let vote = Vote::new(vec![*vote_slot], *vote_hash);
                s.send(vec![VerifiedVoteMetadata {
                    label,
                    vote,
                    packet: Packets::new(vec![Packet::default(); num_packets]),
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
        let mut previously_sent_to_bank_votes = HashSet::new();
        let mut gossip_votes_iterator = ValidatorGossipVotesIterator::new(
            my_leader_bank.clone(),
            &verified_vote_packets,
            &mut previously_sent_to_bank_votes,
        );
        let first_validator_batch: Vec<Packets> = gossip_votes_iterator.next().unwrap();
        assert_eq!(first_validator_batch.len(), slot_hashes.slot_hashes().len());
        assert!(first_validator_batch.iter().all(|p| p.packets.len() == 1));

        let second_validator_batch: Vec<Packets> = gossip_votes_iterator.next().unwrap();
        assert_eq!(
            second_validator_batch.len(),
            slot_hashes.slot_hashes().len()
        );
        assert!(gossip_votes_iterator
            .next()
            .unwrap()
            .iter()
            .all(|p| p.packets.len() == 2));

        assert!(gossip_votes_iterator.next().is_none());

        // If we construct another iterator, should return nothing because `previously_sent_to_bank_votes`
        // should filter out everything
        let mut gossip_votes_iterator = ValidatorGossipVotesIterator::new(
            my_leader_bank.clone(),
            &verified_vote_packets,
            &mut previously_sent_to_bank_votes,
        );
        assert!(gossip_votes_iterator.next().is_none());

        // If we add some new votes, we should return those
        let vote_slot = 1;
        let vote_hash = Hash::new_unique();
        let vote_index = 0;
        let label = CrdsValueLabel::Vote(vote_index, pubkey);
        let vote = Vote::new(vec![vote_slot], vote_hash);
        s.send(vec![VerifiedVoteMetadata {
            label,
            vote,
            packet: Packets::default(),
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
}
