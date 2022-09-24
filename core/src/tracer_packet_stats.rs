use {
    crate::sigverify::SigverifyTracerPacketStats,
    solana_sdk::{pubkey::Pubkey, saturating_add_assign, timing::timestamp},
    std::collections::HashSet,
};

#[derive(Debug, Default)]
pub struct BankingStageTracerPacketStats {
    total_exceeded_banking_stage_buffer: usize,
    // This is the total number of tracer packets removed from the buffer
    // after a leader's set of slots. Of these, only a subset that were in
    // the buffer were actually forwardable (didn't arrive on forward port and haven't been
    // forwarded before)
    total_cleared_from_buffer_after_forward: usize,
    total_forwardable_tracer_packets: usize,
    forward_target_leaders: HashSet<Pubkey>,
}

#[derive(Debug, Default)]
pub struct ModifiableTracerPacketStats {
    sigverify_tracer_packet_stats: SigverifyTracerPacketStats,
    banking_stage_tracer_packet_stats: BankingStageTracerPacketStats,
}

#[derive(Debug, Default)]
pub struct TracerPacketStats {
    id: u32,
    last_report: u64,
    modifiable_tracer_packet_stats: Option<ModifiableTracerPacketStats>,
}

impl TracerPacketStats {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub fn get_mutable_stats(&mut self) -> &mut ModifiableTracerPacketStats {
        if self.modifiable_tracer_packet_stats.is_none() {
            self.modifiable_tracer_packet_stats = Some(ModifiableTracerPacketStats::default());
        }
        self.modifiable_tracer_packet_stats.as_mut().unwrap()
    }

    pub fn aggregate_sigverify_tracer_packet_stats(
        &mut self,
        new_sigverify_stats: &SigverifyTracerPacketStats,
    ) {
        if !new_sigverify_stats.is_default() {
            let stats = self.get_mutable_stats();
            stats
                .sigverify_tracer_packet_stats
                .aggregate(new_sigverify_stats);
        }
    }

    pub fn increment_total_exceeded_banking_stage_buffer(
        &mut self,
        total_exceeded_banking_stage_buffer: usize,
    ) {
        if total_exceeded_banking_stage_buffer != 0 {
            let stats = self.get_mutable_stats();
            saturating_add_assign!(
                stats
                    .banking_stage_tracer_packet_stats
                    .total_exceeded_banking_stage_buffer,
                total_exceeded_banking_stage_buffer
            );
        }
    }

    pub fn increment_total_cleared_from_buffer_after_forward(
        &mut self,
        total_cleared_from_buffer_after_forward: usize,
    ) {
        if total_cleared_from_buffer_after_forward != 0 {
            let stats = self.get_mutable_stats();
            saturating_add_assign!(
                stats
                    .banking_stage_tracer_packet_stats
                    .total_cleared_from_buffer_after_forward,
                total_cleared_from_buffer_after_forward
            );
        }
    }

    pub fn increment_total_forwardable_tracer_packets(
        &mut self,
        total_forwardable_tracer_packets: usize,
        forward_target_leader: Pubkey,
    ) {
        if total_forwardable_tracer_packets != 0 {
            let stats = self.get_mutable_stats();
            stats
                .banking_stage_tracer_packet_stats
                .forward_target_leaders
                .insert(forward_target_leader);
            saturating_add_assign!(
                stats
                    .banking_stage_tracer_packet_stats
                    .total_forwardable_tracer_packets,
                total_forwardable_tracer_packets
            );
        }
    }

    pub fn report(&mut self, report_interval_ms: u64) {
        let now = timestamp();
        const LEADER_REPORT_LIMIT: usize = 4;
        if now.saturating_sub(self.last_report) > report_interval_ms {
            // We don't want to report unless we actually saw/forwarded a tracer packet
            // to prevent noisy metrics
            if let Some(modifiable_tracer_packet_stats) = self.modifiable_tracer_packet_stats.take()
            {
                datapoint_info!(
                    "tracer-packet-stats",
                    ("id", self.id, i64),
                    (
                        "total_removed_before_sigverify",
                        modifiable_tracer_packet_stats
                            .sigverify_tracer_packet_stats
                            .total_removed_before_sigverify_stage as i64,
                        i64
                    ),
                    (
                        "total_tracer_packets_received_in_sigverify",
                        modifiable_tracer_packet_stats
                            .sigverify_tracer_packet_stats
                            .total_tracer_packets_received_in_sigverify_stage
                            as i64,
                        i64
                    ),
                    (
                        "total_tracer_packets_deduped_in_sigverify",
                        modifiable_tracer_packet_stats
                            .sigverify_tracer_packet_stats
                            .total_tracer_packets_deduped as i64,
                        i64
                    ),
                    (
                        "total_excess_tracer_packets_discarded_in_sigverify",
                        modifiable_tracer_packet_stats
                            .sigverify_tracer_packet_stats
                            .total_excess_tracer_packets as i64,
                        i64
                    ),
                    (
                        "total_tracker_packets_passed_sigverify",
                        modifiable_tracer_packet_stats
                            .sigverify_tracer_packet_stats
                            .total_tracker_packets_passed_sigverify as i64,
                        i64
                    ),
                    (
                        "total_exceeded_banking_stage_buffer",
                        modifiable_tracer_packet_stats
                            .banking_stage_tracer_packet_stats
                            .total_exceeded_banking_stage_buffer as i64,
                        i64
                    ),
                    (
                        "total_cleared_from_buffer_after_forward",
                        modifiable_tracer_packet_stats
                            .banking_stage_tracer_packet_stats
                            .total_cleared_from_buffer_after_forward as i64,
                        i64
                    ),
                    (
                        "total_forwardable_tracer_packets",
                        modifiable_tracer_packet_stats
                            .banking_stage_tracer_packet_stats
                            .total_forwardable_tracer_packets as i64,
                        i64
                    ),
                    (
                        "exceeded_expected_forward_leader_count",
                        modifiable_tracer_packet_stats
                            .banking_stage_tracer_packet_stats
                            .forward_target_leaders
                            .len()
                            > LEADER_REPORT_LIMIT,
                        bool
                    ),
                    (
                        "forward_target_leaders",
                        itertools::Itertools::intersperse(
                            modifiable_tracer_packet_stats
                                .banking_stage_tracer_packet_stats
                                .forward_target_leaders
                                .iter()
                                .take(LEADER_REPORT_LIMIT)
                                .map(|leader_pubkey| leader_pubkey.to_string()),
                            ", ".to_string()
                        )
                        .collect::<String>(),
                        String
                    )
                );

                let id = self.id;
                *self = Self::new(id);
                self.last_report = timestamp();
            }
        }
    }
}
