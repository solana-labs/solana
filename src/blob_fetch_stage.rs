//! The `blob_fetch_stage` pulls blobs from UDP sockets and sends it to a channel.

use packet::BlobRecycler;
use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use streamer::{self, BlobReceiver};

pub struct BlobFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl BlobFetchStage {
    pub fn new(
        socket: UdpSocket,
        exit: Arc<AtomicBool>,
        blob_recycler: BlobRecycler,
    ) -> (Self, BlobReceiver) {
        Self::new_multi_socket(vec![socket], exit, blob_recycler)
    }
    pub fn new_multi_socket(
        sockets: Vec<UdpSocket>,
        exit: Arc<AtomicBool>,
        blob_recycler: BlobRecycler,
    ) -> (Self, BlobReceiver) {
        let (blob_sender, blob_receiver) = channel();
        let thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| {
                streamer::blob_receiver(
                    exit.clone(),
                    blob_recycler.clone(),
                    socket,
                    blob_sender.clone(),
                ).expect("blob receiver init")
            })
            .collect();

        (BlobFetchStage { thread_hdls }, blob_receiver)
    }
}

impl Service for BlobFetchStage {
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
