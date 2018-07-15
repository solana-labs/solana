//! The `rpu` module implements the Request Processing Unit, a
//! 3-stage transaction processing pipeline in software. It listens
//! for `Request` messages from clients and replies with `Response`
//! messages.
//!
//! ```text
//!                             .------.
//!                             | Bank |
//!                             `---+--`
//!                                 |
//!              .------------------|-------------------.
//!              |  RPU             |                   |
//!              |                  v                   |
//!  .---------. |  .-------.  .---------.  .---------. |   .---------.
//!  |  Alice  |--->|       |  |         |  |         +---->|  Alice  |
//!  `---------` |  | Blob  |  | Request |  | Respond | |   `---------`
//!              |  | Fetch |->|  Stage  |->|  Stage  | |
//!  .---------. |  | Stage |  |         |  |         | |   .---------.
//!  |   Bob   |--->|       |  |         |  |         +---->|   Bob   |
//!  `---------` |  `-------`  `---------`  `---------` |   `---------`
//!              |                                      |
//!              |                                      |
//!              `--------------------------------------`
//! ```

use bank::Bank;
use blob_fetch_stage::BlobFetchStage;
use packet::BlobRecycler;
use request_processor::RequestProcessor;
use request_stage::RequestStage;
use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use streamer;

pub struct Rpu {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl Rpu {
    pub fn new(
        bank: &Arc<Bank>,
        requests_socket: UdpSocket,
        respond_socket: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let blob_recycler = BlobRecycler::default();
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new(requests_socket, exit, &blob_recycler);

        let request_processor = RequestProcessor::new(bank.clone());
        let (request_stage, blob_receiver) = RequestStage::new(
            request_processor,
            blob_fetch_receiver,
            blob_recycler.clone(),
        );

        let t_responder =
            streamer::responder("rpu", respond_socket, blob_recycler.clone(), blob_receiver);

        let mut thread_hdls = vec![t_responder];
        thread_hdls.extend(fetch_stage.thread_hdls().into_iter());
        thread_hdls.extend(request_stage.thread_hdls().into_iter());
        Rpu { thread_hdls }
    }
}

impl Service for Rpu {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        self.thread_hdls
    }

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls() {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
