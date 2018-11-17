//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use packet::SharedPackets;
use service::Service;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::{self, Builder, JoinHandle};
use streamer::{self, PacketReceiver, PacketSender};

pub struct FetchStage {
    exit: Arc<AtomicBool>,
    leader_addr: Arc<Mutex<Option<SocketAddr>>>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl FetchStage {
    /// If leader_addr is None, sends packets to the next stage in the pipeline. Otherwise,
    /// sends packets to the given leader.
    fn forward_packets(
        shared_packets: SharedPackets,
        sender: &PacketSender,
        leader_addr: &Option<SocketAddr>,
        socket: &UdpSocket,
    ) {
        match leader_addr {
            None => {
                // Ignore failures to downstream channel.
                let _ = sender.send(shared_packets);
            }
            Some(leader_addr) => {
                // Note: Don't use "p.send_to()" because the packets meta points to this
                // fullnode instead of the leader.
                let p = shared_packets.read().unwrap();
                for p in &p.packets {
                    // Ignore failures to socket.
                    let _ = socket.send_to(&p.data[..p.meta.size], leader_addr);
                }
            }
        }
    }

    /// Reads packets from the receiver and then forwards them.
    fn run_proxy(
        receiver: PacketReceiver,
        sender: PacketSender,
        leader_addr: Arc<Mutex<Option<SocketAddr>>>,
    ) {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("couldn't bind for proxy");
        loop {
            // TODO: Fetch everything on the channel before taking the
            // time to lock the mutex.
            if let Ok(p) = receiver.recv() {
                let leader_addr = leader_addr.lock().unwrap();
                Self::forward_packets(p, &sender, &leader_addr, &socket);
            } else {
                break;
            }
        }
    }

    pub fn new(sockets: Vec<UdpSocket>, exit: Arc<AtomicBool>) -> (Self, PacketReceiver) {
        let tx_sockets = sockets.into_iter().map(Arc::new).collect();
        Self::new_multi_socket(tx_sockets, exit)
    }

    pub fn new_multi_socket(
        sockets: Vec<Arc<UdpSocket>>,
        exit: Arc<AtomicBool>,
    ) -> (Self, PacketReceiver) {
        let (sender, receiver) = channel();
        let mut thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| streamer::receiver(socket, exit.clone(), sender.clone(), "fetch-stage"))
            .collect();

        // If we detect we're not the leader, forward messages to it.
        let (proxy_sender, proxy_receiver) = channel();
        let leader_addr = Arc::new(Mutex::new(None));
        let _leader_addr = leader_addr.clone();
        let proxy_thread_hdl = Builder::new()
            .name("solana-fetch-proxy".to_string())
            .spawn(move || Self::run_proxy(receiver, proxy_sender, _leader_addr))
            .unwrap();
        thread_hdls.push(proxy_thread_hdl);

        let fetch_stage = FetchStage {
            exit,
            leader_addr,
            thread_hdls,
        };
        (fetch_stage, proxy_receiver)
    }

    pub fn close(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn clear_leader_addr(&self) {
        *self.leader_addr.lock().unwrap() = None;
    }

    pub fn set_leader_addr(&self, leader_addr: SocketAddr) {
        *self.leader_addr.lock().unwrap() = Some(leader_addr);
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
