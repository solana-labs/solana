//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When perf-libs are available signature verification is offloaded
//! to the GPU.
//!

pub use solana_perf::sigverify::{
    count_packets_in_batches, ed25519_verify_cpu, ed25519_verify_disabled, init, TxOffset,
};
use {
    crate::{
        banking_stage::BankingPacketBatch,
        sigverify_stage::{SigVerifier, SigVerifyServiceError},
    },
    crossbeam_channel::Sender,
    solana_perf::{
        cuda_runtime::PinnedVec, packet::PacketBatch, perf_libs, recycler::Recycler, sigverify,
    },
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{packet::Packet, saturating_add_assign},
};

const SIGVERIFY_TIME_BUDGET_US: usize = 50_000;
// 50ms/(25us/packet) = 2000 packets
const MAX_SIGVERIFY_BATCH_DEFAULT: usize = 2_000;
// 1000 packets / 50ms = 20k TPS floor
const MAX_SIGVERIFY_BATCH_MIN: usize = 1_000;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SigverifyTracerPacketStats {
    pub total_removed_before_sigverify_stage: usize,
    pub total_tracer_packets_received_in_sigverify_stage: usize,
    pub total_tracer_packets_deduped: usize,
    pub total_excess_tracer_packets: usize,
    pub total_tracker_packets_passed_sigverify: usize,
}

impl SigverifyTracerPacketStats {
    pub fn is_default(&self) -> bool {
        *self == SigverifyTracerPacketStats::default()
    }

    pub fn aggregate(&mut self, other: &SigverifyTracerPacketStats) {
        saturating_add_assign!(
            self.total_removed_before_sigverify_stage,
            other.total_removed_before_sigverify_stage
        );
        saturating_add_assign!(
            self.total_tracer_packets_received_in_sigverify_stage,
            other.total_tracer_packets_received_in_sigverify_stage
        );
        saturating_add_assign!(
            self.total_tracer_packets_deduped,
            other.total_tracer_packets_deduped
        );
        saturating_add_assign!(
            self.total_excess_tracer_packets,
            other.total_excess_tracer_packets
        );
        saturating_add_assign!(
            self.total_tracker_packets_passed_sigverify,
            other.total_tracker_packets_passed_sigverify
        );
    }
}

#[derive(Clone)]
pub struct TransactionSigVerifier {
    packet_sender: Sender<<Self as SigVerifier>::SendType>,
    tracer_packet_stats: SigverifyTracerPacketStats,
    recycler: Recycler<TxOffset>,
    recycler_out: Recycler<PinnedVec<u8>>,
    reject_non_vote: bool,
    max_verify_batch: usize,
}

impl TransactionSigVerifier {
    pub fn new_reject_non_vote(packet_sender: Sender<<Self as SigVerifier>::SendType>) -> Self {
        let mut new_self = Self::new(packet_sender);
        new_self.reject_non_vote = true;
        new_self
    }

    pub fn new(packet_sender: Sender<<Self as SigVerifier>::SendType>) -> Self {
        init();
        Self {
            packet_sender,
            tracer_packet_stats: SigverifyTracerPacketStats::default(),
            recycler: Recycler::warmed(50, 4096),
            recycler_out: Recycler::warmed(50, 4096),
            reject_non_vote: false,
            max_verify_batch: MAX_SIGVERIFY_BATCH_DEFAULT,
        }
    }
}

impl SigVerifier for TransactionSigVerifier {
    type SendType = BankingPacketBatch;

    #[inline(always)]
    fn process_received_packet(
        &mut self,
        packet: &mut Packet,
        removed_before_sigverify_stage: bool,
        is_dup: bool,
    ) {
        sigverify::check_for_tracer_packet(packet);
        if packet.meta.is_tracer_packet() {
            if removed_before_sigverify_stage {
                self.tracer_packet_stats
                    .total_removed_before_sigverify_stage += 1;
            } else {
                self.tracer_packet_stats
                    .total_tracer_packets_received_in_sigverify_stage += 1;
                if is_dup {
                    self.tracer_packet_stats.total_tracer_packets_deduped += 1;
                }
            }
        }
    }

    #[inline(always)]
    fn process_excess_packet(&mut self, packet: &Packet) {
        if packet.meta.is_tracer_packet() {
            self.tracer_packet_stats.total_excess_tracer_packets += 1;
        }
    }

    #[inline(always)]
    fn process_passed_sigverify_packet(&mut self, packet: &Packet) {
        if packet.meta.is_tracer_packet() {
            self.tracer_packet_stats
                .total_tracker_packets_passed_sigverify += 1;
        }
    }

    fn send_packets(
        &mut self,
        packet_batches: Vec<PacketBatch>,
    ) -> Result<(), SigVerifyServiceError<Self::SendType>> {
        let mut tracer_packet_stats_to_send = SigverifyTracerPacketStats::default();
        std::mem::swap(
            &mut tracer_packet_stats_to_send,
            &mut self.tracer_packet_stats,
        );
        self.packet_sender
            .send((packet_batches, Some(tracer_packet_stats_to_send)))?;
        Ok(())
    }

    fn verify_batches(
        &self,
        mut batches: Vec<PacketBatch>,
        valid_packets: usize,
    ) -> Vec<PacketBatch> {
        sigverify::ed25519_verify(
            &mut batches,
            &self.recycler,
            &self.recycler_out,
            self.reject_non_vote,
            valid_packets,
        );
        batches
    }

    fn get_max_verify_batch(&self) -> usize {
        warn!("max_verify_batch is {}", self.max_verify_batch);
        self.max_verify_batch
    }

    fn update_max_verify_batch(
        &mut self,
        verify_time_us: usize,
        packet_count: usize,
        active_thread_count: usize,
    ) {
        if packet_count > 0 && verify_time_us > 0 {
            let packets_per_iteration = packet_count * SIGVERIFY_TIME_BUDGET_US / verify_time_us;
            let packets_per_iteration_max = if perf_libs::api().is_none() {
                packets_per_iteration / active_thread_count.min(get_thread_count())
                    * get_thread_count()
            } else {
                // TODO: Scale this using number of GPU cores active during last iteration
                packets_per_iteration.max(MAX_SIGVERIFY_BATCH_DEFAULT)
            };
            self.max_verify_batch -= self.max_verify_batch >> 5;
            self.max_verify_batch += packets_per_iteration_max >> 5;
            // Establish an absolute floor to avoid death spiral overly restricting verification packet count.
            self.max_verify_batch = self.max_verify_batch.max(MAX_SIGVERIFY_BATCH_MIN);
        }
    }
}
