use {
    crate::repair::{quic_endpoint::RemoteRequest, serve_repair::ServeRepair},
    crossbeam_channel::{unbounded, Receiver, Sender},
    solana_ledger::blockstore::Blockstore,
    solana_perf::{packet::PacketBatch, recycler::Recycler},
    solana_streamer::{
        socket::SocketAddrSpace,
        streamer::{self, StreamerReceiveStats},
    },
    std::{
        net::UdpSocket,
        sync::{atomic::AtomicBool, Arc},
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct ServeRepairService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ServeRepairService {
    pub fn new(
        serve_repair: ServeRepair,
        remote_request_sender: Sender<RemoteRequest>,
        remote_request_receiver: Receiver<RemoteRequest>,
        blockstore: Arc<Blockstore>,
        serve_repair_socket: UdpSocket,
        socket_addr_space: SocketAddrSpace,
        stats_reporter_sender: Sender<Box<dyn FnOnce() + Send>>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let (request_sender, request_receiver) = unbounded();
        let serve_repair_socket = Arc::new(serve_repair_socket);
        trace!(
            "ServeRepairService: id: {}, listening on: {:?}",
            &serve_repair.my_id(),
            serve_repair_socket.local_addr().unwrap()
        );
        let t_receiver = streamer::receiver(
            serve_repair_socket.clone(),
            exit.clone(),
            request_sender,
            Recycler::default(),
            Arc::new(StreamerReceiveStats::new("serve_repair_receiver")),
            Duration::from_millis(1), // coalesce
            false,                    // use_pinned_memory
            None,                     // in_vote_only_mode
        );
        let t_packet_adapter = Builder::new()
            .name(String::from("solServRAdapt"))
            .spawn(|| adapt_repair_requests_packets(request_receiver, remote_request_sender))
            .unwrap();
        let (response_sender, response_receiver) = unbounded();
        let t_responder = streamer::responder(
            "Repair",
            serve_repair_socket,
            response_receiver,
            socket_addr_space,
            Some(stats_reporter_sender),
        );
        let t_listen =
            serve_repair.listen(blockstore, remote_request_receiver, response_sender, exit);

        let thread_hdls = vec![t_receiver, t_packet_adapter, t_responder, t_listen];
        Self { thread_hdls }
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        self.thread_hdls.into_iter().try_for_each(JoinHandle::join)
    }
}

// Adapts incoming UDP repair requests into RemoteRequest struct.
pub(crate) fn adapt_repair_requests_packets(
    packets_receiver: Receiver<PacketBatch>,
    remote_request_sender: Sender<RemoteRequest>,
) {
    for packets in packets_receiver {
        for packet in &packets {
            let Some(bytes) = packet.data(..).map(Vec::from) else {
                continue;
            };
            let request = RemoteRequest {
                remote_pubkey: None,
                remote_address: packet.meta().socket_addr(),
                bytes,
                response_sender: None,
            };
            if remote_request_sender.send(request).is_err() {
                return; // The receiver end of the channel is disconnected.
            }
        }
    }
}
