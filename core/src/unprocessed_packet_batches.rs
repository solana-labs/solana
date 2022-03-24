use {
    retain_mut::RetainMut,
    solana_gossip::weighted_shuffle::WeightedShuffle,
    solana_perf::packet::{limited_deserialize, Packet, PacketBatch},
    solana_runtime::bank::Bank,
    solana_sdk::{
        clock::Slot,
        feature_set::tx_wide_compute_cap,
        hash::Hash,
        message::{
            v0::{self},
            Message, SanitizedMessage, VersionedMessage,
        },
        sanitize::Sanitize,
        short_vec::decode_shortu16_len,
        signature::Signature,
        transaction::{AddressLoader, VersionedTransaction},
    },
    std::{
        collections::{BTreeMap, HashMap, VecDeque},
        mem::size_of,
        sync::Arc,
    },
};

/// FeePerCu is valid by up to X slots
#[derive(Debug, Default)]
struct FeePerCu {
    fee_per_cu: u64,
    slot: Slot,
}

impl FeePerCu {
    fn too_old(&self, slot: &Slot) -> bool {
        const MAX_SLOT_AGE: Slot = 1;
        slot - self.slot >= MAX_SLOT_AGE
    }
}

/// Holds deserialized messages, as well as computed message_hash and other things needed to create
/// SanitizedTransaction
#[derive(Debug, Default)]
pub struct DeserializedPacket {
    #[allow(dead_code)]
    versioned_transaction: VersionedTransaction,

    #[allow(dead_code)]
    message_hash: Hash,

    #[allow(dead_code)]
    is_simple_vote: bool,

    fee_per_cu: Option<FeePerCu>,
}

/// Defines the type of entry in `UnprocessedPacketBatches`, it holds original packet_batch
/// for forwarding, as well as `forwarded` flag;
/// Each packet in packet_batch are deserialized upon receiving, the result are stored in
/// `DeserializedPacket` in the same order as packets in `packet_batch`.
#[derive(Debug, Default)]
pub struct DeserializedPacketBatch {
    pub packet_batch: PacketBatch,
    pub forwarded: bool,
    // indexes of valid packets in batch, and their corresponding deserialized_packet
    pub unprocessed_packets: HashMap<usize, DeserializedPacket>,
}

/// References to a packet in `UnprocessedPacketBatches`, where
/// - batch_index references to `DeserializedPacketBatch`,
/// - packet_index references to `packet` within `DeserializedPacketBatch.packet_batch`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PacketLocator {
    #[allow(dead_code)]
    batch_index: usize,
    #[allow(dead_code)]
    packet_index: usize,
}

/// Currently each banking_stage thread has a `UnprocessedPacketBatches` buffer to store
/// PacketBatch's received from sigverify. Banking thread continuously scans the buffer
/// to pick proper packets to add to the block.
#[derive(Default)]
pub struct UnprocessedPacketBatches(VecDeque<DeserializedPacketBatch>);

impl std::ops::Deref for UnprocessedPacketBatches {
    type Target = VecDeque<DeserializedPacketBatch>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for UnprocessedPacketBatches {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl RetainMut<DeserializedPacketBatch> for UnprocessedPacketBatches {
    fn retain_mut<F>(&mut self, f: F)
    where
        F: FnMut(&mut DeserializedPacketBatch) -> bool,
    {
        RetainMut::retain_mut(&mut self.0, f);
    }
}

impl FromIterator<DeserializedPacketBatch> for UnprocessedPacketBatches {
    fn from_iter<I: IntoIterator<Item = DeserializedPacketBatch>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl UnprocessedPacketBatches {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        UnprocessedPacketBatches(VecDeque::with_capacity(capacity))
    }

    /// Insert new `deserizlized_packet_batch` into inner `VecDeque<DeserializedPacketBatch>`,
    /// If buffer is at Max limit, packets (not packet_batch) will be dropped, starting from
    /// the lowest priority until an empty batch is located, then swap it with new batch;
    /// Otherwise, new_batch will be pushed into the end of VecDeque;
    /// returns tuple of (number of batch dropped, number of packets dropped)
    pub fn insert_batch(
        &mut self,
        deserialized_packet_batch: DeserializedPacketBatch,
        batch_limit: usize,
    ) -> (Option<usize>, Option<usize>) {
        if deserialized_packet_batch.unprocessed_packets.is_empty() {
            return (None, None);
        }

        if self.len() >= batch_limit {
            let (_, dropped_batches_count, dropped_packets_count) =
                self.replace_packet_by_priority(deserialized_packet_batch);
            (dropped_batches_count, dropped_packets_count)
        } else {
            self.push_back(deserialized_packet_batch);
            (None, None)
        }
    }

    /// prioritize unprocessed packets by their fee/CU then by sender's stakes
    pub fn prioritize_by_fee_then_stakes(
        &mut self,
        working_bank: Option<Arc<Bank>>,
    ) -> Vec<PacketLocator> {
        let (stakes, locators) = self.get_stakes_and_locators();

        let shuffled_packet_locators = Self::weighted_shuffle(&stakes, &locators);

        self.prioritize_by_fee_per_cu(&shuffled_packet_locators, working_bank)
    }

    /// Returns total number of all packets (including unprocessed and processed) in buffer
    #[allow(dead_code)]
    fn get_packets_count(&self) -> usize {
        self.iter()
            .map(|deserialized_packet_batch| deserialized_packet_batch.packet_batch.packets.len())
            .sum()
    }

    /// Returns total number of unprocessed packets in buffer
    #[allow(dead_code)]
    fn get_unprocessed_packets_count(&self) -> usize {
        self.iter()
            .map(|deserialized_packet_batch| deserialized_packet_batch.unprocessed_packets.len())
            .sum()
    }

    /// Iterates the inner `Vec<DeserializedPacketBatch>`.
    /// Returns the flattened result of mapping each
    /// `DeserializedPacketBatch` to a list the batch's inner
    /// packets' sender's stake and their `PacketLocator`'s within the
    /// `Vec<DeserializedPacketBatch>`.
    #[allow(dead_code)]
    fn get_stakes_and_locators(&self) -> (Vec<u64>, Vec<PacketLocator>) {
        self.iter()
            .enumerate()
            .flat_map(|(batch_index, deserialized_packet_batch)| {
                let packet_batch = &deserialized_packet_batch.packet_batch;
                deserialized_packet_batch
                    .unprocessed_packets
                    .keys()
                    .map(move |packet_index| {
                        let p = &packet_batch.packets[*packet_index];
                        (
                            p.meta.sender_stake,
                            PacketLocator {
                                batch_index,
                                packet_index: *packet_index,
                            },
                        )
                    })
            })
            .unzip()
    }

    fn weighted_shuffle(stakes: &[u64], locators: &[PacketLocator]) -> Vec<PacketLocator> {
        let mut rng = rand::thread_rng();
        WeightedShuffle::new(stakes)
            .unwrap()
            .shuffle(&mut rng)
            .map(|i| locators[i].clone())
            .collect()
    }

    /// Index `locators` by their transaction's fee-per-cu value; For transactions
    /// have same fee-per-cu, their relative order remains same (eg. in sender_stake order).
    fn prioritize_by_fee_per_cu(
        &mut self,
        locators: &Vec<PacketLocator>,
        bank: Option<Arc<Bank>>,
    ) -> Vec<PacketLocator> {
        let mut fee_buckets = BTreeMap::<u64, Vec<PacketLocator>>::new();
        for locator in locators {
            // if unable to compute fee-per-cu for the packet, put it to the `0` bucket
            let fee_per_cu = self.get_computed_fee_per_cu(locator, &bank).unwrap_or(0);

            let bucket = fee_buckets
                .entry(fee_per_cu)
                .or_insert(Vec::<PacketLocator>::new());
            bucket.push(locator.clone());
        }
        fee_buckets
            .iter()
            .rev()
            .flat_map(|(_key, bucket)| bucket.iter().cloned())
            .collect()
    }

    /// get cached fee_per_cu for transaction referenced by `locator`, if cached value is
    /// too old for current `bank`, or no cached value, then (re)compute and cache.
    fn get_computed_fee_per_cu(
        &mut self,
        locator: &PacketLocator,
        bank: &Option<Arc<Bank>>,
    ) -> Option<u64> {
        if bank.is_none() {
            return None;
        }
        let bank = bank.as_ref().unwrap();
        let deserialized_packet = self.locate_packet_mut(locator)?;
        if let Some(cached_fee_per_cu) =
            Self::get_cached_fee_per_cu(deserialized_packet, &bank.slot())
        {
            Some(cached_fee_per_cu)
        } else {
            let computed_fee_per_cu = Self::compute_fee_per_cu(deserialized_packet, bank);
            if let Some(computed_fee_per_cu) = computed_fee_per_cu {
                deserialized_packet.fee_per_cu = Some(FeePerCu {
                    fee_per_cu: computed_fee_per_cu,
                    slot: bank.slot(),
                });
            }
            computed_fee_per_cu
        }
    }

    #[allow(dead_code)]
    fn locate_packet(&self, locator: &PacketLocator) -> Option<&DeserializedPacket> {
        let deserialized_packet_batch = self.get(locator.batch_index)?;
        deserialized_packet_batch
            .unprocessed_packets
            .get(&locator.packet_index)
    }

    fn locate_packet_mut(&mut self, locator: &PacketLocator) -> Option<&mut DeserializedPacket> {
        let deserialized_packet_batch = self.get_mut(locator.batch_index)?;
        deserialized_packet_batch
            .unprocessed_packets
            .get_mut(&locator.packet_index)
    }

    /// Computes `(addition_fee + base_fee / requested_cu)` for packet referenced by `PacketLocator`
    fn compute_fee_per_cu(deserialized_packet: &DeserializedPacket, bank: &Bank) -> Option<u64> {
        let sanitized_message =
            Self::sanitize_message(&deserialized_packet.versioned_transaction.message, bank)?;
        let (total_fee, max_units) = Bank::calculate_fee(
            &sanitized_message,
            bank.get_lamports_per_signature(),
            &bank.fee_structure,
            bank.feature_set.is_active(&tx_wide_compute_cap::id()),
        );
        Some(total_fee / max_units)
    }

    fn get_cached_fee_per_cu(deserialized_packet: &DeserializedPacket, slot: &Slot) -> Option<u64> {
        let cached_fee_per_cu = deserialized_packet.fee_per_cu.as_ref()?;
        if cached_fee_per_cu.too_old(slot) {
            None
        } else {
            Some(cached_fee_per_cu.fee_per_cu)
        }
    }

    fn sanitize_message(
        versioned_message: &VersionedMessage,
        address_loader: impl AddressLoader,
    ) -> Option<SanitizedMessage> {
        versioned_message.sanitize().ok()?;

        match versioned_message {
            VersionedMessage::Legacy(message) => Some(SanitizedMessage::Legacy(message.clone())),
            VersionedMessage::V0(message) => {
                let loaded_addresses = address_loader
                    .load_addresses(&message.address_table_lookups)
                    .ok()?;
                Some(SanitizedMessage::V0(v0::LoadedMessage::new(
                    message.clone(),
                    loaded_addresses,
                )))
            }
        }
    }

    /// This function is called to put new deserialized_packet_batch into buffer when it is
    /// at Max capacity.
    /// It tries to drop lower prioritized transactions in order to find an empty batch to swap
    /// with new deserialized_packet_batch.
    /// Returns the dropped deserialized_packet_batch, dropped batch count and dropped packets count.
    fn replace_packet_by_priority(
        &mut self,
        deserialized_packet_batch: DeserializedPacketBatch,
    ) -> (
        Option<DeserializedPacketBatch>,
        Option<usize>,
        Option<usize>,
    ) {
        // push new batch to the end of Vec to join existing batches for prioritizing and selecting
        self.push_back(deserialized_packet_batch);
        let new_batch_index = self.len() - 1;

        // Right now, it doesn't have a bank that can be used to calculate fee/cu at
        // point of packet receiving.
        let bank_to_compute_fee: Option<Arc<Bank>> = None;
        // Get locators ordered by fee then sender's stake, the highest priority packet's locator
        // at the top.
        let ordered_locators_for_eviction = self.prioritize_by_fee_then_stakes(bank_to_compute_fee);

        // Start from the lowest priority to collect packets as candidates to be dropped, until
        // find a Batch that would no longer have unprocessed packets.
        let mut eviction_batch_index: Option<usize> = None;
        let mut evicting_packets = HashMap::<usize, Vec<usize>>::new();
        for locator in ordered_locators_for_eviction.iter().rev() {
            if let Some(batch) = self.get(locator.batch_index) {
                if batch
                    .unprocessed_packets
                    .contains_key(&locator.packet_index)
                {
                    let packet_indexes = evicting_packets
                        .entry(locator.batch_index)
                        .or_insert(vec![]);
                    packet_indexes.push(locator.packet_index);

                    if Self::would_be_empty_batch(batch, packet_indexes) {
                        // found an empty batch can be swapped with new batch
                        eviction_batch_index = Some(locator.batch_index);
                        break;
                    }
                }
            }
        }
        // remove those evicted packets by removing them from `unprocessed` list
        let mut dropped_packets_count: Option<usize> = None;
        evicting_packets
            .iter()
            .for_each(|(batch_index, evicted_packet_indexes)| {
                if let Some(batch) = self.get_mut(*batch_index) {
                    batch
                        .unprocessed_packets
                        .retain(|&k, _| !evicted_packet_indexes.contains(&k));
                    dropped_packets_count = Some(
                        dropped_packets_count
                            .unwrap_or(0usize)
                            .saturating_add(evicted_packet_indexes.len()),
                    );
                }
            });

        if let Some(eviction_batch_index) = eviction_batch_index {
            if eviction_batch_index == new_batch_index {
                // the new batch is identified to be the one for eviction, just pop it out;
                (self.pop_back(), None, dropped_packets_count)
            } else {
                // we have a spot in the queue, swap it with the new batch at end of queue;
                (
                    self.swap_remove_back(eviction_batch_index),
                    Some(1usize),
                    dropped_packets_count,
                )
            }
        } else {
            warn!("Cannot find eviction candidate from buffer in single iteration. New packet batch is dropped.");
            (self.pop_back(), None, dropped_packets_count)
        }
    }

    /// Returns True if all unprocessed packets in the batch are in eviction list
    fn would_be_empty_batch(
        deserialized_packet_batch: &DeserializedPacketBatch,
        eviction_list: &[usize],
    ) -> bool {
        if deserialized_packet_batch.unprocessed_packets.len() != eviction_list.len() {
            return false;
        }

        for (k, _) in deserialized_packet_batch.unprocessed_packets.iter() {
            if !eviction_list.contains(k) {
                return false;
            }
        }

        true
    }
}

impl DeserializedPacketBatch {
    pub fn new(packet_batch: PacketBatch, packet_indexes: Vec<usize>, forwarded: bool) -> Self {
        let unprocessed_packets = Self::deserialize_packets(&packet_batch, &packet_indexes);
        Self {
            packet_batch,
            unprocessed_packets,
            forwarded,
        }
    }

    fn deserialize_packets(
        packet_batch: &PacketBatch,
        packet_indexes: &[usize],
    ) -> HashMap<usize, DeserializedPacket> {
        packet_indexes
            .iter()
            .filter_map(|packet_index| {
                let deserialized_packet =
                    Self::deserialize_packet(&packet_batch.packets[*packet_index])?;
                Some((*packet_index, deserialized_packet))
            })
            .collect()
    }

    fn deserialize_packet(packet: &Packet) -> Option<DeserializedPacket> {
        let versioned_transaction: VersionedTransaction =
            match limited_deserialize(&packet.data[0..packet.meta.size]) {
                Ok(tx) => tx,
                Err(_) => return None,
            };

        if let Some(message_bytes) = Self::packet_message(packet) {
            let message_hash = Message::hash_raw_message(message_bytes);
            let is_simple_vote = packet.meta.is_simple_vote_tx();
            Some(DeserializedPacket {
                versioned_transaction,
                message_hash,
                is_simple_vote,
                fee_per_cu: None,
            })
        } else {
            None
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

    /// Returns whether the given `PacketBatch` has any more remaining unprocessed
    /// transactions
    pub fn update_buffered_packets_with_new_unprocessed(
        &mut self,
        _original_unprocessed_indexes: &[usize],
        new_unprocessed_indexes: &[usize],
    ) -> bool {
        let has_more_unprocessed_transactions = !new_unprocessed_indexes.is_empty();
        if has_more_unprocessed_transactions {
            self.unprocessed_packets
                .retain(|index, _| new_unprocessed_indexes.contains(index));
        } else {
            self.unprocessed_packets.clear();
        }

        has_more_unprocessed_transactions
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_runtime::{
            bank::goto_end_of_slot,
            genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
        },
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            signature::{Keypair, Signer},
            system_instruction::{self},
            system_transaction,
            transaction::Transaction,
        },
        std::net::IpAddr,
    };

    fn packet_with_sender_stake(sender_stake: u64, ip: Option<IpAddr>) -> Packet {
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
        packet
    }

    #[test]
    fn test_packet_message() {
        let keypair = Keypair::new();
        let pubkey = solana_sdk::pubkey::new_rand();
        let blockhash = Hash::new_unique();
        let transaction = system_transaction::transfer(&keypair, &pubkey, 1, blockhash);
        let packet = Packet::from_data(None, &transaction).unwrap();
        assert_eq!(
            DeserializedPacketBatch::packet_message(&packet)
                .unwrap()
                .to_vec(),
            transaction.message_data()
        );
    }

    #[test]
    fn test_get_packets_count() {
        // create a buffer with 3 batches, each has 2 packets but only first one is valid
        let batch_size = 2usize;
        let batch_count = 3usize;
        let unprocessed_packet_batches: UnprocessedPacketBatches = (0..batch_count)
            .map(|_batch_index| {
                DeserializedPacketBatch::new(
                    PacketBatch::new(
                        (0..batch_size)
                            .map(|packet_index| packet_with_sender_stake(packet_index as u64, None))
                            .collect(),
                    ),
                    vec![0],
                    false,
                )
            })
            .collect();

        // Assert total packets count, and unprocessed packets count
        assert_eq!(
            batch_size * batch_count,
            unprocessed_packet_batches.get_packets_count()
        );
        assert_eq!(
            batch_count,
            unprocessed_packet_batches.get_unprocessed_packets_count()
        );
    }

    #[test]
    fn test_get_stakes_and_locators_from_empty_buffer() {
        let unprocessed_packet_batches = UnprocessedPacketBatches::default();
        let (stakes, locators) = unprocessed_packet_batches.get_stakes_and_locators();

        assert!(stakes.is_empty());
        assert!(locators.is_empty());
    }

    #[test]
    fn test_get_stakes_and_locators() {
        solana_logger::setup();

        // setup senders' address and stake
        let senders: Vec<(IpAddr, u64)> = vec![
            (IpAddr::from([127, 0, 0, 1]), 1),
            (IpAddr::from([127, 0, 0, 2]), 2),
            (IpAddr::from([127, 0, 0, 3]), 3),
        ];
        // create a buffer with 3 batches, each has 2 packet from above sender.
        // buffer looks like:
        // [127.0.0.1, 127.0.0.2]
        // [127.0.0.3, 127.0.0.1]
        // [127.0.0.2, 127.0.0.3]
        let batch_size = 2usize;
        let batch_count = 3usize;
        let unprocessed_packet_batches: UnprocessedPacketBatches = (0..batch_count)
            .map(|batch_index| {
                DeserializedPacketBatch::new(
                    PacketBatch::new(
                        (0..batch_size)
                            .map(|packet_index| {
                                let n = (batch_index * batch_size + packet_index) % senders.len();
                                packet_with_sender_stake(senders[n].1, Some(senders[n].0))
                            })
                            .collect(),
                    ),
                    (0..batch_size).collect(),
                    false,
                )
            })
            .collect();

        let (stakes, locators) = unprocessed_packet_batches.get_stakes_and_locators();

        // Produced stakes and locators should both have "batch_size * batch_count" entries;
        assert_eq!(batch_size * batch_count, stakes.len());
        assert_eq!(batch_size * batch_count, locators.len());
        // Assert stakes and locators are in good order
        locators.iter().enumerate().for_each(|(index, locator)| {
            assert_eq!(
                stakes[index],
                senders[(locator.batch_index * batch_size + locator.packet_index) % senders.len()]
                    .1
            );
        });
    }

    #[test]
    fn test_replace_packet_by_priority() {
        solana_logger::setup();

        let batch = DeserializedPacketBatch::new(
            PacketBatch::new(vec![
                packet_with_sender_stake(200, None),
                packet_with_sender_stake(210, None),
            ]),
            vec![0, 1],
            false,
        );
        let mut unprocessed_packets: UnprocessedPacketBatches = vec![batch].into_iter().collect();

        // try to insert one with weight lesser than anything in buffer.
        // the new one should be rejected, and buffer should be unchanged
        {
            let sender_stake = 0u64;
            let new_batch = DeserializedPacketBatch::new(
                PacketBatch::new(vec![packet_with_sender_stake(sender_stake, None)]),
                vec![0],
                false,
            );
            let (dropped_batch, _, _) = unprocessed_packets.replace_packet_by_priority(new_batch);
            // dropped batch should be the one made from new packet:
            let dropped_packets = dropped_batch.unwrap();
            assert_eq!(1, dropped_packets.packet_batch.packets.len());
            assert_eq!(
                sender_stake,
                dropped_packets.packet_batch.packets[0].meta.sender_stake
            );
            // buffer should be unchanged
            assert_eq!(1, unprocessed_packets.len());
        }

        // try to insert one with sender_stake higher than anything in buffer.
        // the lest sender_stake batch should be dropped, new one will take its palce.
        {
            let sender_stake = 50_000u64;
            let new_batch = DeserializedPacketBatch::new(
                PacketBatch::new(vec![packet_with_sender_stake(sender_stake, None)]),
                vec![0],
                false,
            );
            let (dropped_batch, _, _) = unprocessed_packets.replace_packet_by_priority(new_batch);
            // dropped batch should be the one with lest sender_stake in buffer (the 3rd batch):
            let dropped_packets = dropped_batch.unwrap();
            assert_eq!(2, dropped_packets.packet_batch.packets.len());
            assert_eq!(
                200,
                dropped_packets.packet_batch.packets[0].meta.sender_stake
            );
            assert_eq!(
                210,
                dropped_packets.packet_batch.packets[1].meta.sender_stake
            );
            // buffer should still have 1 batches
            assert_eq!(1, unprocessed_packets.len());
            // ... which should be the new batch with one packet
            assert_eq!(1, unprocessed_packets[0].packet_batch.packets.len());
            assert_eq!(
                sender_stake,
                unprocessed_packets[0].packet_batch.packets[0]
                    .meta
                    .sender_stake
            );
            assert_eq!(1, unprocessed_packets[0].unprocessed_packets.len());
        }
    }

    // build a buffer of four batches, each contains packet with following stake:
    // 0: [ 10, 300]
    // 1: [100, 200, 300]
    // 2: [ 20,  30,  40]
    // 3: [500,  30, 200]
    fn build_unprocessed_packets_buffer() -> UnprocessedPacketBatches {
        vec![
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_sender_stake(10, None),
                    packet_with_sender_stake(300, None),
                    packet_with_sender_stake(200, None),
                ]),
                vec![0, 1],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_sender_stake(100, None),
                    packet_with_sender_stake(200, None),
                    packet_with_sender_stake(300, None),
                ]),
                vec![0, 1, 2],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_sender_stake(20, None),
                    packet_with_sender_stake(30, None),
                    packet_with_sender_stake(40, None),
                ]),
                vec![0, 1, 2],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_sender_stake(500, None),
                    packet_with_sender_stake(30, None),
                    packet_with_sender_stake(200, None),
                ]),
                vec![0, 1, 2],
                false,
            ),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn test_prioritize_by_fee_per_cu() {
        solana_logger::setup();

        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_leader(1_000_000, &leader, 3);
        genesis_config
            .fee_rate_governor
            .target_lamports_per_signature = 1;
        genesis_config.fee_rate_governor.target_signatures_per_slot = 1;

        let bank = Bank::new_for_tests(&genesis_config);
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        goto_end_of_slot(&mut bank);
        // build a "packet" with higher fee-pr-cu with compute_budget instruction.
        let key0 = Keypair::new();
        let key1 = Keypair::new();
        let ix0 = system_instruction::transfer(&key0.pubkey(), &key1.pubkey(), 1);
        let ix1 = system_instruction::transfer(&key1.pubkey(), &key0.pubkey(), 1);
        let ix_cb = ComputeBudgetInstruction::request_units(1000, 20000);
        let message = Message::new(&[ix0, ix1, ix_cb], Some(&key0.pubkey()));
        let tx = Transaction::new(&[&key0, &key1], message, bank.last_blockhash());
        let packet = Packet::from_data(None, &tx).unwrap();

        // build a buffer with 4 batches
        let mut unprocessed_packets = build_unprocessed_packets_buffer();
        // add "packet" with higher fee-per-cu to buffer
        unprocessed_packets.push_back(DeserializedPacketBatch::new(
            PacketBatch::new(vec![packet]),
            vec![0],
            false,
        ));
        // randomly select 4 packets plus the higher fee/cu packets to feed into
        // prioritize_by_fee_per_cu function.
        let locators = vec![
            PacketLocator {
                batch_index: 2,
                packet_index: 2,
            },
            PacketLocator {
                batch_index: 1,
                packet_index: 2,
            },
            PacketLocator {
                batch_index: 3,
                packet_index: 0,
            },
            PacketLocator {
                batch_index: 3,
                packet_index: 2,
            },
            PacketLocator {
                batch_index: 4,
                packet_index: 0,
            },
        ];

        // If no bank is given, fee-per-cu won't calculate, should expect output is same as input
        {
            let prioritized_locators =
                unprocessed_packets.prioritize_by_fee_per_cu(&locators, None);
            assert_eq!(locators, prioritized_locators);
        }

        // If bank is given, fee-per-cu is calculated, should expect higher fee-per-cu come
        // out first
        {
            let expected_locators = vec![
                PacketLocator {
                    batch_index: 4,
                    packet_index: 0,
                },
                PacketLocator {
                    batch_index: 2,
                    packet_index: 2,
                },
                PacketLocator {
                    batch_index: 1,
                    packet_index: 2,
                },
                PacketLocator {
                    batch_index: 3,
                    packet_index: 0,
                },
                PacketLocator {
                    batch_index: 3,
                    packet_index: 2,
                },
            ];

            let prioritized_locators =
                unprocessed_packets.prioritize_by_fee_per_cu(&locators, Some(Arc::new(bank)));
            assert_eq!(expected_locators, prioritized_locators);
        }
    }

    #[test]
    fn test_get_cached_fee_per_cu() {
        let mut deserialized_packet = DeserializedPacket::default();
        let slot: Slot = 100;

        // assert default deserialized_packet has no cached fee-per-cu
        assert!(
            UnprocessedPacketBatches::get_cached_fee_per_cu(&deserialized_packet, &slot).is_none()
        );

        // cache fee-per-cu with slot 100
        let fee_per_cu = 1_000u64;
        deserialized_packet.fee_per_cu = Some(FeePerCu { fee_per_cu, slot });

        // assert cache fee-per-cu is available for same slot
        assert_eq!(
            fee_per_cu,
            UnprocessedPacketBatches::get_cached_fee_per_cu(&deserialized_packet, &slot).unwrap()
        );

        // assert cached value became too old
        assert!(
            UnprocessedPacketBatches::get_cached_fee_per_cu(&deserialized_packet, &(slot + 1))
                .is_none()
        );
    }
}
