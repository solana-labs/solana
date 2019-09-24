//! The `sigverify_stage` implements the signature verification stage of the TPU. It
//! receives a list of lists of packets and outputs the same list, but tags each
//! top-level list with a list of booleans, telling the next stage whether the
//! signature in that packet is valid. It assumes each packet contains one
//! transaction. All processing is done on the CPU by default and on a GPU
//! if perf-libs are available

use crate::cuda_runtime::PinnedVec;
use crate::packet::Packets;
use crate::perf_libs;
use crate::recycler::Recycler;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::sigverify;
use crate::sigverify::TxOffset;
use crate::streamer::{self, PacketReceiver};
use crossbeam_channel::Sender as CrossbeamSender;
use solana_measure::measure::Measure;
use solana_metrics::{datapoint_debug, inc_new_counter_info};
use solana_sdk::timing;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, Builder, JoinHandle};

const RECV_BATCH_MAX_CPU: usize = 1_000;
const RECV_BATCH_MAX_GPU: usize = 5_000;

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

    fn verify_batch(
        batch: Vec<Packets>,
        sigverify_disabled: bool,
        recycler: &Recycler<TxOffset>,
        recycler_out: &Recycler<PinnedVec<u8>>,
    ) -> VerifiedPackets {
        let r = if sigverify_disabled {
            sigverify::ed25519_verify_disabled(&batch)
        } else {
            sigverify::ed25519_verify(&batch, recycler, recycler_out)
        };
        batch.into_iter().zip(r).collect()
    }

    fn verifier(
        recvr: &Arc<Mutex<PacketReceiver>>,
        sendr: &CrossbeamSender<VerifiedPackets>,
        sigverify_disabled: bool,
        id: usize,
        recycler: &Recycler<TxOffset>,
        recycler_out: &Recycler<PinnedVec<u8>>,
    ) -> Result<()> {
        let (batch, len, recv_time) = streamer::recv_batch(
            &recvr.lock().expect("'recvr' lock in fn verifier"),
            if perf_libs::api().is_some() {
                RECV_BATCH_MAX_GPU
            } else {
                RECV_BATCH_MAX_CPU
            },
        )?;
        inc_new_counter_info!("sigverify_stage-packets_received", len);

        let mut verify_batch_time = Measure::start("sigverify_batch_time");
        let batch_len = batch.len();
        debug!(
            "@{:?} verifier: verifying: {} id: {}",
            timing::timestamp(),
            len,
            id
        );

        let verified_batch = Self::verify_batch(batch, sigverify_disabled, recycler, recycler_out);
        inc_new_counter_info!("sigverify_stage-verified_packets_send", len);

        for v in verified_batch {
            if sendr.send(vec![v]).is_err() {
                return Err(Error::SendError);
            }
        }

        verify_batch_time.stop();

        inc_new_counter_info!(
            "sigverify_stage-time_ms",
            (verify_batch_time.as_ms() + recv_time) as usize
        );
        debug!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} id: {} verified: {} v/s {}",
            timing::timestamp(),
            batch_len,
            verify_batch_time.as_ms(),
            id,
            len,
            (len as f32 / verify_batch_time.as_s())
        );

        datapoint_debug!(
            "sigverify_stage-total_verify_time",
            ("batch_len", batch_len, i64),
            ("len", len, i64),
            ("total_time_ms", verify_batch_time.as_ms(), i64)
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
            .spawn(move || {
                let recycler = Recycler::default();
                let recycler_out = Recycler::default();
                loop {
                    if let Err(e) = Self::verifier(
                        &packet_receiver,
                        &verified_sender,
                        sigverify_disabled,
                        id,
                        &recycler,
                        &recycler_out,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            Error::SendError => {
                                break;
                            }
                            _ => error!("{:?}", e),
                        }
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
