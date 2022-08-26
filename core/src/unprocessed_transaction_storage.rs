use {
    crate::{
        banking_stage::{
            weighted_random_order_by_stake, BankingStage, FilterForwardingResults, ForwardOption,
        },
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        latest_unprocessed_votes::{
            self, DeserializedVotePacket, LatestUnprocessedVotes, VoteSource,
        },
        unprocessed_packet_batches::{self, DeserializedPacket, UnprocessedPacketBatches},
    },
    itertools::Itertools,
    min_max_heap::MinMaxHeap,
    solana_perf::packet::PacketBatch,
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::feature_set::{allow_votes_to_directly_update_vote_state, split_banking_threads},
    std::{
        rc::Rc,
        sync::{atomic::Ordering, Arc, RwLock},
    },
};

const MAX_STAKED_VALIDATORS: usize = 10_000;

#[derive(Debug, Default)]
pub struct InsertPacketBatchesSummary {
    pub(crate) num_dropped_packets: usize,
    pub(crate) num_dropped_gossip_vote_packets: usize,
    pub(crate) num_dropped_tpu_vote_packets: usize,
    pub(crate) num_dropped_tracer_packets: usize,
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
pub struct VoteStorage {
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
        if bank.is_none() {
            // Nothing to process
            return;
        }
        let bank = bank.unwrap();

        let latest_votes_per_pubkey = self
            .latest_unprocessed_votes
            .latest_votes_per_pubkey
            .read()
            .unwrap();

        let retryable_votes = weighted_random_order_by_stake(&bank, latest_votes_per_pubkey.keys())
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
pub struct TransactionStorage {
    unprocessed_packet_batches: UnprocessedPacketBatches,
    thread_type: ThreadType,
    hot_swap: Option<Arc<LatestUnprocessedVotes>>,
}

impl TransactionStorage {
    fn is_empty(&self) -> bool {
        self.unprocessed_packet_batches.is_empty()
    }

    pub fn thread_type(&self) -> ThreadType {
        self.thread_type
    }

    fn len(&self) -> usize {
        self.unprocessed_packet_batches.len()
    }

    fn capacity(&self) -> usize {
        self.unprocessed_packet_batches.capacity()
    }

    fn hotswap(mut self) -> VoteStorage {
        if let ThreadType::Voting(vote_source) = self.thread_type {
            if let Some(latest_unprocessed_votes) = self.hot_swap {
                let vote_storage = VoteStorage {
                    latest_unprocessed_votes,
                    vote_source,
                };
                // Move everything over
                let deserialized_vote_packets = self
                    .unprocessed_packet_batches
                    .message_hash_to_transaction
                    .drain()
                    .filter_map(
                        |(
                            _,
                            DeserializedPacket {
                                immutable_section, ..
                            },
                        )| {
                            DeserializedVotePacket::new_from_immutable(
                                immutable_section,
                                vote_source,
                            )
                            .ok()
                        },
                    );
                vote_storage
                    .latest_unprocessed_votes
                    .insert_batch(deserialized_vote_packets);
                return vote_storage;
            }
        }
        panic!("Hotswap failed, manual intervention required")
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
    pub fn new_transaction_storage(
        unprocessed_packet_batches: UnprocessedPacketBatches,
        thread_type: ThreadType,
        hot_swap: Option<Arc<LatestUnprocessedVotes>>,
    ) -> Self {
        Self::TransactionStorage(TransactionStorage {
            unprocessed_packet_batches,
            thread_type,
            hot_swap,
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
        processing_function: F,
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

    pub fn maybe_hot_swap(self, bank_forks: &Arc<RwLock<BankForks>>) -> (bool, Self) {
        match self {
            Self::TransactionStorage(transaction_storage) => {
                if let ThreadType::Voting(_) = transaction_storage.thread_type() {
                    // Check to see if flag was activated
                    if let Ok(bank_forks) = bank_forks.read() {
                        let bank = bank_forks.root_bank();
                        if bank
                            .feature_set
                            .is_active(&allow_votes_to_directly_update_vote_state::id())
                            && bank.feature_set.is_active(&split_banking_threads::id())
                        {
                            // Met all the conditions, time to hotswap
                            return (true, Self::VoteStorage(transaction_storage.hotswap()));
                        }
                    }
                }
                (false, Self::TransactionStorage(transaction_storage))
            }
            vote_storage => (false, vote_storage),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        ::solana_runtime::genesis_utils::{activate_feature, create_genesis_config_with_leader_ex},
        solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo},
        solana_perf::packet::{Packet, PacketFlags},
        solana_sdk::{
            fee_calculator::FeeRateGovernor,
            genesis_config::ClusterType,
            hash::Hash,
            rent::Rent,
            signature::{Keypair, Signer},
            system_transaction,
        },
        solana_vote_program::{
            vote_state::VoteStateUpdate, vote_transaction::new_vote_state_update_transaction,
        },
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
    fn test_unprocessed_transaction_storage_deserialize_and_insert() {
        let keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let pubkey = solana_sdk::pubkey::new_rand();

        let small_transfer = Packet::from_data(
            None,
            system_transaction::transfer(&keypair, &pubkey, 1, Hash::new_unique()),
        )
        .unwrap();
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
        )
        .unwrap();
        vote.meta.flags.set(PacketFlags::SIMPLE_VOTE_TX, true);
        let big_transfer = Packet::from_data(
            None,
            system_transaction::transfer(&keypair, &pubkey, 1000000, Hash::new_unique()),
        )
        .unwrap();

        let packet_batch = PacketBatch::new(vec![
            small_transfer.clone(),
            vote.clone(),
            big_transfer.clone(),
        ]);

        for thread_type in [
            ThreadType::Transactions,
            ThreadType::Voting(VoteSource::Gossip),
            ThreadType::Voting(VoteSource::Tpu),
        ] {
            let mut transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
                UnprocessedPacketBatches::with_capacity(100),
                thread_type,
                None,
            );
            transaction_storage
                .deserialize_and_insert_batch(&packet_batch, &(0..3_usize).collect_vec());
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
            transaction_storage
                .deserialize_and_insert_batch(&packet_batch, &(0..3_usize).collect_vec());
            assert_eq!(1, transaction_storage.len());
        }
    }

    #[test]
    fn test_unprocessed_transaction_storage_maybe_hotswap() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(10000);
        activate_feature(
            &mut genesis_config,
            allow_votes_to_directly_update_vote_state::id(),
        );
        activate_feature(&mut genesis_config, split_banking_threads::id());
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let all_active_bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let mint_keypair = Keypair::new();
        let voting_keypair = Keypair::new();
        let genesis_config = create_genesis_config_with_leader_ex(
            10000,
            &mint_keypair.pubkey(),
            &solana_sdk::pubkey::new_rand(),
            &voting_keypair.pubkey(),
            &solana_sdk::pubkey::new_rand(),
            100,
            42,
            FeeRateGovernor::new(0, 0),
            Rent::free(),
            ClusterType::MainnetBeta,
            vec![],
        );
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let no_active_bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        // Even with all features active vote storage should always be vote storage
        let vote_storage = UnprocessedTransactionStorage::new_vote_storage(
            Arc::new(LatestUnprocessedVotes::new()),
            VoteSource::Gossip,
        );
        assert!(!vote_storage.maybe_hot_swap(&all_active_bank_forks).0);
        let vote_storage = UnprocessedTransactionStorage::new_vote_storage(
            Arc::new(LatestUnprocessedVotes::new()),
            VoteSource::Tpu,
        );
        assert!(!vote_storage.maybe_hot_swap(&all_active_bank_forks).0);

        // Tx threads should never hot swap
        let transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
            UnprocessedPacketBatches::with_capacity(100),
            ThreadType::Transactions,
            None,
        );
        assert!(!transaction_storage.maybe_hot_swap(&all_active_bank_forks).0);
        let transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
            UnprocessedPacketBatches::with_capacity(100),
            ThreadType::Transactions,
            Some(Arc::new(LatestUnprocessedVotes::new())),
        );
        assert!(!transaction_storage.maybe_hot_swap(&all_active_bank_forks).0);

        // Voting threads shouldn't hot swap before features are activated
        let transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
            UnprocessedPacketBatches::with_capacity(100),
            ThreadType::Voting(VoteSource::Gossip),
            Some(Arc::new(LatestUnprocessedVotes::new())),
        );
        assert!(!transaction_storage.maybe_hot_swap(&no_active_bank_forks).0);
        let transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
            UnprocessedPacketBatches::with_capacity(100),
            ThreadType::Voting(VoteSource::Tpu),
            Some(Arc::new(LatestUnprocessedVotes::new())),
        );
        assert!(!transaction_storage.maybe_hot_swap(&no_active_bank_forks).0);

        // Voting threads hot swap and port unprocessed votes
        let keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let vote_hash = Hash::new_unique();
        let mut vote = Packet::from_data(
            None,
            new_vote_state_update_transaction(
                VoteStateUpdate::default(),
                vote_hash.clone(),
                &keypair,
                &vote_keypair,
                &vote_keypair,
                None,
            ),
        )
        .unwrap();
        vote.meta.flags.set(PacketFlags::SIMPLE_VOTE_TX, true);
        let packet_batch = PacketBatch::new(vec![vote.clone(), vote.clone(), vote.clone()]);
        for vote_source in [VoteSource::Gossip, VoteSource::Tpu] {
            let mut transaction_storage = UnprocessedTransactionStorage::new_transaction_storage(
                UnprocessedPacketBatches::with_capacity(100),
                ThreadType::Voting(vote_source),
                Some(Arc::new(LatestUnprocessedVotes::new())),
            );
            transaction_storage
                .deserialize_and_insert_batch(&packet_batch, &(0..3_usize).collect_vec());

            let (success, vote_storage) =
                transaction_storage.maybe_hot_swap(&all_active_bank_forks);
            assert!(success);
            // Only store latest so 3 votes -> 1
            assert_eq!(1, vote_storage.len());
        }
    }
}
