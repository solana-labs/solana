//! The `request_stage` processes thin client Request messages.

use entry::Entry;
use event_processor::EventProcessor;
use packet;
use packet::SharedPackets;
use request_processor::RequestProcessor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::thread::{spawn, JoinHandle};
use streamer;

pub struct RequestStage {
    pub thread_hdl: JoinHandle<()>,
    pub entry_receiver: Receiver<Entry>,
    pub blob_receiver: streamer::BlobReceiver,
    pub request_processor: Arc<RequestProcessor>,
}

impl RequestStage {
    pub fn new(
        request_processor: RequestProcessor,
        event_processor: Arc<EventProcessor>,
        exit: Arc<AtomicBool>,
        verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        packet_recycler: packet::PacketRecycler,
        blob_recycler: packet::BlobRecycler,
    ) -> Self {
        let request_processor = Arc::new(request_processor);
        let request_processor_ = request_processor.clone();
        let (entry_sender, entry_receiver) = channel();
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = spawn(move || loop {
            let e = request_processor_.process_request_packets(
                &event_processor,
                &verified_receiver,
                &entry_sender,
                &blob_sender,
                &packet_recycler,
                &blob_recycler,
            );
            if e.is_err() {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        RequestStage {
            thread_hdl,
            entry_receiver,
            blob_receiver,
            request_processor,
        }
    }
}
