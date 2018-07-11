//! The `window_stage` maintains the blob window

use crdt::Crdt;
use packet::BlobRecycler;
use service::Service;
use std::net::UdpSocket;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use streamer::{self, BlobReceiver, Window};

pub struct WindowStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl WindowStage {
    pub fn new(
        crdt: &Arc<RwLock<Crdt>>,
        window: Window,
        entry_height: u64,
        retransmit_socket: UdpSocket,
        blob_recycler: &BlobRecycler,
        fetch_stage_receiver: BlobReceiver,
    ) -> (Self, BlobReceiver) {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = streamer::retransmitter(
            retransmit_socket,
            crdt.clone(),
            blob_recycler.clone(),
            retransmit_receiver,
        );
        let (blob_sender, blob_receiver) = channel();
        let t_window = streamer::window(
            crdt.clone(),
            window,
            entry_height,
            blob_recycler.clone(),
            fetch_stage_receiver,
            blob_sender,
            retransmit_sender,
        );
        let thread_hdls = vec![t_retransmit, t_window];

        (WindowStage { thread_hdls }, blob_receiver)
    }
}

impl Service for WindowStage {
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
