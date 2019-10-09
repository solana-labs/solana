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
    fn modify_packets<F>(recvr: PacketReceiver, sendr: PacketSender, modify: F)
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

    fn packet_modifier<F>(
        sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: PacketSender,
        recycler: Recycler<PinnedVec<Packet>>,
        modify: F,
    ) -> (Vec<JoinHandle<()>>, JoinHandle<()>)
    where
        F: Fn(&mut Packet) + Send + 'static,
    {
        let (packet_sender, packet_receiver) = channel();
        let streamers = sockets
            .into_iter()
            .map(|s| {
                streamer::receiver(
                    s,
                    &exit,
                    packet_sender.clone(),
                    recycler.clone(),
                    "packet_modifier",
                )
            })
            .collect();
        let sender = sender.clone();
        let modifier_hdl = Builder::new()
            .name("solana-tvu-fetch-stage-packet-modifier".to_string())
            .spawn(|| Self::modify_packets(packet_receiver, sender, modify))
            .unwrap();
        (streamers, modifier_hdl)
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

        let (tvu_forwards_threads, fwd_thread_hdl) = Self::packet_modifier(
            forward_sockets,
            &exit,
            sender.clone(),
            recycler.clone(),
            |p| p.meta.forward = true,
        );

        let (repair_receiver, repair_handler) = Self::packet_modifier(
            vec![repair_socket],
            &exit,
            sender.clone(),
            recycler.clone(),
            |p| p.meta.repair = true,
        );

        let mut thread_hdls: Vec<_> = tvu_threads
            .chain(tvu_forwards_threads.into_iter())
            .collect();
        thread_hdls.extend(repair_receiver.into_iter());
        thread_hdls.push(fwd_thread_hdl);
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
