//! The `shred_fetch_stage` pulls shreds from UDP sockets and sends it to a channel.

use crate::recycler::Recycler;
use crate::service::Service;
use crate::streamer::{self, PacketSender};
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};

pub struct ShredFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ShredFetchStage {
    pub fn new(
        sockets: Vec<Arc<UdpSocket>>,
        forward_sockets: Vec<Arc<UdpSocket>>,
        sender: &PacketSender,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let recycler = Recycler::default();
        let tvu_threads = sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                &exit,
                sender.clone(),
                recycler.clone(),
                "shred_fetch_stage",
            )
        });

        let (forward_sender, forward_receiver) = channel();
        let tvu_forwards_threads = forward_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                &exit,
                forward_sender.clone(),
                recycler.clone(),
                "shred_fetch_stage",
            )
        });

        let sender = sender.clone();
        let fwd_thread_hdl = Builder::new()
            .name("solana-tvu-fetch-stage-fwd-rcvr".to_string())
            .spawn(move || {
                while let Some(mut p) = forward_receiver.iter().next() {
                    p.packets.iter_mut().for_each(|p| p.meta.forward = true);
                    if sender.send(p).is_err() {
                        break;
                    }
                }
            })
            .unwrap();

        let mut thread_hdls: Vec<_> = tvu_threads.chain(tvu_forwards_threads).collect();
        thread_hdls.push(fwd_thread_hdl);

        Self { thread_hdls }
    }
}

impl Service for ShredFetchStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
