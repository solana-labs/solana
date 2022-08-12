use {
    crate::{
        immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket},
        transaction_priority_details::TransactionPriorityDetails,
    },
    min_max_heap::MinMaxHeap,
    solana_perf::packet::{Packet, PacketBatch},
    solana_sdk::{
        feature_set,
        hash::Hash,
        transaction::{AddressLoader, SanitizedTransaction, Transaction},
    },
    std::{
        cmp::Ordering,
        collections::{hash_map::Entry, HashMap},
        rc::Rc,
        sync::Arc,
    },
};

/// Holds deserialized messages, as well as computed message_hash and other things needed to create
/// SanitizedTransaction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializedPacket {
    immutable_section: Rc<ImmutableDeserializedPacket>,
    pub forwarded: bool,
}

impl DeserializedPacket {
    pub fn from_immutable_section(immutable_section: ImmutableDeserializedPacket) -> Self {
        Self {
            immutable_section: Rc::new(immutable_section),
            forwarded: false,
        }
    }

    pub fn new(packet: Packet) -> Result<Self, DeserializedPacketError> {
        Self::new_internal(packet, None)
    }

    #[cfg(test)]
    pub fn new_with_priority_details(
        packet: Packet,
        priority_details: TransactionPriorityDetails,
    ) -> Result<Self, DeserializedPacketError> {
        Self::new_internal(packet, Some(priority_details))
    }

    pub fn new_internal(
        packet: Packet,
        priority_details: Option<TransactionPriorityDetails>,
    ) -> Result<Self, DeserializedPacketError> {
        let immutable_section = ImmutableDeserializedPacket::new(packet, priority_details)?;

        Ok(Self {
            immutable_section: Rc::new(immutable_section),
            forwarded: false,
        })
    }

    pub fn immutable_section(&self) -> &Rc<ImmutableDeserializedPacket> {
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

/// Currently each banking_stage thread has a `UnprocessedPacketBatches` buffer to store
/// PacketBatch's received from sigverify. Banking thread continuously scans the buffer
/// to pick proper packets to add to the block.
#[derive(Default)]
pub struct UnprocessedPacketBatches {
    pub packet_priority_queue: MinMaxHeap<Rc<ImmutableDeserializedPacket>>,
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
    ) -> (usize, usize) {
        let mut num_dropped_packets = 0;
        let mut num_dropped_tracer_packets = 0;
        for deserialized_packet in deserialized_packets {
            if let Some(dropped_packet) = self.push(deserialized_packet) {
                num_dropped_packets += 1;
                if dropped_packet
                    .immutable_section()
                    .original_packet()
                    .meta
                    .is_tracer_packet()
                {
                    num_dropped_tracer_packets += 1;
                }
            }
        }
        (num_dropped_packets, num_dropped_tracer_packets)
    }

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

    /// Iterates DeserializedPackets in descending priority (max-first) order,
    /// calls FnMut for each DeserializedPacket.
    pub fn iter_desc<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut DeserializedPacket) -> bool,
    {
        let mut packet_priority_queue_clone = self.packet_priority_queue.clone();

        for immutable_packet in packet_priority_queue_clone.drain_desc() {
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
                    if !f(occupied_entry.get_mut()) {
                        return;
                    }
                }
            }
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut DeserializedPacket) -> bool,
    {
        // TODO: optimize this only when number of packets
        // with outdated blockhash is high
        let new_packet_priority_queue: MinMaxHeap<Rc<ImmutableDeserializedPacket>> = self
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

    pub fn pop_max(&mut self) -> Option<DeserializedPacket> {
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
    pub fn pop_max_n(&mut self, n: usize) -> Option<Vec<DeserializedPacket>> {
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
}

pub fn deserialize_packets<'a>(
    packet_batch: &'a PacketBatch,
    packet_indexes: &'a [usize],
) -> impl Iterator<Item = DeserializedPacket> + 'a {
    packet_indexes.iter().filter_map(move |packet_index| {
        DeserializedPacket::new(packet_batch[*packet_index].clone()).ok()
    })
}

pub fn transactions_to_deserialized_packets(
    transactions: &[Transaction],
) -> Result<Vec<DeserializedPacket>, DeserializedPacketError> {
    transactions
        .iter()
        .map(|transaction| {
            let packet = Packet::from_data(None, transaction)?;
            DeserializedPacket::new(packet)
        })
        .collect()
}

// This function deserializes packets into transactions, computes the blake3 hash of transaction
// messages, and verifies secp256k1 instructions. A list of sanitized transactions are returned
// with their packet indexes.
#[allow(clippy::needless_collect)]
pub fn transaction_from_deserialized_packet(
    deserialized_packet: &ImmutableDeserializedPacket,
    feature_set: &Arc<feature_set::FeatureSet>,
    votes_only: bool,
    address_loader: impl AddressLoader,
) -> Option<SanitizedTransaction> {
    if votes_only && !deserialized_packet.is_simple_vote() {
        return None;
    }

    let tx = SanitizedTransaction::try_new(
        deserialized_packet.transaction().clone(),
        *deserialized_packet.message_hash(),
        deserialized_packet.is_simple_vote(),
        address_loader,
    )
    .ok()?;
    tx.verify_precompiles(feature_set).ok()?;
    Some(tx)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_perf::packet::PacketFlags,
        solana_sdk::{
            signature::{Keypair, Signer},
            system_transaction,
            transaction::{SimpleAddressLoader, Transaction},
        },
        solana_vote_program::vote_transaction,
    };

    fn simple_deserialized_packet() -> DeserializedPacket {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, &tx).unwrap();
        DeserializedPacket::new(packet).unwrap()
    }

    fn packet_with_priority_details(priority: u64, compute_unit_limit: u64) -> DeserializedPacket {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, &tx).unwrap();
        DeserializedPacket::new_with_priority_details(
            packet,
            TransactionPriorityDetails {
                priority,
                compute_unit_limit,
            },
        )
        .unwrap()
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
            packet_vector.push(Packet::from_data(None, &tx).unwrap());
        }
        for index in vote_indexes.iter() {
            packet_vector[*index].meta.flags |= PacketFlags::SIMPLE_VOTE_TX;
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
                transaction_from_deserialized_packet(
                    tx.immutable_section(),
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(2, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx.immutable_section(),
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
                transaction_from_deserialized_packet(
                    tx.immutable_section(),
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx.immutable_section(),
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
                transaction_from_deserialized_packet(
                    tx.immutable_section(),
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx.immutable_section(),
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());
        }
    }
}
