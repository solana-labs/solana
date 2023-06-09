//! Deserializes packets from sigverify stage. Owned by banking stage.

use {
    super::immutable_deserialized_packet::ImmutableDeserializedPacket,
    crate::{
        banking_trace::{BankingPacketBatch, BankingPacketReceiver},
        sigverify::SigverifyTracerPacketStats,
    },
    crossbeam_channel::RecvTimeoutError,
    solana_perf::packet::PacketBatch,
    solana_runtime::bank_forks::BankForks,
    std::{
        sync::{Arc, RwLock},
        time::{Duration, Instant},
    },
};

/// Results from deserializing packet batches.
pub struct ReceivePacketResults {
    /// Deserialized packets from all received packet batches
    pub deserialized_packets: Vec<ImmutableDeserializedPacket>,
    /// Aggregate tracer stats for all received packet batches
    pub new_tracer_stats_option: Option<SigverifyTracerPacketStats>,
    /// Number of packets passing sigverify
    pub passed_sigverify_count: u64,
    /// Number of packets failing sigverify
    pub failed_sigverify_count: u64,
}

pub struct PacketDeserializer {
    /// Receiver for packet batches from sigverify stage
    packet_batch_receiver: BankingPacketReceiver,
    /// Provides working bank for deserializer to check feature activation
    bank_forks: Arc<RwLock<BankForks>>,
}

impl PacketDeserializer {
    pub fn new(
        packet_batch_receiver: BankingPacketReceiver,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        Self {
            packet_batch_receiver,
            bank_forks,
        }
    }

    /// Handles receiving packet batches from sigverify and returns a vector of deserialized packets
    pub fn receive_packets(
        &self,
        recv_timeout: Duration,
        capacity: usize,
    ) -> Result<ReceivePacketResults, RecvTimeoutError> {
        let (packet_count, packet_batches) = self.receive_until(recv_timeout, capacity)?;

        // Note: this can be removed after feature `round_compute_unit_price` is activated in
        // mainnet-beta
        let _working_bank = self.bank_forks.read().unwrap().working_bank();
        let round_compute_unit_price_enabled = false; // TODO get from working_bank.feature_set

        Ok(Self::deserialize_and_collect_packets(
            packet_count,
            &packet_batches,
            round_compute_unit_price_enabled,
        ))
    }

    /// Deserialize packet batches, aggregates tracer packet stats, and collect
    /// them into ReceivePacketResults
    fn deserialize_and_collect_packets(
        packet_count: usize,
        banking_batches: &[BankingPacketBatch],
        round_compute_unit_price_enabled: bool,
    ) -> ReceivePacketResults {
        let mut passed_sigverify_count: usize = 0;
        let mut failed_sigverify_count: usize = 0;
        let mut deserialized_packets = Vec::with_capacity(packet_count);
        let mut aggregated_tracer_packet_stats_option = None::<SigverifyTracerPacketStats>;

        for banking_batch in banking_batches {
            for packet_batch in &banking_batch.0 {
                let packet_indexes = Self::generate_packet_indexes(packet_batch);

                passed_sigverify_count += packet_indexes.len();
                failed_sigverify_count += packet_batch.len().saturating_sub(packet_indexes.len());

                deserialized_packets.extend(Self::deserialize_packets(
                    packet_batch,
                    &packet_indexes,
                    round_compute_unit_price_enabled,
                ));
            }

            if let Some(tracer_packet_stats) = &banking_batch.1 {
                if let Some(aggregated_tracer_packet_stats) =
                    &mut aggregated_tracer_packet_stats_option
                {
                    aggregated_tracer_packet_stats.aggregate(tracer_packet_stats);
                } else {
                    // BankingPacketBatch is owned by Arc; so we have to clone its internal field
                    // (SigverifyTracerPacketStats).
                    aggregated_tracer_packet_stats_option = Some(tracer_packet_stats.clone());
                }
            }
        }

        ReceivePacketResults {
            deserialized_packets,
            new_tracer_stats_option: aggregated_tracer_packet_stats_option,
            passed_sigverify_count: passed_sigverify_count as u64,
            failed_sigverify_count: failed_sigverify_count as u64,
        }
    }

    /// Receives packet batches from sigverify stage with a timeout
    fn receive_until(
        &self,
        recv_timeout: Duration,
        packet_count_upperbound: usize,
    ) -> Result<(usize, Vec<BankingPacketBatch>), RecvTimeoutError> {
        let start = Instant::now();

        let message = self.packet_batch_receiver.recv_timeout(recv_timeout)?;
        let packet_batches = &message.0;
        let mut num_packets_received = packet_batches
            .iter()
            .map(|batch| batch.len())
            .sum::<usize>();
        let mut messages = vec![message];

        while let Ok(message) = self.packet_batch_receiver.try_recv() {
            let packet_batches = &message.0;
            trace!("got more packet batches in packet deserializer");
            num_packets_received += packet_batches
                .iter()
                .map(|batch| batch.len())
                .sum::<usize>();
            messages.push(message);

            if start.elapsed() >= recv_timeout || num_packets_received >= packet_count_upperbound {
                break;
            }
        }

        Ok((num_packets_received, messages))
    }

    fn generate_packet_indexes(packet_batch: &PacketBatch) -> Vec<usize> {
        packet_batch
            .iter()
            .enumerate()
            .filter(|(_, pkt)| !pkt.meta().discard())
            .map(|(index, _)| index)
            .collect()
    }

    fn deserialize_packets<'a>(
        packet_batch: &'a PacketBatch,
        packet_indexes: &'a [usize],
        round_compute_unit_price_enabled: bool,
    ) -> impl Iterator<Item = ImmutableDeserializedPacket> + 'a {
        packet_indexes.iter().filter_map(move |packet_index| {
            let mut packet_clone = packet_batch[*packet_index].clone();
            packet_clone
                .meta_mut()
                .set_round_compute_unit_price(round_compute_unit_price_enabled);
            ImmutableDeserializedPacket::new(packet_clone).ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_perf::packet::to_packet_batches,
        solana_sdk::{
            hash::Hash, pubkey::Pubkey, signature::Keypair, system_transaction,
            transaction::Transaction,
        },
    };

    fn random_transfer() -> Transaction {
        system_transaction::transfer(&Keypair::new(), &Pubkey::new_unique(), 1, Hash::default())
    }

    #[test]
    fn test_deserialize_and_collect_packets_empty() {
        let results = PacketDeserializer::deserialize_and_collect_packets(0, &[], false);
        assert_eq!(results.deserialized_packets.len(), 0);
        assert!(results.new_tracer_stats_option.is_none());
        assert_eq!(results.passed_sigverify_count, 0);
        assert_eq!(results.failed_sigverify_count, 0);
    }

    #[test]
    fn test_deserialize_and_collect_packets_simple_batches() {
        let transactions = vec![random_transfer(), random_transfer()];
        let packet_batches = to_packet_batches(&transactions, 1);
        assert_eq!(packet_batches.len(), 2);

        let packet_count: usize = packet_batches.iter().map(|x| x.len()).sum();
        let results = PacketDeserializer::deserialize_and_collect_packets(
            packet_count,
            &[BankingPacketBatch::new((packet_batches, None))],
            false,
        );
        assert_eq!(results.deserialized_packets.len(), 2);
        assert!(results.new_tracer_stats_option.is_none());
        assert_eq!(results.passed_sigverify_count, 2);
        assert_eq!(results.failed_sigverify_count, 0);
    }

    #[test]
    fn test_deserialize_and_collect_packets_simple_batches_with_failure() {
        let transactions = vec![random_transfer(), random_transfer()];
        let mut packet_batches = to_packet_batches(&transactions, 1);
        assert_eq!(packet_batches.len(), 2);
        packet_batches[0][0].meta_mut().set_discard(true);

        let packet_count: usize = packet_batches.iter().map(|x| x.len()).sum();
        let results = PacketDeserializer::deserialize_and_collect_packets(
            packet_count,
            &[BankingPacketBatch::new((packet_batches, None))],
            false,
        );
        assert_eq!(results.deserialized_packets.len(), 1);
        assert!(results.new_tracer_stats_option.is_none());
        assert_eq!(results.passed_sigverify_count, 1);
        assert_eq!(results.failed_sigverify_count, 1);
    }
}
