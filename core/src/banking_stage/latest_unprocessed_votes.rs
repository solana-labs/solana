use {
    super::{
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket},
    },
    itertools::Itertools,
    rand::{thread_rng, Rng},
    solana_perf::packet::Packet,
    solana_runtime::bank::Bank,
    solana_sdk::{
        account::from_account,
        clock::{Slot, UnixTimestamp},
        hash::Hash,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        slot_hashes::SlotHashes,
        sysvar,
    },
    solana_vote_program::vote_instruction::VoteInstruction,
    std::{
        cmp,
        collections::HashMap,
        ops::DerefMut,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, RwLock,
        },
    },
};

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum VoteSource {
    Gossip,
    Tpu,
}

/// Holds deserialized vote messages as well as their source, forward status and slot
#[derive(Debug, Clone)]
pub struct LatestValidatorVotePacket {
    vote_source: VoteSource,
    pubkey: Pubkey,
    vote: Option<Arc<ImmutableDeserializedPacket>>,
    slot: Slot,
    hash: Hash,
    forwarded: bool,
    timestamp: Option<UnixTimestamp>,
}

impl LatestValidatorVotePacket {
    pub fn new(packet: Packet, vote_source: VoteSource) -> Result<Self, DeserializedPacketError> {
        if !packet.meta().is_simple_vote_tx() {
            return Err(DeserializedPacketError::VoteTransactionError);
        }

        let vote = Arc::new(ImmutableDeserializedPacket::new(packet)?);
        Self::new_from_immutable(vote, vote_source)
    }

    pub fn new_from_immutable(
        vote: Arc<ImmutableDeserializedPacket>,
        vote_source: VoteSource,
    ) -> Result<Self, DeserializedPacketError> {
        let message = vote.transaction().get_message();
        let (_, instruction) = message
            .program_instructions_iter()
            .next()
            .ok_or(DeserializedPacketError::VoteTransactionError)?;

        match limited_deserialize::<VoteInstruction>(&instruction.data) {
            Ok(vote_state_update_instruction)
                if vote_state_update_instruction.is_single_vote_state_update() =>
            {
                let &pubkey = message
                    .message
                    .static_account_keys()
                    .first()
                    .ok_or(DeserializedPacketError::VoteTransactionError)?;
                let slot = vote_state_update_instruction.last_voted_slot().unwrap_or(0);
                let hash = vote_state_update_instruction.hash();
                let timestamp = vote_state_update_instruction.timestamp();

                Ok(Self {
                    vote: Some(vote),
                    slot,
                    hash,
                    pubkey,
                    vote_source,
                    forwarded: false,
                    timestamp,
                })
            }
            _ => Err(DeserializedPacketError::VoteTransactionError),
        }
    }

    pub fn get_vote_packet(&self) -> Arc<ImmutableDeserializedPacket> {
        self.vote.as_ref().unwrap().clone()
    }

    pub fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub(crate) fn hash(&self) -> Hash {
        self.hash
    }

    pub fn timestamp(&self) -> Option<UnixTimestamp> {
        self.timestamp
    }

    pub fn is_forwarded(&self) -> bool {
        // By definition all gossip votes have been forwarded
        self.forwarded || matches!(self.vote_source, VoteSource::Gossip)
    }

    pub fn is_vote_taken(&self) -> bool {
        self.vote.is_none()
    }

    pub fn take_vote(&mut self) -> Option<Arc<ImmutableDeserializedPacket>> {
        self.vote.take()
    }
}

pub(crate) fn weighted_random_order_by_stake<'a>(
    bank: &Bank,
    pubkeys: impl Iterator<Item = &'a Pubkey>,
) -> impl Iterator<Item = Pubkey> + 'static {
    // Efraimidis and Spirakis algo for weighted random sample without replacement
    let staked_nodes = bank.staked_nodes();
    let mut pubkey_with_weight: Vec<(f64, Pubkey)> = pubkeys
        .filter_map(|&pubkey| {
            let stake = staked_nodes.get(&pubkey).copied().unwrap_or(0);
            if stake == 0 {
                None // Ignore votes from unstaked validators
            } else {
                Some((thread_rng().gen::<f64>().powf(1.0 / (stake as f64)), pubkey))
            }
        })
        .collect::<Vec<_>>();
    pubkey_with_weight.sort_by(|(w1, _), (w2, _)| w1.partial_cmp(w2).unwrap());
    pubkey_with_weight.into_iter().map(|(_, pubkey)| pubkey)
}

#[derive(Default, Debug)]
pub(crate) struct VoteBatchInsertionMetrics {
    pub(crate) num_dropped_gossip: usize,
    pub(crate) num_dropped_tpu: usize,
}

#[derive(Debug, Default)]
pub struct LatestUnprocessedVotes {
    latest_votes_per_pubkey: RwLock<HashMap<Pubkey, Arc<RwLock<LatestValidatorVotePacket>>>>,
    num_unprocessed_votes: AtomicUsize,
}

impl LatestUnprocessedVotes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.num_unprocessed_votes.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn insert_batch(
        &self,
        votes: impl Iterator<Item = LatestValidatorVotePacket>,
        should_replenish_taken_votes: bool,
    ) -> VoteBatchInsertionMetrics {
        let mut num_dropped_gossip = 0;
        let mut num_dropped_tpu = 0;

        for vote in votes {
            if let Some(vote) = self.update_latest_vote(vote, should_replenish_taken_votes) {
                match vote.vote_source {
                    VoteSource::Gossip => num_dropped_gossip += 1,
                    VoteSource::Tpu => num_dropped_tpu += 1,
                }
            }
        }

        VoteBatchInsertionMetrics {
            num_dropped_gossip,
            num_dropped_tpu,
        }
    }

    fn get_entry(&self, pubkey: Pubkey) -> Option<Arc<RwLock<LatestValidatorVotePacket>>> {
        self.latest_votes_per_pubkey
            .read()
            .unwrap()
            .get(&pubkey)
            .cloned()
    }

    /// If this vote causes an unprocessed vote to be removed, returns Some(old_vote)
    /// If there is a newer vote processed / waiting to be processed returns Some(vote)
    /// Otherwise returns None
    pub fn update_latest_vote(
        &self,
        vote: LatestValidatorVotePacket,
        should_replenish_taken_votes: bool,
    ) -> Option<LatestValidatorVotePacket> {
        let pubkey = vote.pubkey();
        let slot = vote.slot();
        let timestamp = vote.timestamp();

        // Allow votes for later slots or the same slot with later timestamp (refreshed votes)
        // We directly compare as options to prioritize votes for same slot with timestamp as
        // Some > None
        let allow_update = |latest_vote: &LatestValidatorVotePacket| -> bool {
            match slot.cmp(&latest_vote.slot()) {
                cmp::Ordering::Less => return false,
                cmp::Ordering::Greater => return true,
                cmp::Ordering::Equal => {}
            };

            // Slots are equal, now check timestamp
            match timestamp.cmp(&latest_vote.timestamp()) {
                cmp::Ordering::Less => return false,
                cmp::Ordering::Greater => return true,
                cmp::Ordering::Equal => {}
            };

            // Timestamps are equal, lastly check if vote was taken previously
            // and should be replenished
            should_replenish_taken_votes && latest_vote.is_vote_taken()
        };

        let with_latest_vote = |latest_vote: &RwLock<LatestValidatorVotePacket>,
                                vote: LatestValidatorVotePacket|
         -> Option<LatestValidatorVotePacket> {
            let should_try_update = allow_update(&latest_vote.read().unwrap());
            if should_try_update {
                let mut latest_vote = latest_vote.write().unwrap();
                if allow_update(&latest_vote) {
                    let old_vote = std::mem::replace(latest_vote.deref_mut(), vote);
                    if old_vote.is_vote_taken() {
                        self.num_unprocessed_votes.fetch_add(1, Ordering::Relaxed);
                        return None;
                    } else {
                        return Some(old_vote);
                    }
                }
            }
            Some(vote)
        };

        if let Some(latest_vote) = self.get_entry(pubkey) {
            with_latest_vote(&latest_vote, vote)
        } else {
            // Grab write-lock to insert new vote.
            match self.latest_votes_per_pubkey.write().unwrap().entry(pubkey) {
                std::collections::hash_map::Entry::Occupied(entry) => {
                    with_latest_vote(entry.get(), vote)
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Arc::new(RwLock::new(vote)));
                    self.num_unprocessed_votes.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        }
    }

    #[cfg(test)]
    pub fn get_latest_vote_slot(&self, pubkey: Pubkey) -> Option<Slot> {
        self.latest_votes_per_pubkey
            .read()
            .unwrap()
            .get(&pubkey)
            .map(|l| l.read().unwrap().slot())
    }

    #[cfg(test)]
    fn get_latest_timestamp(&self, pubkey: Pubkey) -> Option<UnixTimestamp> {
        self.latest_votes_per_pubkey
            .read()
            .unwrap()
            .get(&pubkey)
            .and_then(|l| l.read().unwrap().timestamp())
    }

    /// Returns how many packets were forwardable
    /// Performs a weighted random order based on stake and stops forwarding at the first error
    /// Votes from validators with 0 stakes are ignored
    pub fn get_and_insert_forwardable_packets(
        &self,
        bank: Arc<Bank>,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> usize {
        let pubkeys_by_stake = {
            let binding = self.latest_votes_per_pubkey.read().unwrap();
            weighted_random_order_by_stake(&bank, binding.keys())
        };

        let mut forwarded_count: usize = 0;
        for pubkey in pubkeys_by_stake {
            let Some(vote) = self.get_entry(pubkey) else {
                continue;
            };

            let mut vote = vote.write().unwrap();
            if vote.is_vote_taken() || vote.is_forwarded() {
                continue;
            }

            let deserialized_vote_packet = vote.vote.as_ref().unwrap().clone();
            let Some(sanitized_vote_transaction) = deserialized_vote_packet
                .build_sanitized_transaction(
                    bank.vote_only_bank(),
                    bank.as_ref(),
                    bank.get_reserved_account_keys(),
                )
            else {
                continue;
            };

            let forwarding_successful = forward_packet_batches_by_accounts.try_add_packet(
                &sanitized_vote_transaction,
                deserialized_vote_packet,
                &bank.feature_set,
            );

            if !forwarding_successful {
                // To match behavior of regular transactions we stop forwarding votes as soon as one
                // fails. We are assuming that failure (try_add_packet) means no more space
                // available.
                break;
            }

            vote.forwarded = true;
            forwarded_count += 1;
        }

        forwarded_count
    }

    /// Drains all votes yet to be processed sorted by a weighted random ordering by stake
    /// Do not touch votes that are for a different fork from `bank` as we know they will fail,
    /// however the next bank could be built on a different fork and consume these votes.
    pub fn drain_unprocessed(&self, bank: Arc<Bank>) -> Vec<Arc<ImmutableDeserializedPacket>> {
        let slot_hashes = bank
            .get_account(&sysvar::slot_hashes::id())
            .and_then(|account| from_account::<SlotHashes, _>(&account));
        if slot_hashes.is_none() {
            error!(
                "Slot hashes sysvar doesn't exist on bank {}. Including all votes without filtering",
                bank.slot()
            );
        }

        let pubkeys_by_stake = {
            let binding = self.latest_votes_per_pubkey.read().unwrap();
            weighted_random_order_by_stake(&bank, binding.keys())
        };
        pubkeys_by_stake
            .filter_map(|pubkey| {
                self.get_entry(pubkey).and_then(|lock| {
                    let mut latest_vote = lock.write().unwrap();
                    if !Self::is_valid_for_our_fork(&latest_vote, &slot_hashes) {
                        return None;
                    }
                    latest_vote.take_vote().inspect(|_vote| {
                        self.num_unprocessed_votes.fetch_sub(1, Ordering::Relaxed);
                    })
                })
            })
            .collect_vec()
    }

    /// Check if `vote` can land in our fork based on `slot_hashes`
    fn is_valid_for_our_fork(
        vote: &LatestValidatorVotePacket,
        slot_hashes: &Option<SlotHashes>,
    ) -> bool {
        let Some(slot_hashes) = slot_hashes else {
            // When slot hashes is not present we do not filter
            return true;
        };
        slot_hashes
            .get(&vote.slot())
            .map(|found_hash| *found_hash == vote.hash())
            .unwrap_or(false)
    }

    /// Sometimes we forward and hold the packets, sometimes we forward and clear.
    /// This also clears all gossip votes since by definition they have been forwarded
    pub fn clear_forwarded_packets(&self) {
        self.latest_votes_per_pubkey
            .read()
            .unwrap()
            .values()
            .filter(|lock| lock.read().unwrap().is_forwarded())
            .for_each(|lock| {
                let mut vote = lock.write().unwrap();
                if vote.is_forwarded() && vote.take_vote().is_some() {
                    self.num_unprocessed_votes.fetch_sub(1, Ordering::Relaxed);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        itertools::Itertools,
        rand::{thread_rng, Rng},
        solana_perf::packet::{Packet, PacketBatch, PacketFlags},
        solana_runtime::{
            bank::Bank,
            genesis_utils::{self, ValidatorVoteKeypairs},
        },
        solana_sdk::{hash::Hash, signature::Signer, system_transaction::transfer},
        solana_vote_program::{
            vote_state::TowerSync, vote_transaction::new_tower_sync_transaction,
        },
        std::{sync::Arc, thread::Builder},
    };

    fn from_slots(
        slots: Vec<(u64, u32)>,
        vote_source: VoteSource,
        keypairs: &ValidatorVoteKeypairs,
        timestamp: Option<UnixTimestamp>,
    ) -> LatestValidatorVotePacket {
        let mut vote = TowerSync::from(slots);
        vote.timestamp = timestamp;
        let vote_tx = new_tower_sync_transaction(
            vote,
            Hash::new_unique(),
            &keypairs.node_keypair,
            &keypairs.vote_keypair,
            &keypairs.vote_keypair,
            None,
        );
        let mut packet = Packet::from_data(None, vote_tx).unwrap();
        packet
            .meta_mut()
            .flags
            .set(PacketFlags::SIMPLE_VOTE_TX, true);
        LatestValidatorVotePacket::new(packet, vote_source).unwrap()
    }

    fn deserialize_packets<'a>(
        packet_batch: &'a PacketBatch,
        packet_indexes: &'a [usize],
        vote_source: VoteSource,
    ) -> impl Iterator<Item = LatestValidatorVotePacket> + 'a {
        packet_indexes.iter().filter_map(move |packet_index| {
            LatestValidatorVotePacket::new(packet_batch[*packet_index].clone(), vote_source).ok()
        })
    }

    #[test]
    fn test_deserialize_vote_packets() {
        let keypairs = ValidatorVoteKeypairs::new_rand();
        let blockhash = Hash::new_unique();
        let switch_proof = Hash::new_unique();
        let mut tower_sync = Packet::from_data(
            None,
            new_tower_sync_transaction(
                TowerSync::from(vec![(0, 3), (1, 2), (2, 1)]),
                blockhash,
                &keypairs.node_keypair,
                &keypairs.vote_keypair,
                &keypairs.vote_keypair,
                None,
            ),
        )
        .unwrap();
        tower_sync
            .meta_mut()
            .flags
            .set(PacketFlags::SIMPLE_VOTE_TX, true);
        let mut tower_sync_switch = Packet::from_data(
            None,
            new_tower_sync_transaction(
                TowerSync::from(vec![(0, 3), (1, 2), (3, 1)]),
                blockhash,
                &keypairs.node_keypair,
                &keypairs.vote_keypair,
                &keypairs.vote_keypair,
                Some(switch_proof),
            ),
        )
        .unwrap();
        tower_sync_switch
            .meta_mut()
            .flags
            .set(PacketFlags::SIMPLE_VOTE_TX, true);
        let random_transaction = Packet::from_data(
            None,
            transfer(
                &keypairs.node_keypair,
                &Pubkey::new_unique(),
                1000,
                blockhash,
            ),
        )
        .unwrap();
        let packet_batch =
            PacketBatch::new(vec![tower_sync, tower_sync_switch, random_transaction]);

        let deserialized_packets = deserialize_packets(
            &packet_batch,
            &(0..packet_batch.len()).collect_vec(),
            VoteSource::Gossip,
        )
        .collect_vec();

        assert_eq!(2, deserialized_packets.len());
        assert_eq!(VoteSource::Gossip, deserialized_packets[0].vote_source);
        assert_eq!(VoteSource::Gossip, deserialized_packets[1].vote_source);

        assert_eq!(
            keypairs.node_keypair.pubkey(),
            deserialized_packets[0].pubkey
        );
        assert_eq!(
            keypairs.node_keypair.pubkey(),
            deserialized_packets[1].pubkey
        );

        assert!(deserialized_packets[0].vote.is_some());
        assert!(deserialized_packets[1].vote.is_some());
    }

    #[test]
    fn test_update_latest_vote() {
        let latest_unprocessed_votes = LatestUnprocessedVotes::new();
        let keypair_a = ValidatorVoteKeypairs::new_rand();
        let keypair_b = ValidatorVoteKeypairs::new_rand();

        let vote_a = from_slots(vec![(0, 2), (1, 1)], VoteSource::Gossip, &keypair_a, None);
        let vote_b = from_slots(
            vec![(0, 5), (4, 2), (9, 1)],
            VoteSource::Gossip,
            &keypair_b,
            None,
        );

        assert!(latest_unprocessed_votes
            .update_latest_vote(vote_a, false /* should replenish */)
            .is_none());
        assert!(latest_unprocessed_votes
            .update_latest_vote(vote_b, false /* should replenish */)
            .is_none());
        assert_eq!(2, latest_unprocessed_votes.len());

        assert_eq!(
            Some(1),
            latest_unprocessed_votes.get_latest_vote_slot(keypair_a.node_keypair.pubkey())
        );
        assert_eq!(
            Some(9),
            latest_unprocessed_votes.get_latest_vote_slot(keypair_b.node_keypair.pubkey())
        );

        let vote_a = from_slots(
            vec![(0, 5), (1, 4), (3, 3), (10, 1)],
            VoteSource::Gossip,
            &keypair_a,
            None,
        );
        let vote_b = from_slots(
            vec![(0, 5), (4, 2), (6, 1)],
            VoteSource::Gossip,
            &keypair_b,
            None,
        );

        // Evict previous vote
        assert_eq!(
            1,
            latest_unprocessed_votes
                .update_latest_vote(vote_a, false /* should replenish */)
                .unwrap()
                .slot
        );
        // Drop current vote
        assert_eq!(
            6,
            latest_unprocessed_votes
                .update_latest_vote(vote_b, false /* should replenish */)
                .unwrap()
                .slot
        );

        assert_eq!(2, latest_unprocessed_votes.len());

        // Same votes should be no-ops
        let vote_a = from_slots(
            vec![(0, 5), (1, 4), (3, 3), (10, 1)],
            VoteSource::Gossip,
            &keypair_a,
            None,
        );
        let vote_b = from_slots(
            vec![(0, 5), (4, 2), (9, 1)],
            VoteSource::Gossip,
            &keypair_b,
            None,
        );
        latest_unprocessed_votes.update_latest_vote(vote_a, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_b, false /* should replenish */);

        assert_eq!(2, latest_unprocessed_votes.len());
        assert_eq!(
            10,
            latest_unprocessed_votes
                .get_latest_vote_slot(keypair_a.node_keypair.pubkey())
                .unwrap()
        );
        assert_eq!(
            9,
            latest_unprocessed_votes
                .get_latest_vote_slot(keypair_b.node_keypair.pubkey())
                .unwrap()
        );

        // Same votes with timestamps should override
        let vote_a = from_slots(
            vec![(0, 5), (1, 4), (3, 3), (10, 1)],
            VoteSource::Gossip,
            &keypair_a,
            Some(1),
        );
        let vote_b = from_slots(
            vec![(0, 5), (4, 2), (9, 1)],
            VoteSource::Gossip,
            &keypair_b,
            Some(2),
        );
        latest_unprocessed_votes.update_latest_vote(vote_a, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_b, false /* should replenish */);

        assert_eq!(2, latest_unprocessed_votes.len());
        assert_eq!(
            Some(1),
            latest_unprocessed_votes.get_latest_timestamp(keypair_a.node_keypair.pubkey())
        );
        assert_eq!(
            Some(2),
            latest_unprocessed_votes.get_latest_timestamp(keypair_b.node_keypair.pubkey())
        );

        // Same votes with bigger timestamps should override
        let vote_a = from_slots(
            vec![(0, 5), (1, 4), (3, 3), (10, 1)],
            VoteSource::Gossip,
            &keypair_a,
            Some(5),
        );
        let vote_b = from_slots(
            vec![(0, 5), (4, 2), (9, 1)],
            VoteSource::Gossip,
            &keypair_b,
            Some(6),
        );
        latest_unprocessed_votes.update_latest_vote(vote_a, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_b, false /* should replenish */);

        assert_eq!(2, latest_unprocessed_votes.len());
        assert_eq!(
            Some(5),
            latest_unprocessed_votes.get_latest_timestamp(keypair_a.node_keypair.pubkey())
        );
        assert_eq!(
            Some(6),
            latest_unprocessed_votes.get_latest_timestamp(keypair_b.node_keypair.pubkey())
        );

        // Same votes with smaller timestamps should not override
        let vote_a = from_slots(
            vec![(0, 5), (1, 4), (3, 3), (10, 1)],
            VoteSource::Gossip,
            &keypair_a,
            Some(2),
        );
        let vote_b = from_slots(
            vec![(0, 5), (4, 2), (9, 1)],
            VoteSource::Gossip,
            &keypair_b,
            Some(3),
        );
        latest_unprocessed_votes
            .update_latest_vote(vote_a.clone(), false /* should replenish */);
        latest_unprocessed_votes
            .update_latest_vote(vote_b.clone(), false /* should replenish */);

        assert_eq!(2, latest_unprocessed_votes.len());
        assert_eq!(
            Some(5),
            latest_unprocessed_votes.get_latest_timestamp(keypair_a.node_keypair.pubkey())
        );
        assert_eq!(
            Some(6),
            latest_unprocessed_votes.get_latest_timestamp(keypair_b.node_keypair.pubkey())
        );

        // Drain all latest votes
        for packet in latest_unprocessed_votes
            .latest_votes_per_pubkey
            .read()
            .unwrap()
            .values()
        {
            packet.write().unwrap().take_vote().inspect(|_vote| {
                latest_unprocessed_votes
                    .num_unprocessed_votes
                    .fetch_sub(1, Ordering::Relaxed);
            });
        }
        assert_eq!(0, latest_unprocessed_votes.len());

        // Same votes with same timestamps should not replenish without flag
        latest_unprocessed_votes
            .update_latest_vote(vote_a.clone(), false /* should replenish */);
        latest_unprocessed_votes
            .update_latest_vote(vote_b.clone(), false /* should replenish */);
        assert_eq!(0, latest_unprocessed_votes.len());

        // Same votes with same timestamps should replenish with the flag
        latest_unprocessed_votes.update_latest_vote(vote_a, true /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_b, true /* should replenish */);
        assert_eq!(0, latest_unprocessed_votes.len());
    }

    #[test]
    fn test_update_latest_vote_race() {
        // There was a race condition in updating the same pubkey in the hashmap
        // when the entry does not initially exist.
        let latest_unprocessed_votes = Arc::new(LatestUnprocessedVotes::new());

        const NUM_VOTES: usize = 100;
        let keypairs = Arc::new(
            (0..NUM_VOTES)
                .map(|_| ValidatorVoteKeypairs::new_rand())
                .collect_vec(),
        );

        // Insert votes in parallel
        let insert_vote = |latest_unprocessed_votes: &LatestUnprocessedVotes,
                           keypairs: &Arc<Vec<ValidatorVoteKeypairs>>,
                           i: usize| {
            let vote = from_slots(vec![(i as u64, 1)], VoteSource::Gossip, &keypairs[i], None);
            latest_unprocessed_votes.update_latest_vote(vote, false /* should replenish */);
        };

        let hdl = Builder::new()
            .spawn({
                let latest_unprocessed_votes = latest_unprocessed_votes.clone();
                let keypairs = keypairs.clone();
                move || {
                    for i in 0..NUM_VOTES {
                        insert_vote(&latest_unprocessed_votes, &keypairs, i);
                    }
                }
            })
            .unwrap();

        for i in 0..NUM_VOTES {
            insert_vote(&latest_unprocessed_votes, &keypairs, i);
        }

        hdl.join().unwrap();
        assert_eq!(NUM_VOTES, latest_unprocessed_votes.len());
    }

    #[test]
    fn test_simulate_threads() {
        let latest_unprocessed_votes = Arc::new(LatestUnprocessedVotes::new());
        let latest_unprocessed_votes_tpu = latest_unprocessed_votes.clone();
        let keypairs = Arc::new(
            (0..10)
                .map(|_| ValidatorVoteKeypairs::new_rand())
                .collect_vec(),
        );
        let keypairs_tpu = keypairs.clone();
        let vote_limit = 1000;

        let gossip = Builder::new()
            .spawn(move || {
                let mut rng = thread_rng();
                for i in 0..vote_limit {
                    let vote = from_slots(
                        vec![(i, 1)],
                        VoteSource::Gossip,
                        &keypairs[rng.gen_range(0..10)],
                        None,
                    );
                    latest_unprocessed_votes
                        .update_latest_vote(vote, false /* should replenish */);
                }
            })
            .unwrap();

        let tpu = Builder::new()
            .spawn(move || {
                let mut rng = thread_rng();
                for i in 0..vote_limit {
                    let vote = from_slots(
                        vec![(i, 1)],
                        VoteSource::Tpu,
                        &keypairs_tpu[rng.gen_range(0..10)],
                        None,
                    );
                    latest_unprocessed_votes_tpu
                        .update_latest_vote(vote, false /* should replenish */);
                    if i % 214 == 0 {
                        // Simulate draining and processing packets
                        let latest_votes_per_pubkey = latest_unprocessed_votes_tpu
                            .latest_votes_per_pubkey
                            .read()
                            .unwrap();
                        latest_votes_per_pubkey.iter().for_each(|(_pubkey, lock)| {
                            let mut latest_vote = lock.write().unwrap();
                            if !latest_vote.is_vote_taken() {
                                latest_vote.take_vote();
                                latest_unprocessed_votes_tpu
                                    .num_unprocessed_votes
                                    .fetch_sub(1, Ordering::Relaxed);
                            }
                        });
                    }
                }
            })
            .unwrap();
        gossip.join().unwrap();
        tpu.join().unwrap();
    }

    #[test]
    fn test_forwardable_packets() {
        let latest_unprocessed_votes = LatestUnprocessedVotes::new();
        let bank = Arc::new(Bank::default_for_tests());
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits();

        let keypair_a = ValidatorVoteKeypairs::new_rand();
        let keypair_b = ValidatorVoteKeypairs::new_rand();

        let vote_a = from_slots(vec![(1, 1)], VoteSource::Gossip, &keypair_a, None);
        let vote_b = from_slots(vec![(2, 1)], VoteSource::Tpu, &keypair_b, None);
        latest_unprocessed_votes.update_latest_vote(vote_a, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_b, false /* should replenish */);

        // Don't forward 0 stake accounts
        let forwarded = latest_unprocessed_votes
            .get_and_insert_forwardable_packets(bank, &mut forward_packet_batches_by_accounts);
        assert_eq!(0, forwarded);
        assert_eq!(
            0,
            forward_packet_batches_by_accounts
                .iter_batches()
                .filter(|&batch| !batch.is_empty())
                .count()
        );

        let config = genesis_utils::create_genesis_config_with_leader(
            100,
            &keypair_a.node_keypair.pubkey(),
            200,
        )
        .genesis_config;
        let bank = Bank::new_for_tests(&config);
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits();

        // Don't forward votes from gossip
        let forwarded = latest_unprocessed_votes.get_and_insert_forwardable_packets(
            Arc::new(bank),
            &mut forward_packet_batches_by_accounts,
        );

        assert_eq!(0, forwarded);
        assert_eq!(
            0,
            forward_packet_batches_by_accounts
                .iter_batches()
                .filter(|&batch| !batch.is_empty())
                .count()
        );

        let config = genesis_utils::create_genesis_config_with_leader(
            100,
            &keypair_b.node_keypair.pubkey(),
            200,
        )
        .genesis_config;
        let bank = Arc::new(Bank::new_for_tests(&config));
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits();

        // Forward from TPU
        let forwarded = latest_unprocessed_votes.get_and_insert_forwardable_packets(
            bank.clone(),
            &mut forward_packet_batches_by_accounts,
        );

        assert_eq!(1, forwarded);
        assert_eq!(
            1,
            forward_packet_batches_by_accounts
                .iter_batches()
                .filter(|&batch| !batch.is_empty())
                .count()
        );

        // Don't forward again
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
        let forwarded = latest_unprocessed_votes
            .get_and_insert_forwardable_packets(bank, &mut forward_packet_batches_by_accounts);

        assert_eq!(0, forwarded);
        assert_eq!(
            0,
            forward_packet_batches_by_accounts
                .iter_batches()
                .filter(|&batch| !batch.is_empty())
                .count()
        );
    }

    #[test]
    fn test_clear_forwarded_packets() {
        let latest_unprocessed_votes = LatestUnprocessedVotes::new();
        let keypair_a = ValidatorVoteKeypairs::new_rand();
        let keypair_b = ValidatorVoteKeypairs::new_rand();
        let keypair_c = ValidatorVoteKeypairs::new_rand();
        let keypair_d = ValidatorVoteKeypairs::new_rand();

        let vote_a = from_slots(vec![(1, 1)], VoteSource::Gossip, &keypair_a, None);
        let mut vote_b = from_slots(vec![(2, 1)], VoteSource::Tpu, &keypair_b, None);
        vote_b.forwarded = true;
        let vote_c = from_slots(vec![(3, 1)], VoteSource::Tpu, &keypair_c, None);
        let vote_d = from_slots(vec![(4, 1)], VoteSource::Gossip, &keypair_d, None);

        latest_unprocessed_votes.update_latest_vote(vote_a, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_b, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_c, false /* should replenish */);
        latest_unprocessed_votes.update_latest_vote(vote_d, false /* should replenish */);
        assert_eq!(4, latest_unprocessed_votes.len());

        latest_unprocessed_votes.clear_forwarded_packets();
        assert_eq!(1, latest_unprocessed_votes.len());

        assert_eq!(
            Some(1),
            latest_unprocessed_votes.get_latest_vote_slot(keypair_a.node_keypair.pubkey())
        );
        assert_eq!(
            Some(2),
            latest_unprocessed_votes.get_latest_vote_slot(keypair_b.node_keypair.pubkey())
        );
        assert_eq!(
            Some(3),
            latest_unprocessed_votes.get_latest_vote_slot(keypair_c.node_keypair.pubkey())
        );
        assert_eq!(
            Some(4),
            latest_unprocessed_votes.get_latest_vote_slot(keypair_d.node_keypair.pubkey())
        );
    }
}
