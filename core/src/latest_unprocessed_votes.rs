use {
    crate::{
        banking_stage::weighted_random_order_by_stake,
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket}
    },
    solana_perf::packet::{Packet, PacketBatch},
    solana_runtime::bank::Bank,
    solana_sdk::{
        clock::Slot,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
    },
    solana_vote_program::vote_instruction::VoteInstruction,
    std::{
        cell::RefCell,
        collections::HashMap,
        ops::DerefMut,
        rc::Rc,
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

        let vote = ImmutableDeserializedPacket::new(packet, None)?;
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
                    vote: Some(Rc::new(vote)),
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
        bank: &Arc<Bank>,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> usize {
        let mut continue_forwarding = true;
        if let Ok(latest_votes_per_pubkey) = self.latest_votes_per_pubkey.read() {
            return weighted_random_order_by_stake(bank)
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
                            vote.clear();
                        }
                    }
                });
        }
    }
}
