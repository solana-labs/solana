use {
    super::BankingStageStats,
    crate::{
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        leader_slot_banking_stage_metrics::LeaderSlotMetricsTracker,
        packet_deserializer::{PacketDeserializer, ReceivePacketResults},
        tracer_packet_stats::TracerPacketStats,
        unprocessed_transaction_storage::UnprocessedTransactionStorage,
    },
    crossbeam_channel::RecvTimeoutError,
    solana_measure::measure::Measure,
    solana_sdk::{
        saturating_add_assign,
        timing::{duration_as_ms, timestamp},
    },
    std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    },
};

pub struct PacketReceiver;

impl PacketReceiver {
    #[allow(clippy::too_many_arguments)]
    /// Receive incoming packets, push into unprocessed buffer with packet indexes
    pub fn receive_and_buffer_packets(
        packet_deserializer: &mut PacketDeserializer,
        recv_start: &mut Instant,
        recv_timeout: Duration,
        id: u32,
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        banking_stage_stats: &mut BankingStageStats,
        tracer_packet_stats: &mut TracerPacketStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) -> Result<(), RecvTimeoutError> {
        let mut recv_time = Measure::start("receive_and_buffer_packets_recv");
        let ReceivePacketResults {
            deserialized_packets,
            new_tracer_stats_option,
            passed_sigverify_count,
            failed_sigverify_count,
        } = packet_deserializer.handle_received_packets(
            recv_timeout,
            unprocessed_transaction_storage.max_receive_size(),
        )?;
        let packet_count = deserialized_packets.len();
        debug!(
            "@{:?} process start stalled for: {:?}ms txs: {} id: {}",
            timestamp(),
            duration_as_ms(&recv_start.elapsed()),
            packet_count,
            id,
        );

        if let Some(new_sigverify_stats) = &new_tracer_stats_option {
            tracer_packet_stats.aggregate_sigverify_tracer_packet_stats(new_sigverify_stats);
        }

        // Track all the packets incoming from sigverify, both valid and invalid
        slot_metrics_tracker.increment_total_new_valid_packets(passed_sigverify_count);
        slot_metrics_tracker.increment_newly_failed_sigverify_count(failed_sigverify_count);

        let mut dropped_packets_count = 0;
        let mut newly_buffered_packets_count = 0;
        Self::push_unprocessed(
            unprocessed_transaction_storage,
            deserialized_packets,
            &mut dropped_packets_count,
            &mut newly_buffered_packets_count,
            banking_stage_stats,
            slot_metrics_tracker,
            tracer_packet_stats,
        );
        recv_time.stop();

        banking_stage_stats
            .receive_and_buffer_packets_elapsed
            .fetch_add(recv_time.as_us(), Ordering::Relaxed);
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
        *recv_start = Instant::now();
        Ok(())
    }

    fn push_unprocessed(
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        deserialized_packets: Vec<ImmutableDeserializedPacket>,
        dropped_packets_count: &mut usize,
        newly_buffered_packets_count: &mut usize,
        banking_stage_stats: &mut BankingStageStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        tracer_packet_stats: &mut TracerPacketStats,
    ) {
        if !deserialized_packets.is_empty() {
            let _ = banking_stage_stats
                .batch_packet_indexes_len
                .increment(deserialized_packets.len() as u64);

            *newly_buffered_packets_count += deserialized_packets.len();
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
