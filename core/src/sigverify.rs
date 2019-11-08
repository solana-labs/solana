//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When perf-libs are available signature verification is offloaded
//! to the GPU.
//!

use crate::sigverify_stage::SigVerifier;
use solana_perf::cuda_runtime::PinnedVec;
use solana_perf::packet::Packets;
use solana_perf::recycler::Recycler;
use solana_perf::sigverify;
pub use solana_perf::sigverify::{
    batch_size, ed25519_verify_cpu, ed25519_verify_disabled, init, TxOffset,
};

#[derive(Clone)]
pub struct TransactionSigVerifier {
    recycler: Recycler<TxOffset>,
    recycler_out: Recycler<PinnedVec<u8>>,
}

impl Default for TransactionSigVerifier {
    fn default() -> Self {
        init();
        Self {
            recycler: Recycler::warmed(50, 4096),
            recycler_out: Recycler::warmed(50, 4096),
        }
    }
}

impl SigVerifier for TransactionSigVerifier {
    fn verify_batch(&self, mut batch: Vec<Packets>) -> Vec<Packets> {
        let r = sigverify::ed25519_verify(&batch, &self.recycler, &self.recycler_out);
        mark_disabled(&mut batch, &r);
        batch
    }
}

pub fn mark_disabled(batches: &mut Vec<Packets>, r: &[Vec<u8>]) {
    batches.iter_mut().zip(r).for_each(|(b, v)| {
        b.packets
            .iter_mut()
            .zip(v)
            .for_each(|(p, f)| p.meta.discard = *f == 0)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_perf::packet::Packet;

    #[test]
    fn test_mark_disabled() {
        let mut batch = Packets::default();
        batch.packets.push(Packet::default());
        let mut batches: Vec<Packets> = vec![batch];
        mark_disabled(&mut batches, &[vec![0]]);
        assert_eq!(batches[0].packets[0].meta.discard, true);
        mark_disabled(&mut batches, &[vec![1]]);
        assert_eq!(batches[0].packets[0].meta.discard, false);
    }
}
