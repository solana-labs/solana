//! Deserializes packets from sigverify stage. Owned by banking stage.

use {
    crate::{
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        sigverify::SigverifyTracerPacketStats,
    },
    crossbeam_channel::{
        Receiver as CrossbeamReceiver, RecvTimeoutError, Sender as CrossbeamSender,
    },
    solana_perf::packet::PacketBatch,
    std::{
        thread::Builder,
        time::{Duration, Instant},
    },
};

pub type BankingPacketBatch = (Vec<PacketBatch>, Option<SigverifyTracerPacketStats>);
pub type BankingPacketReceiver = CrossbeamReceiver<BankingPacketBatch>;

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

pub type DeserializedPacketSender = CrossbeamSender<ReceivePacketResults>;

/// Wrapper for arguments to create a group of packet deserializer threads.
pub struct PacketDeserializationGroup {
    pub group_name: &'static str,
    pub receiver: BankingPacketReceiver,
    pub sender: DeserializedPacketSender,
    pub thread_count: usize,
}

/// Receive packets from sigverify stage, deserialize them, and send to banking stage.
pub struct PacketDeserializerStage {
    /// Packet deserialization threads
    pub thread_handles: Vec<std::thread::JoinHandle<()>>,
}

impl PacketDeserializerStage {
    pub(crate) fn new(deserialization_groups: &[PacketDeserializationGroup]) -> Self {
        let mut thread_handles = Vec::with_capacity(
            deserialization_groups
                .iter()
                .map(|group| group.thread_count)
                .sum(),
        );

        for group in deserialization_groups {
            for thread_index in 0..group.thread_count {
                let receiver = group.receiver.clone();
                let sender = group.sender.clone();
                let packet_deserializer = PacketDeserializer::new(receiver);
                const RECV_TIMEOUT: Duration = Duration::from_millis(10);
                thread_handles.push(
                    Builder::new()
                        .name(format!("{}{:02}", group.group_name, thread_index))
                        .spawn(move || {
                            // Loop while receiving from sigverify
                            loop {
                                match packet_deserializer.handle_received_packets(RECV_TIMEOUT) {
                                    Ok(deserialized_packet_results) => {
                                        // Send deserialized packets to banking stage, exit on error (if banking threads stopped)
                                        if sender.send(deserialized_packet_results).is_err() {
                                            break;
                                        }
                                    }
                                    Err(err) => match err {
                                        RecvTimeoutError::Disconnected => break,
                                        RecvTimeoutError::Timeout => {}
                                    },
                                }
                            }
                        })
                        .unwrap(),
                );
            }
        }

        Self { thread_handles }
    }

    pub(crate) fn join(self) -> std::thread::Result<()> {
        self.thread_handles
            .into_iter()
            .try_for_each(std::thread::JoinHandle::join)
    }
}

pub struct PacketDeserializer {
    /// Receiver for packet batches from sigverify stage
    packet_batch_receiver: BankingPacketReceiver,
}

impl PacketDeserializer {
    pub fn new(packet_batch_receiver: BankingPacketReceiver) -> Self {
        Self {
            packet_batch_receiver,
        }
    }

    /// Handles receiving packet batches from sigverify and returns a vector of deserialized packets
    pub fn handle_received_packets(
        &self,
        recv_timeout: Duration,
    ) -> Result<ReceivePacketResults, RecvTimeoutError> {
        let (packet_batches, sigverify_tracer_stats_option) = self.receive_until(recv_timeout)?;
        Ok(Self::deserialize_and_collect_packets(
            &packet_batches,
            sigverify_tracer_stats_option,
        ))
    }

    /// Deserialize packet batches and collect them into ReceivePacketResults
    pub fn deserialize_and_collect_packets(
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

    /// Receives packet batches from sigverify stage with a timeout, and aggregates tracer packet stats
    fn receive_until(
        &self,
        recv_timeout: Duration,
    ) -> Result<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>), RecvTimeoutError> {
        let start = Instant::now();
        let (mut packet_batches, mut aggregated_tracer_packet_stats_option) =
            self.packet_batch_receiver.recv_timeout(recv_timeout)?;

        let mut num_packets_received: usize = packet_batches.iter().map(|batch| batch.len()).sum();
        while let Ok((packet_batch, tracer_packet_stats_option)) =
            self.packet_batch_receiver.try_recv()
        {
            trace!("got more packet batches in packet deserializer");
            let (packets_received, packet_count_overflowed) = num_packets_received
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

            if start.elapsed() >= recv_timeout || packet_count_overflowed {
                break;
            }
            num_packets_received = packets_received;
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
        let results = PacketDeserializer::deserialize_and_collect_packets(&[], None);
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

        let results = PacketDeserializer::deserialize_and_collect_packets(&packet_batches, None);
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

        let results = PacketDeserializer::deserialize_and_collect_packets(&packet_batches, None);
        assert_eq!(results.deserialized_packets.len(), 1);
        assert!(results.new_tracer_stats_option.is_none());
        assert_eq!(results.passed_sigverify_count, 1);
        assert_eq!(results.failed_sigverify_count, 1);
    }
}
