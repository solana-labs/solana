use {
    retain_mut::RetainMut,
    solana_perf::packet::{limited_deserialize, Packet, PacketBatch},
    solana_sdk::{
        hash::Hash, message::Message, short_vec::decode_shortu16_len, signature::Signature,
        transaction::VersionedTransaction,
    },
    std::{
        collections::{HashMap, VecDeque},
        mem::size_of,
    },
};

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
#[derive(Debug, Default)]
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

    /// Iterates packets in buffered packet_batches, returns all unprocessed packet's stake,
    /// and its locator
    #[allow(dead_code)]
    fn get_stakes_and_locators(&self) -> (Vec<u64>, Vec<PacketLocator>) {
        let num_unprocessed_packets = self.get_unprocessed_packets_count();
        let mut stakes = Vec::<u64>::with_capacity(num_unprocessed_packets);
        let mut locators = Vec::<PacketLocator>::with_capacity(num_unprocessed_packets);

        self.iter()
            .enumerate()
            .for_each(|(batch_index, deserialized_packet_batch)| {
                let packet_batch = &deserialized_packet_batch.packet_batch;
                deserialized_packet_batch
                    .unprocessed_packets
                    .keys()
                    .for_each(|packet_index| {
                        let p = &packet_batch.packets[*packet_index];
                        stakes.push(p.meta.sender_stake);
                        locators.push(PacketLocator {
                            batch_index,
                            packet_index: *packet_index,
                        });
                    });
            });

        (stakes, locators)
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
        solana_sdk::{signature::Keypair, system_transaction},
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
}
