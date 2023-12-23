use {
    crate::{
        nonblocking::quic::ALPN_TPU_PROTOCOL_ID, streamer::StakedNodes,
        tls_certificates::new_self_signed_tls_certificate,
    },
    crossbeam_channel::Sender,
    pem::Pem,
    quinn::{Endpoint, IdleTimeout, ServerConfig},
    rustls::{server::ClientCertVerified, Certificate, DistinguishedName},
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        packet::PACKET_DATA_SIZE,
        quic::{QUIC_MAX_TIMEOUT, QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS},
        signature::Keypair,
    },
    std::{
        net::{IpAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread,
        time::{Duration, SystemTime},
    },
    tokio::runtime::Runtime,
};

pub const MAX_STAKED_CONNECTIONS: usize = 2000;
pub const MAX_UNSTAKED_CONNECTIONS: usize = 500;

pub struct SkipClientVerification;

impl SkipClientVerification {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::server::ClientCertVerifier for SkipClientVerification {
    fn client_auth_root_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _now: SystemTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(rustls::server::ClientCertVerified::assertion())
    }
}

/// Returns default server configuration along with its PEM certificate chain.
#[allow(clippy::field_reassign_with_default)] // https://github.com/rust-lang/rust-clippy/issues/6527
pub(crate) fn configure_server(
    identity_keypair: &Keypair,
    gossip_host: IpAddr,
) -> Result<(ServerConfig, String), QuicServerError> {
    let (cert, priv_key) = new_self_signed_tls_certificate(identity_keypair, gossip_host)?;
    let cert_chain_pem_parts = vec![Pem {
        tag: "CERTIFICATE".to_string(),
        contents: cert.0.clone(),
    }];
    let cert_chain_pem = pem::encode_many(&cert_chain_pem_parts);

    let mut server_tls_config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(SkipClientVerification::new())
        .with_single_cert(vec![cert], priv_key)?;
    server_tls_config.alpn_protocols = vec![ALPN_TPU_PROTOCOL_ID.to_vec()];

    let mut server_config = ServerConfig::with_crypto(Arc::new(server_tls_config));
    server_config.use_retry(true);
    let config = Arc::get_mut(&mut server_config.transport).unwrap();

    // QUIC_MAX_CONCURRENT_STREAMS doubled, which was found to improve reliability
    const MAX_CONCURRENT_UNI_STREAMS: u32 =
        (QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS.saturating_mul(2)) as u32;
    config.max_concurrent_uni_streams(MAX_CONCURRENT_UNI_STREAMS.into());
    config.stream_receive_window((PACKET_DATA_SIZE as u32).into());
    config.receive_window(
        (PACKET_DATA_SIZE as u32)
            .saturating_mul(MAX_CONCURRENT_UNI_STREAMS)
            .into(),
    );
    let timeout = IdleTimeout::try_from(QUIC_MAX_TIMEOUT).unwrap();
    config.max_idle_timeout(Some(timeout));

    // disable bidi & datagrams
    const MAX_CONCURRENT_BIDI_STREAMS: u32 = 0;
    config.max_concurrent_bidi_streams(MAX_CONCURRENT_BIDI_STREAMS.into());
    config.datagram_receive_buffer_size(None);

    Ok((server_config, cert_chain_pem))
}

fn rt() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("quic-server")
        .enable_all()
        .build()
        .unwrap()
}

#[derive(thiserror::Error, Debug)]
pub enum QuicServerError {
    #[error("Endpoint creation failed: {0}")]
    EndpointFailed(std::io::Error),
    #[error("Certificate error: {0}")]
    CertificateError(#[from] rcgen::RcgenError),
    #[error("TLS error: {0}")]
    TlsError(#[from] rustls::Error),
}

#[derive(Default)]
pub struct StreamStats {
    pub(crate) total_connections: AtomicUsize,
    pub(crate) total_new_connections: AtomicUsize,
    pub(crate) total_streams: AtomicUsize,
    pub(crate) total_new_streams: AtomicUsize,
    pub(crate) total_invalid_chunks: AtomicUsize,
    pub(crate) total_invalid_chunk_size: AtomicUsize,
    pub(crate) total_packets_allocated: AtomicUsize,
    pub(crate) total_packet_batches_allocated: AtomicUsize,
    pub(crate) total_chunks_received: AtomicUsize,
    pub(crate) total_staked_chunks_received: AtomicUsize,
    pub(crate) total_unstaked_chunks_received: AtomicUsize,
    pub(crate) total_packet_batch_send_err: AtomicUsize,
    pub(crate) total_handle_chunk_to_packet_batcher_send_err: AtomicUsize,
    pub(crate) total_packet_batches_sent: AtomicUsize,
    pub(crate) total_packet_batches_none: AtomicUsize,
    pub(crate) total_packets_sent_for_batching: AtomicUsize,
    pub(crate) total_bytes_sent_for_batching: AtomicUsize,
    pub(crate) total_chunks_sent_for_batching: AtomicUsize,
    pub(crate) total_packets_sent_to_consumer: AtomicUsize,
    pub(crate) total_bytes_sent_to_consumer: AtomicUsize,
    pub(crate) total_chunks_processed_by_batcher: AtomicUsize,
    pub(crate) total_stream_read_errors: AtomicUsize,
    pub(crate) total_stream_read_timeouts: AtomicUsize,
    pub(crate) num_evictions: AtomicUsize,
    pub(crate) connection_added_from_staked_peer: AtomicUsize,
    pub(crate) connection_added_from_unstaked_peer: AtomicUsize,
    pub(crate) connection_add_failed: AtomicUsize,
    pub(crate) connection_add_failed_invalid_stream_count: AtomicUsize,
    pub(crate) connection_add_failed_staked_node: AtomicUsize,
    pub(crate) connection_add_failed_unstaked_node: AtomicUsize,
    pub(crate) connection_add_failed_on_pruning: AtomicUsize,
    pub(crate) connection_setup_timeout: AtomicUsize,
    pub(crate) connection_setup_error: AtomicUsize,
    pub(crate) connection_setup_error_closed: AtomicUsize,
    pub(crate) connection_setup_error_timed_out: AtomicUsize,
    pub(crate) connection_setup_error_transport: AtomicUsize,
    pub(crate) connection_setup_error_app_closed: AtomicUsize,
    pub(crate) connection_setup_error_reset: AtomicUsize,
    pub(crate) connection_setup_error_locally_closed: AtomicUsize,
    pub(crate) connection_removed: AtomicUsize,
    pub(crate) connection_remove_failed: AtomicUsize,
    pub(crate) throttled_streams: AtomicUsize,
}

impl StreamStats {
    pub fn report(&self, name: &'static str) {
        datapoint_info!(
            name,
            (
                "active_connections",
                self.total_connections.load(Ordering::Relaxed),
                i64
            ),
            (
                "active_streams",
                self.total_streams.load(Ordering::Relaxed),
                i64
            ),
            (
                "new_connections",
                self.total_new_connections.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "new_streams",
                self.total_new_streams.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "evictions",
                self.num_evictions.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_added_from_staked_peer",
                self.connection_added_from_staked_peer
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_added_from_unstaked_peer",
                self.connection_added_from_unstaked_peer
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_add_failed",
                self.connection_add_failed.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_add_failed_invalid_stream_count",
                self.connection_add_failed_invalid_stream_count
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_add_failed_staked_node",
                self.connection_add_failed_staked_node
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_add_failed_unstaked_node",
                self.connection_add_failed_unstaked_node
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_add_failed_on_pruning",
                self.connection_add_failed_on_pruning
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_removed",
                self.connection_removed.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_remove_failed",
                self.connection_remove_failed.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_timeout",
                self.connection_setup_timeout.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error",
                self.connection_setup_error.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error_timed_out",
                self.connection_setup_error_timed_out
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error_closed",
                self.connection_setup_error_closed
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error_transport",
                self.connection_setup_error_transport
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error_app_closed",
                self.connection_setup_error_app_closed
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error_reset",
                self.connection_setup_error_reset.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_setup_error_locally_closed",
                self.connection_setup_error_locally_closed
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_chunk",
                self.total_invalid_chunks.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_chunk_size",
                self.total_invalid_chunk_size.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packets_allocated",
                self.total_packets_allocated.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packet_batches_allocated",
                self.total_packet_batches_allocated
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packets_sent_for_batching",
                self.total_packets_sent_for_batching
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bytes_sent_for_batching",
                self.total_bytes_sent_for_batching
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "chunks_sent_for_batching",
                self.total_chunks_sent_for_batching
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packets_sent_to_consumer",
                self.total_packets_sent_to_consumer
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bytes_sent_to_consumer",
                self.total_bytes_sent_to_consumer.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "chunks_processed_by_batcher",
                self.total_chunks_processed_by_batcher
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "chunks_received",
                self.total_chunks_received.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "staked_chunks_received",
                self.total_staked_chunks_received.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "unstaked_chunks_received",
                self.total_unstaked_chunks_received
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packet_batch_send_error",
                self.total_packet_batch_send_err.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "handle_chunk_to_packet_batcher_send_error",
                self.total_handle_chunk_to_packet_batcher_send_err
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packet_batches_sent",
                self.total_packet_batches_sent.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packet_batch_empty",
                self.total_packet_batches_none.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "stream_read_errors",
                self.total_stream_read_errors.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "stream_read_timeouts",
                self.total_stream_read_timeouts.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "throttled_streams",
                self.throttled_streams.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_server(
    name: &'static str,
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_peer: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    wait_for_chunk_timeout: Duration,
    coalesce: Duration,
) -> Result<(Endpoint, thread::JoinHandle<()>), QuicServerError> {
    let runtime = rt();
    let (endpoint, _stats, task) = {
        let _guard = runtime.enter();
        crate::nonblocking::quic::spawn_server(
            name,
            sock,
            keypair,
            gossip_host,
            packet_sender,
            exit,
            max_connections_per_peer,
            staked_nodes,
            max_staked_connections,
            max_unstaked_connections,
            wait_for_chunk_timeout,
            coalesce,
        )
    }?;
    let handle = thread::Builder::new()
        .name("solQuicServer".into())
        .spawn(move || {
            if let Err(e) = runtime.block_on(task) {
                warn!("error from runtime.block_on: {:?}", e);
            }
        })
        .unwrap();
    Ok((endpoint, handle))
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::nonblocking::quic::{test::*, DEFAULT_WAIT_FOR_CHUNK_TIMEOUT},
        crossbeam_channel::unbounded,
        solana_sdk::net::DEFAULT_TPU_COALESCE,
        std::net::SocketAddr,
    };

    fn setup_quic_server() -> (
        std::thread::JoinHandle<()>,
        Arc<AtomicBool>,
        crossbeam_channel::Receiver<PacketBatch>,
        SocketAddr,
    ) {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (_, t) = spawn_server(
            "quic_streamer_test",
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            DEFAULT_TPU_COALESCE,
        )
        .unwrap();
        (t, exit, receiver, server_address)
    }

    #[test]
    fn test_quic_server_exit() {
        let (t, exit, _receiver, _server_address) = setup_quic_server();
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_timeout() {
        solana_logger::setup();
        let (t, exit, receiver, server_address) = setup_quic_server();
        let runtime = rt();
        runtime.block_on(check_timeout(receiver, server_address));
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_server_block_multiple_connections() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address) = setup_quic_server();

        let runtime = rt();
        runtime.block_on(check_block_multiple_connections(server_address));
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_server_multiple_streams() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (_, t) = spawn_server(
            "quic_streamer_test",
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            2,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            DEFAULT_TPU_COALESCE,
        )
        .unwrap();

        let runtime = rt();
        runtime.block_on(check_multiple_streams(receiver, server_address));
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_server_multiple_writes() {
        solana_logger::setup();
        let (t, exit, receiver, server_address) = setup_quic_server();

        let runtime = rt();
        runtime.block_on(check_multiple_writes(receiver, server_address, None));
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_server_unstaked_node_connect_failure() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, _) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (_, t) = spawn_server(
            "quic_streamer_test",
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            0, // Do not allow any connection from unstaked clients/nodes
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            DEFAULT_TPU_COALESCE,
        )
        .unwrap();

        let runtime = rt();
        runtime.block_on(check_unstaked_node_connect_failure(server_address));
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }
}
