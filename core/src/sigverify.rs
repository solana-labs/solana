//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When perf-libs are available signature verification is offloaded
//! to the GPU.
//!

pub use solana_perf::sigverify::{
    ed25519_verify_cpu, ed25519_verify_disabled, init, TxOffset,
};
use {
<<<<<<< HEAD
    crate::{
        banking_trace::{BankingPacketBatch, BankingPacketSender},
        sigverify_stage::{SigVerifier, SigVerifyServiceError},
    },
=======
    crate::{banking_stage::BankingPacketBatch, sigverify_stage::SigVerifyServiceError},
    crossbeam_channel::Sender,
<<<<<<< HEAD
>>>>>>> e59cd309c9 (Rip out SigVerifier trait)
    solana_perf::{cuda_runtime::PinnedVec, packet::PacketBatch, recycler::Recycler, sigverify},
=======
    solana_perf::{
        cuda_runtime::PinnedVec, packet::PacketBatch, recycler::Recycler, sigverify,
        tx_packet_batch::TxPacketViewMut,
    },
>>>>>>> 4ba1998ccb (Plumbing TxPacketBatch through Tx packet path)
    solana_sdk::{packet::Packet, saturating_add_assign},
};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

pub struct TransactionSigVerifier {
<<<<<<< HEAD
    packet_sender: BankingPacketSender,
=======
    packet_sender: Sender<BankingPacketBatch>,
>>>>>>> e59cd309c9 (Rip out SigVerifier trait)
    tracer_packet_stats: SigverifyTracerPacketStats,
    recycler: Recycler<TxOffset>,
    recycler_out: Recycler<PinnedVec<u8>>,
    reject_non_vote: bool,
}

impl TransactionSigVerifier {
<<<<<<< HEAD
    pub fn new_reject_non_vote(packet_sender: BankingPacketSender) -> Self {
=======
    pub fn new_reject_non_vote(packet_sender: Sender<BankingPacketBatch>) -> Self {
>>>>>>> e59cd309c9 (Rip out SigVerifier trait)
        let mut new_self = Self::new(packet_sender);
        new_self.reject_non_vote = true;
        new_self
    }

<<<<<<< HEAD
    pub fn new(packet_sender: BankingPacketSender) -> Self {
=======
    pub fn new(packet_sender: Sender<BankingPacketBatch>) -> Self {
>>>>>>> e59cd309c9 (Rip out SigVerifier trait)
        init();
        Self {
            packet_sender,
            tracer_packet_stats: SigverifyTracerPacketStats::default(),
            recycler: Recycler::warmed(50, 4096),
            recycler_out: Recycler::warmed(50, 4096),
            reject_non_vote: false,
        }
    }

    #[inline(always)]
    pub fn process_received_packet(
        &mut self,
        packet: &TxPacketViewMut,
        removed_before_sigverify_stage: bool,
        is_dup: bool,
    ) {
        // TODO: re-enable tracer check
        //sigverify::check_for_tracer_packet(packet);
        if packet.meta().is_tracer_packet() {
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
    pub fn process_excess_packet(&mut self, packet: &TxPacketViewMut) {
        if packet.meta().is_tracer_packet() {
            self.tracer_packet_stats.total_excess_tracer_packets += 1;
        }
    }

    #[inline(always)]
    pub fn process_passed_sigverify_packet(&mut self, packet: &Packet) {
        if packet.meta().is_tracer_packet() {
            self.tracer_packet_stats
                .total_tracker_packets_passed_sigverify += 1;
        }
    }

    pub fn send_packets(
        &mut self,
        packet_batches: Vec<PacketBatch>,
    ) -> Result<(), SigVerifyServiceError<BankingPacketBatch>> {
        let tracer_packet_stats_to_send = std::mem::take(&mut self.tracer_packet_stats);
        self.packet_sender.send(BankingPacketBatch::new((
            packet_batches,
            Some(tracer_packet_stats_to_send),
        )))?;
        Ok(())
    }

    pub fn verify_batches(
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
}
