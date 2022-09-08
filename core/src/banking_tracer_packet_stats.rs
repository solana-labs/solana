use {
    solana_sdk::{pubkey::Pubkey, saturating_add_assign, timing::AtomicInterval},
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

#[derive(Debug)]
pub struct IntervalBankingStageTracerPacketStats {
    id: u32,
    last_report: AtomicInterval,
    stats: BankingStageTracerPacketStats,
}

impl IntervalBankingStageTracerPacketStats {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            last_report: AtomicInterval::default(),
            stats: BankingStageTracerPacketStats::default(),
        }
    }

    pub fn increment_total_exceeded_banking_stage_buffer(
        &mut self,
        total_exceeded_banking_stage_buffer: usize,
    ) {
        if total_exceeded_banking_stage_buffer != 0 {
            saturating_add_assign!(
                self.stats.total_exceeded_banking_stage_buffer,
                total_exceeded_banking_stage_buffer
            );
        }
    }

    pub fn increment_total_cleared_from_buffer_after_forward(
        &mut self,
        total_cleared_from_buffer_after_forward: usize,
    ) {
        if total_cleared_from_buffer_after_forward != 0 {
            saturating_add_assign!(
                self.stats.total_cleared_from_buffer_after_forward,
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
            self.stats
                .forward_target_leaders
                .insert(forward_target_leader);
            saturating_add_assign!(
                self.stats.total_forwardable_tracer_packets,
                total_forwardable_tracer_packets
            );
        }
    }

    pub fn report(&mut self, report_interval_ms: u64) {
        const LEADER_REPORT_LIMIT: usize = 4;

        if self.received_data() && self.last_report.should_update(report_interval_ms) {
            datapoint_info!(
                "banking_stage_tracer_packet_stats",
                ("id", self.id, i64),
                (
                    "total_exceeded_banking_stage_buffer",
                    self.stats.total_exceeded_banking_stage_buffer,
                    i64
                ),
                (
                    "total_cleared_from_buffer_after_forward",
                    self.stats.total_cleared_from_buffer_after_forward,
                    i64
                ),
                (
                    "total_forwardable_tracer_packets",
                    self.stats.total_forwardable_tracer_packets,
                    i64
                ),
                (
                    "exceeded_expected_forward_leader_count",
                    self.stats.forward_target_leaders.len() > LEADER_REPORT_LIMIT,
                    bool
                ),
                (
                    "forward_target_leaders",
                    self.stats.forward_target_leaders.len() as i64,
                    i64
                ),
                (
                    "forward_target_leaders",
                    itertools::Itertools::intersperse(
                        self.stats
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

            self.stats = BankingStageTracerPacketStats::default();
        }
    }

    fn received_data(&self) -> bool {
        self.stats.total_cleared_from_buffer_after_forward != 0
            || self.stats.total_exceeded_banking_stage_buffer != 0
            || self.stats.total_forwardable_tracer_packets != 0
            || !self.stats.forward_target_leaders.is_empty()
    }
}
