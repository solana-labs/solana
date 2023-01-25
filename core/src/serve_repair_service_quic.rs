use {
    crate::{serve_repair::ServeRepair, tpu::MAX_QUIC_CONNECTIONS_PER_PEER, tvu::RepairQuicConfig},
    crossbeam_channel::{unbounded, Sender},
    solana_client::connection_cache::ConnectionCache,
    solana_ledger::blockstore::Blockstore,
    solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool},
    solana_sdk::signer::Signer,
    solana_streamer::{
        quic::{spawn_server, StreamStats, MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
        socket::SocketAddrSpace,
        streamer::{self, ResponderOption},
    },
    std::{
        sync::{atomic::AtomicBool, Arc},
        thread::{self, JoinHandle},
    },
};

pub struct ServeRepairService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ServeRepairService {
    /// Creates Quic based server serving repair requests. It starts a Quic server which forwards
    /// received PacketBatch to the ServeRepair which in turns sends responses to the
    /// Quic based responder.
    pub fn new(
        serve_repair: ServeRepair,
        blockstore: Arc<Blockstore>,
        repair_quic_config: RepairQuicConfig,
        socket_addr_space: SocketAddrSpace,
        stats_reporter_sender: Sender<Box<dyn FnOnce() + Send>>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        trace!(
            "ServeRepairService: id: {}, listening on: {:?}",
            &serve_repair.my_id(),
            repair_quic_config
                .serve_repair_address
                .local_addr()
                .unwrap()
        );

        let (response_sender_quic, response_receiver_quic) = unbounded();

        let host = repair_quic_config
            .serve_repair_address
            .as_ref()
            .local_addr()
            .unwrap()
            .ip();
        let stats = Arc::new(StreamStats::default());

        let (request_sender_quic, request_receiver_quic) = unbounded();
        // Repair server using quic
        let (serve_repair_endpoint, repair_quic_t) = spawn_server(
            repair_quic_config.serve_repair_address.try_clone().unwrap(),
            &repair_quic_config.identity_keypair,
            host,
            request_sender_quic,
            exit.clone(),
            MAX_QUIC_CONNECTIONS_PER_PEER,
            repair_quic_config.staked_nodes.clone(),
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats,
            repair_quic_config.wait_for_chunk_timeout_ms,
            repair_quic_config.repair_packet_coalesce_timeout_ms,
        )
        .unwrap();

        // The connection cache used to send to repair responses back to the client.
        let connection_cache = ConnectionCache::new_with_client_and_port_offset_options(
            1,
            Some(serve_repair_endpoint),
            Some((&repair_quic_config.identity_keypair, host)),
            Some((
                &repair_quic_config.staked_nodes,
                &repair_quic_config.identity_keypair.pubkey(),
            )),
            false, // This is the connection cache used to sending responses back to the client, we know the exact port
        );

        let connection_cache = match connection_cache {
            ConnectionCache::Quic(connection_cache) => connection_cache,
            ConnectionCache::Udp(_) => panic!("Do not expect UDP connection cache in this case"),
        };
        let responder_quic_t = streamer::responder::<QuicPool, QuicConnectionManager, QuicConfig>(
            "RepairQuic",
            ResponderOption::ConnectionCache(connection_cache),
            response_receiver_quic,
            socket_addr_space,
            Some(stats_reporter_sender),
        );

        let listen_quic_t = serve_repair.listen(
            blockstore,
            request_receiver_quic,
            response_sender_quic,
            exit,
        );

        let thread_hdls = vec![repair_quic_t, responder_quic_t, listen_quic_t];
        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
