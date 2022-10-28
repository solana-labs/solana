use {
    crate::{
        banking_stage::{FilterForwardingResults, ForwardOption},
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        latest_unprocessed_votes::{
            LatestUnprocessedVotes, LatestValidatorVotePacket, VoteBatchInsertionMetrics,
            VoteSource,
        },
        unprocessed_packet_batches::{
            DeserializedPacket, PacketBatchInsertionMetrics, UnprocessedPacketBatches,
        },
    },
    itertools::Itertools,
    min_max_heap::MinMaxHeap,
    solana_measure::measure,
    solana_runtime::bank::Bank,
    solana_sdk::{
        clock::FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET, saturating_add_assign,
        transaction::SanitizedTransaction,
    },
    std::sync::Arc,
};

pub const UNPROCESSED_BUFFER_STEP_SIZE: usize = 128;
/// Maximum numer of votes a single receive call will accept
const MAX_NUM_VOTES_RECEIVE: usize = 10_000;

#[derive(Debug)]
pub enum UnprocessedTransactionStorage {
    VoteStorage(VoteStorage),
    LocalTransactionStorage(ThreadLocalUnprocessedPackets),
}

#[derive(Debug)]
pub struct ThreadLocalUnprocessedPackets {
    unprocessed_packet_batches: UnprocessedPacketBatches,
    thread_type: ThreadType,
}

#[derive(Debug)]
pub struct VoteStorage {
    latest_unprocessed_votes: Arc<LatestUnprocessedVotes>,
    vote_source: VoteSource,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ThreadType {
    Voting(VoteSource),
    Transactions,
}

#[derive(Debug)]
pub(crate) enum InsertPacketBatchSummary {
    VoteBatchInsertionMetrics(VoteBatchInsertionMetrics),
    PacketBatchInsertionMetrics(PacketBatchInsertionMetrics),
}

impl InsertPacketBatchSummary {
    pub fn total_dropped_packets(&self) -> usize {
        match self {
            Self::VoteBatchInsertionMetrics(metrics) => {
                metrics.num_dropped_gossip + metrics.num_dropped_tpu
            }
            Self::PacketBatchInsertionMetrics(metrics) => metrics.num_dropped_packets,
        }
    }

    pub fn dropped_gossip_packets(&self) -> usize {
        match self {
            Self::VoteBatchInsertionMetrics(metrics) => metrics.num_dropped_gossip,
            _ => 0,
        }
    }

    pub fn dropped_tpu_packets(&self) -> usize {
        match self {
            Self::VoteBatchInsertionMetrics(metrics) => metrics.num_dropped_tpu,
            _ => 0,
        }
    }

    pub fn dropped_tracer_packets(&self) -> usize {
        match self {
            Self::PacketBatchInsertionMetrics(metrics) => metrics.num_dropped_tracer_packets,
            _ => 0,
        }
    }
}

impl From<VoteBatchInsertionMetrics> for InsertPacketBatchSummary {
    fn from(metrics: VoteBatchInsertionMetrics) -> Self {
        Self::VoteBatchInsertionMetrics(metrics)
    }
}

impl From<PacketBatchInsertionMetrics> for InsertPacketBatchSummary {
    fn from(metrics: PacketBatchInsertionMetrics) -> Self {
        Self::PacketBatchInsertionMetrics(metrics)
    }
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

impl UnprocessedTransactionStorage {
    pub fn new_transaction_storage(
        unprocessed_packet_batches: UnprocessedPacketBatches,
        thread_type: ThreadType,
    ) -> Self {
        Self::LocalTransactionStorage(ThreadLocalUnprocessedPackets {
            unprocessed_packet_batches,
            thread_type,
        })
    }

    pub fn new_vote_storage(
        latest_unprocessed_votes: Arc<LatestUnprocessedVotes>,
        vote_source: VoteSource,
    ) -> Self {
        Self::VoteStorage(VoteStorage {
            latest_unprocessed_votes,
            vote_source,
        })
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.is_empty(),
            Self::LocalTransactionStorage(transaction_storage) => transaction_storage.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.len(),
            Self::LocalTransactionStorage(transaction_storage) => transaction_storage.len(),
        }
    }

    /// Returns the maximum number of packets a receive should accept
    pub fn max_receive_size(&self) -> usize {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.max_receive_size(),
            Self::LocalTransactionStorage(transaction_storage) => {
                transaction_storage.max_receive_size()
            }
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
            Self::LocalTransactionStorage(transaction_storage) => transaction_storage.iter(),
            _ => panic!(),
        }
    }

    pub fn forward_option(&self) -> ForwardOption {
        match self {
            Self::VoteStorage(vote_storage) => vote_storage.forward_option(),
            Self::LocalTransactionStorage(transaction_storage) => {
                transaction_storage.forward_option()
            }
        }
    }

    pub fn clear_forwarded_packets(&mut self) {
        match self {
            Self::LocalTransactionStorage(transaction_storage) => transaction_storage.clear(), // Since we set everything as forwarded this is the same
            Self::VoteStorage(vote_storage) => vote_storage.clear_forwarded_packets(),
        }
    }

    pub(crate) fn insert_batch(
        &mut self,
        deserialized_packets: Vec<ImmutableDeserializedPacket>,
    ) -> InsertPacketBatchSummary {
        match self {
            Self::VoteStorage(vote_storage) => {
                InsertPacketBatchSummary::from(vote_storage.insert_batch(deserialized_packets))
            }
            Self::LocalTransactionStorage(transaction_storage) => InsertPacketBatchSummary::from(
                transaction_storage.insert_batch(deserialized_packets),
            ),
        }
    }

    pub fn filter_forwardable_packets_and_add_batches(
        &mut self,
        bank: Arc<Bank>,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        match self {
            Self::LocalTransactionStorage(transaction_storage) => transaction_storage
                .filter_forwardable_packets_and_add_batches(
                    bank,
                    forward_packet_batches_by_accounts,
                ),
            Self::VoteStorage(vote_storage) => vote_storage
                .filter_forwardable_packets_and_add_batches(
                    bank,
                    forward_packet_batches_by_accounts,
                ),
        }
    }

    /// The processing function takes a stream of packets ready to process, and returns the indices
    /// of the unprocessed packets that are eligible for retry. A return value of None means that
    /// all packets are unprocessed and eligible for retry.
    pub fn process_packets<F>(
        &mut self,
        bank: Option<Arc<Bank>>,
        batch_size: usize,
        processing_function: F,
    ) where
        F: FnMut(&Vec<Arc<ImmutableDeserializedPacket>>) -> Option<Vec<usize>>,
    {
        match (self, bank) {
            (Self::LocalTransactionStorage(transaction_storage), _) => {
                transaction_storage.process_packets(batch_size, processing_function)
            }
            (Self::VoteStorage(vote_storage), Some(bank)) => {
                vote_storage.process_packets(bank, batch_size, processing_function)
            }
            _ => {}
        }
    }
}

impl VoteStorage {
    fn is_empty(&self) -> bool {
        self.latest_unprocessed_votes.is_empty()
    }

    fn len(&self) -> usize {
        self.latest_unprocessed_votes.len()
    }

    fn max_receive_size(&self) -> usize {
        MAX_NUM_VOTES_RECEIVE
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

    fn insert_batch(
        &mut self,
        deserialized_packets: Vec<ImmutableDeserializedPacket>,
    ) -> VoteBatchInsertionMetrics {
        self.latest_unprocessed_votes
            .insert_batch(
                deserialized_packets
                    .into_iter()
                    .filter_map(|deserialized_packet| {
                        LatestValidatorVotePacket::new_from_immutable(
                            Arc::new(deserialized_packet),
                            self.vote_source,
                        )
                        .ok()
                    }),
            )
    }

    fn filter_forwardable_packets_and_add_batches(
        &mut self,
        bank: Arc<Bank>,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        if matches!(self.vote_source, VoteSource::Tpu) {
            let total_forwardable_packets = self
                .latest_unprocessed_votes
                .get_and_insert_forwardable_packets(bank, forward_packet_batches_by_accounts);
            return FilterForwardingResults {
                total_forwardable_packets,
                ..FilterForwardingResults::default()
            };
        }
        FilterForwardingResults::default()
    }

    fn process_packets<F>(&mut self, bank: Arc<Bank>, batch_size: usize, mut processing_function: F)
    where
        F: FnMut(&Vec<Arc<ImmutableDeserializedPacket>>) -> Option<Vec<usize>>,
    {
        if matches!(self.vote_source, VoteSource::Gossip) {
            panic!("Gossip vote thread should not be processing transactions");
        }

        // Insert the retryable votes back in
        self.latest_unprocessed_votes.insert_batch(
            // Based on the stake distribution present in the supplied bank, drain the unprocessed votes
            // from each validator using a weighted random ordering. Votes from validators with
            // 0 stake are ignored.
            self.latest_unprocessed_votes
                .drain_unprocessed(bank)
                .into_iter()
                .chunks(batch_size)
                .into_iter()
                .flat_map(|vote_packets| {
                    let vote_packets = vote_packets.into_iter().collect_vec();
                    if let Some(retryable_vote_indices) = processing_function(&vote_packets) {
                        retryable_vote_indices
                            .iter()
                            .map(|i| vote_packets[*i].clone())
                            .collect_vec()
                    } else {
                        vote_packets
                    }
                })
                .filter_map(|packet| {
                    LatestValidatorVotePacket::new_from_immutable(packet, self.vote_source).ok()
                }),
        );
    }
}

impl ThreadLocalUnprocessedPackets {
    fn is_empty(&self) -> bool {
        self.unprocessed_packet_batches.is_empty()
    }

    pub fn thread_type(&self) -> ThreadType {
        self.thread_type
    }

    fn len(&self) -> usize {
        self.unprocessed_packet_batches.len()
    }

    fn max_receive_size(&self) -> usize {
        self.unprocessed_packet_batches.capacity() - self.unprocessed_packet_batches.len()
    }

    #[cfg(test)]
    fn iter(&mut self) -> impl Iterator<Item = &DeserializedPacket> {
        self.unprocessed_packet_batches.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut DeserializedPacket> {
        self.unprocessed_packet_batches.iter_mut()
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

    fn insert_batch(
        &mut self,
        deserialized_packets: Vec<ImmutableDeserializedPacket>,
    ) -> PacketBatchInsertionMetrics {
        self.unprocessed_packet_batches.insert_batch(
            deserialized_packets
                .into_iter()
                .map(DeserializedPacket::from_immutable_section),
        )
    }

    fn filter_forwardable_packets_and_add_batches(
        &mut self,
        bank: Arc<Bank>,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        self.filter_and_forward_with_account_limits(
            bank,
            forward_packet_batches_by_accounts,
            UNPROCESSED_BUFFER_STEP_SIZE,
        )
    }

    /// Filter out packets that fail to sanitize, or are no longer valid (could be
    /// too old, a duplicate of something already processed). Doing this in batches to avoid
    /// checking bank's blockhash and status cache per transaction which could be bad for performance.
    /// Added valid and sanitized packets to forwarding queue.
    fn filter_and_forward_with_account_limits(
        &mut self,
        bank: Arc<Bank>,
        forward_buffer: &mut ForwardPacketBatchesByAccounts,
        batch_size: usize,
    ) -> FilterForwardingResults {
        let mut total_forwardable_tracer_packets: usize = 0;
        let mut total_tracer_packets_in_buffer: usize = 0;
        let mut total_forwardable_packets: usize = 0;
        let mut total_packet_conversion_us: u64 = 0;
        let mut total_filter_packets_us: u64 = 0;
        let mut dropped_tx_before_forwarding_count: usize = 0;

        let mut original_priority_queue = self.take_priority_queue();
        let original_capacity = original_priority_queue.capacity();
        let mut new_priority_queue = MinMaxHeap::with_capacity(original_capacity);

        // indicates if `forward_buffer` still accept more packets, see details at
        // `ForwardPacketBatchesByAccounts.rs`.
        let mut accepting_packets = true;
        // batch iterate through self.unprocessed_packet_batches in desc priority order
        new_priority_queue.extend(
            original_priority_queue
                .drain_desc()
                .chunks(batch_size)
                .into_iter()
                .flat_map(|packets_to_process| {
                    let packets_to_process = packets_to_process.into_iter().collect_vec();

                    // Vec<bool> of same size of `packets_to_process`, each indicates
                    // corresponding packet is tracer packet.
                    let tracer_packet_indexes = packets_to_process
                        .iter()
                        .map(|deserialized_packet| {
                            deserialized_packet
                                .original_packet()
                                .meta
                                .is_tracer_packet()
                        })
                        .collect::<Vec<_>>();
                    saturating_add_assign!(
                        total_tracer_packets_in_buffer,
                        tracer_packet_indexes
                            .iter()
                            .filter(|is_tracer| **is_tracer)
                            .count()
                    );

                    if accepting_packets {
                        let (
                            (sanitized_transactions, transaction_to_packet_indexes),
                            packet_conversion_time,
                        ): ((Vec<SanitizedTransaction>, Vec<usize>), _) = measure!(
                            self.sanitize_unforwarded_packets(&packets_to_process, &bank,),
                            "sanitize_packet",
                        );
                        saturating_add_assign!(
                            total_packet_conversion_us,
                            packet_conversion_time.as_us()
                        );

                        let (forwardable_transaction_indexes, filter_packets_time) = measure!(
                            Self::filter_invalid_transactions(&sanitized_transactions, &bank,),
                            "filter_packets",
                        );
                        saturating_add_assign!(
                            total_filter_packets_us,
                            filter_packets_time.as_us()
                        );

                        for forwardable_transaction_index in &forwardable_transaction_indexes {
                            saturating_add_assign!(total_forwardable_packets, 1);
                            let forwardable_packet_index =
                                transaction_to_packet_indexes[*forwardable_transaction_index];
                            if tracer_packet_indexes[forwardable_packet_index] {
                                saturating_add_assign!(total_forwardable_tracer_packets, 1);
                            }
                        }

                        let accepted_packet_indexes = Self::add_filtered_packets_to_forward_buffer(
                            forward_buffer,
                            &packets_to_process,
                            &sanitized_transactions,
                            &transaction_to_packet_indexes,
                            &forwardable_transaction_indexes,
                            &mut dropped_tx_before_forwarding_count,
                        );
                        accepting_packets =
                            accepted_packet_indexes.len() == forwardable_transaction_indexes.len();

                        self.unprocessed_packet_batches
                            .mark_accepted_packets_as_forwarded(
                                &packets_to_process,
                                &accepted_packet_indexes,
                            );

                        self.collect_retained_packets(
                            &packets_to_process,
                            &Self::prepare_filtered_packet_indexes(
                                &transaction_to_packet_indexes,
                                &forwardable_transaction_indexes,
                            ),
                        )
                    } else {
                        // skip sanitizing and filtering if not longer able to add more packets for forwarding
                        saturating_add_assign!(
                            dropped_tx_before_forwarding_count,
                            packets_to_process.len()
                        );
                        packets_to_process
                    }
                }),
        );

        // replace packet priority queue
        self.unprocessed_packet_batches.packet_priority_queue = new_priority_queue;
        self.verify_priority_queue(original_capacity);

        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            dropped_tx_before_forwarding_count
        );

        FilterForwardingResults {
            total_forwardable_packets,
            total_tracer_packets_in_buffer,
            total_forwardable_tracer_packets,
            total_packet_conversion_us,
            total_filter_packets_us,
        }
    }

    /// Take self.unprocessed_packet_batches's priority_queue out, leave empty MinMaxHeap in its place.
    fn take_priority_queue(&mut self) -> MinMaxHeap<Arc<ImmutableDeserializedPacket>> {
        std::mem::replace(
            &mut self.unprocessed_packet_batches.packet_priority_queue,
            MinMaxHeap::new(), // <-- no need to reserve capacity as we will replace this
        )
    }

    /// Verify that the priority queue and map are consistent and that original capacity is maintained.
    fn verify_priority_queue(&self, original_capacity: usize) {
        // Assert unprocessed queue is still consistent and maintains original capacity
        assert_eq!(
            self.unprocessed_packet_batches
                .packet_priority_queue
                .capacity(),
            original_capacity
        );
        assert_eq!(
            self.unprocessed_packet_batches.packet_priority_queue.len(),
            self.unprocessed_packet_batches
                .message_hash_to_transaction
                .len()
        );
    }

    /// sanitize un-forwarded packet into SanitizedTransaction for validation and forwarding.
    fn sanitize_unforwarded_packets(
        &mut self,
        packets_to_process: &[Arc<ImmutableDeserializedPacket>],
        bank: &Arc<Bank>,
    ) -> (Vec<SanitizedTransaction>, Vec<usize>) {
        // Get ref of ImmutableDeserializedPacket
        let deserialized_packets = packets_to_process.iter().map(|p| &**p);
        let (transactions, transaction_to_packet_indexes): (Vec<SanitizedTransaction>, Vec<usize>) =
            deserialized_packets
                .enumerate()
                .filter_map(|(packet_index, deserialized_packet)| {
                    if !self
                        .unprocessed_packet_batches
                        .is_forwarded(deserialized_packet)
                    {
                        deserialized_packet
                            .build_sanitized_transaction(
                                &bank.feature_set,
                                bank.vote_only_bank(),
                                bank.as_ref(),
                            )
                            .map(|transaction| (transaction, packet_index))
                    } else {
                        None
                    }
                })
                .unzip();

        // report metrics
        inc_new_counter_info!("banking_stage-packet_conversion", 1);
        let unsanitized_packets_filtered_count =
            packets_to_process.len().saturating_sub(transactions.len());
        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            unsanitized_packets_filtered_count
        );

        (transactions, transaction_to_packet_indexes)
    }

    /// Checks sanitized transactions against bank, returns valid transaction indexes
    fn filter_invalid_transactions(
        transactions: &[SanitizedTransaction],
        bank: &Arc<Bank>,
    ) -> Vec<usize> {
        let filter = vec![Ok(()); transactions.len()];
        let results = bank.check_transactions_with_forwarding_delay(
            transactions,
            &filter,
            FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET,
        );
        // report metrics
        let filtered_out_transactions_count = transactions.len().saturating_sub(results.len());
        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            filtered_out_transactions_count
        );

        results
            .iter()
            .enumerate()
            .filter_map(
                |(tx_index, (result, _))| if result.is_ok() { Some(tx_index) } else { None },
            )
            .collect_vec()
    }

    fn prepare_filtered_packet_indexes(
        transaction_to_packet_indexes: &[usize],
        retained_transaction_indexes: &[usize],
    ) -> Vec<usize> {
        retained_transaction_indexes
            .iter()
            .map(|tx_index| transaction_to_packet_indexes[*tx_index])
            .collect_vec()
    }

    /// try to add filtered forwardable and valid packets to forward buffer;
    /// returns vector of packet indexes that were accepted for forwarding.
    fn add_filtered_packets_to_forward_buffer(
        forward_buffer: &mut ForwardPacketBatchesByAccounts,
        packets_to_process: &[Arc<ImmutableDeserializedPacket>],
        transactions: &[SanitizedTransaction],
        transaction_to_packet_indexes: &[usize],
        forwardable_transaction_indexes: &[usize],
        dropped_tx_before_forwarding_count: &mut usize,
    ) -> Vec<usize> {
        let mut added_packets_count: usize = 0;
        let mut accepted_packet_indexes = Vec::with_capacity(transaction_to_packet_indexes.len());
        for forwardable_transaction_index in forwardable_transaction_indexes {
            let sanitized_transaction = &transactions[*forwardable_transaction_index];
            let forwardable_packet_index =
                transaction_to_packet_indexes[*forwardable_transaction_index];
            let immutable_deserialized_packet =
                packets_to_process[forwardable_packet_index].clone();
            if !forward_buffer.try_add_packet(sanitized_transaction, immutable_deserialized_packet)
            {
                break;
            }
            accepted_packet_indexes.push(forwardable_packet_index);
            saturating_add_assign!(added_packets_count, 1);
        }

        // count the packets not being forwarded in this batch
        saturating_add_assign!(
            *dropped_tx_before_forwarding_count,
            forwardable_transaction_indexes.len() - added_packets_count
        );

        accepted_packet_indexes
    }

    fn collect_retained_packets(
        &mut self,
        packets_to_process: &[Arc<ImmutableDeserializedPacket>],
        retained_packet_indexes: &[usize],
    ) -> Vec<Arc<ImmutableDeserializedPacket>> {
        self.remove_non_retained_packets(packets_to_process, retained_packet_indexes);
        retained_packet_indexes
            .iter()
            .map(|i| packets_to_process[*i].clone())
            .collect_vec()
    }

    /// remove packets from UnprocessedPacketBatches.message_hash_to_transaction after they have
    /// been removed from UnprocessedPacketBatches.packet_priority_queue
    fn remove_non_retained_packets(
        &mut self,
        packets_to_process: &[Arc<ImmutableDeserializedPacket>],
        retained_packet_indexes: &[usize],
    ) {
        filter_processed_packets(
            retained_packet_indexes
                .iter()
                .chain(std::iter::once(&packets_to_process.len())),
            |start, end| {
                for processed_packet in &packets_to_process[start..end] {
                    self.unprocessed_packet_batches
                        .message_hash_to_transaction
                        .remove(processed_packet.message_hash());
                }
            },
        )
    }

    fn process_packets<F>(&mut self, batch_size: usize, mut processing_function: F)
    where
        F: FnMut(&Vec<Arc<ImmutableDeserializedPacket>>) -> Option<Vec<usize>>,
    {
        let mut retryable_packets = self.take_priority_queue();
        let original_capacity = retryable_packets.capacity();
        let mut new_retryable_packets = MinMaxHeap::with_capacity(original_capacity);
        new_retryable_packets.extend(
            retryable_packets
                .drain_desc()
                .chunks(batch_size)
                .into_iter()
                .flat_map(|packets_to_process| {
                    let packets_to_process = packets_to_process.into_iter().collect_vec();
                    if let Some(retryable_transaction_indexes) =
                        processing_function(&packets_to_process)
                    {
                        self.collect_retained_packets(
                            &packets_to_process,
                            &retryable_transaction_indexes,
                        )
                    } else {
                        packets_to_process
                    }
                }),
        );

        self.unprocessed_packet_batches.packet_priority_queue = new_retryable_packets;
        self.verify_priority_queue(original_capacity);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo},
        solana_perf::packet::{Packet, PacketFlags},
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            system_transaction,
            transaction::Transaction,
        },
        solana_vote_program::{
            vote_state::VoteStateUpdate, vote_transaction::new_vote_state_update_transaction,
        },
        std::error::Error,
    };

    #[test]
    fn test_filter_processed_packets() {
        let retryable_indexes = [0, 1, 2, 3];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        filter_processed_packets(retryable_indexes.iter(), f);
        assert!(non_retryable_indexes.is_empty());

        let retryable_indexes = [0, 1, 2, 3, 5];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(4, 5)]);

        let retryable_indexes = [1, 2, 3];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1)]);

        let retryable_indexes = [1, 2, 3, 5];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1), (4, 5)]);

        let retryable_indexes = [1, 2, 3, 5, 8];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1), (4, 5), (6, 8)]);

        let retryable_indexes = [1, 2, 3, 5, 8, 8];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1), (4, 5), (6, 8)]);
    }

    #[test]
    fn test_filter_and_forward_with_account_limits() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10);
        let current_bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let simple_transactions: Vec<Transaction> = (0..256)
            .map(|_id| {
                // packets are deserialized upon receiving, failed packets will not be
                // forwarded; Therefore we need to create real packets here.
                let key1 = Keypair::new();
                system_transaction::transfer(
                    &mint_keypair,
                    &key1.pubkey(),
                    genesis_config.rent.minimum_balance(0),
                    genesis_config.hash(),
                )
            })
            .collect_vec();

        let mut packets: Vec<DeserializedPacket> = simple_transactions
            .iter()
            .enumerate()
            .map(|(packets_id, transaction)| {
                let mut p = Packet::from_data(None, transaction).unwrap();
                p.meta.port = packets_id as u16;
                p.meta.set_tracer(true);
                DeserializedPacket::new(p).unwrap()
            })
            .collect_vec();

        // all packets are forwarded
        {
            let buffered_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(packets.clone().into_iter(), packets.len());
            let mut transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
                buffered_packet_batches,
                ThreadType::Transactions,
            );
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();

            let FilterForwardingResults {
                total_forwardable_packets,
                total_tracer_packets_in_buffer,
                total_forwardable_tracer_packets,
                ..
            } = transaction_storage.filter_forwardable_packets_and_add_batches(
                current_bank.clone(),
                &mut forward_packet_batches_by_accounts,
            );
            assert_eq!(total_forwardable_packets, 256);
            assert_eq!(total_tracer_packets_in_buffer, 256);
            assert_eq!(total_forwardable_tracer_packets, 256);

            // packets in a batch are forwarded in arbitrary order; verify the ports match after
            // sorting
            let expected_ports: Vec<_> = (0..256).collect();
            let mut forwarded_ports: Vec<_> = forward_packet_batches_by_accounts
                .iter_batches()
                .flat_map(|batch| {
                    batch
                        .get_forwardable_packets()
                        .into_iter()
                        .map(|p| p.meta.port)
                })
                .collect();
            forwarded_ports.sort_unstable();
            assert_eq!(expected_ports, forwarded_ports);
        }

        // some packets are forwarded
        {
            let num_already_forwarded = 16;
            for packet in &mut packets[0..num_already_forwarded] {
                packet.forwarded = true;
            }
            let buffered_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(packets.clone().into_iter(), packets.len());
            let mut transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
                buffered_packet_batches,
                ThreadType::Transactions,
            );
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
            let FilterForwardingResults {
                total_forwardable_packets,
                total_tracer_packets_in_buffer,
                total_forwardable_tracer_packets,
                ..
            } = transaction_storage.filter_forwardable_packets_and_add_batches(
                current_bank.clone(),
                &mut forward_packet_batches_by_accounts,
            );
            assert_eq!(
                total_forwardable_packets,
                packets.len() - num_already_forwarded
            );
            assert_eq!(total_tracer_packets_in_buffer, packets.len());
            assert_eq!(
                total_forwardable_tracer_packets,
                packets.len() - num_already_forwarded
            );
        }

        // some packets are invalid (already processed)
        {
            let num_already_processed = 16;
            for tx in &simple_transactions[0..num_already_processed] {
                assert_eq!(current_bank.process_transaction(tx), Ok(()));
            }
            let buffered_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(packets.clone().into_iter(), packets.len());
            let mut transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
                buffered_packet_batches,
                ThreadType::Transactions,
            );
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
            let FilterForwardingResults {
                total_forwardable_packets,
                total_tracer_packets_in_buffer,
                total_forwardable_tracer_packets,
                ..
            } = transaction_storage.filter_forwardable_packets_and_add_batches(
                current_bank,
                &mut forward_packet_batches_by_accounts,
            );
            assert_eq!(
                total_forwardable_packets,
                packets.len() - num_already_processed
            );
            assert_eq!(total_tracer_packets_in_buffer, packets.len());
            assert_eq!(
                total_forwardable_tracer_packets,
                packets.len() - num_already_processed
            );
        }
    }

    #[test]
    fn test_unprocessed_transaction_storage_insert() -> Result<(), Box<dyn Error>> {
        let keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let pubkey = solana_sdk::pubkey::new_rand();

        let small_transfer = Packet::from_data(
            None,
            system_transaction::transfer(&keypair, &pubkey, 1, Hash::new_unique()),
        )?;
        let mut vote = Packet::from_data(
            None,
            new_vote_state_update_transaction(
                VoteStateUpdate::default(),
                Hash::new_unique(),
                &keypair,
                &vote_keypair,
                &vote_keypair,
                None,
            ),
        )?;
        vote.meta.flags.set(PacketFlags::SIMPLE_VOTE_TX, true);
        let big_transfer = Packet::from_data(
            None,
            system_transaction::transfer(&keypair, &pubkey, 1000000, Hash::new_unique()),
        )?;

        for thread_type in [
            ThreadType::Transactions,
            ThreadType::Voting(VoteSource::Gossip),
            ThreadType::Voting(VoteSource::Tpu),
        ] {
            let mut transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
                UnprocessedPacketBatches::with_capacity(100),
                thread_type,
            );
            transaction_storage.insert_batch(vec![
                ImmutableDeserializedPacket::new(small_transfer.clone(), None)?,
                ImmutableDeserializedPacket::new(vote.clone(), None)?,
                ImmutableDeserializedPacket::new(big_transfer.clone(), None)?,
            ]);
            let deserialized_packets = transaction_storage
                .iter()
                .map(|packet| packet.immutable_section().original_packet().clone())
                .collect_vec();
            assert_eq!(3, deserialized_packets.len());
            assert!(deserialized_packets.contains(&small_transfer));
            assert!(deserialized_packets.contains(&vote));
            assert!(deserialized_packets.contains(&big_transfer));
        }

        for vote_source in [VoteSource::Gossip, VoteSource::Tpu] {
            let mut transaction_storage = UnprocessedTransactionStorage::new_vote_storage(
                Arc::new(LatestUnprocessedVotes::new()),
                vote_source,
            );
            transaction_storage.insert_batch(vec![
                ImmutableDeserializedPacket::new(small_transfer.clone(), None)?,
                ImmutableDeserializedPacket::new(vote.clone(), None)?,
                ImmutableDeserializedPacket::new(big_transfer.clone(), None)?,
            ]);
            assert_eq!(1, transaction_storage.len());
        }
        Ok(())
    }
}
