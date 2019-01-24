//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use crate::service::Service;
use crate::streamer::{self, PacketReceiver};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub struct FetchStage {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl FetchStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(sockets: Vec<UdpSocket>, exit: Arc<AtomicBool>) -> (Self, PacketReceiver) {
        let tx_sockets = sockets.into_iter().map(Arc::new).collect();
        Self::new_multi_socket(tx_sockets, exit)
    }
    fn new_multi_socket(
        sockets: Vec<Arc<UdpSocket>>,
        exit: Arc<AtomicBool>,
    ) -> (Self, PacketReceiver) {
        let (sender, receiver) = channel();
        let thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| streamer::receiver(socket, exit.clone(), sender.clone(), "fetch-stage"))
            .collect();

        (Self { exit, thread_hdls }, receiver)
    }

    pub fn close(&self) {
        self.exit.store(true, Ordering::Relaxed);
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
