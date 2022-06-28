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
    solana_perf::{cuda_runtime::PinnedVec, packet::PacketBatch, recycler::Recycler, sigverify},
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{packet::Packet, saturating_add_assign},
};

// "Thumb in the air" number to ensure reasonable latencies through sigverify.
const MAX_SIGVERIFY_PERIOD_NS: usize = 50_000_000;
// Rough timing measured from mainnet machines (w/ thread concurrency).
const DEFAULT_SIGVERIFY_NS_PER_PACKET: usize = 5_000;
pub const DEFAULT_MAX_SIGVERIFY_BATCH: usize =
    MAX_SIGVERIFY_PERIOD_NS / DEFAULT_SIGVERIFY_NS_PER_PACKET;
// Cap the absolute max packets that go into verify to avoid runaway, which could result
// in the packet queue growing too large and consuming too much memory.
const MAX_MAX_SIGVERIFY_BATCH: usize = 100_000;

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
    max_packets: usize,
    recompute_max_packet_threshold: usize,
}

impl TransactionSigVerifier {
    pub fn new_reject_non_vote(packet_sender: Sender<<Self as SigVerifier>::SendType>) -> Self {
        let mut new_self = Self::new(packet_sender);
        new_self.reject_non_vote = true;
        new_self
    }

    pub fn new(packet_sender: Sender<<Self as SigVerifier>::SendType>) -> Self {
        init();
        let recompute_max_packet_threshold =
            sigverify::VERIFY_MIN_PACKETS_PER_THREAD * get_thread_count();
        debug!(
            "verify recompute packet threshold {} with max concurrency of {}",
            recompute_max_packet_threshold,
            get_thread_count()
        );
        Self {
            packet_sender,
            tracer_packet_stats: SigverifyTracerPacketStats::default(),
            recycler: Recycler::warmed(50, 4096),
            recycler_out: Recycler::warmed(50, 4096),
            reject_non_vote: false,
            max_packets: recompute_max_packet_threshold.max(DEFAULT_MAX_SIGVERIFY_BATCH),
            recompute_max_packet_threshold,
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

    fn get_max_packets(&self) -> usize {
        self.max_packets
    }

    fn update_max_packets(&mut self, packets: u64, time: u64) {
        if packets < self.recompute_max_packet_threshold as u64 || time == 0 {
            // Need the following to compute meaninful packet processing ability:
            // 1. Meaningful timing data
            // 2. Enough packets to amortize fixed cost
            // 3. Enough packets to engage full concurrency
            return;
        }
        // Compute verify max capability based on most recent iteration.
        let ns_per_packet = time.saturating_div(packets).max(1);
        let max_verify_batch = MAX_SIGVERIFY_PERIOD_NS.saturating_div(ns_per_packet as usize);
        // Algorithm uses a moving average where each new data point has an initial
        // weighting of ~3% and grows smaller over time.
        const MAX_PACKET_WEIGHTING_SHIFT: usize = 5;
        self.max_packets = self.max_packets.saturating_sub(
            self.max_packets
                .checked_shr(MAX_PACKET_WEIGHTING_SHIFT as u32)
                .unwrap_or_default(),
        );
        self.max_packets = self.max_packets.saturating_add(
            max_verify_batch
                .checked_shr(MAX_PACKET_WEIGHTING_SHIFT as u32)
                .unwrap_or_default(),
        );
        // Protect from shedding way too many packets.
        self.max_packets = self.max_packets.max(self.recompute_max_packet_threshold);
        // Protect from  OOM due to queue growing too large.
        self.max_packets = self.max_packets.min(MAX_MAX_SIGVERIFY_BATCH);
        trace!(
            "update verify: new limit = {}, this iteration = {}, per packet = {}ns",
            self.max_packets,
            max_verify_batch,
            ns_per_packet
        )
    }
}
