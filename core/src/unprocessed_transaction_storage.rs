use {
    crate::{
        banking_stage::{
            weighted_random_order_by_stake, BankingStage, FilterForwardingResults, ForwardOption,
        },
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        latest_unprocessed_votes::{self, LatestUnprocessedVotes, VoteSource},
        unprocessed_packet_batches::{self, UnprocessedPacketBatches},
    },
    itertools::Itertools,
    min_max_heap::MinMaxHeap,
    solana_perf::packet::PacketBatch,
    solana_runtime::bank::Bank,
    std::{rc::Rc, sync::Arc, sync::atomic::Ordering},
};

const MAX_STAKED_VALIDATORS: usize = 10_000;

#[derive(Debug, Default)]
pub struct InsertPacketBatchesSummary {
    num_dropped_packets: usize,
    num_dropped_gossip_vote_packets: usize,
    num_dropped_tpu_vote_packets: usize,
    num_dropped_tracer_packets: usize,
}

fn filter_processed_packets<'a, F>(
    retryable_transaction_indexes: impl Iterator<Item = &'a usize>,
    mut f: F,
) where
    F: FnMut(usize, usize),
{
    let mut prev_retryable_index = 0;
    for (i, retryable_index) in retryable_transaction_indexes.enumerate() {
        let start = if i == 0 { 0 } else { prev_retryable_index + 1 };

        let end = *retryable_index;
        prev_retryable_index = *retryable_index;

        if start < end {
            f(start, end)
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ThreadType {
    Voting(VoteSource),
    Transactions,
}

#[derive(Debug)]
struct VoteStorage {
    latest_unprocessed_votes: Arc<LatestUnprocessedVotes>,
    vote_source: VoteSource,
}

impl VoteStorage {
    fn is_empty(&self) -> bool {
        self.latest_unprocessed_votes.is_empty()
    }

    fn len(&self) -> usize {
        self.latest_unprocessed_votes.len()
    }

    fn capacity(&self) -> usize {
        MAX_STAKED_VALIDATORS
    }

    fn forward_option(&self) -> ForwardOption {
        match self.vote_source {
            VoteSource::Tpu => ForwardOption::ForwardTpuVote,
            VoteSource::Gossip => ForwardOption::NotForward,
        }
    }

    fn clear_forwarded_packets(&mut self) {
        self.latest_unprocessed_votes.clear_forwarded_packets();
    }

    fn deserialize_and_insert_batch(
        &mut self,
        packet_batch: &PacketBatch,
        packet_indexes: &[usize],
    ) -> (usize, usize) {
        self.latest_unprocessed_votes
            .insert_batch(latest_unprocessed_votes::deserialize_packets(
                packet_batch,
                packet_indexes,
                self.vote_source,
            ))
    }

    fn filter_forwardable_packets_and_add_batches(
        &mut self,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        if matches!(self.vote_source, VoteSource::Tpu) {
            let total_forwardable_packets = self
                .latest_unprocessed_votes
                .get_and_insert_forwardable_packets(forward_packet_batches_by_accounts);
            return FilterForwardingResults {
                total_forwardable_packets,
                ..FilterForwardingResults::default()
            };
        }
        FilterForwardingResults::default()
    }

    fn process_packets<F>(
        &mut self,
        bank: Option<Arc<Bank>>,
        batch_size: usize,
        mut processing_function: F,
    ) where
        F: FnMut(&Vec<Rc<ImmutableDeserializedPacket>>) -> Option<Vec<usize>>,
    {
        if matches!(self.vote_source, VoteSource::Gossip) {
            panic!("Gossip vote thread should not be processing transactions");
        }

        // Based on the stake distribution present in the supplied bank, drain the unprocessed votes
        // from each validator using a weighted random ordering. Votes from validators with
        // 0 stake are ignored.
        // TODO: Add proper error handling in case bank is missing?
        let bank = bank.unwrap();

        let latest_votes_per_pubkey = self
            .latest_unprocessed_votes
            .latest_votes_per_pubkey
            .read()
            .unwrap();

        let retryable_votes = weighted_random_order_by_stake(&bank)
            .filter_map(|pubkey| {
                if let Some(lock) = latest_votes_per_pubkey.get(&pubkey) {
                    if let Ok(latest_vote) = lock.write() {
                        if let Ok(mut latest_vote) = latest_vote.try_borrow_mut() {
                            if !latest_vote.is_processed() {
                                let packet = latest_vote.clone();
                                latest_vote.clear();
                                self.latest_unprocessed_votes
                                    .size
                                    .fetch_sub(1, Ordering::AcqRel);
                                return Some(packet);
                            }
                        }
                    }
                }
                None
            })
            .chunks(batch_size)
            .into_iter()
            .flat_map(|vote_packets| {
                let vote_packets = vote_packets.into_iter().collect_vec();
                let packets_to_process = vote_packets
                    .iter()
                    .map(&DeserializedVotePacket::get_vote_packet)
                    .collect_vec();
                if let Some(retryable_vote_indices) = processing_function(&packets_to_process) {
                    retryable_vote_indices
                        .iter()
                        .map(|i| vote_packets[*i].clone())
                        .collect_vec()
                } else {
                    vote_packets
                }
            })
            .collect_vec();
        // Insert the retryable votes back in
        self.latest_unprocessed_votes
            .insert_batch(retryable_votes.into_iter());
    }
}

#[derive(Debug)]
struct TransactionStorage {
    unprocessed_packet_batches: UnprocessedPacketBatches,
    thread_type: ThreadType,
}

impl TransactionStorage {
    fn is_empty(&self) -> bool {
        self.unprocessed_packet_batches.is_empty()
    }

    fn len(&self) -> usize {
        self.unprocessed_packet_batches.len()
    }

    fn capacity(&self) -> usize {
        self.unprocessed_packet_batches.capacity()
    }

    #[cfg(test)]
    fn iter(&mut self) -> impl Iterator<Item = &DeserializedPacket> {
        self.unprocessed_packet_batches.iter()
    }

    fn forward_option(&self) -> ForwardOption {
        match self.thread_type {
            ThreadType::Transactions => ForwardOption::ForwardTransaction,
            ThreadType::Voting(VoteSource::Tpu) => ForwardOption::ForwardTpuVote,
            ThreadType::Voting(VoteSource::Gossip) => ForwardOption::NotForward,
        }
    }

    fn clear(&mut self) {
        self.unprocessed_packet_batches.clear();
    }

    fn deserialize_and_insert_batch(
        &mut self,
        packet_batch: &PacketBatch,
        packet_indexes: &[usize],
    ) -> (usize, usize) {
        self.unprocessed_packet_batches.insert_batch(
            unprocessed_packet_batches::deserialize_packets(packet_batch, packet_indexes),
        )
    }

    fn filter_forwardable_packets_and_add_batches(
        &mut self,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        BankingStage::filter_valid_packets_for_forwarding(
            &mut self.unprocessed_packet_batches,
            forward_packet_batches_by_accounts,
        )
    }

    fn process_packets<F>(&mut self, batch_size: usize, mut processing_function: F)
    where
        F: FnMut(&Vec<Rc<ImmutableDeserializedPacket>>) -> Option<Vec<usize>>,
    {
        let mut retryable_packets = {
            let capacity = self.unprocessed_packet_batches.capacity();
            std::mem::replace(
                &mut self.unprocessed_packet_batches.packet_priority_queue,
                MinMaxHeap::with_capacity(capacity),
            )
        };
        let retryable_packets: MinMaxHeap<Rc<ImmutableDeserializedPacket>> = retryable_packets
            .drain_desc()
            .chunks(batch_size)
            .into_iter()
            .flat_map(|packets_to_process| {
                let packets_to_process = packets_to_process.into_iter().collect_vec();
                if let Some(retryable_transaction_indexes) =
                    processing_function(&packets_to_process)
                {
                    // Remove the non-retryable packets, packets that were either:
                    // 1) Successfully processed
                    // 2) Failed but not retryable
                    filter_processed_packets(
                        retryable_transaction_indexes
                            .iter()
                            .chain(std::iter::once(&packets_to_process.len())),
                        |start, end| {
                            for processed_packet in &packets_to_process[start..end] {
                                self.unprocessed_packet_batches
                                    .message_hash_to_transaction
                                    .remove(processed_packet.message_hash());
                            }
                        },
                    );
                    retryable_transaction_indexes
                        .iter()
                        .map(|i| packets_to_process[*i].clone())
                        .collect_vec()
                } else {
                    packets_to_process
                }
            })
            .collect::<MinMaxHeap<_>>();

        self.unprocessed_packet_batches.packet_priority_queue = retryable_packets;

        // Assert unprocessed queue is still consistent
        assert_eq!(
            self.unprocessed_packet_batches.packet_priority_queue.len(),
            self.unprocessed_packet_batches
                .message_hash_to_transaction
                .len()
        );
    }
}

#[derive(Debug)]
pub enum UnprocessedTransactionStorage {
    VoteStorage(VoteStorage),
    TransactionStorage(TransactionStorage),
}

impl UnprocessedTransactionStorage {
    pub fn new_transaction_storage(unprocessed_packet_batches : UnprocessedPacketBatches, thread_type : ThreadType) -> Self {
        Self::TransactionStorage(TransactionStorage {
            unprocessed_packet_batches,
            thread_type
        })
    }

    pub fn new_vote_storage(latest_unprocessed_votes : Arc<LatestUnprocessedVotes>, vote_source : VoteSource) -> Self {
        Self::VoteStorage(VoteStorage {
            latest_unprocessed_votes,
            vote_source,
        })
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.is_empty(),
            Self::TransactionStorage(transaction_storage) => transaction_storage.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.len(),
            Self::TransactionStorage(transaction_storage) => transaction_storage.len(),
        }
    }

    pub fn capacity(&self) -> usize {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.capacity(),
            Self::TransactionStorage(transaction_storage) => transaction_storage.capacity(),
        }
    }

    pub fn should_not_process(&self) -> bool {
        // The gossip vote thread does not need to process or forward any votes, that is
        // handled by the tpu vote thread
        if let Self::VoteStorage(vote_storage) = self {
            return matches!(vote_storage.vote_source, VoteSource::Gossip);
        }
        false
    }

    #[cfg(test)]
    pub fn iter(&mut self) -> impl Iterator<Item = &DeserializedPacket> {
        match self {
            Self::TransactionStorage(transaction_storage) => transaction_storage.iter(),
            _ => panic!(),
        }
    }

    pub fn forward_option(&self) -> ForwardOption {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.forward_option(),
            Self::TransactionStorage(transaction_storage) => transaction_storage.forward_option(),
        }
    }

    pub fn clear_forwarded_packets(&mut self) {
        match self {
            Self::TransactionStorage(transaction_storage) => transaction_storage.clear(), // Since we set everything as forwarded this is the same
            Self::VoteStorage(vote_storage) => vote_storage.clear_forwarded_packets(),
        }
    }

    pub fn deserialize_and_insert_batch(
        &mut self,
        packet_batch: &PacketBatch,
        packet_indexes: &[usize],
    ) -> InsertPacketBatchesSummary {
        match self {
            Self::VoteStorage(vote_storage) => {
                let (num_dropped_gossip_vote_packets, num_dropped_tpu_vote_packets) =
                    vote_storage.deserialize_and_insert_batch(packet_batch, packet_indexes);
                InsertPacketBatchesSummary {
                    num_dropped_packets: num_dropped_gossip_vote_packets
                        + num_dropped_tpu_vote_packets,
                    num_dropped_gossip_vote_packets,
                    num_dropped_tpu_vote_packets,
                    ..InsertPacketBatchesSummary::default()
                }
            }
            Self::TransactionStorage(transaction_storage) => {
                let (num_dropped_packets, num_dropped_tracer_packets) =
                    transaction_storage.deserialize_and_insert_batch(packet_batch, packet_indexes);
                InsertPacketBatchesSummary {
                    num_dropped_packets,
                    num_dropped_tracer_packets,
                    ..InsertPacketBatchesSummary::default()
                }
            }
        }
    }

    pub fn filter_forwardable_packets_and_add_batches(
        &mut self,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        match self {
            Self::TransactionStorage(transaction_storage) => transaction_storage
                .filter_forwardable_packets_and_add_batches(forward_packet_batches_by_accounts),
            Self::VoteStorage(vote_storage) => vote_storage
                .filter_forwardable_packets_and_add_batches(forward_packet_batches_by_accounts),
        }
    }

    /// The processing function takes a stream of packets ready to process, and returns the indices
    /// of the unprocessed packets that are eligible for retry. A return value of None means that
    /// all packets are unprocessed and eligible for retry.
    pub fn process_packets<F>(
        &mut self,
        bank: Option<Arc<Bank>>,
        batch_size: usize,
        mut processing_function: F,
    ) where
        F: FnMut(&Vec<Rc<ImmutableDeserializedPacket>>) -> Option<Vec<usize>>,
    {
        match self {
            Self::TransactionStorage(transaction_storage) => {
                transaction_storage.process_packets(batch_size, processing_function)
            }
            Self::VoteStorage(vote_storage) => {
                vote_storage.process_packets(bank, batch_size, processing_function)
            }
        }
    }
}
