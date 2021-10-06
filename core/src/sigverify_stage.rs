//! The `sigverify_stage` implements the signature verification stage of the TPU. It
//! receives a list of lists of packets and outputs the same list, but tags each
//! top-level list with a list of booleans, telling the next stage whether the
//! signature in that packet is valid. It assumes each packet contains one
//! transaction. All processing is done on the CPU by default and on a GPU
//! if perf-libs are available

use crate::sigverify;
use crossbeam_channel::{SendError, Sender as CrossbeamSender};
use solana_measure::measure::Measure;
use solana_metrics::datapoint_debug;
use solana_perf::packet::Packets;
use solana_sdk::timing;
use solana_streamer::streamer::{self, PacketReceiver, StreamerError};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::{self, Builder, JoinHandle};
use thiserror::Error;

const MAX_SIGVERIFY_BATCH: usize = 10_000;

#[derive(Error, Debug)]
pub enum SigVerifyServiceError {
    #[error("send packets batch error")]
    Send(#[from] SendError<Vec<Packets>>),

    #[error("streamer error")]
    Streamer(#[from] StreamerError),
}

type Result<T> = std::result::Result<T, SigVerifyServiceError>;

pub struct SigVerifyStage {
    thread_hdl: JoinHandle<()>,
}

pub trait SigVerifier {
    fn verify_batch(&self, batch: Vec<Packets>) -> Vec<Packets>;
}

#[derive(Default, Clone)]
pub struct DisabledSigVerifier {}

impl SigVerifier for DisabledSigVerifier {
    fn verify_batch(&self, mut batch: Vec<Packets>) -> Vec<Packets> {
        sigverify::ed25519_verify_disabled(&mut batch);
        batch
    }
}

impl SigVerifyStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: SigVerifier + 'static + Send + Clone>(
        packet_receiver: Receiver<Packets>,
        verified_sender: CrossbeamSender<Vec<Packets>>,
        verifier: T,
    ) -> Self {
        let thread_hdl = Self::verifier_services(packet_receiver, verified_sender, verifier);
        Self { thread_hdl }
    }

    pub fn discard_excess_packets(batches: &mut Vec<Packets>, max_packets: usize) {
        let mut received_ips = HashMap::new();
        for (batch_index, batch) in batches.iter().enumerate() {
            for (packet_index, packets) in batch.packets.iter().enumerate() {
                let e = received_ips
                    .entry(packets.meta.addr().ip())
                    .or_insert_with(Vec::new);
                e.push((batch_index, packet_index));
            }
        }
        let mut batch_len = 0;
        while batch_len < max_packets {
            for (_ip, indexes) in received_ips.iter_mut() {
                if !indexes.is_empty() {
                    indexes.remove(0);
                    batch_len += 1;
                    if batch_len >= MAX_SIGVERIFY_BATCH {
                        break;
                    }
                }
            }
        }
        for (_addr, indexes) in received_ips {
            for (batch_index, packet_index) in indexes {
                batches[batch_index].packets[packet_index].meta.discard = true;
            }
        }
    }

    fn verifier<T: SigVerifier>(
        recvr: &PacketReceiver,
        sendr: &CrossbeamSender<Vec<Packets>>,
        verifier: &T,
    ) -> Result<()> {
        let (mut batches, len, recv_time) = streamer::recv_batch(recvr)?;

        let mut verify_batch_time = Measure::start("sigverify_batch_time");
        let batches_len = batches.len();
        debug!("@{:?} verifier: verifying: {}", timing::timestamp(), len,);
        if len > MAX_SIGVERIFY_BATCH {
            Self::discard_excess_packets(&mut batches, MAX_SIGVERIFY_BATCH);
        }
        sendr.send(verifier.verify_batch(batches))?;
        verify_batch_time.stop();

        debug!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} verified: {} v/s {}",
            timing::timestamp(),
            batches_len,
            verify_batch_time.as_ms(),
            len,
            (len as f32 / verify_batch_time.as_s())
        );

        datapoint_debug!(
            "sigverify_stage-total_verify_time",
            ("num_batches", batches_len, i64),
            ("num_packets", len, i64),
            ("verify_time_ms", verify_batch_time.as_ms(), i64),
            ("recv_time", recv_time, i64),
        );

        Ok(())
    }

    fn verifier_service<T: SigVerifier + 'static + Send + Clone>(
        packet_receiver: PacketReceiver,
        verified_sender: CrossbeamSender<Vec<Packets>>,
        verifier: &T,
    ) -> JoinHandle<()> {
        let verifier = verifier.clone();
        Builder::new()
            .name("solana-verifier".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::verifier(&packet_receiver, &verified_sender, &verifier) {
                    match e {
                        SigVerifyServiceError::Streamer(StreamerError::RecvTimeout(
                            RecvTimeoutError::Disconnected,
                        )) => break,
                        SigVerifyServiceError::Streamer(StreamerError::RecvTimeout(
                            RecvTimeoutError::Timeout,
                        )) => (),
                        SigVerifyServiceError::Send(_) => {
                            break;
                        }
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap()
    }

    fn verifier_services<T: SigVerifier + 'static + Send + Clone>(
        packet_receiver: PacketReceiver,
        verified_sender: CrossbeamSender<Vec<Packets>>,
        verifier: T,
    ) -> JoinHandle<()> {
        Self::verifier_service(packet_receiver, verified_sender, &verifier)
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_perf::packet::Packet;

    fn count_non_discard(packets: &[Packets]) -> usize {
        packets
            .iter()
            .map(|pp| {
                pp.packets
                    .iter()
                    .map(|p| if p.meta.discard { 0 } else { 1 })
                    .sum::<usize>()
            })
            .sum::<usize>()
    }

    #[test]
    fn test_packet_discard() {
        solana_logger::setup();
        let mut p = Packets::default();
        p.packets.resize(10, Packet::default());
        p.packets[3].meta.addr = [1u16; 8];
        let mut packets = vec![p];
        let max = 3;
        SigVerifyStage::discard_excess_packets(&mut packets, max);
        assert_eq!(count_non_discard(&packets), max);
        assert!(!packets[0].packets[0].meta.discard);
        assert!(!packets[0].packets[3].meta.discard);
    }
}
