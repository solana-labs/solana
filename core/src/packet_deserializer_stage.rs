//! Deserializes packets from sigverify stage. Owned by banking stage.

use {
    crate::{
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        sigverify::{IntervalSigverifyTracerPacketStats, SigverifyTracerPacketStats},
    },
    crossbeam_channel::{
        unbounded, Receiver as CrossbeamReceiver, RecvTimeoutError, Sender as CrossbeamSender,
    },
    solana_perf::packet::PacketBatch,
    std::{
        thread::{Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

pub type BankingPacketBatch = (Vec<PacketBatch>, Option<SigverifyTracerPacketStats>);
pub type BankingPacketReceiver = CrossbeamReceiver<BankingPacketBatch>;
pub type DeserializedPacketBatch = Vec<ImmutableDeserializedPacket>;
pub type DeserializedPacketBatchReceiver = CrossbeamReceiver<DeserializedPacketBatch>;

/// Defines an interface for getting deserialized packet batches,
/// either from the sigverify stage with inline deserialization,
/// or from a separate deserialization thread.
/// This is used by the banking stage to get packets to process.
pub trait DeserializedPacketBatchGetter {
    /// Get packet batches to process. Returns timeout error so the caller can
    /// decide what to do if no packets are available, e.g. continue or exit.
    fn get_deserialized_packets(
        &mut self,
        timeout: Duration,
        packet_limit: usize,
    ) -> Result<DeserializedPacketBatch, RecvTimeoutError>;

    /// Joins the deserialization thread if it exists.
    fn join(self) -> std::thread::Result<()>;
}

/// An inline deserialization implementation of the DeserializedPacketBatchGetter.
pub struct InlinePacketDeserializer {
    packet_batch_receiver: BankingPacketReceiver,
    sigverify_tracer_stats: IntervalSigverifyTracerPacketStats,
}

impl DeserializedPacketBatchGetter for InlinePacketDeserializer {
    fn get_deserialized_packets(
        &mut self,
        timeout: Duration,
        packet_limit: usize,
    ) -> Result<DeserializedPacketBatch, RecvTimeoutError> {
        let deserialized_packet_results = self.handle_received_packets(timeout, packet_limit)?;
        self.update_tracer_packet_stats(&deserialized_packet_results);
        Ok(deserialized_packet_results.deserialized_packets)
    }

    fn join(self) -> std::thread::Result<()> {
        Ok(())
    }
}

impl InlinePacketDeserializer {
    pub fn new(packet_batch_receiver: BankingPacketReceiver, id: u32) -> Self {
        Self {
            packet_batch_receiver,
            sigverify_tracer_stats: IntervalSigverifyTracerPacketStats::new(id),
        }
    }

    /// Handles receiving packet batches from sigverify and returns a vector of deserialized packets
    fn handle_received_packets(
        &self,
        recv_timeout: Duration,
        packet_limit: usize,
    ) -> Result<ReceivePacketResults, RecvTimeoutError> {
        let (packet_batches, sigverify_tracer_stats_option) =
            self.receive_until(recv_timeout, packet_limit)?;
        Ok(Self::deserialize_and_collect_packets(
            &packet_batches,
            sigverify_tracer_stats_option,
        ))
    }

    /// Deserialize packet batches and collect them into ReceivePacketResults
    fn deserialize_and_collect_packets(
        packet_batches: &[PacketBatch],
        sigverify_tracer_stats_option: Option<SigverifyTracerPacketStats>,
    ) -> ReceivePacketResults {
        let packet_count: usize = packet_batches.iter().map(|x| x.len()).sum();
        let mut passed_sigverify_count: usize = 0;
        let mut failed_sigverify_count: usize = 0;
        let mut deserialized_packets = Vec::with_capacity(packet_count);
        for packet_batch in packet_batches {
            let packet_indexes = Self::generate_packet_indexes(packet_batch);

            passed_sigverify_count += packet_indexes.len();
            failed_sigverify_count += packet_batch.len().saturating_sub(packet_indexes.len());

            deserialized_packets.extend(Self::deserialize_packets(packet_batch, &packet_indexes));
        }

        ReceivePacketResults {
            deserialized_packets,
            new_tracer_stats_option: sigverify_tracer_stats_option,
            passed_sigverify_count: passed_sigverify_count as u64,
            failed_sigverify_count: failed_sigverify_count as u64,
        }
    }

    /// Update tracer packet stats
    fn update_tracer_packet_stats(&mut self, deserialized_packet_results: &ReceivePacketResults) {
        // Update sigverify stats
        if let Some(new_tracer_stats) = &deserialized_packet_results.new_tracer_stats_option {
            self.sigverify_tracer_stats
                .aggregate_sigverify_tracer_packet_stats(new_tracer_stats);
        }

        self.sigverify_tracer_stats
            .aggregate_new_valid_packets(deserialized_packet_results.passed_sigverify_count);
        self.sigverify_tracer_stats
            .aggregate_newly_failed_sigverify_count(
                deserialized_packet_results.failed_sigverify_count,
            );

        // Potentially report sigverify stats
        self.sigverify_tracer_stats.report(1000);
    }

    /// Receives packet batches from sigverify stage with a timeout, and aggregates tracer packet stats
    fn receive_until(
        &self,
        recv_timeout: Duration,
        packet_limit: usize,
    ) -> Result<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>), RecvTimeoutError> {
        let start = Instant::now();
        let (mut packet_batches, mut aggregated_tracer_packet_stats_option) =
            self.packet_batch_receiver.recv_timeout(recv_timeout)?;

        let mut num_packets_received: usize = packet_batches.iter().map(|batch| batch.len()).sum();
        let mut packet_count_overflow = false;

        while start.elapsed() < recv_timeout
            && num_packets_received < packet_limit
            && !packet_count_overflow
        {
            if let Ok((packet_batch, tracer_packet_stats_option)) =
                self.packet_batch_receiver.try_recv()
            {
                trace!("got more packet batches in packet deserializer");
                let (packets_received, new_packet_count_overflowed) = num_packets_received
                    .overflowing_add(packet_batch.iter().map(|batch| batch.len()).sum());
                packet_batches.extend(packet_batch);

                if let Some(tracer_packet_stats) = &tracer_packet_stats_option {
                    if let Some(aggregated_tracer_packet_stats) =
                        &mut aggregated_tracer_packet_stats_option
                    {
                        aggregated_tracer_packet_stats.aggregate(tracer_packet_stats);
                    } else {
                        aggregated_tracer_packet_stats_option = tracer_packet_stats_option;
                    }
                }

                packet_count_overflow = new_packet_count_overflowed;
                num_packets_received = packets_received;
            } else {
                break;
            }
        }

        Ok((packet_batches, aggregated_tracer_packet_stats_option))
    }

    fn generate_packet_indexes(packet_batch: &PacketBatch) -> Vec<usize> {
        packet_batch
            .iter()
            .enumerate()
            .filter(|(_, pkt)| !pkt.meta.discard())
            .map(|(index, _)| index)
            .collect()
    }

    fn deserialize_packets<'a>(
        packet_batch: &'a PacketBatch,
        packet_indexes: &'a [usize],
    ) -> impl Iterator<Item = ImmutableDeserializedPacket> + 'a {
        packet_indexes.iter().filter_map(move |packet_index| {
            ImmutableDeserializedPacket::new(packet_batch[*packet_index].clone(), None).ok()
        })
    }
}

/// Packet deserializer that deserializes packets in a separate thread
pub struct PacketDeserializerHandle {
    deserialized_packets_receiver: DeserializedPacketBatchReceiver,
    thread_hdl: JoinHandle<()>,
}

impl DeserializedPacketBatchGetter for PacketDeserializerHandle {
    fn get_deserialized_packets(
        &mut self,
        timeout: Duration,
        packet_limit: usize,
    ) -> Result<DeserializedPacketBatch, RecvTimeoutError> {
        let start = Instant::now();
        let mut deserialized_packets = self.deserialized_packets_receiver.recv_timeout(timeout)?;

        let mut num_packets_received: usize = deserialized_packets.len();
        let mut packet_count_overflow = false;
        while start.elapsed() < timeout
            && num_packets_received < packet_limit
            && !packet_count_overflow
        {
            if let Ok(new_deserialized_packets) = self.deserialized_packets_receiver.try_recv() {
                let (packets_received, new_packet_count_overflowed) =
                    num_packets_received.overflowing_add(new_deserialized_packets.len());
                deserialized_packets.extend(new_deserialized_packets);
                packet_count_overflow = new_packet_count_overflowed;
                num_packets_received = packets_received;
            } else {
                break;
            }
        }

        Ok(deserialized_packets)
    }

    fn join(self) -> std::thread::Result<()> {
        self.thread_hdl.join()
    }
}

impl PacketDeserializerHandle {
    pub fn new(packet_batch_receiver: BankingPacketReceiver, id: u32) -> Self {
        let mut packet_deserializer = InlinePacketDeserializer::new(packet_batch_receiver, id);
        let (deserialized_packets_sender, deserialized_packets_receiver) = unbounded();
        let thread_hdl = Builder::new()
            .name(format!("solPktDesr{id:02}"))
            .spawn(move || {
                const RECV_TIMEOUT: Duration = Duration::from_millis(10);
                const PACKET_BATCHES_SIZE_LIMIT: usize = 1024 * 1024;
                loop {
                    match packet_deserializer
                        .get_deserialized_packets(RECV_TIMEOUT, PACKET_BATCHES_SIZE_LIMIT)
                    {
                        Ok(deserialized_packets) => {
                            if deserialized_packets_sender
                                .send(deserialized_packets)
                                .is_err()
                            {
                                // break if the receiver is dropped
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break, // break if sender is dropped
                        Err(RecvTimeoutError::Timeout) => {}          // do nothing
                    }
                }
            })
            .unwrap();

        Self {
            deserialized_packets_receiver,
            thread_hdl,
        }
    }
}

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

pub type DeserializedPacketSender = CrossbeamSender<Vec<ImmutableDeserializedPacket>>;

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
        let results = InlinePacketDeserializer::deserialize_and_collect_packets(&[], None);
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

        let results =
            InlinePacketDeserializer::deserialize_and_collect_packets(&packet_batches, None);
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
        packet_batches[0][0].meta.set_discard(true);

        let results =
            InlinePacketDeserializer::deserialize_and_collect_packets(&packet_batches, None);
        assert_eq!(results.deserialized_packets.len(), 1);
        assert!(results.new_tracer_stats_option.is_none());
        assert_eq!(results.passed_sigverify_count, 1);
        assert_eq!(results.failed_sigverify_count, 1);
    }
}
