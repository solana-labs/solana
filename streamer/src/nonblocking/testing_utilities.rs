//! Contains utility functions to create server and client for test purposes.
use {
    super::quic::{
        spawn_server_multi, SpawnNonBlockingServerResult, ALPN_TPU_PROTOCOL_ID,
        DEFAULT_MAX_CONNECTIONS_PER_IPADDR_PER_MINUTE, DEFAULT_MAX_STREAMS_PER_MS,
    },
    crate::{
        quic::{StreamerStats, MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
        streamer::StakedNodes,
        tls_certificates::new_dummy_x509_certificate,
    },
    crossbeam_channel::unbounded,
    quinn::{ClientConfig, Connection, EndpointConfig, IdleTimeout, TokioRuntime, TransportConfig},
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        net::DEFAULT_TPU_COALESCE,
        quic::{QUIC_KEEP_ALIVE, QUIC_MAX_TIMEOUT},
        signer::keypair::Keypair,
    },
    std::{
        net::{SocketAddr, UdpSocket},
        sync::{atomic::AtomicBool, Arc, RwLock},
        time::Duration,
    },
    tokio::task::JoinHandle,
};

struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub fn get_client_config(keypair: &Keypair) -> ClientConfig {
    let (cert, key) = new_dummy_x509_certificate(keypair);

    let mut crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_client_auth_cert(vec![cert], key)
        .expect("Provided key should be correctly set.");

    crypto.enable_early_data = true;
    crypto.alpn_protocols = vec![ALPN_TPU_PROTOCOL_ID.to_vec()];

    let mut config = ClientConfig::new(Arc::new(crypto));

    let mut transport_config = TransportConfig::default();
    let timeout = IdleTimeout::try_from(QUIC_MAX_TIMEOUT).unwrap();
    transport_config.max_idle_timeout(Some(timeout));
    transport_config.keep_alive_interval(Some(QUIC_KEEP_ALIVE));
    config.transport_config(Arc::new(transport_config));

    config
}

#[derive(Debug, Clone)]
pub struct TestServerConfig {
    pub max_connections_per_peer: usize,
    pub max_staked_connections: usize,
    pub max_unstaked_connections: usize,
    pub max_streams_per_ms: u64,
    pub max_connections_per_ipaddr_per_minute: u64,
}

impl Default for TestServerConfig {
    fn default() -> Self {
        Self {
            max_connections_per_peer: 1,
            max_staked_connections: MAX_STAKED_CONNECTIONS,
            max_unstaked_connections: MAX_UNSTAKED_CONNECTIONS,
            max_streams_per_ms: DEFAULT_MAX_STREAMS_PER_MS,
            max_connections_per_ipaddr_per_minute: DEFAULT_MAX_CONNECTIONS_PER_IPADDR_PER_MINUTE,
        }
    }
}

pub struct SpawnTestServerResult {
    pub join_handle: JoinHandle<()>,
    pub exit: Arc<AtomicBool>,
    pub receiver: crossbeam_channel::Receiver<PacketBatch>,
    pub server_address: SocketAddr,
    pub stats: Arc<StreamerStats>,
}

pub fn setup_quic_server(
    option_staked_nodes: Option<StakedNodes>,
    TestServerConfig {
        max_connections_per_peer,
        max_staked_connections,
        max_unstaked_connections,
        max_streams_per_ms,
        max_connections_per_ipaddr_per_minute,
    }: TestServerConfig,
) -> SpawnTestServerResult {
    let sockets = {
        #[cfg(not(target_os = "windows"))]
        {
            use std::{
                os::fd::{FromRawFd, IntoRawFd},
                str::FromStr as _,
            };
            (0..10)
                .map(|_| {
                    let sock = socket2::Socket::new(
                        socket2::Domain::IPV4,
                        socket2::Type::DGRAM,
                        Some(socket2::Protocol::UDP),
                    )
                    .unwrap();
                    sock.set_reuse_port(true).unwrap();
                    sock.bind(&SocketAddr::from_str("127.0.0.1:0").unwrap().into())
                        .unwrap();
                    unsafe { UdpSocket::from_raw_fd(sock.into_raw_fd()) }
                })
                .collect::<Vec<_>>()
        }
        #[cfg(target_os = "windows")]
        {
            vec![UdpSocket::bind("127.0.0.1:0").unwrap()]
        }
    };

    let exit = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = unbounded();
    let keypair = Keypair::new();
    let server_address = sockets[0].local_addr().unwrap();
    let staked_nodes = Arc::new(RwLock::new(option_staked_nodes.unwrap_or_default()));
    let SpawnNonBlockingServerResult {
        endpoints: _,
        stats,
        thread: handle,
        max_concurrent_connections: _,
    } = spawn_server_multi(
        "quic_streamer_test",
        sockets,
        &keypair,
        sender,
        exit.clone(),
        max_connections_per_peer,
        staked_nodes,
        max_staked_connections,
        max_unstaked_connections,
        max_streams_per_ms,
        max_connections_per_ipaddr_per_minute,
        Duration::from_secs(2),
        DEFAULT_TPU_COALESCE,
    )
    .unwrap();
    SpawnTestServerResult {
        join_handle: handle,
        exit,
        receiver,
        server_address,
        stats,
    }
}

pub async fn make_client_endpoint(
    addr: &SocketAddr,
    client_keypair: Option<&Keypair>,
) -> Connection {
    let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut endpoint = quinn::Endpoint::new(
        EndpointConfig::default(),
        None,
        client_socket,
        Arc::new(TokioRuntime),
    )
    .unwrap();
    let default_keypair = Keypair::new();
    endpoint.set_default_client_config(get_client_config(
        client_keypair.unwrap_or(&default_keypair),
    ));
    endpoint
        .connect(*addr, "localhost")
        .expect("Endpoint configuration should be correct")
        .await
        .expect("Test server should be already listening on 'localhost'")
}
