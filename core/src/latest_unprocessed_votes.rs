use {
    crate::{
        banking_stage::weighted_random_order_by_stake,
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket},
    },
    solana_perf::packet::{Packet, PacketBatch},
    solana_sdk::{clock::Slot, program_utils::limited_deserialize, pubkey::Pubkey},
    solana_vote_program::vote_instruction::VoteInstruction,
    std::{
        cell::RefCell,
        collections::HashMap,
        ops::DerefMut,
        rc::Rc,
        sync::{
            atomic::{AtomicUsize, Ordering},
            RwLock,
        },
    },
};

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum VoteSource {
    Gossip,
    Tpu,
}

/// Holds deserialized vote messages as well as their source, foward status and slot
#[derive(Debug, Clone)]
pub struct DeserializedVotePacket {
    vote_source: VoteSource,
    pubkey: Pubkey,
    vote: Option<Rc<ImmutableDeserializedPacket>>,
    slot: Slot,
    forwarded: bool,
}

impl DeserializedVotePacket {
    pub fn new(packet: Packet, vote_source: VoteSource) -> Result<Self, DeserializedPacketError> {
        if !packet.meta.is_simple_vote_tx() {
            return Err(DeserializedPacketError::VoteTransactionError);
        }

        let vote = Rc::new(ImmutableDeserializedPacket::new(packet, None)?);
        Self::new_from_immutable(vote, vote_source)
    }

    pub fn new_from_immutable(
        vote: Rc<ImmutableDeserializedPacket>,
        vote_source: VoteSource,
    ) -> Result<Self, DeserializedPacketError> {
        let message = vote.transaction().get_message();
        let (_, instruction) = message
            .program_instructions_iter()
            .next()
            .ok_or(DeserializedPacketError::VoteTransactionError)?;

        match limited_deserialize::<VoteInstruction>(&instruction.data) {
            Ok(VoteInstruction::UpdateVoteState(vote_state_update))
            | Ok(VoteInstruction::UpdateVoteStateSwitch(vote_state_update, _)) => {
                let &pubkey = message
                    .message
                    .static_account_keys()
                    .get(0)
                    .ok_or(DeserializedPacketError::VoteTransactionError)?;
                let slot = vote_state_update.last_voted_slot().unwrap_or(0);

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

    pub fn get_vote_packet(&self) -> Rc<ImmutableDeserializedPacket> {
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

    pub fn is_processed(&self) -> bool {
        self.vote.is_none()
    }

    pub fn clear(&mut self) {
        self.vote = None;
    }
}

pub fn deserialize_packets<'a>(
    packet_batch: &'a PacketBatch,
    packet_indexes: &'a [usize],
    vote_source: VoteSource,
) -> impl Iterator<Item = DeserializedVotePacket> + 'a {
    packet_indexes.iter().filter_map(move |packet_index| {
        DeserializedVotePacket::new(packet_batch[*packet_index].clone(), vote_source).ok()
    })
}

#[derive(Debug, Default)]
pub struct LatestUnprocessedVotes {
    pub(crate) latest_votes_per_pubkey:
        RwLock<HashMap<Pubkey, RwLock<RefCell<DeserializedVotePacket>>>>,
    pub(crate) size: AtomicUsize,
}

unsafe impl Send for LatestUnprocessedVotes {}
unsafe impl Sync for LatestUnprocessedVotes {}

impl LatestUnprocessedVotes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert_batch(
        &self,
        votes: impl Iterator<Item = DeserializedVotePacket>,
    ) -> (usize, usize) {
        let mut num_dropped_gossip = 0;
        let mut num_dropped_tpu = 0;

        for vote in votes {
            if let Some(vote) = self.update_latest_vote(vote) {
                assert!(!vote.is_processed());
                match vote.vote_source {
                    VoteSource::Gossip => num_dropped_gossip += 1,
                    VoteSource::Tpu => num_dropped_tpu += 1,
                }
            }
        }

        (num_dropped_gossip, num_dropped_tpu)
    }

    /// If this vote causes an unprocessed vote to be removed, returns Some(old_vote)
    /// If there is a newer vote processed / waiting to be processed returns Some(vote)
    /// Otherwise returns None
    pub fn update_latest_vote(
        &self,
        vote: DeserializedVotePacket,
    ) -> Option<DeserializedVotePacket> {
        let pubkey = vote.pubkey();
        let slot = vote.slot();
        if let Some(latest_vote) = self.latest_votes_per_pubkey.read().unwrap().get(&pubkey) {
            let mut latest_slot = 0;
            {
                if let Some(latest) = latest_vote
                    .read()
                    .ok()
                    .and_then(|v| v.try_borrow().ok().map(|vote| vote.slot()))
                {
                    latest_slot = latest;
                }
            }
            if slot > latest_slot {
                if let Ok(latest_vote) = latest_vote.write() {
                    // At this point no one should have a borrow to this refcell as all borrows are
                    // hidden behind read()
                    if let Ok(mut latest_vote) = latest_vote.try_borrow_mut() {
                        let latest_slot = latest_vote.slot();
                        if slot > latest_slot {
                            let old_vote = std::mem::replace(latest_vote.deref_mut(), vote);
                            if old_vote.is_processed() {
                                self.size.fetch_add(1, Ordering::AcqRel);
                                return None;
                            } else {
                                return Some(old_vote);
                            }
                        }
                    } else {
                        error!("Implementation error {} {} {:?}", slot, latest_slot, self);
                    }
                }
            }
            return Some(vote);
        }

        // Should have low lock contention because this is only hit on the first few blocks of startup
        // and when a new vote account starts voting.
        let mut latest_votes_per_pubkey = self.latest_votes_per_pubkey.write().unwrap();
        latest_votes_per_pubkey.insert(pubkey, RwLock::new(RefCell::new(vote)));
        self.size.fetch_add(1, Ordering::AcqRel);
        None
    }

    pub fn get_latest_vote_slot(&self, pubkey: Pubkey) -> Option<Slot> {
        self.latest_votes_per_pubkey
            .read()
            .ok()
            .and_then(|latest_votes_per_pubkey| {
                latest_votes_per_pubkey
                    .get(&pubkey)
                    .and_then(|l| l.read().ok())
                    .and_then(|c| c.try_borrow().ok().map(|v| (*v).slot()))
            })
    }

    /// Returns how many packets were forwardable
    /// Performs a weighted random order based on stake and stops forwarding at the first error
    /// Votes from validators with 0 stakes are ignored
    pub fn get_and_insert_forwardable_packets(
        &self,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> usize {
        let mut continue_forwarding = true;
        if let Ok(latest_votes_per_pubkey) = self.latest_votes_per_pubkey.read() {
            return weighted_random_order_by_stake(
                &forward_packet_batches_by_accounts.current_bank,
                latest_votes_per_pubkey.keys(),
            )
            .filter(|pubkey| {
                if let Some(lock) = latest_votes_per_pubkey.get(pubkey) {
                    if let Ok(cell) = lock.write() {
                        if let Ok(mut vote) = cell.try_borrow_mut() {
                            if !vote.is_processed() && !vote.is_forwarded() {
                                if continue_forwarding {
                                    if forward_packet_batches_by_accounts
                                        .add_packet(vote.vote.as_ref().unwrap().clone())
                                    {
                                        vote.forwarded = true;
                                    } else {
                                        // To match behavior of regular transactions we stop
                                        // forwarding votes as soon as one fails
                                        continue_forwarding = false;
                                    }
                                }
                                return true;
                            }
                        }
                    }
                }
                false
            })
            .count();
        }
        0
    }

    /// Sometimes we forward and hold the packets, sometimes we forward and clear.
    /// This also clears all gosisp votes since by definition they have been forwarded
    pub fn clear_forwarded_packets(&self) {
        if let Ok(latest_votes_per_pubkey) = self.latest_votes_per_pubkey.read() {
            latest_votes_per_pubkey
                .values()
                .filter(|lock| {
                    if let Ok(cell) = lock.read() {
                        if let Ok(vote) = cell.try_borrow() {
                            return vote.is_forwarded();
                        }
                    }
                    false
                })
                .for_each(|lock| {
                    if let Ok(cell) = lock.write() {
                        if let Ok(mut vote) = cell.try_borrow_mut() {
                            if vote.is_forwarded() {
                                vote.clear();
                                self.size.fetch_sub(1, Ordering::Relaxed);
                            }
                        }
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        itertools::Itertools,
        rand::{thread_rng, Rng},
        solana_perf::packet::{Packet, PacketFlags},
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
    ) -> DeserializedVotePacket {
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
        packet.meta.flags.set(PacketFlags::SIMPLE_VOTE_TX, true);
        DeserializedVotePacket::new(packet, vote_source).unwrap()
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
        vote.meta.flags.set(PacketFlags::SIMPLE_VOTE_TX, true);
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
            .meta
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
            .meta
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
            .meta
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
                            let latest_vote = lock.write().unwrap();
                            let mut latest_vote = latest_vote.try_borrow_mut().unwrap();
                            if !latest_vote.is_processed() {
                                latest_vote.clear();
                                latest_unprocessed_votes_tpu
                                    .size
                                    .fetch_sub(1, Ordering::AcqRel);
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
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits(Arc::new(
                Bank::default_for_tests(),
            ));

        let keypair_a = ValidatorVoteKeypairs::new_rand();
        let keypair_b = ValidatorVoteKeypairs::new_rand();

        let vote_a = from_slots(vec![(1, 1)], VoteSource::Gossip, &keypair_a);
        let vote_b = from_slots(vec![(2, 1)], VoteSource::Tpu, &keypair_b);
        latest_unprocessed_votes.update_latest_vote(vote_a);
        latest_unprocessed_votes.update_latest_vote(vote_b);

        // Don't forward 0 stake accounts
        let forwarded = latest_unprocessed_votes
            .get_and_insert_forwardable_packets(&mut forward_packet_batches_by_accounts);
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
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits(Arc::new(bank));

        // Don't forward votes from gossip
        let forwarded = latest_unprocessed_votes
            .get_and_insert_forwardable_packets(&mut forward_packet_batches_by_accounts);

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
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits(bank.clone());

        // Forward from TPU
        let forwarded = latest_unprocessed_votes
            .get_and_insert_forwardable_packets(&mut forward_packet_batches_by_accounts);

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
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits(bank);
        let forwarded = latest_unprocessed_votes
            .get_and_insert_forwardable_packets(&mut forward_packet_batches_by_accounts);

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
