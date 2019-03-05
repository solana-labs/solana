//! The `blob_fetch_stage` pulls blobs from UDP sockets and sends it to a channel.

use crate::service::Service;
use crate::streamer::{self, BlobSender};
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub struct BlobFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl BlobFetchStage {
    pub fn new(socket: Arc<UdpSocket>, sender: &BlobSender, exit: &Arc<AtomicBool>) -> Self {
        Self::new_multi_socket(vec![socket], sender, exit)
    }
    pub fn new_multi_socket(
        sockets: Vec<Arc<UdpSocket>>,
        sender: &BlobSender,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| streamer::blob_receiver(socket, &exit, sender.clone()))
            .collect();

        Self { thread_hdls }
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
