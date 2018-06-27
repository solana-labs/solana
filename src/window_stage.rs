//! The `window_stage` maintains the blob window

use crdt::Crdt;
use packet::BlobRecycler;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use streamer::{self, BlobReceiver, Window};

pub struct WindowStage {
    pub blob_receiver: BlobReceiver,
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl WindowStage {
    pub fn new(
        crdt: Arc<RwLock<Crdt>>,
        window: Window,
        entry_height: u64,
        retransmit_socket: UdpSocket,
        exit: Arc<AtomicBool>,
        blob_recycler: BlobRecycler,
        fetch_stage_receiver: BlobReceiver,
    ) -> Self {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = streamer::retransmitter(
            retransmit_socket,
            exit.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            retransmit_receiver,
        );
        let (blob_sender, blob_receiver) = channel();
        let t_window = streamer::window(
            exit.clone(),
            crdt.clone(),
            window,
            entry_height,
            blob_recycler.clone(),
            fetch_stage_receiver,
            blob_sender,
            retransmit_sender,
        );
        let thread_hdls = vec![t_retransmit, t_window];

        WindowStage {
            blob_receiver,
            thread_hdls,
        }
    }
}
