//! The `request_stage` processes thin client Request messages.

use bincode::deserialize;
use packet::{to_blobs, BlobRecycler, SharedBlobs};
use rayon::prelude::*;
use request::Request;
use request_processor::RequestProcessor;
use result::{Error, Result};
use service::Service;
use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::{BlobReceiver, BlobSender};
use timing;

pub struct RequestStage {
    thread_hdl: JoinHandle<()>,
    pub request_processor: Arc<RequestProcessor>,
}

impl RequestStage {
    pub fn deserialize_requests(blobs: &SharedBlobs) -> Vec<Option<(Request, SocketAddr)>> {
        blobs
            .par_iter()
            .map(|x| {
                let x = x.read().unwrap();
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            })
            .collect()
    }

    pub fn process_request_packets(
        request_processor: &RequestProcessor,
        blob_receiver: &Receiver<SharedBlobs>,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = blob_receiver.recv_timeout(timer)?;

        debug!("request_stage: processing: {}", blobs.len());

        let mut reqs_len = 0;
        let proc_start = Instant::now();

        let reqs: Vec<_> = Self::deserialize_requests(&blobs)
            .into_iter()
            .filter_map(|x| x)
            .collect();
        reqs_len += reqs.len();
        for blob in blobs {
            blob_recycler.recycle(blob);
        }

        let rsps = request_processor.process_requests(reqs);

        let blobs = to_blobs(rsps, blob_recycler)?;
        if !blobs.is_empty() {
            info!("process: sending blobs: {}", blobs.len());
            //don't wake up the other side if there is nothing
            blob_sender.send(blobs)?;
        }

        let total_time_s = timing::duration_as_s(&proc_start.elapsed());
        let total_time_ms = timing::duration_as_ms(&proc_start.elapsed());
        debug!(
            "done process blobs: time: {:?}ms reqs: {} reqs/s: {}",
            total_time_ms,
            reqs_len,
            (reqs_len as f32) / (total_time_s)
        );
        Ok(())
    }
    pub fn new(
        request_processor: RequestProcessor,
        blob_fetch_receiver: Receiver<SharedBlobs>,
        blob_recycler: BlobRecycler,
    ) -> (Self, BlobReceiver) {
        let request_processor = Arc::new(request_processor);
        let request_processor_ = request_processor.clone();
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-request-stage".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::process_request_packets(
                    &request_processor_,
                    &blob_fetch_receiver,
                    &blob_sender,
                    &blob_recycler,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();
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
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        vec![self.thread_hdl]
    }

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
