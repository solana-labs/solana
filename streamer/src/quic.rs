use {
    crate::streamer::StakedNodes,
    crossbeam_channel::Sender,
    pem::Pem,
    pkcs8::{der::Document, AlgorithmIdentifier, ObjectIdentifier},
    quinn::{IdleTimeout, ServerConfig, VarInt},
    rcgen::{CertificateParams, DistinguishedName, DnType, SanType},
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        packet::PACKET_DATA_SIZE,
        quic::{QUIC_MAX_TIMEOUT_MS, QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS},
        signature::Keypair,
    },
    std::{
        error::Error,
        net::{IpAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread,
    },
    tokio::runtime::{Builder, Runtime},
};

pub const MAX_STAKED_CONNECTIONS: usize = 2000;
pub const MAX_UNSTAKED_CONNECTIONS: usize = 500;
const NUM_QUIC_STREAMER_WORKER_THREADS: usize = 4;

/// Returns default server configuration along with its PEM certificate chain.
#[allow(clippy::field_reassign_with_default)] // https://github.com/rust-lang/rust-clippy/issues/6527
pub(crate) fn configure_server(
    identity_keypair: &Keypair,
    gossip_host: IpAddr,
) -> Result<(ServerConfig, String), QuicServerError> {
    let (cert_chain, priv_key) =
        new_cert(identity_keypair, gossip_host).map_err(|_e| QuicServerError::ConfigureFailed)?;
    let cert_chain_pem_parts: Vec<Pem> = cert_chain
        .iter()
        .map(|cert| Pem {
            tag: "CERTIFICATE".to_string(),
            contents: cert.0.clone(),
        })
        .collect();
    let cert_chain_pem = pem::encode_many(&cert_chain_pem_parts);

    let mut server_config = ServerConfig::with_single_cert(cert_chain, priv_key)
        .map_err(|_e| QuicServerError::ConfigureFailed)?;
    let config = Arc::get_mut(&mut server_config.transport).unwrap();

    // QUIC_MAX_CONCURRENT_STREAMS doubled, which was found to improve reliability
    const MAX_CONCURRENT_UNI_STREAMS: u32 = (QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS * 2) as u32;
    config.max_concurrent_uni_streams(MAX_CONCURRENT_UNI_STREAMS.into());
    config.stream_receive_window((PACKET_DATA_SIZE as u32).into());
    config.receive_window((PACKET_DATA_SIZE as u32 * MAX_CONCURRENT_UNI_STREAMS).into());
    let timeout = IdleTimeout::from(VarInt::from_u32(QUIC_MAX_TIMEOUT_MS));
    config.max_idle_timeout(Some(timeout));

    // disable bidi & datagrams
    const MAX_CONCURRENT_BIDI_STREAMS: u32 = 0;
    config.max_concurrent_bidi_streams(MAX_CONCURRENT_BIDI_STREAMS.into());
    config.datagram_receive_buffer_size(None);

    Ok((server_config, cert_chain_pem))
}

fn new_cert(
    identity_keypair: &Keypair,
    san: IpAddr,
) -> Result<(Vec<rustls::Certificate>, rustls::PrivateKey), Box<dyn Error>> {
    // Generate a self-signed cert from validator identity key
    let cert_params = new_cert_params(identity_keypair, san);
    let cert = rcgen::Certificate::from_params(cert_params)?;
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der)];
    Ok((cert_chain, priv_key))
}

fn convert_to_rcgen_keypair(identity_keypair: &Keypair) -> rcgen::KeyPair {
    // from https://datatracker.ietf.org/doc/html/rfc8410#section-3
    const ED25519_IDENTIFIER: [u32; 4] = [1, 3, 101, 112];
    let mut private_key = Vec::<u8>::with_capacity(34);
    private_key.extend_from_slice(&[0x04, 0x20]); // ASN.1 OCTET STRING
    private_key.extend_from_slice(identity_keypair.secret().as_bytes());
    let key_pkcs8 = pkcs8::PrivateKeyInfo {
        algorithm: AlgorithmIdentifier {
            oid: ObjectIdentifier::from_arcs(&ED25519_IDENTIFIER).unwrap(),
            parameters: None,
        },
        private_key: &private_key,
        public_key: None,
    };
    let key_pkcs8_der = key_pkcs8
        .to_der()
        .expect("Failed to convert keypair to DER")
        .to_der();

    // Parse private key into rcgen::KeyPair struct.
    rcgen::KeyPair::from_der(&key_pkcs8_der).expect("Failed to parse keypair from DER")
}

fn new_cert_params(identity_keypair: &Keypair, san: IpAddr) -> CertificateParams {
    // TODO(terorie): Is it safe to sign the TLS cert with the identity private key?

    // Unfortunately, rcgen does not accept a "raw" Ed25519 key.
    // We have to convert it to DER and pass it to the library.

    // Convert private key into PKCS#8 v1 object.
    // RFC 8410, Section 7: Private Key Format
    // https://datatracker.ietf.org/doc/html/rfc8410#section-

    let keypair = convert_to_rcgen_keypair(identity_keypair);

    let mut cert_params = CertificateParams::default();
    cert_params.subject_alt_names = vec![SanType::IpAddress(san)];
    cert_params.alg = &rcgen::PKCS_ED25519;
    cert_params.key_pair = Some(keypair);
    cert_params.distinguished_name = DistinguishedName::new();
    cert_params
        .distinguished_name
        .push(DnType::CommonName, "Solana node");
    cert_params
}

fn rt() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(NUM_QUIC_STREAMER_WORKER_THREADS)
        .enable_all()
        .build()
        .unwrap()
}

#[derive(thiserror::Error, Debug)]
pub enum QuicServerError {
    #[error("Server configure failed")]
    ConfigureFailed,

    #[error("Endpoint creation failed")]
    EndpointFailed,
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
    pub(crate) total_chunks_received: AtomicUsize,
    pub(crate) total_packet_batch_send_err: AtomicUsize,
    pub(crate) total_packet_batches_sent: AtomicUsize,
    pub(crate) total_packet_batches_none: AtomicUsize,
    pub(crate) total_stream_read_errors: AtomicUsize,
    pub(crate) total_stream_read_timeouts: AtomicUsize,
    pub(crate) num_evictions: AtomicUsize,
    pub(crate) connection_add_failed: AtomicUsize,
    pub(crate) connection_add_failed_invalid_stream_count: AtomicUsize,
    pub(crate) connection_add_failed_unstaked_node: AtomicUsize,
    pub(crate) connection_add_failed_on_pruning: AtomicUsize,
    pub(crate) connection_setup_timeout: AtomicUsize,
    pub(crate) connection_removed: AtomicUsize,
    pub(crate) connection_remove_failed: AtomicUsize,
}

impl StreamStats {
    pub fn report(&self) {
        datapoint_info!(
            "quic-connections",
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
                "chunks_received",
                self.total_chunks_received.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "packet_batch_send_error",
                self.total_packet_batch_send_err.swap(0, Ordering::Relaxed),
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
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_server(
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_ip: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
) -> Result<thread::JoinHandle<()>, QuicServerError> {
    let runtime = rt();
    let task = {
        let _guard = runtime.enter();
        crate::nonblocking::quic::spawn_server(
            sock,
            keypair,
            gossip_host,
            packet_sender,
            exit,
            max_connections_per_ip,
            staked_nodes,
            max_staked_connections,
            max_unstaked_connections,
            stats,
        )
    }?;
    let handle = thread::spawn(move || {
        if let Err(e) = runtime.block_on(task) {
            warn!("error from runtime.block_on: {:?}", e);
        }
    });
    Ok(handle)
}

#[cfg(test)]
mod test {
    use {
        super::*, crate::nonblocking::quic::test::*, crossbeam_channel::unbounded,
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
        let stats = Arc::new(StreamStats::default());
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats,
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
        let stats = Arc::new(StreamStats::default());
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            2,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats,
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
        runtime.block_on(check_multiple_writes(receiver, server_address));
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
        let stats = Arc::new(StreamStats::default());
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            0, // Do not allow any connection from unstaked clients/nodes
            stats,
        )
        .unwrap();

        let runtime = rt();
        runtime.block_on(check_unstaked_node_connect_failure(server_address));
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }
}
