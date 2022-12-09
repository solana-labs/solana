use {
    crate::{
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket},
    },
    itertools::Itertools,
    rand::{thread_rng, Rng},
    solana_perf::packet::Packet,
    solana_runtime::bank::Bank,
    solana_sdk::{clock::Slot, program_utils::limited_deserialize, pubkey::Pubkey},
    solana_vote_program::vote_instruction::VoteInstruction,
    std::{
        collections::HashMap,
        ops::DerefMut,
        sync::{Arc, RwLock},
    },
};

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum VoteSource {
    Gossip,
    Tpu,
}

/// Holds deserialized vote messages as well as their source, foward status and slot
#[derive(Debug, Clone)]
pub struct LatestValidatorVotePacket {
    vote_source: VoteSource,
    pubkey: Pubkey,
    vote: Option<Arc<ImmutableDeserializedPacket>>,
    slot: Slot,
    forwarded: bool,
}

impl LatestValidatorVotePacket {
    pub fn new(packet: Packet, vote_source: VoteSource) -> Result<Self, DeserializedPacketError> {
        if !packet.meta().is_simple_vote_tx() {
            return Err(DeserializedPacketError::VoteTransactionError);
        }

        let vote = Arc::new(ImmutableDeserializedPacket::new(packet, None)?);
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
                    .get(0)
                    .ok_or(DeserializedPacketError::VoteTransactionError)?;
                let slot = vote_state_update_instruction.last_voted_slot().unwrap_or(0);

                Ok(Self {
                    vote: Some(vote),
                    slot,
                    pubkey,
                    vote_source,
                    forwarded: false,
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

// TODO: replace this with rand::seq::index::sample_weighted once we can update rand to 0.8+
// This requires updating dependencies of ed25519-dalek as rand_core is not compatible cross
// version https://github.com/dalek-cryptography/ed25519-dalek/pull/214
pub(crate) fn weighted_random_order_by_stake<'a>(
    bank: &Arc<Bank>,
    pubkeys: impl Iterator<Item = &'a Pubkey>,
) -> impl Iterator<Item = Pubkey> {
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
}

impl LatestUnprocessedVotes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Expensive because this involves iterating through and locking every unprocessed vote
    pub fn len(&self) -> usize {
        self.latest_votes_per_pubkey
            .read()
            .unwrap()
            .values()
            .filter(|lock| !lock.read().unwrap().is_vote_taken())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn insert_batch(
        &self,
        votes: impl Iterator<Item = LatestValidatorVotePacket>,
    ) -> VoteBatchInsertionMetrics {
        let mut num_dropped_gossip = 0;
        let mut num_dropped_tpu = 0;

        for vote in votes {
            if let Some(vote) = self.update_latest_vote(vote) {
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
    ) -> Option<LatestValidatorVotePacket> {
        let pubkey = vote.pubkey();
        let slot = vote.slot();
        if let Some(latest_vote) = self.get_entry(pubkey) {
            let latest_slot = latest_vote.read().unwrap().slot();
            if slot > latest_slot {
                let mut latest_vote = latest_vote.write().unwrap();
                let latest_slot = latest_vote.slot();
                if slot > latest_slot {
                    let old_vote = std::mem::replace(latest_vote.deref_mut(), vote);
                    if old_vote.is_vote_taken() {
                        return None;
                    } else {
                        return Some(old_vote);
                    }
                }
            }
            return Some(vote);
        }

        // Should have low lock contention because this is only hit on the first few blocks of startup
        // and when a new vote account starts voting.
        let mut latest_votes_per_pubkey = self.latest_votes_per_pubkey.write().unwrap();
        latest_votes_per_pubkey.insert(pubkey, Arc::new(RwLock::new(vote)));
        None
    }

    pub fn get_latest_vote_slot(&self, pubkey: Pubkey) -> Option<Slot> {
        self.latest_votes_per_pubkey
            .read()
            .unwrap()
            .get(&pubkey)
            .map(|l| l.read().unwrap().slot())
    }

    /// Returns how many packets were forwardable
    /// Performs a weighted random order based on stake and stops forwarding at the first error
    /// Votes from validators with 0 stakes are ignored
    pub fn get_and_insert_forwardable_packets(
        &self,
        bank: Arc<Bank>,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> usize {
        let mut continue_forwarding = true;
        let pubkeys_by_stake = weighted_random_order_by_stake(
            &bank,
            self.latest_votes_per_pubkey.read().unwrap().keys(),
        )
        .collect_vec();
        pubkeys_by_stake
            .into_iter()
            .filter(|&pubkey| {
                if !continue_forwarding {
                    return false;
                }
                if let Some(lock) = self.get_entry(pubkey) {
                    let mut vote = lock.write().unwrap();
                    if !vote.is_vote_taken() && !vote.is_forwarded() {
                        let deserialized_vote_packet = vote.vote.as_ref().unwrap().clone();
                        if let Some(sanitized_vote_transaction) = deserialized_vote_packet
                            .build_sanitized_transaction(
                                &bank.feature_set,
                                bank.vote_only_bank(),
                                bank.as_ref(),
                            )
                        {
                            if forward_packet_batches_by_accounts.try_add_packet(
                                &sanitized_vote_transaction,
                                deserialized_vote_packet,
                                &bank.feature_set,
                            ) {
                                vote.forwarded = true;
                            } else {
                                // To match behavior of regular transactions we stop
                                // forwarding votes as soon as one fails
                                continue_forwarding = false;
                            }
                            return true;
                        } else {
                            return false;
                        }
                    }
                }
                false
            })
            .count()
    }

    /// Drains all votes yet to be processed sorted by a weighted random ordering by stake
    pub fn drain_unprocessed(&self, bank: Arc<Bank>) -> Vec<Arc<ImmutableDeserializedPacket>> {
        let pubkeys_by_stake = weighted_random_order_by_stake(
            &bank,
            self.latest_votes_per_pubkey.read().unwrap().keys(),
        )
        .collect_vec();
        pubkeys_by_stake
            .into_iter()
            .filter_map(|pubkey| {
                self.get_entry(pubkey).and_then(|lock| {
                    let mut latest_vote = lock.write().unwrap();
                    latest_vote.take_vote()
                })
            })
            .collect_vec()
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
                if vote.is_forwarded() {
                    vote.take_vote();
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
            vote_state::VoteStateUpdate,
            vote_transaction::{new_vote_state_update_transaction, new_vote_transaction},
        },
        std::{sync::Arc, thread::Builder},
    };

    fn from_slots(
        slots: Vec<(u64, u32)>,
        vote_source: VoteSource,
        keypairs: &ValidatorVoteKeypairs,
    ) -> LatestValidatorVotePacket {
        let vote = VoteStateUpdate::from(slots);
        let vote_tx = new_vote_state_update_transaction(
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
        let bankhash = Hash::new_unique();
        let blockhash = Hash::new_unique();
        let switch_proof = Hash::new_unique();
        let mut vote = Packet::from_data(
            None,
            new_vote_transaction(
                vec![0, 1, 2],
                bankhash,
                blockhash,
                &keypairs.node_keypair,
                &keypairs.vote_keypair,
                &keypairs.vote_keypair,
                None,
            ),
        )
        .unwrap();
        vote.meta_mut().flags.set(PacketFlags::SIMPLE_VOTE_TX, true);
        let mut vote_switch = Packet::from_data(
            None,
            new_vote_transaction(
                vec![0, 1, 2],
                bankhash,
                blockhash,
                &keypairs.node_keypair,
                &keypairs.vote_keypair,
                &keypairs.vote_keypair,
                Some(switch_proof),
            ),
        )
        .unwrap();
        vote_switch
            .meta_mut()
            .flags
            .set(PacketFlags::SIMPLE_VOTE_TX, true);
        let mut vote_state_update = Packet::from_data(
            None,
            new_vote_state_update_transaction(
                VoteStateUpdate::from(vec![(0, 3), (1, 2), (2, 1)]),
                blockhash,
                &keypairs.node_keypair,
                &keypairs.vote_keypair,
                &keypairs.vote_keypair,
                None,
            ),
        )
        .unwrap();
        vote_state_update
            .meta_mut()
            .flags
            .set(PacketFlags::SIMPLE_VOTE_TX, true);
        let mut vote_state_update_switch = Packet::from_data(
            None,
            new_vote_state_update_transaction(
                VoteStateUpdate::from(vec![(0, 3), (1, 2), (3, 1)]),
                blockhash,
                &keypairs.node_keypair,
                &keypairs.vote_keypair,
                &keypairs.vote_keypair,
                Some(switch_proof),
            ),
        )
        .unwrap();
        vote_state_update_switch
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
        let packet_batch = PacketBatch::new(vec![
            vote,
            vote_switch,
            vote_state_update,
            vote_state_update_switch,
            random_transaction,
        ]);

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

        let vote_a = from_slots(vec![(0, 2), (1, 1)], VoteSource::Gossip, &keypair_a);
        let vote_b = from_slots(vec![(0, 5), (4, 2), (9, 1)], VoteSource::Gossip, &keypair_b);

        assert!(latest_unprocessed_votes
            .update_latest_vote(vote_a)
            .is_none());
        assert!(latest_unprocessed_votes
            .update_latest_vote(vote_b)
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
        );
        let vote_b = from_slots(vec![(0, 5), (4, 2), (6, 1)], VoteSource::Gossip, &keypair_a);

        // Evict previous vote
        assert_eq!(
            1,
            latest_unprocessed_votes
                .update_latest_vote(vote_a)
                .unwrap()
                .slot
        );
        // Drop current vote
        assert_eq!(
            6,
            latest_unprocessed_votes
                .update_latest_vote(vote_b)
                .unwrap()
                .slot
        );

        assert_eq!(2, latest_unprocessed_votes.len());
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
                        &keypairs[rng.gen_range(0, 10)],
                    );
                    latest_unprocessed_votes.update_latest_vote(vote);
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
                        &keypairs_tpu[rng.gen_range(0, 10)],
                    );
                    latest_unprocessed_votes_tpu.update_latest_vote(vote);
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

        let vote_a = from_slots(vec![(1, 1)], VoteSource::Gossip, &keypair_a);
        let vote_b = from_slots(vec![(2, 1)], VoteSource::Tpu, &keypair_b);
        latest_unprocessed_votes.update_latest_vote(vote_a);
        latest_unprocessed_votes.update_latest_vote(vote_b);

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

        let vote_a = from_slots(vec![(1, 1)], VoteSource::Gossip, &keypair_a);
        let mut vote_b = from_slots(vec![(2, 1)], VoteSource::Tpu, &keypair_b);
        vote_b.forwarded = true;
        let vote_c = from_slots(vec![(3, 1)], VoteSource::Tpu, &keypair_c);
        let vote_d = from_slots(vec![(4, 1)], VoteSource::Gossip, &keypair_d);

        latest_unprocessed_votes.update_latest_vote(vote_a);
        latest_unprocessed_votes.update_latest_vote(vote_b);
        latest_unprocessed_votes.update_latest_vote(vote_c);
        latest_unprocessed_votes.update_latest_vote(vote_d);
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
