//! The `sigverify_stage` implements the signature verification stage of the TPU. It
//! receives a list of lists of packets and outputs the same list, but tags each
//! top-level list with a list of booleans, telling the next stage whether the
//! signature in that packet is valid. It assumes each packet contains one
//! transaction. All processing is done on the CPU by default and on a GPU
//! if the `cuda` feature is enabled with `--features=cuda`.

use packet::SharedPackets;
use rand::{thread_rng, Rng};
use result::Result;
use sigverify;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{spawn, JoinHandle};
use std::time::Instant;
use streamer::{self, PacketReceiver};
use timing;

pub struct SigVerifyStage {
    pub verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl SigVerifyStage {
    pub fn new(exit: Arc<AtomicBool>, packet_receiver: Receiver<SharedPackets>) -> Self {
        let (verified_sender, verified_receiver) = channel();
        let thread_hdls = Self::verifier_services(exit, packet_receiver, verified_sender);
        SigVerifyStage {
            thread_hdls,
            verified_receiver,
        }
    }

    fn verify_batch(batch: Vec<SharedPackets>) -> Vec<(SharedPackets, Vec<u8>)> {
        let r = sigverify::ed25519_verify(&batch);
        batch.into_iter().zip(r).collect()
    }

    fn verifier(
        recvr: &Arc<Mutex<PacketReceiver>>,
        sendr: &Arc<Mutex<Sender<Vec<(SharedPackets, Vec<u8>)>>>>,
    ) -> Result<()> {
        let (batch, len) =
            streamer::recv_batch(&recvr.lock().expect("'recvr' lock in fn verifier"))?;

        let now = Instant::now();
        let batch_len = batch.len();
        let rand_id = thread_rng().gen_range(0, 100);
        info!(
            "@{:?} verifier: verifying: {} id: {}",
            timing::timestamp(),
            batch.len(),
            rand_id
        );

        let verified_batch = Self::verify_batch(batch);
        sendr
            .lock()
            .expect("lock in fn verify_batch in tpu")
            .send(verified_batch)?;

        let total_time_ms = timing::duration_as_ms(&now.elapsed());
        let total_time_s = timing::duration_as_s(&now.elapsed());
        info!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} id: {} verified: {} v/s {}",
            timing::timestamp(),
            batch_len,
            total_time_ms,
            rand_id,
            len,
            (len as f32 / total_time_s)
        );
        Ok(())
    }

    fn verifier_service(
        exit: Arc<AtomicBool>,
        packet_receiver: Arc<Mutex<PacketReceiver>>,
        verified_sender: Arc<Mutex<Sender<Vec<(SharedPackets, Vec<u8>)>>>>,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            let e = Self::verifier(&packet_receiver.clone(), &verified_sender.clone());
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        })
    }

    fn verifier_services(
        exit: Arc<AtomicBool>,
        packet_receiver: PacketReceiver,
        verified_sender: Sender<Vec<(SharedPackets, Vec<u8>)>>,
    ) -> Vec<JoinHandle<()>> {
        let sender = Arc::new(Mutex::new(verified_sender));
        let receiver = Arc::new(Mutex::new(packet_receiver));
        (0..4)
            .map(|_| Self::verifier_service(exit.clone(), receiver.clone(), sender.clone()))
            .collect()
    }
}
