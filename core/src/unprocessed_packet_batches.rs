use {
    crate::{
        banking_stage::TOTAL_BUFFERED_PACKETS,
        packet_sender_info::{PacketSenderInfo, SenderDetailInfo},
    },
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

// To locate a packet in banking_stage's buffered packet batches.
#[derive(Debug, Default)]
pub struct PacketLocator {
    #[allow(dead_code)]
    batch_index: usize,
    #[allow(dead_code)]
    packet_index: usize,
}

// hold deserialized messages, as well as computed message_hash and other things needed to create
// SanitizedTransaction
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
        let mut unprocessed_packets =
            HashMap::<usize, DeserializedPacket>::with_capacity(packet_indexes.len());
        packet_indexes.iter().for_each(|packet_index| {
            // only those packets can be deserialized are considered as valid.
            if let Some(deserialized_packet) =
                Self::deserialize_packet(&packet_batch.packets[*packet_index])
            {
                unprocessed_packets.insert(*packet_index, deserialized_packet);
            }
        });
        unprocessed_packets
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
    fn packet_message(packet: &Packet) -> Option<&[u8]> {
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

// TODO TAO - refactor type into struct
pub type UnprocessedPacketBatches = VecDeque<DeserializedPacketBatch>;

// Iterates packets in buffered batches, returns all unprocessed packet's stake,
// and its location (batch_index plus packet_index within batch)
pub fn get_stakes_and_locators(
    unprocessed_packet_batches: &UnprocessedPacketBatches,
    packet_sender_info: &mut Option<PacketSenderInfo>,
) -> (Vec<u64>, Vec<PacketLocator>) {
    let mut stakes = Vec::<u64>::with_capacity(TOTAL_BUFFERED_PACKETS);
    let mut locators = Vec::<PacketLocator>::with_capacity(TOTAL_BUFFERED_PACKETS);

    unprocessed_packet_batches.iter().enumerate().for_each(
        |(batch_index, deserialized_packet_batch)| {
            let packet_batch = &deserialized_packet_batch.packet_batch;
            deserialized_packet_batch
                .unprocessed_packets
                .keys()
                //                    .cloned()
                //                    .collect::<Vec<usize>>();
                //                original_unprocessed_indexes
                //                    .iter()
                .for_each(|packet_index| {
                    debug!("----- packet index {}", packet_index);
                    let p = &packet_batch.packets[*packet_index];
                    stakes.push(p.meta.weight);
                    locators.push(PacketLocator {
                        batch_index,
                        packet_index: *packet_index,
                    });

                    if let Some(packet_sender_info) = packet_sender_info {
                        update_packet_sender_info(packet_sender_info, p);
                    }
                });
        },
    );

    (stakes, locators)
}

fn update_packet_sender_info(packet_sender_info: &mut PacketSenderInfo, packet: &Packet) {
    let ip = packet.meta.addr;
    packet_sender_info.packet_senders_ip.push(ip);
    let sender_detail = packet_sender_info
        .senders_detail
        .entry(ip)
        .or_insert(SenderDetailInfo {
            stake: packet.meta.weight,
            packet_count: 0u64,
        });
    sender_detail.packet_count = sender_detail.packet_count.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{signature::Keypair, system_transaction},
        std::net::IpAddr,
    };

    fn packet_with_weight(weight: u64, ip: IpAddr) -> Packet {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let mut packet = Packet::from_data(None, &tx).unwrap();
        packet.meta.weight = weight;
        packet.meta.addr = ip;
        packet
    }

    #[test]
    fn test_get_stakes_and_locators_with_sender_info() {
        solana_logger::setup();

        // setup senders' addr and stake
        let senders: Vec<(IpAddr, u64)> = vec![
            (IpAddr::from([127, 0, 0, 1]), 1),
            (IpAddr::from([127, 0, 0, 2]), 2),
            (IpAddr::from([127, 0, 0, 3]), 3),
        ];
        // create a buffer with 3 batches, each has 2 packet from above sender.
        // so: [1, 2] [3, 1] [2, 3]
        let batch_size = 2usize;
        let batch_count = 3usize;
        let unprocessed_packets = (0..batch_count)
            .map(|batch_index| {
                DeserializedPacketBatch::new(
                    PacketBatch::new(
                        (0..batch_size)
                            .map(|packet_index| {
                                let n = (batch_index * batch_size + packet_index) % senders.len();
                                packet_with_weight(senders[n].1, senders[n].0)
                            })
                            .collect(),
                    ),
                    (0..batch_size).collect(),
                    false,
                )
            })
            .collect();
        debug!("unprocessed batches: {:?}", unprocessed_packets);

        let mut packet_sender_info = Some(PacketSenderInfo::default());

        let (stakes, locators) =
            get_stakes_and_locators(&unprocessed_packets, &mut packet_sender_info);
        debug!("stakes: {:?}, locators: {:?}", stakes, locators);
        assert_eq!(batch_size * batch_count, stakes.len());
        assert_eq!(batch_size * batch_count, locators.len());
        locators.iter().enumerate().for_each(|(index, locator)| {
            assert_eq!(
                stakes[index],
                senders[(locator.batch_index * batch_size + locator.packet_index) % senders.len()]
                    .1
            );
        });

        // verify the sender info are collected correctly
        let packet_sender_info = packet_sender_info.unwrap();
        assert_eq!(
            batch_size * batch_count,
            packet_sender_info.packet_senders_ip.len()
        );
        locators.iter().enumerate().for_each(|(index, locator)| {
            assert_eq!(
                packet_sender_info.packet_senders_ip[index],
                senders[(locator.batch_index * batch_size + locator.packet_index) % senders.len()]
                    .0
            );
        });
        assert_eq!(senders.len(), packet_sender_info.senders_detail.len());
        senders.into_iter().for_each(|(ip, stake)| {
            let sender_detail = packet_sender_info.senders_detail.get(&ip).unwrap();
            assert_eq!(stake, sender_detail.stake);
            assert_eq!(2u64, sender_detail.packet_count);
        });
    }
}
