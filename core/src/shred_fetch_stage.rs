//! The `shred_fetch_stage` pulls shreds from UDP sockets and sends it to a channel.

use crate::cuda_runtime::PinnedVec;
use crate::packet::Packet;
use crate::recycler::Recycler;
use crate::service::Service;
use crate::streamer::{self, PacketReceiver, PacketSender};
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};

pub struct ShredFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ShredFetchStage {
    // updates packets received on a channel and sends them on another channel
    fn modify_packets<F>(recvr: &PacketReceiver, sendr: &PacketSender, modify: F)
    where
        F: Fn(&mut Packet),
    {
        while let Some(mut p) = recvr.iter().next() {
            p.packets.iter_mut().for_each(|p| modify(p));
            if sendr.send(p).is_err() {
                break;
            }
        }
    }

    fn setup_repair_handler(
        repair_socket: Arc<UdpSocket>,
        exit: &Arc<AtomicBool>,
        sender: PacketSender,
        recycler: Recycler<PinnedVec<Packet>>,
    ) -> (JoinHandle<()>, JoinHandle<()>) {
        let (repair_sender, repair_receiver) = channel();
        let repair_streamer = streamer::receiver(
            repair_socket,
            &exit,
            repair_sender.clone(),
            recycler,
            "repair_response_handler",
        );
        let repair_handler_hdl = Builder::new()
            .name("solana-tvu-fetch-stage-repair-recvr".to_string())
            .spawn(move || {
                Self::modify_packets(&repair_receiver, &sender, |p| p.meta.repair = true)
            })
            .unwrap();
        (repair_streamer, repair_handler_hdl)
    }

    pub fn new(
        sockets: Vec<Arc<UdpSocket>>,
        forward_sockets: Vec<Arc<UdpSocket>>,
        repair_socket: Arc<UdpSocket>,
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

        let fwd_sender = sender.clone();
        let fwd_thread_hdl = Builder::new()
            .name("solana-tvu-fetch-stage-fwd-rcvr".to_string())
            .spawn(move || {
                Self::modify_packets(&forward_receiver, &fwd_sender, |p| p.meta.forward = true)
            })
            .unwrap();

        let (repair_receiver, repair_handler) =
            Self::setup_repair_handler(repair_socket, &exit, sender.clone(), recycler.clone());

        let mut thread_hdls: Vec<_> = tvu_threads.chain(tvu_forwards_threads).collect();
        thread_hdls.push(fwd_thread_hdl);
        thread_hdls.push(repair_receiver);
        thread_hdls.push(repair_handler);

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
