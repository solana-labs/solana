use {
    crate::immutable_deserialized_packet::{DeserializedPacketError, ImmutableDeserializedPacket},
    solana_perf::packet::{Packet, PacketBatch},
    solana_runtime::transaction_priority_details::TransactionPriorityDetails,
    solana_sdk::transaction::Transaction,
    std::{cmp::Ordering, rc::Rc},
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
