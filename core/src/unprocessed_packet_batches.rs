use {
    crate::{
        banking_stage::TOTAL_BUFFERED_PACKETS,
        packet_sender_info::{PacketSenderInfo, SenderDetailInfo},
    },
    solana_gossip::weighted_shuffle::WeightedShuffle,
    solana_perf::packet::{limited_deserialize, Packet, PacketBatch},
    solana_sdk::{
        feature_set,
        hash::Hash,
        message::Message,
        short_vec::decode_shortu16_len,
        signature::Signature,
        transaction::{AddressLoader, SanitizedTransaction, VersionedTransaction},
    },
    std::{
        collections::{HashMap, VecDeque},
        mem::size_of,
        net::IpAddr,
        sync::Arc,
    },
};

// To locate a packet in banking_stage's buffered packet batches.
#[derive(Clone, Debug, Default, PartialEq)]
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
    pub versioned_transaction: VersionedTransaction,

    #[allow(dead_code)]
    pub message_hash: Hash,

    #[allow(dead_code)]
    pub is_simple_vote: bool,
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
                .for_each(|packet_index| {
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

pub fn weighted_shuffle(
    stakes: &[u64],
    locators: &[PacketLocator],
    packet_sender_info: &mut Option<PacketSenderInfo>,
) -> Vec<PacketLocator> {
    let need_to_shuffle_sender_ips = packet_sender_info.is_some();
    let mut shuffled_packet_senders_ip = Vec::<IpAddr>::new();

    let mut rng = rand::thread_rng();
    let shuffled_locators: Vec<PacketLocator> = WeightedShuffle::new(stakes)
        .unwrap()
        .shuffle(&mut rng)
        .map(|i| {
            if need_to_shuffle_sender_ips {
                let packet_sender_info = packet_sender_info.as_ref().unwrap();
                shuffled_packet_senders_ip.push(packet_sender_info.packet_senders_ip[i]);
            }
            locators[i].clone()
        })
        .collect();
    if let Some(packet_sender_info) = packet_sender_info {
        packet_sender_info.packet_senders_ip = shuffled_packet_senders_ip;
    }
    shuffled_locators
}

// This function creates SanitizedTransactions from deseralized VersionedTransactions.i
// A list of sanitized transactions are returned
// with their packet locators.
pub fn sanitize_transactions(
    unprocessed_packet_batches: &UnprocessedPacketBatches,
    packet_locators: &[PacketLocator],
    feature_set: &Arc<feature_set::FeatureSet>,
    votes_only: bool,
    address_loader: &impl AddressLoader,
) -> (Vec<SanitizedTransaction>, Vec<PacketLocator>) {
    packet_locators
        .iter()
        .filter_map(|locator| {
            let deserialized_packet_batch = unprocessed_packet_batches.get(locator.batch_index)?;
            let deserialized_packet = deserialized_packet_batch
                .unprocessed_packets
                .get(&locator.packet_index)?;

            if votes_only && !deserialized_packet.is_simple_vote {
                return None;
            }

            let tx = SanitizedTransaction::try_create(
                deserialized_packet.versioned_transaction.clone(),
                deserialized_packet.message_hash,
                Some(deserialized_packet.is_simple_vote),
                address_loader,
            )
            .ok()?;
            tx.verify_precompiles(feature_set).ok()?;
            Some((tx, locator.clone()))
        })
        .unzip()
}

// insert new packet batch into buffer,
// if buffer is at limit, using eviction strategy to evict lower priority packets
// until an empty batch is located, swap that with new batch
pub fn insert_or_swap_batch(
    unprocessed_packet_batches: &mut UnprocessedPacketBatches,
    deserialized_packet_batch: DeserializedPacketBatch,
    batch_limit: usize,
) {
    if deserialized_packet_batch.unprocessed_packets.len() == 0 {
        return;
    }

    if unprocessed_packet_batches.len() >= batch_limit {
        swap_packet_with_eviction_strategy(unprocessed_packet_batches, deserialized_packet_batch);
    } else {
        unprocessed_packet_batches.push_back(deserialized_packet_batch);
    }
}

fn swap_packet_with_eviction_strategy(
    buffered_packet_batches: &mut UnprocessedPacketBatches,
    deserialized_packet_batch: DeserializedPacketBatch,
) -> Option<DeserializedPacketBatch> {
    // add new batch into into selection process
    buffered_packet_batches.push_back(deserialized_packet_batch);
    let new_batch_index = buffered_packet_batches.len() - 1;

    let ordered_locators_for_eviction = create_evictioin_locators(buffered_packet_batches);

    let mut eviction_batch_index: Option<usize> = None;
    let mut evicting_packets = HashMap::<usize, Vec<usize>>::new();
    for locator in ordered_locators_for_eviction.iter().rev() {
        let batch = buffered_packet_batches.get(locator.batch_index)?;
        if batch
            .unprocessed_packets
            .contains_key(&locator.packet_index)
        {
            let packet_indexes = evicting_packets
                .entry(locator.batch_index)
                .or_insert(vec![]);
            packet_indexes.push(locator.packet_index);

            if would_be_empty_batch(batch, packet_indexes) {
                // found an empty batch can be swapped with new batch
                eviction_batch_index = Some(locator.batch_index);
                break;
            }
        }
    }
    // remove those evicted packets
    evicting_packets
        .iter()
        .for_each(|(batch_index, evicted_packet_indexes)| {
            if let Some(batch) = buffered_packet_batches.get_mut(*batch_index) {
                batch
                    .unprocessed_packets
                    .retain(|&k, _| !evicted_packet_indexes.contains(&k));
            }
        });

    if let Some(eviction_batch_index) = eviction_batch_index {
        if eviction_batch_index == new_batch_index {
            // the new batch is identified to be the one for eviction
            buffered_packet_batches.pop_back()
        } else {
            // we have a spot in the queue for new item, which is at the back of queue right now
            buffered_packet_batches.swap_remove_back(eviction_batch_index)
        }
    } else {
        // should not be here
        warn!("Cannot find eviction candidate from buffer");
        buffered_packet_batches.pop_back()
    }
}

// would be empty batch if all unprocessed packets are in eviction list
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

// Creates an ordered packet locators vector, close to head are the packets preferred to be kept,
// close to tail are packets should be removed from buffer
fn create_evictioin_locators(
    buffered_packet_batches: &UnprocessedPacketBatches,
) -> Vec<PacketLocator> {
    // NOTE: currently evicting packets by sender stake weight prioritization, can add fee/CU
    // prioritization later
    let (stakes, locators) = get_stakes_and_locators(buffered_packet_batches, &mut None);
    weighted_shuffle(&stakes, &locators, &mut None)
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
        solana_perf::packet::PacketFlags,
        solana_sdk::{
            signature::{Keypair, Signer},
            system_transaction,
            transaction::DisabledAddressLoader,
        },
        solana_vote_program::vote_transaction,
    };

    fn packet_with_weight(weight: u64, ip: Option<IpAddr>) -> Packet {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let mut packet = Packet::from_data(None, &tx).unwrap();
        packet.meta.weight = weight;
        if let Some(ip) = ip {
            packet.meta.addr = ip;
        }
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
                                packet_with_weight(senders[n].1, Some(senders[n].0))
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

    #[test]
    fn test_weighted_shuffle_with_sender_info() {
        solana_logger::setup();

        // setup senders' addr and stake
        let senders: Vec<(IpAddr, u64)> = vec![
            (IpAddr::from([127, 0, 0, 1]), 100),
            (IpAddr::from([127, 0, 0, 2]), 200),
            (IpAddr::from([127, 0, 0, 3]), 0),
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
                                packet_with_weight(senders[n].1, Some(senders[n].0))
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
        let shuffled_packet_locators =
            weighted_shuffle(&stakes, &locators, &mut packet_sender_info);
        debug!(
            "shuffled locators: {:?}, shuffled sender_ips: {:?}",
            shuffled_packet_locators,
            packet_sender_info.as_ref().unwrap().packet_senders_ip
        );

        // verify after shuffle, each locator:(batch_index, packet_index) still in sync with
        // sender_info
        let packet_sender_info = packet_sender_info.unwrap();
        assert_eq!(
            batch_size * batch_count,
            packet_sender_info.packet_senders_ip.len()
        );
        shuffled_packet_locators
            .iter()
            .enumerate()
            .for_each(|(index, locator)| {
                assert_eq!(
                    packet_sender_info.packet_senders_ip[index],
                    senders
                        [(locator.batch_index * batch_size + locator.packet_index) % senders.len()]
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

    #[test]
    fn test_sanitize_transactions() {
        solana_logger::setup();
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

        let transfer_packet = Packet::from_data(None, &transfer_tx).unwrap();
        let mut vote_packet = Packet::from_data(None, &vote_tx).unwrap();
        vote_packet.meta.flags |= PacketFlags::SIMPLE_VOTE_TX;

        // packet_batch with one votes, one transfer txs
        // two such batches in the buffer
        let packets = vec![transfer_packet, vote_packet];
        let packet_batch = PacketBatch::new(packets);
        let unprocessed_packets = (0..2usize)
            .map(|_| {
                DeserializedPacketBatch::new(packet_batch.clone(), (0..2usize).collect(), false)
            })
            .collect();
        let (_, locators) = get_stakes_and_locators(&unprocessed_packets, &mut None);
        {
            let votes_only = false;
            let (txs, tx_locators) = sanitize_transactions(
                &unprocessed_packets,
                &locators,
                &Arc::new(FeatureSet::default()),
                votes_only,
                &DisabledAddressLoader,
            );
            assert_eq!(4, txs.len());
            assert_eq!(locators, tx_locators);
        }

        {
            let votes_only = true;
            let (txs, tx_locators) = sanitize_transactions(
                &unprocessed_packets,
                &locators,
                &Arc::new(FeatureSet::default()),
                votes_only,
                &DisabledAddressLoader,
            );
            assert_eq!(2, txs.len());
            assert_eq!(locators[1], tx_locators[0]);
            assert_eq!(locators[3], tx_locators[1]);
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
                    packet_with_weight(10, None),
                    packet_with_weight(300, None),
                    packet_with_weight(200, None),
                ]),
                vec![0, 1],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_weight(100, None),
                    packet_with_weight(200, None),
                    packet_with_weight(300, None),
                ]),
                vec![0, 1, 2],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_weight(20, None),
                    packet_with_weight(30, None),
                    packet_with_weight(40, None),
                ]),
                vec![0, 1, 2],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![
                    packet_with_weight(500, None),
                    packet_with_weight(30, None),
                    packet_with_weight(200, None),
                ]),
                vec![0, 1, 2],
                false,
            ),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn test_swap_packet_with_eviction_strategy() {
        solana_logger::setup();

        let mut unprocessed_packets = build_unprocessed_packets_buffer();

        // try to insert one with weight lesser than anything in buffer.
        // the new one should be rejected, and buffer should be unchanged
        {
            let weight = 0u64;
            let new_batch = DeserializedPacketBatch::new(
                PacketBatch::new(vec![packet_with_weight(weight, None)]),
                vec![0],
                false,
            );
            let dropped_batch =
                swap_packet_with_eviction_strategy(&mut unprocessed_packets, new_batch);
            // dropped batch should be the one made from new packet:
            let dropped_packets = dropped_batch.unwrap();
            assert_eq!(1, dropped_packets.packet_batch.packets.len());
            assert_eq!(weight, dropped_packets.packet_batch.packets[0].meta.weight);
            // buffer should be unchanged
            assert_eq!(4, unprocessed_packets.len());
        }

        // try to insert one with weight higher than anything in buffer.
        // the new one should be rejected, and buffer should be unchanged
        {
            let weight = 5000u64;
            let new_batch = DeserializedPacketBatch::new(
                PacketBatch::new(vec![packet_with_weight(weight, None)]),
                vec![0],
                false,
            );
            let dropped_batch =
                swap_packet_with_eviction_strategy(&mut unprocessed_packets, new_batch);
            // dropped batch should be the one with lest weight in buffer (the 3rd batch):
            let dropped_packets = dropped_batch.unwrap();
            assert_eq!(3, dropped_packets.packet_batch.packets.len());
            assert_eq!(20, dropped_packets.packet_batch.packets[0].meta.weight);
            assert_eq!(30, dropped_packets.packet_batch.packets[1].meta.weight);
            assert_eq!(40, dropped_packets.packet_batch.packets[2].meta.weight);
            // buffer should still have 4 batches
            assert_eq!(4, unprocessed_packets.len());
            // the 3rd item should be the new batch with one packet
            assert_eq!(1, unprocessed_packets[2].packet_batch.packets.len());
            assert_eq!(
                weight,
                unprocessed_packets[2].packet_batch.packets[0].meta.weight
            );
            assert_eq!(1, unprocessed_packets[2].unprocessed_packets.len());
        }
    }
}
