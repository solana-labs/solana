//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use crate::service::Service;
use crate::streamer::{self, PacketReceiver, PacketSender};
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub struct FetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl FetchStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        sockets: Vec<UdpSocket>,
        tpu_via_blobs_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
    ) -> (Self, PacketReceiver) {
        let (sender, receiver) = channel();
        (
            Self::new_with_sender(sockets, tpu_via_blobs_sockets, exit, &sender),
            receiver,
        )
    }
    pub fn new_with_sender(
        sockets: Vec<UdpSocket>,
        tpu_via_blobs_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        sender: &PacketSender,
    ) -> Self {
        let tx_sockets = sockets.into_iter().map(Arc::new).collect();
        let tpu_via_blobs_sockets = tpu_via_blobs_sockets.into_iter().map(Arc::new).collect();
        Self::new_multi_socket(tx_sockets, tpu_via_blobs_sockets, exit, &sender)
    }

    fn new_multi_socket(
        sockets: Vec<Arc<UdpSocket>>,
        tpu_via_blobs_sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: &PacketSender,
    ) -> Self {
        let tpu_threads = sockets
            .into_iter()
            .map(|socket| streamer::receiver(socket, &exit, sender.clone()));

        let tpu_via_blobs_threads = tpu_via_blobs_sockets
            .into_iter()
            .map(|socket| streamer::blob_packet_receiver(socket, &exit, sender.clone()));

        let thread_hdls: Vec<_> = tpu_threads.chain(tpu_via_blobs_threads).collect();
        Self { thread_hdls }
    }
}

impl Service for FetchStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
