//! The `sigverify_stage` implements the signature verification stage of the TPU. It
//! receives a list of lists of packets and outputs the same list, but tags each
//! top-level list with a list of booleans, telling the next stage whether the
//! signature in that packet is valid. It assumes each packet contains one
//! transaction. All processing is done on the CPU by default and on a GPU
//! if the `cuda` feature is enabled with `--features=cuda`.

use crate::packet::Packets;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::sigverify;
use crate::streamer::{self, PacketReceiver};
use crossbeam_channel::Sender as CrossbeamSender;
use solana_metrics::{datapoint_info, inc_new_counter_info};
use solana_sdk::timing;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, Builder, JoinHandle};
use std::time::Instant;

#[cfg(feature = "cuda")]
const RECV_BATCH_MAX: usize = 60_000;

#[cfg(not(feature = "cuda"))]
const RECV_BATCH_MAX: usize = 1000;

pub type VerifiedPackets = Vec<(Packets, Vec<u8>)>;

pub struct SigVerifyStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl SigVerifyStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        packet_receiver: Receiver<Packets>,
        sigverify_disabled: bool,
        verified_sender: CrossbeamSender<VerifiedPackets>,
    ) -> Self {
        sigverify::init();
        let thread_hdls =
            Self::verifier_services(packet_receiver, verified_sender, sigverify_disabled);
        Self { thread_hdls }
    }

    fn verify_batch(batch: Vec<Packets>, sigverify_disabled: bool) -> VerifiedPackets {
        let r = if sigverify_disabled {
            sigverify::ed25519_verify_disabled(&batch)
        } else {
            sigverify::ed25519_verify(&batch)
        };
        batch.into_iter().zip(r).collect()
    }

    fn verifier(
        recvr: &Arc<Mutex<PacketReceiver>>,
        sendr: &CrossbeamSender<VerifiedPackets>,
        sigverify_disabled: bool,
        id: usize,
    ) -> Result<()> {
        let (batch, len, recv_time) = streamer::recv_batch(
            &recvr.lock().expect("'recvr' lock in fn verifier"),
            RECV_BATCH_MAX,
        )?;
        inc_new_counter_info!("sigverify_stage-packets_received", len);

        let now = Instant::now();
        let batch_len = batch.len();
        debug!(
            "@{:?} verifier: verifying: {} id: {}",
            timing::timestamp(),
            batch.len(),
            id
        );

        let verified_batch = Self::verify_batch(batch, sigverify_disabled);
        inc_new_counter_info!("sigverify_stage-verified_packets_send", len);

        if sendr.send(verified_batch).is_err() {
            return Err(Error::SendError);
        }

        let total_time_ms = timing::duration_as_ms(&now.elapsed());
        let total_time_s = timing::duration_as_s(&now.elapsed());
        inc_new_counter_info!(
            "sigverify_stage-time_ms",
            (total_time_ms + recv_time) as usize
        );
        debug!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} id: {} verified: {} v/s {}",
            timing::timestamp(),
            batch_len,
            total_time_ms,
            id,
            len,
            (len as f32 / total_time_s)
        );

        datapoint_info!(
            "sigverify_stage-total_verify_time",
            ("batch_len", batch_len, i64),
            ("len", len, i64),
            ("total_time_ms", total_time_ms, i64)
        );

        Ok(())
    }

    fn verifier_service(
        packet_receiver: Arc<Mutex<PacketReceiver>>,
        verified_sender: CrossbeamSender<VerifiedPackets>,
        sigverify_disabled: bool,
        id: usize,
    ) -> JoinHandle<()> {
        Builder::new()
            .name(format!("solana-verifier-{}", id))
            .spawn(move || loop {
                if let Err(e) =
                    Self::verifier(&packet_receiver, &verified_sender, sigverify_disabled, id)
                {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        Error::SendError => {
                            break;
                        }
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap()
    }

    fn verifier_services(
        packet_receiver: PacketReceiver,
        verified_sender: CrossbeamSender<VerifiedPackets>,
        sigverify_disabled: bool,
    ) -> Vec<JoinHandle<()>> {
        let receiver = Arc::new(Mutex::new(packet_receiver));
        (0..4)
            .map(|id| {
                Self::verifier_service(
                    receiver.clone(),
                    verified_sender.clone(),
                    sigverify_disabled,
                    id,
                )
            })
            .collect()
    }
}

impl Service for SigVerifyStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
