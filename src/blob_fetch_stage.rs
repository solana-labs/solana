//! The `blob_fetch_stage` pulls blobs from UDP sockets and sends it to a channel.

use packet;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;
use streamer;

pub struct BlobFetchStage {
    pub blob_receiver: streamer::BlobReceiver,
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl BlobFetchStage {
    pub fn new(
        socket: UdpSocket,
        exit: Arc<AtomicBool>,
        blob_recycler: packet::BlobRecycler,
    ) -> Self {
        Self::new_multi_socket(vec![socket], exit, blob_recycler)
    }
    pub fn new_multi_socket(
        sockets: Vec<UdpSocket>,
        exit: Arc<AtomicBool>,
        blob_recycler: packet::BlobRecycler,
    ) -> Self {
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

        BlobFetchStage {
            blob_receiver,
            thread_hdls,
        }
    }
}
