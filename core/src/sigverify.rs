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

    use rayon::iter::IntoParallelIterator;
    use rayon::iter::ParallelIterator;
    use solana_perf::packet::to_packets;
    use solana_perf::perf_libs::init_cuda;
    use solana_perf::recycler::Recycler;
    use solana_perf::sigverify;
    use solana_perf::test_tx::test_tx;
    use std::thread::Builder;
    use std::time::Instant;

    fn run_verify(
        batch_size: usize,
        recycler: &Recycler<TxOffset>,
        recycler_out: &Recycler<PinnedVec<u8>>,
        use_gpu: bool,
    ) {
        if use_gpu {
            init_cuda();
        }
        let tx = test_tx();
        let batches = to_packets(&vec![tx; batch_size]);
        let now = Instant::now();
        let _ans = sigverify::ed25519_verify(&batches, &recycler, &recycler_out);
        println!(
            "verifying {:?} txs took {:?} with GPU:{:?} ",
            batch_size,
            now.elapsed(),
            use_gpu
        );
    }

    #[test]
    fn test_sigverify() {
        //        let recycler = Recycler::default();
        //        let recycler_out = Recycler::default();
        //first do it without cuda
        //128
        println!("manual thread creation");
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let recycler = Recycler::warmed(10, 256);
                let recycler_out = Recycler::warmed(10, 256);
                Builder::new()
                    .name("solana-bench-verify-tx".to_string())
                    .spawn(move || run_verify(128, &recycler, &recycler_out, false))
                    .unwrap()
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());

        //        println!("par_iter thread handling");
        //        (0..16)
        //            .into_par_iter()
        //            .for_each(|_| run_verify(128, &recycler, &recycler_out, false));

        //512
        //        run_verify(512, &recycler, &recycler_out, false);
        //        //1024
        //        run_verify(1024, &recycler, &recycler_out, false);
        //        //2048
        //        run_verify(2048, &recycler, &recycler_out, false);
        //        //15360
        //        run_verify(15360, &recycler, &recycler_out, false);
        //128
        //        (0..16)
        //            .into_par_iter()
        //            .for_each(|_| run_verify(15360, &recycler, &recycler_out, false));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let recycler = Recycler::warmed(10, 256);
                let recycler_out = Recycler::warmed(10, 256);
                Builder::new()
                    .name("solana-bench-verify-tx".to_string())
                    .spawn(move || run_verify(15360, &recycler, &recycler_out, false))
                    .unwrap()
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());

        //        //128
        //        run_verify(128, &recycler, &recycler_out, true);
        //        //128
        //        run_verify(128, &recycler, &recycler_out, true);
        //        (0..16)
        //            .into_par_iter()
        //            .for_each(|_| run_verify(128, &recycler, &recycler_out, true));
        //        //512
        //        run_verify(512, &recycler, &recycler_out, true);
        //        //1024
        //        run_verify(1024, &recycler, &recycler_out, true);
        //        //2048
        //        run_verify(2048, &recycler, &recycler_out, true);
        //        //15360
        //        run_verify(15360, &recycler, &recycler_out, true);
        //        (0..16)
        //            .into_par_iter()
        //            .for_each(|_| run_verify(15360, &recycler, &recycler_out, true));
        //128
        let recycler = Recycler::warmed(10, 256);
        let recycler_out = Recycler::warmed(10, 256);
        run_verify(128, &recycler, &recycler_out, true);
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let recycler = Recycler::warmed(10, 256);
                let recycler_out = Recycler::warmed(10, 256);
                Builder::new()
                    .name("solana-bench-verify-tx".to_string())
                    .spawn(move || run_verify(128, &recycler, &recycler_out, true))
                    .unwrap()
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let recycler = Recycler::warmed(10, 256);
                let recycler_out = Recycler::warmed(10, 256);
                Builder::new()
                    .name("solana-bench-verify-tx".to_string())
                    .spawn(move || run_verify(15360, &recycler, &recycler_out, true))
                    .unwrap()
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
    }

}
