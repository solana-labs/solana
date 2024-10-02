use {
    bytes::Bytes,
    crossbeam_channel::Sender,
    futures::future::{TryJoin, TryJoin3},
    log::error,
    quinn::{
        crypto::rustls::{QuicClientConfig, QuicServerConfig},
        ClientConfig, ConnectError, Connecting, Connection, ConnectionError, Endpoint,
        EndpointConfig, IdleTimeout, SendDatagramError, ServerConfig, TokioRuntime,
        TransportConfig, VarInt,
    },
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        CertificateError, KeyLogFile,
    },
    solana_gossip::contact_info::Protocol,
    solana_quic_client::nonblocking::quic_client::SkipServerVerification,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    solana_streamer::{quic::SkipClientVerification, tls_certificates::new_dummy_x509_certificate},
    std::{
        cmp::Reverse,
        collections::{hash_map::Entry, HashMap},
        io::Error as IoError,
        net::{SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, RwLock,
        },
        time::Duration,
    },
    thiserror::Error,
    tokio::{
        sync::{
            mpsc::{error::TrySendError, Receiver as AsyncReceiver, Sender as AsyncSender},
            Mutex, RwLock as AsyncRwLock,
        },
        task::JoinHandle,
    },
};

// Incoming packets can be either:
//   RepairProtocol
//   RepairResponse or Shred + repair Nonce
//   AncestorHashesResponse
// So, we need 3 QUIC endpoints on 3 separate sockets to correctly distinguish
// between these packets and send them down the right channel.
// 1) serve_repair_quic:
//   The server side receives incoming RepairProtocols from the cluster and
//   channels them to serve_repair using a Sender<RemoteRequest> channel.
//   The outgoing repair (or ancestor hashes) responses from serve_repair are
//   sent back to the client side through a AsyncReceiver<(SocketAddr, Bytes)>
//   channel and sent back to the remote node.
// 2) repair_quic:
//   Outgoing repair requests from the repair_service are received by the
//   client through a AsyncReceiver<(SocketAddr, Bytes)> channel and sent to
//   serve_repair_quic socket of the remote node.
//   Incoming repair responses (RepairResponse or Shred + repair Nonce) are
//   channeled to shred-fetch-stage using a Sender<(Pubkey, SocketAddr, Bytes)>
//   channel.
// 3) ancestor_hashes_requests_quic:
//   Outgoing RepairProtocol::AncestorHashes requests from the
//   ancestor_hashes_service are received by the client through a
//   AsyncReceiver<(SocketAddr, Bytes)> channel and sent to serve_repair_quic
//   socket of the remote node.
//   Incoming AncestorHashesResponse are channeled back to
//   ancestor_hashes_service using a Sender<(Pubkey, SocketAddr, Bytes)>
//   channel.

const CLIENT_CHANNEL_BUFFER: usize = 1 << 14;
const ROUTER_CHANNEL_BUFFER: usize = 64;
const CONNECTION_CACHE_CAPACITY: usize = 3072;
const ALPN_REPAIR_PROTOCOL_ID: &[u8] = b"solana-repair";
const CONNECT_SERVER_NAME: &str = "solana-repair";

// Transport config.
const DATAGRAM_RECEIVE_BUFFER_SIZE: usize = 256 * 1024 * 1024;
const DATAGRAM_SEND_BUFFER_SIZE: usize = 128 * 1024 * 1024;
const INITIAL_MAXIMUM_TRANSMISSION_UNIT: u16 = MINIMUM_MAXIMUM_TRANSMISSION_UNIT;
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(4);
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const MINIMUM_MAXIMUM_TRANSMISSION_UNIT: u16 = 1280;

const CONNECTION_CLOSE_ERROR_CODE_SHUTDOWN: VarInt = VarInt::from_u32(1);
const CONNECTION_CLOSE_ERROR_CODE_DROPPED: VarInt = VarInt::from_u32(2);
const CONNECTION_CLOSE_ERROR_CODE_INVALID_IDENTITY: VarInt = VarInt::from_u32(3);
const CONNECTION_CLOSE_ERROR_CODE_REPLACED: VarInt = VarInt::from_u32(4);
const CONNECTION_CLOSE_ERROR_CODE_PRUNED: VarInt = VarInt::from_u32(5);

const CONNECTION_CLOSE_REASON_SHUTDOWN: &[u8] = b"SHUTDOWN";
const CONNECTION_CLOSE_REASON_DROPPED: &[u8] = b"DROPPED";
const CONNECTION_CLOSE_REASON_INVALID_IDENTITY: &[u8] = b"INVALID_IDENTITY";
const CONNECTION_CLOSE_REASON_REPLACED: &[u8] = b"REPLACED";
const CONNECTION_CLOSE_REASON_PRUNED: &[u8] = b"PRUNED";

pub(crate) type AsyncTryJoinHandle = TryJoin3<
    TryJoin<JoinHandle<()>, JoinHandle<()>>,
    TryJoin<JoinHandle<()>, JoinHandle<()>>,
    TryJoin<JoinHandle<()>, JoinHandle<()>>,
>;

// Incoming requests from remote nodes.
// remote_pubkey is None only when adapting UDP packets.
pub(crate) struct RemoteRequest {
    pub(crate) remote_pubkey: Option<Pubkey>,
    pub(crate) remote_address: SocketAddr,
    pub(crate) bytes: Bytes,
}

// Async sender channel for directing outgoing packets from validator threads
// to QUIC clients.
pub(crate) struct RepairQuicAsyncSenders {
    // Outgoing repair responses to remote repair requests from serve_repair.
    pub(crate) repair_response_quic_sender: AsyncSender<(SocketAddr, Bytes)>,
    // Outgoing local repair requests from repair_service.
    pub(crate) repair_request_quic_sender: AsyncSender<(SocketAddr, Bytes)>,
    // Outgoing RepairProtocol::AncestorHashes requests from
    // ancestor_hashes_service.
    pub(crate) ancestor_hashes_request_quic_sender: AsyncSender<(SocketAddr, Bytes)>,
}

pub(crate) struct RepairQuicSockets {
    // Socket receiving remote repair or ancestor hashes requests from the
    // cluster, and sending back repair and ancestor hashes responses.
    pub(crate) repair_server_quic_socket: UdpSocket,
    // Socket sending out local repair requests,
    // and receiving repair responses from the cluster.
    pub(crate) repair_client_quic_socket: UdpSocket,
    // Socket sending out local RepairProtocol::AncestorHashes,
    // and receiving AncestorHashesResponse from the cluster.
    pub(crate) ancestor_hashes_quic_socket: UdpSocket,
}

// Sender channel for directing incoming packets from QUIC servers to validator
// threads processing those packets.
pub(crate) struct RepairQuicSenders {
    // Channel to send incoming repair requests from the cluster.
    pub(crate) repair_request_quic_sender: Sender<RemoteRequest>,
    // Channel to send incoming repair responses from the cluster.
    pub(crate) repair_response_quic_sender: Sender<(Pubkey, SocketAddr, Bytes)>,
    // Channel to send incoming ancestor hashes responses from the cluster.
    pub(crate) ancestor_hashes_response_quic_sender: Sender<(Pubkey, SocketAddr, Bytes)>,
}

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Error {
    #[error("Channel Send Error")]
    ChannelSendError,
    #[error(transparent)]
    ConnectError(#[from] ConnectError),
    #[error(transparent)]
    ConnectionError(#[from] ConnectionError),
    #[error("Invalid Identity: {0:?}")]
    InvalidIdentity(SocketAddr),
    #[error(transparent)]
    IoError(#[from] IoError),
    #[error(transparent)]
    SendDatagramError(#[from] SendDatagramError),
    #[error(transparent)]
    TlsError(#[from] rustls::Error),
}

macro_rules! add_metric {
    ($metric: expr) => {{
        $metric.fetch_add(1, Ordering::Relaxed);
    }};
}

pub(crate) fn new_quic_endpoints(
    runtime: &tokio::runtime::Handle,
    keypair: &Keypair,
    sockets: RepairQuicSockets,
    senders: RepairQuicSenders,
    bank_forks: Arc<RwLock<BankForks>>,
) -> Result<([Endpoint; 3], RepairQuicAsyncSenders, AsyncTryJoinHandle), Error> {
    let (repair_server_quic_endpoint, repair_response_quic_sender, repair_server_join_handle) =
        new_quic_endpoint(
            runtime,
            "repair_server_quic_client",
            "repair_server_quic_server",
            keypair,
            sockets.repair_server_quic_socket,
            senders.repair_request_quic_sender,
            bank_forks.clone(),
        )?;
    let (repair_client_quic_endpoint, repair_request_quic_sender, repair_client_join_handle) =
        new_quic_endpoint(
            runtime,
            "repair_client_quic_client",
            "repair_client_quic_server",
            keypair,
            sockets.repair_client_quic_socket,
            senders.repair_response_quic_sender,
            bank_forks.clone(),
        )?;
    let (
        ancestor_hashes_quic_endpoint,
        ancestor_hashes_request_quic_sender,
        ancestor_hashes_join_handle,
    ) = new_quic_endpoint(
        runtime,
        "ancestor_hashes_quic_client",
        "ancestor_hashes_quic_server",
        keypair,
        sockets.ancestor_hashes_quic_socket,
        senders.ancestor_hashes_response_quic_sender,
        bank_forks,
    )?;
    Ok((
        [
            repair_server_quic_endpoint,
            repair_client_quic_endpoint,
            ancestor_hashes_quic_endpoint,
        ],
        RepairQuicAsyncSenders {
            repair_response_quic_sender,
            repair_request_quic_sender,
            ancestor_hashes_request_quic_sender,
        },
        futures::future::try_join3(
            repair_server_join_handle,
            repair_client_join_handle,
            ancestor_hashes_join_handle,
        ),
    ))
}

#[allow(clippy::type_complexity)]
fn new_quic_endpoint<T>(
    runtime: &tokio::runtime::Handle,
    client_name: &'static str,
    server_name: &'static str,
    keypair: &Keypair,
    socket: UdpSocket,
    sender: Sender<T>,
    bank_forks: Arc<RwLock<BankForks>>,
) -> Result<
    (
        Endpoint,
        AsyncSender<(SocketAddr, Bytes)>,
        TryJoin<JoinHandle<()>, JoinHandle<()>>,
    ),
    Error,
>
where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    let (cert, key) = new_dummy_x509_certificate(keypair);
    let server_config = new_server_config(cert.clone(), key.clone_key())?;
    let client_config = new_client_config(cert, key)?;
    let mut endpoint = {
        // Endpoint::new requires entering the runtime context,
        // otherwise the code below will panic.
        let _guard = runtime.enter();
        Endpoint::new(
            EndpointConfig::default(),
            Some(server_config),
            socket,
            Arc::new(TokioRuntime),
        )?
    };
    endpoint.set_default_client_config(client_config);
    let prune_cache_pending = Arc::<AtomicBool>::default();
    let cache = Arc::<Mutex<HashMap<Pubkey, Connection>>>::default();
    let router = Arc::<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>::default();
    let (client_sender, client_receiver) = tokio::sync::mpsc::channel(CLIENT_CHANNEL_BUFFER);
    let server_task = runtime.spawn(run_server(
        endpoint.clone(),
        server_name,
        sender.clone(),
        bank_forks.clone(),
        prune_cache_pending.clone(),
        router.clone(),
        cache.clone(),
    ));
    let client_task = runtime.spawn(run_client(
        endpoint.clone(),
        client_name,
        client_receiver,
        sender,
        bank_forks,
        prune_cache_pending,
        router,
        cache,
    ));
    let task = futures::future::try_join(server_task, client_task);
    Ok((endpoint, client_sender, task))
}

pub(crate) fn close_quic_endpoint(endpoint: &Endpoint) {
    endpoint.close(
        CONNECTION_CLOSE_ERROR_CODE_SHUTDOWN,
        CONNECTION_CLOSE_REASON_SHUTDOWN,
    );
}

fn new_server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, rustls::Error> {
    let mut config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(SkipClientVerification::new())
        .with_single_cert(vec![cert], key)?;
    config.alpn_protocols = vec![ALPN_REPAIR_PROTOCOL_ID.to_vec()];
    config.key_log = Arc::new(KeyLogFile::new());
    let Ok(config) = QuicServerConfig::try_from(config) else {
        return Err(rustls::Error::InvalidCertificate(
            CertificateError::BadSignature,
        ));
    };
    let mut config = ServerConfig::with_crypto(Arc::new(config));
    config
        .transport_config(Arc::new(new_transport_config()))
        .migration(false);
    Ok(config)
}

fn new_client_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<ClientConfig, rustls::Error> {
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_client_auth_cert(vec![cert], key)?;
    config.enable_early_data = true;
    config.alpn_protocols = vec![ALPN_REPAIR_PROTOCOL_ID.to_vec()];
    let mut config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
    config.transport_config(Arc::new(new_transport_config()));
    Ok(config)
}

fn new_transport_config() -> TransportConfig {
    let max_idle_timeout = IdleTimeout::try_from(MAX_IDLE_TIMEOUT).unwrap();
    let mut config = TransportConfig::default();
    config
        .datagram_receive_buffer_size(Some(DATAGRAM_RECEIVE_BUFFER_SIZE))
        .datagram_send_buffer_size(DATAGRAM_SEND_BUFFER_SIZE)
        .initial_mtu(INITIAL_MAXIMUM_TRANSMISSION_UNIT)
        .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
        .max_concurrent_bidi_streams(VarInt::from(0u8))
        .max_concurrent_uni_streams(VarInt::from(0u8))
        .max_idle_timeout(Some(max_idle_timeout))
        .min_mtu(MINIMUM_MAXIMUM_TRANSMISSION_UNIT)
        .mtu_discovery_config(None);
    config
}

async fn run_server<T>(
    endpoint: Endpoint,
    server_name: &'static str,
    sender: Sender<T>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    let stats = Arc::<RepairQuicStats>::default();
    let report_metrics_task = tokio::task::spawn(report_metrics_task(server_name, stats.clone()));
    while let Some(incoming) = endpoint.accept().await {
        let remote_addr: SocketAddr = incoming.remote_address();
        match incoming.accept() {
            Ok(connecting) => {
                tokio::task::spawn(handle_connecting_task(
                    endpoint.clone(),
                    connecting,
                    sender.clone(),
                    bank_forks.clone(),
                    prune_cache_pending.clone(),
                    router.clone(),
                    cache.clone(),
                    stats.clone(),
                ));
            }
            Err(err) => {
                debug!("Error while accepting incoming connection: {err:?} from {remote_addr}");
                record_error(&Error::from(err), &stats);
            }
        }
    }
    report_metrics_task.abort();
}

async fn run_client<T>(
    endpoint: Endpoint,
    client_name: &'static str,
    mut receiver: AsyncReceiver<(SocketAddr, Bytes)>,
    sender: Sender<T>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    let stats = Arc::<RepairQuicStats>::default();
    let report_metrics_task = tokio::task::spawn(report_metrics_task(client_name, stats.clone()));
    while let Some((remote_address, bytes)) = receiver.recv().await {
        let Some(bytes) = try_route_bytes(&remote_address, bytes, &*router.read().await, &stats)
        else {
            continue;
        };
        let receiver = {
            let mut router = router.write().await;
            let Some(bytes) = try_route_bytes(&remote_address, bytes, &router, &stats) else {
                continue;
            };
            let (sender, receiver) = tokio::sync::mpsc::channel(ROUTER_CHANNEL_BUFFER);
            sender.try_send(bytes).unwrap();
            router.insert(remote_address, sender);
            receiver
        };
        tokio::task::spawn(make_connection_task(
            endpoint.clone(),
            remote_address,
            sender.clone(),
            receiver,
            bank_forks.clone(),
            prune_cache_pending.clone(),
            router.clone(),
            cache.clone(),
            stats.clone(),
        ));
    }
    close_quic_endpoint(&endpoint);
    // Drop sender channels to unblock threads waiting on the receiving end.
    router.write().await.clear();
    report_metrics_task.abort();
}

// Routes the payload to respective channel.
// Drops the payload if the channel is full.
// Bounces the payload back if the channel is closed or does not exist.
fn try_route_bytes(
    remote_address: &SocketAddr,
    bytes: Bytes,
    router: &HashMap<SocketAddr, AsyncSender<Bytes>>,
    stats: &RepairQuicStats,
) -> Option<Bytes> {
    match router.get(remote_address) {
        None => Some(bytes),
        Some(sender) => match sender.try_send(bytes) {
            Ok(()) => None,
            Err(TrySendError::Full(_)) => {
                debug!("TrySendError::Full {remote_address}");
                add_metric!(stats.router_try_send_error_full);
                None
            }
            Err(TrySendError::Closed(bytes)) => Some(bytes),
        },
    }
}

async fn handle_connecting_task<T>(
    endpoint: Endpoint,
    connecting: Connecting,
    sender: Sender<T>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    if let Err(err) = handle_connecting(
        endpoint,
        connecting,
        sender,
        bank_forks,
        prune_cache_pending,
        router,
        cache,
        stats.clone(),
    )
    .await
    {
        debug!("handle_connecting: {err:?}");
        record_error(&err, &stats);
    }
}

async fn handle_connecting<T>(
    endpoint: Endpoint,
    connecting: Connecting,
    sender: Sender<T>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error>
where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    let connection = connecting.await?;
    let remote_address = connection.remote_address();
    let remote_pubkey = get_remote_pubkey(&connection)?;
    let receiver = {
        let (sender, receiver) = tokio::sync::mpsc::channel(ROUTER_CHANNEL_BUFFER);
        router.write().await.insert(remote_address, sender);
        receiver
    };
    handle_connection(
        endpoint,
        remote_address,
        remote_pubkey,
        connection,
        sender,
        receiver,
        bank_forks,
        prune_cache_pending,
        router,
        cache,
        stats,
    )
    .await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection<T>(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: Connection,
    sender: Sender<T>,
    receiver: AsyncReceiver<Bytes>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    cache_connection(
        remote_pubkey,
        connection.clone(),
        bank_forks,
        prune_cache_pending,
        router.clone(),
        cache.clone(),
    )
    .await;
    let send_datagram_task = tokio::task::spawn(send_datagram_task(connection.clone(), receiver));
    let read_datagram_task = tokio::task::spawn(read_datagram_task(
        endpoint,
        remote_address,
        remote_pubkey,
        connection.clone(),
        sender,
        stats.clone(),
    ));
    match futures::future::try_join(send_datagram_task, read_datagram_task).await {
        Err(err) => error!("handle_connection: {remote_pubkey}, {remote_address}, {err:?}"),
        Ok(out) => {
            if let (Err(ref err), _) = out {
                debug!("send_datagram_task: {remote_pubkey}, {remote_address}, {err:?}");
                record_error(err, &stats);
            }
            if let (_, Err(ref err)) = out {
                debug!("read_datagram_task: {remote_pubkey}, {remote_address}, {err:?}");
                record_error(err, &stats);
            }
        }
    }
    drop_connection(remote_pubkey, &connection, &cache).await;
    if let Entry::Occupied(entry) = router.write().await.entry(remote_address) {
        if entry.get().is_closed() {
            entry.remove();
        }
    }
}

async fn read_datagram_task<T>(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: Connection,
    sender: Sender<T>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error>
where
    T: From<(Pubkey, SocketAddr, Bytes)>,
{
    // Assert that send won't block.
    debug_assert_eq!(sender.capacity(), None);
    loop {
        match connection.read_datagram().await {
            Ok(bytes) => {
                let value = T::from((remote_pubkey, remote_address, bytes));
                if let Err(err) = sender.send(value) {
                    close_quic_endpoint(&endpoint);
                    return Err(Error::from(err));
                }
            }
            Err(err) => {
                if let Some(err) = connection.close_reason() {
                    return Err(Error::from(err));
                }
                debug!("connection.read_datagram: {remote_pubkey}, {remote_address}, {err:?}");
                record_error(&Error::from(err), &stats);
            }
        };
    }
}

async fn send_datagram_task(
    connection: Connection,
    mut receiver: AsyncReceiver<Bytes>,
) -> Result<(), Error> {
    tokio::pin! {
        let connection_closed = connection.closed();
    }
    loop {
        tokio::select! {
            biased;
            bytes = receiver.recv() => {
                match bytes {
                    None => return Ok(()),
                    Some(bytes) => connection.send_datagram(bytes)?,
                }
            }
            err = &mut connection_closed => return Err(Error::from(err)),
        }
    }
}

async fn make_connection_task<T>(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    sender: Sender<T>,
    receiver: AsyncReceiver<Bytes>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    if let Err(err) = make_connection(
        endpoint,
        remote_address,
        sender,
        receiver,
        bank_forks,
        prune_cache_pending,
        router,
        cache,
        stats.clone(),
    )
    .await
    {
        debug!("make_connection: {remote_address}, {err:?}");
        record_error(&err, &stats);
    }
}

async fn make_connection<T>(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    sender: Sender<T>,
    receiver: AsyncReceiver<Bytes>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error>
where
    T: 'static + From<(Pubkey, SocketAddr, Bytes)> + Send,
{
    let connection = endpoint
        .connect(remote_address, CONNECT_SERVER_NAME)?
        .await?;
    handle_connection(
        endpoint,
        connection.remote_address(),
        get_remote_pubkey(&connection)?,
        connection,
        sender,
        receiver,
        bank_forks,
        prune_cache_pending,
        router,
        cache,
        stats,
    )
    .await;
    Ok(())
}

fn get_remote_pubkey(connection: &Connection) -> Result<Pubkey, Error> {
    match solana_streamer::nonblocking::quic::get_remote_pubkey(connection) {
        Some(remote_pubkey) => Ok(remote_pubkey),
        None => {
            connection.close(
                CONNECTION_CLOSE_ERROR_CODE_INVALID_IDENTITY,
                CONNECTION_CLOSE_REASON_INVALID_IDENTITY,
            );
            Err(Error::InvalidIdentity(connection.remote_address()))
        }
    }
}

async fn cache_connection(
    remote_pubkey: Pubkey,
    connection: Connection,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) {
    let (old, should_prune_cache) = {
        let mut cache = cache.lock().await;
        (
            cache.insert(remote_pubkey, connection),
            cache.len() >= CONNECTION_CACHE_CAPACITY.saturating_mul(2),
        )
    };
    if let Some(old) = old {
        old.close(
            CONNECTION_CLOSE_ERROR_CODE_REPLACED,
            CONNECTION_CLOSE_REASON_REPLACED,
        );
    }
    if should_prune_cache && !prune_cache_pending.swap(true, Ordering::Relaxed) {
        tokio::task::spawn(prune_connection_cache(
            bank_forks,
            prune_cache_pending,
            router,
            cache,
        ));
    }
}

async fn drop_connection(
    remote_pubkey: Pubkey,
    connection: &Connection,
    cache: &Mutex<HashMap<Pubkey, Connection>>,
) {
    connection.close(
        CONNECTION_CLOSE_ERROR_CODE_DROPPED,
        CONNECTION_CLOSE_REASON_DROPPED,
    );
    if let Entry::Occupied(entry) = cache.lock().await.entry(remote_pubkey) {
        if entry.get().stable_id() == connection.stable_id() {
            entry.remove();
        }
    }
}

async fn prune_connection_cache(
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<Bytes>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) {
    debug_assert!(prune_cache_pending.load(Ordering::Relaxed));
    let staked_nodes = {
        let root_bank = bank_forks.read().unwrap().root_bank();
        root_bank.current_epoch_staked_nodes()
    };
    {
        let mut cache = cache.lock().await;
        if cache.len() < CONNECTION_CACHE_CAPACITY.saturating_mul(2) {
            prune_cache_pending.store(false, Ordering::Relaxed);
            return;
        }
        let mut connections: Vec<_> = cache
            .drain()
            .filter(|(_, connection)| connection.close_reason().is_none())
            .map(|entry @ (pubkey, _)| {
                let stake = staked_nodes.get(&pubkey).copied().unwrap_or_default();
                (stake, entry)
            })
            .collect();
        connections
            .select_nth_unstable_by_key(CONNECTION_CACHE_CAPACITY, |&(stake, _)| Reverse(stake));
        for (_, (_, connection)) in &connections[CONNECTION_CACHE_CAPACITY..] {
            connection.close(
                CONNECTION_CLOSE_ERROR_CODE_PRUNED,
                CONNECTION_CLOSE_REASON_PRUNED,
            );
        }
        cache.extend(
            connections
                .into_iter()
                .take(CONNECTION_CACHE_CAPACITY)
                .map(|(_, entry)| entry),
        );
        prune_cache_pending.store(false, Ordering::Relaxed);
    }
    router.write().await.retain(|_, sender| !sender.is_closed());
}

impl RemoteRequest {
    #[inline]
    pub(crate) fn protocol(&self) -> Protocol {
        // remote_pubkey is only available with QUIC.
        if self.remote_pubkey.is_some() {
            Protocol::QUIC
        } else {
            Protocol::UDP
        }
    }
}

impl From<(Pubkey, SocketAddr, Bytes)> for RemoteRequest {
    #[inline]
    fn from((pubkey, remote_address, bytes): (Pubkey, SocketAddr, Bytes)) -> Self {
        Self {
            remote_pubkey: Some(pubkey),
            remote_address,
            bytes,
        }
    }
}

impl RepairQuicAsyncSenders {
    pub(crate) fn new_dummy() -> Self {
        Self {
            repair_response_quic_sender: tokio::sync::mpsc::channel(1).0,
            repair_request_quic_sender: tokio::sync::mpsc::channel(1).0,
            ancestor_hashes_request_quic_sender: tokio::sync::mpsc::channel(1).0,
        }
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        Error::ChannelSendError
    }
}

#[derive(Default)]
struct RepairQuicStats {
    connect_error_cids_exhausted: AtomicU64,
    connect_error_invalid_remote_address: AtomicU64,
    connect_error_other: AtomicU64,
    connection_error_application_closed: AtomicU64,
    connection_error_cids_exhausted: AtomicU64,
    connection_error_connection_closed: AtomicU64,
    connection_error_locally_closed: AtomicU64,
    connection_error_reset: AtomicU64,
    connection_error_timed_out: AtomicU64,
    connection_error_transport_error: AtomicU64,
    connection_error_version_mismatch: AtomicU64,
    invalid_identity: AtomicU64,
    router_try_send_error_full: AtomicU64,
    send_datagram_error_connection_lost: AtomicU64,
    send_datagram_error_too_large: AtomicU64,
    send_datagram_error_unsupported_by_peer: AtomicU64,
}

async fn report_metrics_task(name: &'static str, stats: Arc<RepairQuicStats>) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        report_metrics(name, &stats);
    }
}

fn record_error(err: &Error, stats: &RepairQuicStats) {
    match err {
        Error::ChannelSendError => (),
        Error::ConnectError(ConnectError::CidsExhausted) => {
            add_metric!(stats.connect_error_cids_exhausted)
        }
        Error::ConnectError(ConnectError::EndpointStopping) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectError(ConnectError::InvalidRemoteAddress(_)) => {
            add_metric!(stats.connect_error_invalid_remote_address)
        }
        Error::ConnectError(ConnectError::InvalidServerName(_)) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectError(ConnectError::NoDefaultClientConfig) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectError(ConnectError::UnsupportedVersion) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectionError(ConnectionError::CidsExhausted) => {
            add_metric!(stats.connection_error_cids_exhausted)
        }
        Error::ConnectionError(ConnectionError::VersionMismatch) => {
            add_metric!(stats.connection_error_version_mismatch)
        }
        Error::ConnectionError(ConnectionError::TransportError(_)) => {
            add_metric!(stats.connection_error_transport_error)
        }
        Error::ConnectionError(ConnectionError::ConnectionClosed(_)) => {
            add_metric!(stats.connection_error_connection_closed)
        }
        Error::ConnectionError(ConnectionError::ApplicationClosed(_)) => {
            add_metric!(stats.connection_error_application_closed)
        }
        Error::ConnectionError(ConnectionError::Reset) => add_metric!(stats.connection_error_reset),
        Error::ConnectionError(ConnectionError::TimedOut) => {
            add_metric!(stats.connection_error_timed_out)
        }
        Error::ConnectionError(ConnectionError::LocallyClosed) => {
            add_metric!(stats.connection_error_locally_closed)
        }
        Error::InvalidIdentity(_) => add_metric!(stats.invalid_identity),
        Error::IoError(_) => (),
        Error::SendDatagramError(SendDatagramError::UnsupportedByPeer) => {
            add_metric!(stats.send_datagram_error_unsupported_by_peer)
        }
        Error::SendDatagramError(SendDatagramError::Disabled) => (),
        Error::SendDatagramError(SendDatagramError::TooLarge) => {
            add_metric!(stats.send_datagram_error_too_large)
        }
        Error::SendDatagramError(SendDatagramError::ConnectionLost(_)) => {
            add_metric!(stats.send_datagram_error_connection_lost)
        }
        Error::TlsError(_) => (),
    }
}

fn report_metrics(name: &'static str, stats: &RepairQuicStats) {
    macro_rules! reset_metric {
        ($metric: expr) => {
            $metric.swap(0, Ordering::Relaxed)
        };
    }
    datapoint_info!(
        name,
        (
            "connect_error_cids_exhausted",
            reset_metric!(stats.connect_error_cids_exhausted),
            i64
        ),
        (
            "connect_error_invalid_remote_address",
            reset_metric!(stats.connect_error_invalid_remote_address),
            i64
        ),
        (
            "connect_error_other",
            reset_metric!(stats.connect_error_other),
            i64
        ),
        (
            "connection_error_application_closed",
            reset_metric!(stats.connection_error_application_closed),
            i64
        ),
        (
            "connection_error_cids_exhausted",
            reset_metric!(stats.connection_error_cids_exhausted),
            i64
        ),
        (
            "connection_error_connection_closed",
            reset_metric!(stats.connection_error_connection_closed),
            i64
        ),
        (
            "connection_error_locally_closed",
            reset_metric!(stats.connection_error_locally_closed),
            i64
        ),
        (
            "connection_error_reset",
            reset_metric!(stats.connection_error_reset),
            i64
        ),
        (
            "connection_error_timed_out",
            reset_metric!(stats.connection_error_timed_out),
            i64
        ),
        (
            "connection_error_transport_error",
            reset_metric!(stats.connection_error_transport_error),
            i64
        ),
        (
            "connection_error_version_mismatch",
            reset_metric!(stats.connection_error_version_mismatch),
            i64
        ),
        (
            "invalid_identity",
            reset_metric!(stats.invalid_identity),
            i64
        ),
        (
            "router_try_send_error_full",
            reset_metric!(stats.router_try_send_error_full),
            i64
        ),
        (
            "send_datagram_error_connection_lost",
            reset_metric!(stats.send_datagram_error_connection_lost),
            i64
        ),
        (
            "send_datagram_error_too_large",
            reset_metric!(stats.send_datagram_error_too_large),
            i64
        ),
        (
            "send_datagram_error_unsupported_by_peer",
            reset_metric!(stats.send_datagram_error_unsupported_by_peer),
            i64
        ),
    );
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        itertools::{izip, multiunzip},
        solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo},
        solana_runtime::bank::Bank,
        solana_sdk::signature::Signer,
        std::{iter::repeat_with, net::Ipv4Addr, time::Duration},
    };

    #[test]
    fn test_quic_endpoint() {
        const NUM_ENDPOINTS: usize = 3;
        const RECV_TIMEOUT: Duration = Duration::from_secs(60);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(8)
            .enable_all()
            .build()
            .unwrap();
        let keypairs: Vec<Keypair> = repeat_with(Keypair::new).take(NUM_ENDPOINTS).collect();
        let sockets: Vec<UdpSocket> = repeat_with(|| UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)))
            .take(NUM_ENDPOINTS)
            .collect::<Result<_, _>>()
            .unwrap();
        let addresses: Vec<SocketAddr> = sockets
            .iter()
            .map(UdpSocket::local_addr)
            .collect::<Result<_, _>>()
            .unwrap();
        let (senders, receivers): (Vec<_>, Vec<_>) =
            repeat_with(crossbeam_channel::unbounded::<(Pubkey, SocketAddr, Bytes)>)
                .take(NUM_ENDPOINTS)
                .unzip();
        let bank_forks = {
            let GenesisConfigInfo { genesis_config, .. } =
                create_genesis_config(/*mint_lamports:*/ 100_000);
            let bank = Bank::new_for_tests(&genesis_config);
            BankForks::new_rw_arc(bank)
        };
        let (endpoints, senders, tasks): (Vec<_>, Vec<_>, Vec<_>) =
            multiunzip(keypairs.iter().zip(sockets).zip(senders).map(
                |((keypair, socket), sender)| {
                    new_quic_endpoint(
                        runtime.handle(),
                        "test_quic_client",
                        "test_quic_server",
                        keypair,
                        socket,
                        sender,
                        bank_forks.clone(),
                    )
                    .unwrap()
                },
            ));
        // Send a unique message from each endpoint to every other endpoint.
        for (i, (keypair, &address, sender)) in izip!(&keypairs, &addresses, &senders).enumerate() {
            for (j, &address) in addresses.iter().enumerate() {
                if i != j {
                    let bytes = Bytes::from(format!("{i}=>{j}"));
                    sender.blocking_send((address, bytes)).unwrap();
                }
            }
            // Verify all messages are received.
            for (j, receiver) in receivers.iter().enumerate() {
                if i != j {
                    let bytes = Bytes::from(format!("{i}=>{j}"));
                    let entry = (keypair.pubkey(), address, bytes);
                    assert_eq!(receiver.recv_timeout(RECV_TIMEOUT).unwrap(), entry);
                }
            }
        }
        drop(senders);
        for endpoint in endpoints {
            close_quic_endpoint(&endpoint);
        }
        for task in tasks {
            runtime.block_on(task).unwrap();
        }
    }
}
