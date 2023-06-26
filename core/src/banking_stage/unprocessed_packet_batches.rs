use {
    super::immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket},
    min_max_heap::MinMaxHeap,
    solana_perf::packet::Packet,
    solana_sdk::hash::Hash,
    std::{
        cmp::Ordering,
        collections::{hash_map::Entry, HashMap},
        sync::Arc,
    },
};

/// Holds deserialized messages, as well as computed message_hash and other things needed to create
/// SanitizedTransaction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializedPacket {
    immutable_section: Arc<ImmutableDeserializedPacket>,
    pub forwarded: bool,
}

impl DeserializedPacket {
    pub fn from_immutable_section(immutable_section: ImmutableDeserializedPacket) -> Self {
        Self {
            immutable_section: Arc::new(immutable_section),
            forwarded: false,
        }
    }

    pub fn new(packet: Packet) -> Result<Self, DeserializedPacketError> {
        let immutable_section = ImmutableDeserializedPacket::new(packet)?;

        Ok(Self {
            immutable_section: Arc::new(immutable_section),
            forwarded: false,
        })
    }

    pub fn immutable_section(&self) -> &Arc<ImmutableDeserializedPacket> {
        &self.immutable_section
    }
}

impl PartialOrd for DeserializedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DeserializedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        self.immutable_section()
            .priority()
            .cmp(&other.immutable_section().priority())
    }
}

#[derive(Debug)]
pub struct PacketBatchInsertionMetrics {
    pub(crate) num_dropped_packets: usize,
    pub(crate) num_dropped_tracer_packets: usize,
}

/// Currently each banking_stage thread has a `UnprocessedPacketBatches` buffer to store
/// PacketBatch's received from sigverify. Banking thread continuously scans the buffer
/// to pick proper packets to add to the block.
#[derive(Debug, Default)]
pub struct UnprocessedPacketBatches {
    pub packet_priority_queue: MinMaxHeap<Arc<ImmutableDeserializedPacket>>,
    pub message_hash_to_transaction: HashMap<Hash, DeserializedPacket>,
    batch_limit: usize,
}

impl UnprocessedPacketBatches {
    pub fn from_iter<I: IntoIterator<Item = DeserializedPacket>>(iter: I, capacity: usize) -> Self {
        let mut unprocessed_packet_batches = Self::with_capacity(capacity);
        for deserialized_packet in iter.into_iter() {
            unprocessed_packet_batches.push(deserialized_packet);
        }

        unprocessed_packet_batches
    }

    pub fn with_capacity(capacity: usize) -> Self {
        UnprocessedPacketBatches {
            packet_priority_queue: MinMaxHeap::with_capacity(capacity),
            message_hash_to_transaction: HashMap::with_capacity(capacity),
            batch_limit: capacity,
        }
    }

    pub fn clear(&mut self) {
        self.packet_priority_queue.clear();
        self.message_hash_to_transaction.clear();
    }

    /// Insert new `deserialized_packet_batch` into inner `MinMaxHeap<DeserializedPacket>`,
    /// ordered by the tx priority.
    /// If buffer is at the max limit, the lowest priority packet is dropped
    ///
    /// Returns tuple of number of packets dropped
    pub fn insert_batch(
        &mut self,
        deserialized_packets: impl Iterator<Item = DeserializedPacket>,
    ) -> PacketBatchInsertionMetrics {
        let mut num_dropped_packets = 0;
        let mut num_dropped_tracer_packets = 0;
        for deserialized_packet in deserialized_packets {
            if let Some(dropped_packet) = self.push(deserialized_packet) {
                num_dropped_packets += 1;
                if dropped_packet
                    .immutable_section()
                    .original_packet()
                    .meta()
                    .is_tracer_packet()
                {
                    num_dropped_tracer_packets += 1;
                }
            }
        }
        PacketBatchInsertionMetrics {
            num_dropped_packets,
            num_dropped_tracer_packets,
        }
    }

    /// Pushes a new `deserialized_packet` into the unprocessed packet batches if it does not already
    /// exist.
    ///
    /// Returns and drops the lowest priority packet if the buffer is at capacity.
    pub fn push(&mut self, deserialized_packet: DeserializedPacket) -> Option<DeserializedPacket> {
        if self
            .message_hash_to_transaction
            .contains_key(deserialized_packet.immutable_section().message_hash())
        {
            return None;
        }

        if self.len() == self.batch_limit {
            // Optimized to not allocate by calling `MinMaxHeap::push_pop_min()`
            Some(self.push_pop_min(deserialized_packet))
        } else {
            self.push_internal(deserialized_packet);
            None
        }
    }

    pub fn iter(&mut self) -> impl Iterator<Item = &DeserializedPacket> {
        self.message_hash_to_transaction.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut DeserializedPacket> {
        self.message_hash_to_transaction.iter_mut().map(|(_k, v)| v)
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut DeserializedPacket) -> bool,
    {
        // TODO: optimize this only when number of packets
        // with outdated blockhash is high
        let new_packet_priority_queue: MinMaxHeap<Arc<ImmutableDeserializedPacket>> = self
            .packet_priority_queue
            .drain()
            .filter(|immutable_packet| {
                match self
                    .message_hash_to_transaction
                    .entry(*immutable_packet.message_hash())
                {
                    Entry::Vacant(_vacant_entry) => {
                        panic!(
                            "entry {} must exist to be consistent with `packet_priority_queue`",
                            immutable_packet.message_hash()
                        );
                    }
                    Entry::Occupied(mut occupied_entry) => {
                        let should_retain = f(occupied_entry.get_mut());
                        if !should_retain {
                            occupied_entry.remove_entry();
                        }
                        should_retain
                    }
                }
            })
            .collect();
        self.packet_priority_queue = new_packet_priority_queue;
    }

    pub fn len(&self) -> usize {
        self.packet_priority_queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packet_priority_queue.is_empty()
    }

    fn push_internal(&mut self, deserialized_packet: DeserializedPacket) {
        // Push into the priority queue
        self.packet_priority_queue
            .push(deserialized_packet.immutable_section().clone());

        // Keep track of the original packet in the tracking hashmap
        self.message_hash_to_transaction.insert(
            *deserialized_packet.immutable_section().message_hash(),
            deserialized_packet,
        );
    }

    /// Returns the popped minimum packet from the priority queue.
    fn push_pop_min(&mut self, deserialized_packet: DeserializedPacket) -> DeserializedPacket {
        let immutable_packet = deserialized_packet.immutable_section().clone();

        // Push into the priority queue
        let popped_immutable_packet = self.packet_priority_queue.push_pop_min(immutable_packet);

        if popped_immutable_packet.message_hash()
            != deserialized_packet.immutable_section().message_hash()
        {
            // Remove the popped entry from the tracking hashmap. Unwrap call is safe
            // because the priority queue and hashmap are kept consistent at all times.
            let removed_min = self
                .message_hash_to_transaction
                .remove(popped_immutable_packet.message_hash())
                .unwrap();

            // Keep track of the original packet in the tracking hashmap
            self.message_hash_to_transaction.insert(
                *deserialized_packet.immutable_section().message_hash(),
                deserialized_packet,
            );
            removed_min
        } else {
            deserialized_packet
        }
    }

    #[cfg(test)]
    fn pop_max(&mut self) -> Option<DeserializedPacket> {
        self.packet_priority_queue
            .pop_max()
            .map(|immutable_packet| {
                self.message_hash_to_transaction
                    .remove(immutable_packet.message_hash())
                    .unwrap()
            })
    }

    /// Pop up to the next `n` highest priority transactions from the queue.
    /// Returns `None` if the queue is empty
    #[cfg(test)]
    fn pop_max_n(&mut self, n: usize) -> Option<Vec<DeserializedPacket>> {
        let current_len = self.len();
        if self.is_empty() {
            None
        } else {
            let num_to_pop = std::cmp::min(current_len, n);
            Some(
                std::iter::from_fn(|| Some(self.pop_max().unwrap()))
                    .take(num_to_pop)
                    .collect::<Vec<DeserializedPacket>>(),
            )
        }
    }

    pub fn capacity(&self) -> usize {
        self.packet_priority_queue.capacity()
    }

    pub fn is_forwarded(&self, immutable_packet: &ImmutableDeserializedPacket) -> bool {
        self.message_hash_to_transaction
            .get(immutable_packet.message_hash())
            .map_or(true, |p| p.forwarded)
    }

    pub fn mark_accepted_packets_as_forwarded(
        &mut self,
        packets_to_process: &[Arc<ImmutableDeserializedPacket>],
        accepted_packet_indexes: &[usize],
    ) {
        accepted_packet_indexes
            .iter()
            .for_each(|accepted_packet_index| {
                let accepted_packet = packets_to_process[*accepted_packet_index].clone();
                if let Some(deserialized_packet) = self
                    .message_hash_to_transaction
                    .get_mut(accepted_packet.message_hash())
                {
                    deserialized_packet.forwarded = true;
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_perf::packet::PacketFlags,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            message::Message,
            signature::{Keypair, Signer},
            system_instruction, system_transaction,
            transaction::{SimpleAddressLoader, Transaction},
        },
        solana_vote_program::vote_transaction,
        std::sync::Arc,
    };

    fn simple_deserialized_packet() -> DeserializedPacket {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, tx).unwrap();
        DeserializedPacket::new(packet).unwrap()
    }

    fn packet_with_priority_details(priority: u64, compute_unit_limit: u64) -> DeserializedPacket {
        let from_account = solana_sdk::pubkey::new_rand();
        let tx = Transaction::new_unsigned(Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(compute_unit_limit as u32),
                ComputeBudgetInstruction::set_compute_unit_price(priority),
                system_instruction::transfer(&from_account, &solana_sdk::pubkey::new_rand(), 1),
            ],
            Some(&from_account),
        ));
        DeserializedPacket::new(Packet::from_data(None, tx).unwrap()).unwrap()
    }

    #[test]
    fn test_unprocessed_packet_batches_insert_pop_same_packet() {
        let packet = simple_deserialized_packet();
        let mut unprocessed_packet_batches = UnprocessedPacketBatches::with_capacity(2);
        unprocessed_packet_batches.push(packet.clone());
        unprocessed_packet_batches.push(packet.clone());

        // There was only one unique packet, so that one should be the
        // only packet returned
        assert_eq!(
            unprocessed_packet_batches.pop_max_n(2).unwrap(),
            vec![packet]
        );
    }

    #[test]
    fn test_unprocessed_packet_batches_insert_minimum_packet_over_capacity() {
        let heavier_packet_weight = 2;
        let heavier_packet = packet_with_priority_details(heavier_packet_weight, 200_000);

        let lesser_packet_weight = heavier_packet_weight - 1;
        let lesser_packet = packet_with_priority_details(lesser_packet_weight, 200_000);

        // Test that the heavier packet is actually heavier
        let mut unprocessed_packet_batches = UnprocessedPacketBatches::with_capacity(2);
        unprocessed_packet_batches.push(heavier_packet.clone());
        unprocessed_packet_batches.push(lesser_packet.clone());
        assert_eq!(
            unprocessed_packet_batches.pop_max().unwrap(),
            heavier_packet
        );

        let mut unprocessed_packet_batches = UnprocessedPacketBatches::with_capacity(1);
        unprocessed_packet_batches.push(heavier_packet);

        // Buffer is now at capacity, pushing the smaller weighted
        // packet should immediately pop it
        assert_eq!(
            unprocessed_packet_batches
                .push(lesser_packet.clone())
                .unwrap(),
            lesser_packet
        );
    }

    #[test]
    fn test_unprocessed_packet_batches_pop_max_n() {
        let num_packets = 10;
        let packets_iter = std::iter::repeat_with(simple_deserialized_packet).take(num_packets);
        let mut unprocessed_packet_batches =
            UnprocessedPacketBatches::from_iter(packets_iter.clone(), num_packets);

        // Test with small step size
        let step_size = 1;
        for _ in 0..num_packets {
            assert_eq!(
                unprocessed_packet_batches
                    .pop_max_n(step_size)
                    .unwrap()
                    .len(),
                step_size
            );
        }

        assert!(unprocessed_packet_batches.is_empty());
        assert!(unprocessed_packet_batches.pop_max_n(0).is_none());
        assert!(unprocessed_packet_batches.pop_max_n(1).is_none());

        // Test with step size larger than `num_packets`
        let step_size = num_packets + 1;
        let mut unprocessed_packet_batches =
            UnprocessedPacketBatches::from_iter(packets_iter.clone(), num_packets);
        assert_eq!(
            unprocessed_packet_batches
                .pop_max_n(step_size)
                .unwrap()
                .len(),
            num_packets
        );
        assert!(unprocessed_packet_batches.is_empty());
        assert!(unprocessed_packet_batches.pop_max_n(0).is_none());

        // Test with step size equal to `num_packets`
        let step_size = num_packets;
        let mut unprocessed_packet_batches =
            UnprocessedPacketBatches::from_iter(packets_iter, num_packets);
        assert_eq!(
            unprocessed_packet_batches
                .pop_max_n(step_size)
                .unwrap()
                .len(),
            step_size
        );
        assert!(unprocessed_packet_batches.is_empty());
        assert!(unprocessed_packet_batches.pop_max_n(0).is_none());
    }

    #[cfg(test)]
    fn make_test_packets(
        transactions: Vec<Transaction>,
        vote_indexes: Vec<usize>,
    ) -> Vec<DeserializedPacket> {
        let capacity = transactions.len();
        let mut packet_vector = Vec::with_capacity(capacity);
        for tx in transactions.iter() {
            packet_vector.push(Packet::from_data(None, tx).unwrap());
        }
        for index in vote_indexes.iter() {
            packet_vector[*index].meta_mut().flags |= PacketFlags::SIMPLE_VOTE_TX;
        }

        packet_vector
            .into_iter()
            .map(|p| DeserializedPacket::new(p).unwrap())
            .collect()
    }

    #[test]
    fn test_transaction_from_deserialized_packet() {
        use solana_sdk::feature_set::FeatureSet;
        let keypair = Keypair::new();
        let transfer_tx =
            system_transaction::transfer(&keypair, &keypair.pubkey(), 1, Hash::default());
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![42],
            Hash::default(),
            Hash::default(),
            &keypair,
            &keypair,
            &keypair,
            None,
        );

        // packets with no votes
        {
            let vote_indexes = vec![];
            let packet_vector =
                make_test_packets(vec![transfer_tx.clone(), transfer_tx.clone()], vote_indexes);

            let mut votes_only = false;
            let txs = packet_vector.iter().filter_map(|tx| {
                tx.immutable_section().build_sanitized_transaction(
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(2, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                tx.immutable_section().build_sanitized_transaction(
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(0, txs.count());
        }

        // packets with some votes
        {
            let vote_indexes = vec![0, 2];
            let packet_vector = make_test_packets(
                vec![vote_tx.clone(), transfer_tx, vote_tx.clone()],
                vote_indexes,
            );

            let mut votes_only = false;
            let txs = packet_vector.iter().filter_map(|tx| {
                tx.immutable_section().build_sanitized_transaction(
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                tx.immutable_section().build_sanitized_transaction(
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(2, txs.count());
        }

        // packets with all votes
        {
            let vote_indexes = vec![0, 1, 2];
            let packet_vector = make_test_packets(
                vec![vote_tx.clone(), vote_tx.clone(), vote_tx],
                vote_indexes,
            );

            let mut votes_only = false;
            let txs = packet_vector.iter().filter_map(|tx| {
                tx.immutable_section().build_sanitized_transaction(
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                tx.immutable_section().build_sanitized_transaction(
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());
        }
    }
}
