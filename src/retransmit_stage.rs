//! The `retransmit_stage` retransmits blobs between validators

use broadcaster;
use crdt::Crdt;
use packet::BlobRecycler;
use service::Service;
use std::net::UdpSocket;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use streamer::BlobReceiver;
use window::{self, SharedWindow};

pub struct RetransmitStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl RetransmitStage {
    pub fn new(
        crdt: &Arc<RwLock<Crdt>>,
        window: SharedWindow,
        entry_height: u64,
        retransmit_socket: UdpSocket,
        blob_recycler: &BlobRecycler,
        fetch_stage_receiver: BlobReceiver,
    ) -> (Self, BlobReceiver) {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = broadcaster::retransmitter(
            retransmit_socket,
            crdt.clone(),
            blob_recycler.clone(),
            retransmit_receiver,
        );
        let (blob_sender, blob_receiver) = channel();
        let t_window = window::window(
            crdt.clone(),
            window,
            entry_height,
            blob_recycler.clone(),
            fetch_stage_receiver,
            blob_sender,
            retransmit_sender,
        );
        let thread_hdls = vec![t_retransmit, t_window];

        (RetransmitStage { thread_hdls }, blob_receiver)
    }
}

impl Service for RetransmitStage {
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
