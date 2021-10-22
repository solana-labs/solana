use crate::{cluster_info_vote_listener::VerifiedLabelVotePacketsReceiver, result::Result};
use crossbeam_channel::Select;
use solana_perf::packet::Packets;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::from_account, clock::Slot, hash::Hash, pubkey::Pubkey, slot_hashes::SlotHashes, sysvar,
};
use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

const MAX_VOTES_PER_VALIDATOR: usize = 1000;

pub struct ValidatorGossipVotesIterator<'a> {
    my_leader_bank: Arc<Bank>,
    slot_hashes: SlotHashes,
    verified_vote_packets: &'a VerifiedVotePackets,
    vote_account_keys: Vec<Pubkey>,
}

impl<'a> ValidatorGossipVotesIterator<'a> {
    pub fn new(my_leader_bank: Arc<Bank>, verified_vote_packets: &'a VerifiedVotePackets) -> Self {
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
                                // Filter out the votes that are outdated or on wrong fork
                                validator_gossip_votes
                                    .range((latest_vote + 1, Hash::default())..)
                                    .filter_map(|((slot, hash), packet)| {
                                        // Check the hash is actually on this fork
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

#[derive(Default)]
pub struct VerifiedVotePackets(HashMap<Pubkey, BTreeMap<(Slot, Hash), Packets>>);

impl VerifiedVotePackets {
    pub fn receive_and_process_vote_packets(
        &mut self,
        vote_packets_receiver: &VerifiedLabelVotePacketsReceiver,
        last_update_version: &mut u64,
        would_be_leader: bool,
    ) -> Result<()> {
        let mut sel = Select::new();
        sel.recv(vote_packets_receiver);
        let _ = sel.ready_timeout(Duration::from_millis(200))?;

        for gossip_votes in vote_packets_receiver.try_iter() {
            if would_be_leader {
                for (label, vote, packet) in gossip_votes {
                    let validator_key = label.pubkey();
                    if vote.slots.is_empty() {
                        error!("Empty votes should have been filtered out earlier in the pipeline");
                        continue;
                    }
                    let slot = vote.slots.last().unwrap();
                    let hash = vote.hash;

                    let validator_votes = self.0.entry(validator_key).or_default();

                    validator_votes.insert((*slot, hash), packet);

                    if validator_votes.len() > MAX_VOTES_PER_VALIDATOR {
                        let smallest_key = validator_votes.keys().next().cloned().unwrap();
                        validator_votes.remove(&smallest_key).unwrap();
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn get_vote_packets(&self, key: &CrdsValueLabel) -> Option<&(u64, Slot, Packets)> {
        self.0.get(key)
    }

    /*pub fn get_latest_votes(&self, last_update_version: u64) -> (u64, Packets) {
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
    }*/
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Error;
    use crossbeam_channel::{unbounded, RecvTimeoutError};
    use solana_perf::packet::{Meta, Packet};

    #[test]
    fn test_get_latest_votes() {
        let pubkey = solana_sdk::pubkey::new_rand();
        let label1 = CrdsValueLabel::Vote(0, pubkey);
        let label2 = CrdsValueLabel::Vote(1, pubkey);
        let mut verified_vote_packets = VerifiedVotePackets(HashMap::new());

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
