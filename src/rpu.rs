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
//!  `---------` |  | Fetch |  | Request |  | Respond | |   `---------`
//!              |  | Stage |->|  Stage  |->|  Stage  | |
//!  .---------. |  |       |  |         |  |         | |   .---------.
//!  |   Bob   |--->|       |  |         |  |         +---->|   Bob   |
//!  `---------` |  `-------`  `---------`  `---------` |   `---------`
//!              |                                      |
//!              |                                      |
//!              `--------------------------------------`
//! ```

use bank::Bank;
use packet::{BlobRecycler, PacketRecycler};
use request_processor::RequestProcessor;
use request_stage::RequestStage;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;
use streamer;

pub struct Rpu {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Rpu {
    pub fn new(
        bank: Arc<Bank>,
        requests_socket: UdpSocket,
        respond_socket: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let packet_recycler = PacketRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_receiver = streamer::receiver(
            requests_socket,
            exit.clone(),
            packet_recycler.clone(),
            packet_sender,
        );

        let blob_recycler = BlobRecycler::default();
        let request_processor = RequestProcessor::new(bank.clone());
        let (request_stage, blob_receiver) = RequestStage::new(
            request_processor,
            exit.clone(),
            packet_receiver,
            packet_recycler.clone(),
            blob_recycler.clone(),
        );

        let t_responder = streamer::responder(
            respond_socket,
            exit.clone(),
            blob_recycler.clone(),
            blob_receiver,
        );

        let thread_hdls = vec![t_receiver, t_responder, request_stage.thread_hdl];
        Rpu { thread_hdls }
    }
}
