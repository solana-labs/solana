use {
    super::{
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        leader_slot_metrics::LeaderSlotMetricsTracker,
        packet_deserializer::{PacketDeserializer, ReceivePacketResults},
        unprocessed_transaction_storage::UnprocessedTransactionStorage,
        BankingStageStats,
    },
    crate::{banking_trace::BankingPacketReceiver, tracer_packet_stats::TracerPacketStats},
    crossbeam_channel::RecvTimeoutError,
    solana_measure::{measure::Measure, measure_us},
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{saturating_add_assign, timing::timestamp},
    std::{
        sync::{atomic::Ordering, Arc, RwLock},
        time::Duration,
    },
};

pub struct PacketReceiver {
    id: u32,
    packet_deserializer: PacketDeserializer,
}

impl PacketReceiver {
    pub fn new(
        id: u32,
        banking_packet_receiver: BankingPacketReceiver,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        Self {
            id,
            packet_deserializer: PacketDeserializer::new(banking_packet_receiver, bank_forks),
        }
    }

    /// Receive incoming packets, push into unprocessed buffer with packet indexes
    pub fn receive_and_buffer_packets(
        &mut self,
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        banking_stage_stats: &mut BankingStageStats,
        tracer_packet_stats: &mut TracerPacketStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) -> Result<(), RecvTimeoutError> {
        let (result, recv_time_us) = measure_us!({
            let recv_timeout = Self::get_receive_timeout(unprocessed_transaction_storage);
            let mut recv_and_buffer_measure = Measure::start("recv_and_buffer");
            self.packet_deserializer
                .receive_packets(
                    recv_timeout,
                    unprocessed_transaction_storage.max_receive_size(),
                )
                // Consumes results if Ok, otherwise we keep the Err
                .map(|receive_packet_results| {
                    self.buffer_packets(
                        receive_packet_results,
                        unprocessed_transaction_storage,
                        banking_stage_stats,
                        tracer_packet_stats,
                        slot_metrics_tracker,
                    );
                    recv_and_buffer_measure.stop();

                    // Only incremented if packets are received
                    banking_stage_stats
                        .receive_and_buffer_packets_elapsed
                        .fetch_add(recv_and_buffer_measure.as_us(), Ordering::Relaxed);
                })
        });

        slot_metrics_tracker.increment_receive_and_buffer_packets_us(recv_time_us);

        result
    }

    fn get_receive_timeout(
        unprocessed_transaction_storage: &UnprocessedTransactionStorage,
    ) -> Duration {
        // Gossip thread will almost always not wait because the transaction storage will most likely not be empty
        if !unprocessed_transaction_storage.is_empty() {
            // If there are buffered packets, run the equivalent of try_recv to try reading more
            // packets. This prevents starving BankingStage::consume_buffered_packets due to
            // buffered_packet_batches containing transactions that exceed the cost model for
            // the current bank.
            Duration::from_millis(0)
        } else {
            // Default wait time
            Duration::from_millis(100)
        }
    }

    fn buffer_packets(
        &self,
        ReceivePacketResults {
            deserialized_packets,
            new_tracer_stats_option,
            passed_sigverify_count,
            failed_sigverify_count,
        }: ReceivePacketResults,
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        banking_stage_stats: &mut BankingStageStats,
        tracer_packet_stats: &mut TracerPacketStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) {
        let packet_count = deserialized_packets.len();
        debug!("@{:?} txs: {} id: {}", timestamp(), packet_count, self.id);

        if let Some(new_sigverify_stats) = &new_tracer_stats_option {
            tracer_packet_stats.aggregate_sigverify_tracer_packet_stats(new_sigverify_stats);
        }

        // Track all the packets incoming from sigverify, both valid and invalid
        slot_metrics_tracker.increment_total_new_valid_packets(passed_sigverify_count);
        slot_metrics_tracker.increment_newly_failed_sigverify_count(failed_sigverify_count);

        let mut dropped_packets_count = 0;
        let mut newly_buffered_packets_count = 0;
        let mut newly_buffered_forwarded_packets_count = 0;
        Self::push_unprocessed(
            unprocessed_transaction_storage,
            deserialized_packets,
            &mut dropped_packets_count,
            &mut newly_buffered_packets_count,
            &mut newly_buffered_forwarded_packets_count,
            banking_stage_stats,
            slot_metrics_tracker,
            tracer_packet_stats,
        );

        banking_stage_stats
            .receive_and_buffer_packets_count
            .fetch_add(packet_count, Ordering::Relaxed);
        banking_stage_stats
            .dropped_packets_count
            .fetch_add(dropped_packets_count, Ordering::Relaxed);
        banking_stage_stats
            .newly_buffered_packets_count
            .fetch_add(newly_buffered_packets_count, Ordering::Relaxed);
        banking_stage_stats
            .current_buffered_packets_count
            .swap(unprocessed_transaction_storage.len(), Ordering::Relaxed);
    }

    fn push_unprocessed(
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        deserialized_packets: Vec<ImmutableDeserializedPacket>,
        dropped_packets_count: &mut usize,
        newly_buffered_packets_count: &mut usize,
        newly_buffered_forwarded_packets_count: &mut usize,
        banking_stage_stats: &mut BankingStageStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        tracer_packet_stats: &mut TracerPacketStats,
    ) {
        if !deserialized_packets.is_empty() {
            let _ = banking_stage_stats
                .batch_packet_indexes_len
                .increment(deserialized_packets.len() as u64);

            *newly_buffered_packets_count += deserialized_packets.len();
            *newly_buffered_forwarded_packets_count += deserialized_packets
                .iter()
                .filter(|p| p.original_packet().meta().forwarded())
                .count();
            slot_metrics_tracker
                .increment_newly_buffered_packets_count(deserialized_packets.len() as u64);

            let insert_packet_batches_summary =
                unprocessed_transaction_storage.insert_batch(deserialized_packets);
            slot_metrics_tracker
                .accumulate_insert_packet_batches_summary(&insert_packet_batches_summary);
            saturating_add_assign!(
                *dropped_packets_count,
                insert_packet_batches_summary.total_dropped_packets()
            );
            tracer_packet_stats.increment_total_exceeded_banking_stage_buffer(
                insert_packet_batches_summary.dropped_tracer_packets(),
            );
        }
    }
}
