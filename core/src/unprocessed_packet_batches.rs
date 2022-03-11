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

/// hold deserialized messages, as well as computed message_hash and other things needed to create
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

#[derive(Debug, Default)]
pub struct DeserializedPacketBatch {
    pub packet_batch: PacketBatch,
    pub forwarded: bool,
    // indexes of valid packets in batch, and their corrersponding deserialized_packet
    pub unprocessed_packets: HashMap<usize, DeserializedPacket>,
}

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

impl Default for UnprocessedPacketBatches {
    fn default() -> Self {
        Self::new()
    }
}

impl UnprocessedPacketBatches {
    pub fn new() -> Self {
        UnprocessedPacketBatches(VecDeque::new())
    }

    pub fn with_capacity(capacity: usize) -> Self {
        UnprocessedPacketBatches(VecDeque::with_capacity(capacity))
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

    // Returns whether the given `PacketBatch` has any more remaining unprocessed
    // transactions
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
    };

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
}
