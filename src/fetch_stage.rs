//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use packet;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;
use streamer;

pub struct FetchStage {
    pub packet_receiver: streamer::PacketReceiver,
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl FetchStage {
    pub fn new(
        socket: UdpSocket,
        exit: Arc<AtomicBool>,
        packet_recycler: packet::PacketRecycler,
    ) -> Self {
        Self::new_multi_socket(vec![socket], exit, packet_recycler)
    }
    pub fn new_multi_socket(
        sockets: Vec<UdpSocket>,
        exit: Arc<AtomicBool>,
        packet_recycler: packet::PacketRecycler,
    ) -> Self {
        let (packet_sender, packet_receiver) = channel();
        let thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| {
                streamer::receiver(
                    socket,
                    exit.clone(),
                    packet_recycler.clone(),
                    packet_sender.clone(),
                )
            })
            .collect();

        FetchStage {
            packet_receiver,
            thread_hdls,
        }
    }
}
