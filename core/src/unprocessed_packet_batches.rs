use {
    min_max_heap::MinMaxHeap,
    solana_perf::packet::{limited_deserialize, Packet, PacketBatch},
    solana_runtime::bank::Bank,
    solana_sdk::{
        hash::Hash,
        message::{Message, VersionedMessage},
        short_vec::decode_shortu16_len,
        signature::Signature,
        saturating_add_assign,
        transaction::{Transaction, VersionedTransaction},
    },
    std::{
        cell::RefCell,
        cmp::Ordering,
        collections::{HashMap, VecDeque},
        mem::size_of,
        rc::{Rc, Weak},
        sync::Arc
    },
};

#[derive(Debug, Default, PartialEq, Eq)]
struct ImmutableDeserializedPacket {
    packet_index: usize,
    versioned_transaction: VersionedTransaction,
    message_hash: Hash,
    is_simple_vote: bool,
    fee_per_cu: u64,
}

/// Holds deserialized messages, as well as computed message_hash and other things needed to create
/// SanitizedTransaction
#[derive(Debug, Default)]
pub struct DeserializedPacket {
    immutable_section: Rc<ImmutableDeserializedPacket>,
    pub forwarded: bool,
    // reference back to batch that has/owns this packet
    owner: Weak<RefCell<DeserializedPacketBatch>>,
}

impl DeserializedPacket {
    pub fn new(
        packet: &Packet,
        packet_index: &usize,
        bank: &Option<Arc<Bank>>, 
        owner: Weak<RefCell<DeserializedPacketBatch>>
    ) -> Option<Self> {
        Self::new_internal(packet, packet_index, bank, None, owner)
    }

    #[cfg(test)]
    fn new_with_fee_per_cu(
        packet: &Packet,
        packet_index: &usize,
        fee_per_cu: u64,
        owner: Weak<RefCell<DeserializedPacketBatch>>
    ) -> Option<Self> {
        Self::new_internal(packet, packet_index, &None, Some(fee_per_cu), owner)
    }

    pub fn new_internal(
        packet: &Packet,
        packet_index: &usize,
        bank: &Option<Arc<Bank>>,
        fee_per_cu: Option<u64>,
        owner: Weak<RefCell<DeserializedPacketBatch>>,
    ) -> Option<Self> {
        let versioned_transaction: VersionedTransaction =
            match limited_deserialize(&packet.data[0..packet.meta.size]) {
                Ok(tx) => tx,
                Err(_) => return None,
            };

        if let Some(message_bytes) = packet_message(packet) {
            let message_hash = Message::hash_raw_message(message_bytes);
            let is_simple_vote = packet.meta.is_simple_vote_tx();

            let fee_per_cu = fee_per_cu.unwrap_or_else(|| {
                bank.as_ref()
                    .map(|bank| compute_fee_per_cu(&versioned_transaction.message, &*bank))
                    .unwrap_or(0)
            });
            Some(Self {
                immutable_section: Rc::new(ImmutableDeserializedPacket {
                    packet_index: *packet_index,
                    versioned_transaction,
                    message_hash,
                    is_simple_vote,
                    fee_per_cu,
                }),
                forwarded: false,
                owner,
            })
        } else {
            None
        }
    }

    /* TODO - to figure out how to return &Packet from borrowed RefCell
    pub fn original_packet(&self) -> &Packet {
        let deserialized_packet_batch = self.owner.upgrade().unwrap();
        &deserialized_packet_batch.borrow().original_packet_batch.packets[self.immutable_section.packet_index]
    }
    // */

    pub fn packet_index(&self) -> usize {
        self.immutable_section.packet_index
    }

    pub fn is_simple_vote_transaction(&self) -> bool {
        self.immutable_section.is_simple_vote
    }

    pub fn versioned_transaction(&self) -> &VersionedTransaction {
        &self.immutable_section.versioned_transaction
    }

    pub fn sender_stake(&self) -> u64 {
        /* TODO - need to figure out `original_packet()` 
        self.original_packet().meta.sender_stake
        // */
        1
    }

    pub fn message_hash(&self) -> Hash {
        self.immutable_section.message_hash
    }

    pub fn fee_per_cu(&self) -> u64 {
        self.immutable_section.fee_per_cu
    }
}

impl PartialOrd for DeserializedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DeserializedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.fee_per_cu().cmp(&other.fee_per_cu()) {
            Ordering::Equal => self.sender_stake().cmp(&other.sender_stake()),
            ordering => ordering,
        }
    }
}

impl PartialEq for DeserializedPacket {
    fn eq(&self, other: &Self) -> bool {
        self.fee_per_cu() == other.fee_per_cu() &&
        self.sender_stake() == other.sender_stake()
    }
}

impl Eq for DeserializedPacket {}

#[derive(Debug, Default)]
pub struct DeserializedPacketBatch {
    original_packet_batch: PacketBatch,
    // For each unprocessed packet in original_packet_batch, deserialize
    // and store with its packet_index as key
    deserialized_packets: HashMap<usize, Rc<DeserializedPacket>>,
}

impl DeserializedPacketBatch {

    /// New from original PacketBatch, 
    /// deserialized_packet_batch takes ownership of original packet_batch, for each packet in
    /// it, does:
    /// 1. deserialize packet to versioned_transaction
    /// 2. establish parent/children relationship between deserialized_packet_batch and 
    ///    deserialized_packets;
    /// 3. update packet_priority_queue
    pub fn new_from(
        packet_batch: PacketBatch,
        packet_indexes: &[usize],
        bank: &Option<Arc<Bank>>,
        priority_queue: &mut MinMaxHeap<Rc<DeserializedPacket>>,
    ) -> Rc<RefCell<Self>> {
        let batch = Rc::new(RefCell::new(Self::default()));
        (*batch.borrow_mut()).deserialized_packets =
            packet_indexes
                .iter()
                .filter_map(|packet_index| {
                    let deserialized_packet =
                        DeserializedPacket::new(
                            &packet_batch.packets[*packet_index],
                            packet_index,
                            bank,
                            Rc::downgrade(&batch.clone()))?;
                    let packet = Rc::new(deserialized_packet);
                    // update packet priority_queue on insertion
                    priority_queue.push(Rc::clone(&packet));
                    Some((*packet_index, packet))
                })
                .collect();
        // take ownership of original packet_batch
        (*batch.borrow_mut()).original_packet_batch = packet_batch;
        batch
    }

    /// Is empty if the batch has no more unprocessed packets
    pub fn is_empty(&self) -> bool {
        self.deserialized_packets.is_empty()
    }
}

pub trait PacketBuffer {
    fn insert_batch(
        &mut self,
        packet_batch: PacketBatch,
        packet_indexes: &[usize],
        bank: &Option<Arc<Bank>>,
    ) -> usize;

    fn pop_max_n(&mut self, n: usize) -> Option<Vec<Rc<DeserializedPacket>>>;
    fn insert_packets(&mut self, packets: &[Rc<DeserializedPacket>]);

    fn len(&self) -> usize;
    fn capacity(&self) -> usize;

    fn clear(&mut self);
    fn is_empty(&self) -> bool;
}

/// Currently each banking_stage thread has a `UnprocessedPacketBatches` buffer to store
/// PacketBatch's received from sigverify. Banking thread continuously scans the buffer
/// to pick proper packets to add to the block.
#[derive(Default)]
pub struct UnprocessedPacketBatches {
    packet_priority_queue: MinMaxHeap<Rc<DeserializedPacket>>,
    packet_batches: VecDeque<Rc<RefCell<DeserializedPacketBatch>>>,
    batch_limit: usize,
}

impl PacketBuffer for UnprocessedPacketBatches {

    /// Insert new `deserizlized_packet_batch` into inner `packet_batches`,
    /// Also add ref of each packet in the batch to `packet_priority_queue`,
    /// weighted first by the fee-per-cu, then the stake of the sender.
    /// If buffer is at the max limit, the lowest weighted packet is dropped
    ///
    /// Returns tuple of number of packets dropped
    fn insert_batch(
        &mut self,
        packet_batch: PacketBatch,
        packet_indexes: &[usize],
        bank: &Option<Arc<Bank>>,
    ) -> usize {
        // deserialized_packet_batch takes ownership of original packet_batch, for each packet in
        // it, does:
        // 1. deserialize packet to versioned_transaction
        // 2. establish parent/children relationship between deserialized_packet_batch and 
        //    deserialized_packets;
        // 3. update packet_priority_queue
        let deserialized_packet_batch = DeserializedPacketBatch::new_from(
            packet_batch,
            packet_indexes,
            bank,
            &mut self.packet_priority_queue);

        // insert new deserialized_packet_batch into self.packet_batches, drop low priority packets
        // to free spot for batch
        if deserialized_packet_batch.borrow().is_empty() {
            return 0;
        }
        // new batch are subject to the same rule to push-out low priority packets if capacity is
        // exceeded
        self.packet_batches.push_back(deserialized_packet_batch);
        let number_batches_to_remove = self.len().saturating_sub(self.batch_limit);
        if number_batches_to_remove > 0 {
            let (number_packets_dropped, _number_batches_dropped) =
                self.pop_min_batches_n(number_batches_to_remove);
            number_packets_dropped
        }
        else {
            0
        }
    }

    /// Pop the next `n` highest priority transactions from the queue.
    /// Returns `None` if the queue is empty
    ///
    /// Note - the pop_max work flow should be:
    ///  1. pop_max_n() -> vec<Rc<DeserializedPacket>> that pops Rc<DeserializedPacket> from
    ///     priority_queue, and remove them from Batch's unprocessed list; Note the buffer is not
    ///     purged for empty batch, cause retryable packets are expected to be reinstated. 
    ///     priority_queue is out of sync with packet_batches storage, until step #3 is done.
    ///  2. call site do business with Vec<Rc<DeserializedPacket>>, end with a collection of
    ///     retryable Rc<DeserializedPacket>s, 
    ///  3. add retryable packets to their batches unprocessed list, also insert back into
    ///     priority_queue. At the end, empty batches are purged from buffer.
    /// NOte: the workflow could be cleaner if Priority_queue support peek_max_n(). 
    fn pop_max_n(&mut self, n: usize) -> Option<Vec<Rc<DeserializedPacket>>> {
        let current_len = self.len();
        if current_len == 0 {
            None
        } else {
            let num_to_pop = std::cmp::min(current_len, n);
            Some(
                std::iter::repeat_with(|| self.pop_max().unwrap())
                    .take(num_to_pop)
                    .collect(),
            )
        }
    }

    /// insert Deserializedpackets that were returned from pop_max_n() back to buffer. 
    /// The `packet_batches` should not have changed, which means DeserializedPacket.owner should
    /// still be valid.
    fn insert_packets(&mut self, packets: &[Rc<DeserializedPacket>]) {
        for pkt in packets.iter() {
            let batch = pkt.owner.upgrade().unwrap();
            batch.borrow_mut().deserialized_packets.insert(pkt.packet_index(), Rc::clone(pkt));
            self.packet_priority_queue.push(Rc::clone(pkt));
        }
        // purge empty batches
        self.purge_empty_batch();
    }

    fn clear(&mut self) {
        self.packet_priority_queue.clear();
        self.packet_batches.clear();
    }

    // returns number of unprocessed packets in buffer
    fn len(&self) -> usize {
        self.packet_priority_queue.len()
    }

    fn is_empty(&self) -> bool {
        self.packet_priority_queue.is_empty()
    }
    fn capacity(&self) -> usize {
        self.packet_priority_queue.capacity()
    }
}

impl UnprocessedPacketBatches {
    pub fn with_capacity(capacity: usize) -> Self {
        UnprocessedPacketBatches {
            packet_priority_queue: MinMaxHeap::with_capacity(capacity),
            packet_batches: VecDeque::with_capacity(capacity),
            batch_limit: capacity,
        }
    }

    pub fn set_all_packets_forwarded(&mut self) {
        for batch in self.packet_batches.iter() {
            for (_k, deserialized_packet) in batch.borrow_mut().deserialized_packets.iter_mut() {
                if let Some(packet) = Rc::get_mut(deserialized_packet) {
                    packet.forwarded = true;
                }
            }
        }
    }

    pub fn iter(&mut self) -> impl Iterator<Item = &Rc<DeserializedPacket>> {
        self.packet_priority_queue.iter()
    }

    fn pop_max(&mut self) -> Option<Rc<DeserializedPacket>> {
        self.packet_priority_queue
            .pop_max()
            .map(|pkt| {
                // get the batch that owns the pkt
                let batch = pkt.owner.upgrade().unwrap();
                // remove packet from batch's unprocessed packet list
                let removed_packet = batch.borrow_mut().deserialized_packets
                    .remove(&pkt.packet_index())
                    .unwrap();
                removed_packet
            })
    }

    /// Utilizing existing priority packet index to efficiently drop low priority packets.
    /// Compare to other approach, at the time of drop batch, it does not need to do:
    /// 1. Scan and index buffer -- it is eagerly prepared at batch insertion;
    /// 2. Lookup batch to remove low priority packet from its unprocessed list.
    /// 3. Also added a option to drop multiple batches at a time to further improve efficiency.
    fn pop_min_batches_n(
        &mut self,
        num_batches_to_remove: usize,
    ) -> (usize, usize) {
        let mut removed_packet_count = 0usize;
        let mut removed_batch_count = 0usize;
        while let Some(pkt) = self.packet_priority_queue.pop_min() {
            debug!("popped min from index: {:?}",  pkt);

            // pkt is the lowest priority packet, its `owner` references back to its parent batch
            let batch = pkt.owner.upgrade().unwrap();
            // remove the low priority packet from batch's unprocessed packet list
            let _popped_packet = batch.borrow_mut().deserialized_packets.remove(&pkt.packet_index()).unwrap();
            saturating_add_assign!(removed_packet_count, 1);

            // be more efficient to remove multiple batches at one go
            if batch.borrow().is_empty() {
                saturating_add_assign!(removed_batch_count, 1);
                if removed_batch_count >= num_batches_to_remove {
                    break;
                }
            }
        }
        // still need to iterate through VecDeque buffer to remove empty batches
        self.purge_empty_batch();

        (removed_packet_count, removed_batch_count)
    }

    fn purge_empty_batch(&mut self) {
        self.packet_batches.retain(|batch| {
            !batch.borrow().is_empty()
        });
    }
}

/// Read the transaction message from packet data
pub fn packet_message(packet: &Packet) -> Option<&[u8]> {
    let (sig_len, sig_size) = decode_shortu16_len(&packet.data).ok()?;
    let msg_start = sig_len
        .checked_mul(size_of::<Signature>())
        .and_then(|v| v.checked_add(sig_size))?;
    let msg_end = packet.meta.size;
    Some(&packet.data[msg_start..msg_end])
}

/// Computes `(addition_fee + base_fee / requested_cu)` for `deserialized_packet`
fn compute_fee_per_cu(_message: &VersionedMessage, _bank: &Bank) -> u64 {
    1
}

/* TODO - this is for banking_stage tests, rework later
pub fn transactions_to_deserialized_packets(
    transactions: &[Transaction],
) -> Vec<DeserializedPacket> {
    transactions
        .iter()
        .map(|transaction| {
            let packet = Packet::from_data(None, transaction).unwrap();
            DeserializedPacket::new(packet, &None).unwrap()
        })
        .collect()
}
// */

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{signature::Keypair, system_transaction},
        std::net::IpAddr,
    };

    fn packet_with_sender_stake(sender_stake: u64, ip: Option<IpAddr>) -> DeserializedPacket {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let mut packet = Packet::from_data(None, &tx).unwrap();
        packet.meta.sender_stake = sender_stake;
        if let Some(ip) = ip {
            packet.meta.addr = ip;
        }
        DeserializedPacket::new(packet, &None).unwrap()
    }

    fn packet_with_fee_per_cu(fee_per_cu: u64) -> DeserializedPacket {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, &tx).unwrap();
        DeserializedPacket::new_with_fee_per_cu(packet, fee_per_cu).unwrap()
    }

    #[test]
    fn test_unprocessed_packet_batches_insert_pop_same_packet() {
        let packet = packet_with_sender_stake(1, None);
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
        let heavier_packet = packet_with_fee_per_cu(heavier_packet_weight);

        let lesser_packet_weight = heavier_packet_weight - 1;
        let lesser_packet = packet_with_fee_per_cu(lesser_packet_weight);

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
        let packets_iter =
            std::iter::repeat_with(|| packet_with_sender_stake(1, None)).take(num_packets);
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
}
