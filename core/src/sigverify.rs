//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When perf-libs are available signature verification is offloaded
//! to the GPU.
//!

pub use solana_perf::sigverify::{
    count_packets_in_batches, ed25519_verify_cpu, ed25519_verify_disabled, init, TxOffset,
};
use {
    crate::sigverify_stage::SigVerifier,
    solana_perf::{cuda_runtime::PinnedVec, packet::PacketBatch, recycler::Recycler, sigverify},
    solana_sdk::packet::PacketInterface,
    std::marker::PhantomData,
};

#[derive(Clone)]
pub struct TransactionSigVerifier<P: 'static + PacketInterface> {
    recycler: Recycler<TxOffset>,
    recycler_out: Recycler<PinnedVec<u8>>,
    reject_non_vote: bool,
    _phantom: PhantomData<P>,
}

impl<P: 'static + PacketInterface> TransactionSigVerifier<P> {
    pub fn new_reject_non_vote() -> Self {
        TransactionSigVerifier {
            reject_non_vote: true,
            ..TransactionSigVerifier::default()
        }
    }
}

impl<P: 'static + PacketInterface> Default for TransactionSigVerifier<P> {
    fn default() -> Self {
        init();
        Self {
            recycler: Recycler::warmed(50, 4096),
            recycler_out: Recycler::warmed(50, 4096),
            reject_non_vote: false,
            _phantom: PhantomData::default(),
        }
    }
}

impl<P: 'static + PacketInterface> SigVerifier<P> for TransactionSigVerifier<P> {
    fn verify_batches(&self, mut batches: Vec<PacketBatch<P>>) -> Vec<PacketBatch<P>> {
        sigverify::ed25519_verify(
            &mut batches,
            &self.recycler,
            &self.recycler_out,
            self.reject_non_vote,
        );
        batches
    }
}
