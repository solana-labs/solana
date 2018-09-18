//! The `blob_fetch_stage` pulls blobs from UDP sockets and sends it to a channel.

use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use streamer::{self, BlobReceiver};

pub struct BlobFetchStage {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl BlobFetchStage {
    pub fn new(socket: Arc<UdpSocket>, exit: Arc<AtomicBool>) -> (Self, BlobReceiver) {
        Self::new_multi_socket(vec![socket], exit)
    }
    pub fn new_multi_socket(
        sockets: Vec<Arc<UdpSocket>>,
        exit: Arc<AtomicBool>,
    ) -> (Self, BlobReceiver) {
        let (sender, receiver) = channel();
        let thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| streamer::blob_receiver(socket, exit.clone(), sender.clone()))
            .collect();

        (BlobFetchStage { exit, thread_hdls }, receiver)
    }

    pub fn close(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }
}

impl Service for BlobFetchStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
