//! The `request_stage` processes thin client Request messages.

use bincode::deserialize;
use counter::Counter;
use log::Level;
use packet::{to_blobs, Packets, SharedPackets};
use rayon::prelude::*;
use request::Request;
use request_processor::RequestProcessor;
use result::{Error, Result};
use service::Service;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Instant;
use streamer::{self, BlobReceiver, BlobSender};
use timing;

pub struct RequestStage {
    thread_hdl: JoinHandle<()>,
    pub request_processor: Arc<RequestProcessor>,
}

impl RequestStage {
    pub fn deserialize_requests(p: &Packets) -> Vec<Option<(Request, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            }).collect()
    }

    pub fn process_request_packets(
        request_processor: &RequestProcessor,
        packet_receiver: &Receiver<SharedPackets>,
        blob_sender: &BlobSender,
    ) -> Result<()> {
        let (batch, batch_len, _recv_time) = streamer::recv_batch(packet_receiver)?;

        debug!(
            "@{:?} request_stage: processing: {}",
            timing::timestamp(),
            batch_len
        );

        let mut reqs_len = 0;
        let proc_start = Instant::now();
        for msgs in batch {
            let reqs: Vec<_> = Self::deserialize_requests(&msgs.read())
                .into_iter()
                .filter_map(|x| x)
                .collect();
            reqs_len += reqs.len();

            let rsps = request_processor.process_requests(reqs);

            let blobs = to_blobs(rsps)?;
            if !blobs.is_empty() {
                info!("process: sending blobs: {}", blobs.len());
                //don't wake up the other side if there is nothing
                blob_sender.send(blobs)?;
            }
        }
        let total_time_s = timing::duration_as_s(&proc_start.elapsed());
        let total_time_ms = timing::duration_as_ms(&proc_start.elapsed());
        inc_new_counter_info!("request_stage-time_ms", total_time_ms as usize);
        debug!(
            "@{:?} done process batches: {} time: {:?}ms reqs: {} reqs/s: {}",
            timing::timestamp(),
            batch_len,
            total_time_ms,
            reqs_len,
            (reqs_len as f32) / (total_time_s)
        );
        Ok(())
    }
    pub fn new(
        request_processor: RequestProcessor,
        packet_receiver: Receiver<SharedPackets>,
    ) -> (Self, BlobReceiver) {
        let request_processor = Arc::new(request_processor);
        let request_processor_ = request_processor.clone();
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-request-stage".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::process_request_packets(
                    &request_processor_,
                    &packet_receiver,
                    &blob_sender,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => error!("{:?}", e),
                    }
                }
            }).unwrap();
        (
            RequestStage {
                thread_hdl,
                request_processor,
            },
            blob_receiver,
        )
    }
}

impl Service for RequestStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
