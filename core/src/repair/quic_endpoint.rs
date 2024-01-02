use {
    bincode::Options,
    crossbeam_channel::Sender,
    futures::future::TryJoin,
    itertools::Itertools,
    log::error,
    quinn::{
        ClientConfig, ConnectError, Connecting, Connection, ConnectionError, Endpoint,
        EndpointConfig, IdleTimeout, ReadError, ReadToEndError, RecvStream, SendStream,
        ServerConfig, TokioRuntime, TransportConfig, VarInt, WriteError,
    },
    rcgen::RcgenError,
    rustls::{Certificate, PrivateKey},
    serde_bytes::ByteBuf,
    solana_quic_client::nonblocking::quic_client::SkipServerVerification,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{packet::PACKET_DATA_SIZE, pubkey::Pubkey, signature::Keypair},
    solana_streamer::{
        quic::SkipClientVerification, tls_certificates::new_self_signed_tls_certificate,
    },
    std::{
        cmp::Reverse,
        collections::{hash_map::Entry, HashMap},
        io::{Cursor, Error as IoError},
        net::{IpAddr, SocketAddr, UdpSocket},
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
            oneshot::Sender as OneShotSender,
            Mutex, RwLock as AsyncRwLock,
        },
        task::JoinHandle,
    },
};

const ALPN_REPAIR_PROTOCOL_ID: &[u8] = b"solana-repair";
const CONNECT_SERVER_NAME: &str = "solana-repair";

const CLIENT_CHANNEL_BUFFER: usize = 1 << 14;
const ROUTER_CHANNEL_BUFFER: usize = 64;
const CONNECTION_CACHE_CAPACITY: usize = 3072;

// Transport config.
// Repair randomly samples peers, uses bi-directional streams and generally has
// low to moderate load and so is configured separately from other protocols.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(4);
const MAX_CONCURRENT_BIDI_STREAMS: VarInt = VarInt::from_u32(512);
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

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

pub(crate) type AsyncTryJoinHandle = TryJoin<JoinHandle<()>, JoinHandle<()>>;

// Outgoing local requests.
pub struct LocalRequest {
    pub(crate) remote_address: SocketAddr,
    pub(crate) bytes: Vec<u8>,
    pub(crate) num_expected_responses: usize,
    pub(crate) response_sender: Sender<(SocketAddr, Vec<u8>)>,
}

// Incomming requests from remote nodes.
// remote_pubkey and response_sender are None only when adapting UDP packets.
pub struct RemoteRequest {
    pub(crate) remote_pubkey: Option<Pubkey>,
    pub(crate) remote_address: SocketAddr,
    pub(crate) bytes: Vec<u8>,
    pub(crate) response_sender: Option<OneShotSender<Vec<Vec<u8>>>>,
}

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Error {
    #[error(transparent)]
    CertificateError(#[from] RcgenError),
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
    #[error("No Response Received")]
    NoResponseReceived,
    #[error(transparent)]
    ReadToEndError(#[from] ReadToEndError),
    #[error("read_to_end Timeout")]
    ReadToEndTimeout,
    #[error(transparent)]
    TlsError(#[from] rustls::Error),
    #[error(transparent)]
    WriteError(#[from] WriteError),
}

macro_rules! add_metric {
    ($metric: expr) => {{
        $metric.fetch_add(1, Ordering::Relaxed);
    }};
}

#[allow(clippy::type_complexity)]
pub(crate) fn new_quic_endpoint(
    runtime: &tokio::runtime::Handle,
    keypair: &Keypair,
    socket: UdpSocket,
    address: IpAddr,
    remote_request_sender: Sender<RemoteRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
) -> Result<(Endpoint, AsyncSender<LocalRequest>, AsyncTryJoinHandle), Error> {
    let (cert, key) = new_self_signed_tls_certificate(keypair, address)?;
    let server_config = new_server_config(cert.clone(), key.clone())?;
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
    let (client_sender, client_receiver) = tokio::sync::mpsc::channel(CLIENT_CHANNEL_BUFFER);
    let router = Arc::<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>::default();
    let server_task = runtime.spawn(run_server(
        endpoint.clone(),
        remote_request_sender.clone(),
        bank_forks.clone(),
        prune_cache_pending.clone(),
        router.clone(),
        cache.clone(),
    ));
    let client_task = runtime.spawn(run_client(
        endpoint.clone(),
        client_receiver,
        remote_request_sender,
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

fn new_server_config(cert: Certificate, key: PrivateKey) -> Result<ServerConfig, rustls::Error> {
    let mut config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(Arc::new(SkipClientVerification {}))
        .with_single_cert(vec![cert], key)?;
    config.alpn_protocols = vec![ALPN_REPAIR_PROTOCOL_ID.to_vec()];
    let mut config = ServerConfig::with_crypto(Arc::new(config));
    config
        .transport_config(Arc::new(new_transport_config()))
        .use_retry(true)
        .migration(false);
    Ok(config)
}

fn new_client_config(cert: Certificate, key: PrivateKey) -> Result<ClientConfig, rustls::Error> {
    let mut config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification {}))
        .with_client_auth_cert(vec![cert], key)?;
    config.enable_early_data = true;
    config.alpn_protocols = vec![ALPN_REPAIR_PROTOCOL_ID.to_vec()];
    let mut config = ClientConfig::new(Arc::new(config));
    config.transport_config(Arc::new(new_transport_config()));
    Ok(config)
}

fn new_transport_config() -> TransportConfig {
    let max_idle_timeout = IdleTimeout::try_from(MAX_IDLE_TIMEOUT).unwrap();
    let mut config = TransportConfig::default();
    // Disable datagrams and uni streams.
    config
        .datagram_receive_buffer_size(None)
        .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
        .max_concurrent_bidi_streams(MAX_CONCURRENT_BIDI_STREAMS)
        .max_concurrent_uni_streams(VarInt::from(0u8))
        .max_idle_timeout(Some(max_idle_timeout));
    config
}

async fn run_server(
    endpoint: Endpoint,
    remote_request_sender: Sender<RemoteRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) {
    let stats = Arc::<RepairQuicStats>::default();
    let report_metrics_task =
        tokio::task::spawn(report_metrics_task("repair_quic_server", stats.clone()));
    while let Some(connecting) = endpoint.accept().await {
        tokio::task::spawn(handle_connecting_task(
            endpoint.clone(),
            connecting,
            remote_request_sender.clone(),
            bank_forks.clone(),
            prune_cache_pending.clone(),
            router.clone(),
            cache.clone(),
            stats.clone(),
        ));
    }
    report_metrics_task.abort();
}

async fn run_client(
    endpoint: Endpoint,
    mut receiver: AsyncReceiver<LocalRequest>,
    remote_request_sender: Sender<RemoteRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) {
    let stats = Arc::<RepairQuicStats>::default();
    let report_metrics_task =
        tokio::task::spawn(report_metrics_task("repair_quic_client", stats.clone()));
    while let Some(request) = receiver.recv().await {
        let Some(request) = try_route_request(request, &*router.read().await, &stats) else {
            continue;
        };
        let remote_address = request.remote_address;
        let receiver = {
            let mut router = router.write().await;
            let Some(request) = try_route_request(request, &router, &stats) else {
                continue;
            };
            let (sender, receiver) = tokio::sync::mpsc::channel(ROUTER_CHANNEL_BUFFER);
            sender.try_send(request).unwrap();
            router.insert(remote_address, sender);
            receiver
        };
        tokio::task::spawn(make_connection_task(
            endpoint.clone(),
            remote_address,
            remote_request_sender.clone(),
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

// Routes the local request to respective channel. Drops the request if the
// channel is full. Bounces the request back if the channel is closed or does
// not exist.
fn try_route_request(
    request: LocalRequest,
    router: &HashMap<SocketAddr, AsyncSender<LocalRequest>>,
    stats: &RepairQuicStats,
) -> Option<LocalRequest> {
    match router.get(&request.remote_address) {
        None => Some(request),
        Some(sender) => match sender.try_send(request) {
            Ok(()) => None,
            Err(TrySendError::Full(request)) => {
                debug!("TrySendError::Full {}", request.remote_address);
                add_metric!(stats.router_try_send_error_full);
                None
            }
            Err(TrySendError::Closed(request)) => Some(request),
        },
    }
}

async fn handle_connecting_task(
    endpoint: Endpoint,
    connecting: Connecting,
    remote_request_sender: Sender<RemoteRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) {
    if let Err(err) = handle_connecting(
        endpoint,
        connecting,
        remote_request_sender,
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

async fn handle_connecting(
    endpoint: Endpoint,
    connecting: Connecting,
    remote_request_sender: Sender<RemoteRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error> {
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
        remote_request_sender,
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
async fn handle_connection(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: Connection,
    remote_request_sender: Sender<RemoteRequest>,
    receiver: AsyncReceiver<LocalRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) {
    cache_connection(
        remote_pubkey,
        connection.clone(),
        bank_forks,
        prune_cache_pending,
        router.clone(),
        cache.clone(),
    )
    .await;
    let send_requests_task = tokio::task::spawn(send_requests_task(
        endpoint.clone(),
        remote_address,
        connection.clone(),
        receiver,
        stats.clone(),
    ));
    let recv_requests_task = tokio::task::spawn(recv_requests_task(
        endpoint,
        remote_address,
        remote_pubkey,
        connection.clone(),
        remote_request_sender,
        stats.clone(),
    ));
    match futures::future::try_join(send_requests_task, recv_requests_task).await {
        Err(err) => error!("handle_connection: {remote_pubkey}, {remote_address}, {err:?}"),
        Ok(out) => {
            if let (Err(ref err), _) = out {
                debug!("send_requests_task: {remote_pubkey}, {remote_address}, {err:?}");
                record_error(err, &stats);
            }
            if let (_, Err(ref err)) = out {
                debug!("recv_requests_task: {remote_pubkey}, {remote_address}, {err:?}");
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

async fn recv_requests_task(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: Connection,
    remote_request_sender: Sender<RemoteRequest>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error> {
    loop {
        let (send_stream, recv_stream) = connection.accept_bi().await?;
        tokio::task::spawn(handle_streams_task(
            endpoint.clone(),
            remote_address,
            remote_pubkey,
            send_stream,
            recv_stream,
            remote_request_sender.clone(),
            stats.clone(),
        ));
    }
}

async fn handle_streams_task(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    send_stream: SendStream,
    recv_stream: RecvStream,
    remote_request_sender: Sender<RemoteRequest>,
    stats: Arc<RepairQuicStats>,
) {
    if let Err(err) = handle_streams(
        &endpoint,
        remote_address,
        remote_pubkey,
        send_stream,
        recv_stream,
        &remote_request_sender,
    )
    .await
    {
        debug!("handle_stream: {remote_address}, {remote_pubkey}, {err:?}");
        record_error(&err, &stats);
    }
}

async fn handle_streams(
    endpoint: &Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    mut send_stream: SendStream,
    mut recv_stream: RecvStream,
    remote_request_sender: &Sender<RemoteRequest>,
) -> Result<(), Error> {
    // Assert that send won't block.
    debug_assert_eq!(remote_request_sender.capacity(), None);
    const READ_TIMEOUT_DURATION: Duration = Duration::from_secs(2);
    let bytes = tokio::time::timeout(
        READ_TIMEOUT_DURATION,
        recv_stream.read_to_end(PACKET_DATA_SIZE),
    )
    .await
    .map_err(|_| Error::ReadToEndTimeout)??;
    let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
    let remote_request = RemoteRequest {
        remote_pubkey: Some(remote_pubkey),
        remote_address,
        bytes,
        response_sender: Some(response_sender),
    };
    if let Err(err) = remote_request_sender.send(remote_request) {
        close_quic_endpoint(endpoint);
        return Err(Error::from(err));
    }
    let Ok(response) = response_receiver.await else {
        return Err(Error::NoResponseReceived);
    };
    for chunk in response {
        let size = chunk.len() as u64;
        send_stream.write_all(&size.to_le_bytes()).await?;
        send_stream.write_all(&chunk).await?;
    }
    send_stream.finish().await.map_err(Error::from)
}

async fn send_requests_task(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    connection: Connection,
    mut receiver: AsyncReceiver<LocalRequest>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error> {
    tokio::pin! {
        let connection_closed = connection.closed();
    }
    loop {
        tokio::select! {
            biased;
            request = receiver.recv() => {
                match request {
                    None => return Ok(()),
                    Some(request) => tokio::task::spawn(send_request_task(
                        endpoint.clone(),
                        remote_address,
                        connection.clone(),
                        request,
                        stats.clone(),
                    )),
                };
            }
            err = &mut connection_closed => return Err(Error::from(err)),
        }
    }
}

async fn send_request_task(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    connection: Connection,
    request: LocalRequest,
    stats: Arc<RepairQuicStats>,
) {
    if let Err(err) = send_request(endpoint, connection, request).await {
        debug!("send_request: {remote_address}, {err:?}");
        record_error(&err, &stats);
    }
}

async fn send_request(
    endpoint: Endpoint,
    connection: Connection,
    LocalRequest {
        remote_address: _,
        bytes,
        num_expected_responses,
        response_sender,
    }: LocalRequest,
) -> Result<(), Error> {
    // Assert that send won't block.
    debug_assert_eq!(response_sender.capacity(), None);
    const READ_TIMEOUT_DURATION: Duration = Duration::from_secs(10);
    let (mut send_stream, mut recv_stream) = connection.open_bi().await?;
    send_stream.write_all(&bytes).await?;
    send_stream.finish().await?;
    // Each response is at most PACKET_DATA_SIZE bytes and requires
    // an additional 8 bytes to encode its length.
    let size = PACKET_DATA_SIZE
        .saturating_add(8)
        .saturating_mul(num_expected_responses);
    let response = tokio::time::timeout(READ_TIMEOUT_DURATION, recv_stream.read_to_end(size))
        .await
        .map_err(|_| Error::ReadToEndTimeout)??;
    let remote_address = connection.remote_address();
    let mut cursor = Cursor::new(&response[..]);
    std::iter::repeat_with(|| {
        bincode::options()
            .with_limit(response.len() as u64)
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .deserialize_from::<_, ByteBuf>(&mut cursor)
            .map(ByteBuf::into_vec)
            .ok()
    })
    .while_some()
    .try_for_each(|chunk| {
        response_sender
            .send((remote_address, chunk))
            .map_err(|err| {
                close_quic_endpoint(&endpoint);
                Error::from(err)
            })
    })
}

async fn make_connection_task(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_request_sender: Sender<RemoteRequest>,
    receiver: AsyncReceiver<LocalRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) {
    if let Err(err) = make_connection(
        endpoint,
        remote_address,
        remote_request_sender,
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

async fn make_connection(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_request_sender: Sender<RemoteRequest>,
    receiver: AsyncReceiver<LocalRequest>,
    bank_forks: Arc<RwLock<BankForks>>,
    prune_cache_pending: Arc<AtomicBool>,
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
    stats: Arc<RepairQuicStats>,
) -> Result<(), Error> {
    let connection = endpoint
        .connect(remote_address, CONNECT_SERVER_NAME)?
        .await?;
    handle_connection(
        endpoint,
        connection.remote_address(),
        get_remote_pubkey(&connection)?,
        connection,
        remote_request_sender,
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
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
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
    router: Arc<AsyncRwLock<HashMap<SocketAddr, AsyncSender<LocalRequest>>>>,
    cache: Arc<Mutex<HashMap<Pubkey, Connection>>>,
) {
    debug_assert!(prune_cache_pending.load(Ordering::Relaxed));
    let staked_nodes = {
        let root_bank = bank_forks.read().unwrap().root_bank();
        root_bank.staked_nodes()
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

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        Error::ChannelSendError
    }
}

#[derive(Default)]
struct RepairQuicStats {
    connect_error_invalid_remote_address: AtomicU64,
    connect_error_other: AtomicU64,
    connect_error_too_many_connections: AtomicU64,
    connection_error_application_closed: AtomicU64,
    connection_error_connection_closed: AtomicU64,
    connection_error_locally_closed: AtomicU64,
    connection_error_reset: AtomicU64,
    connection_error_timed_out: AtomicU64,
    connection_error_transport_error: AtomicU64,
    connection_error_version_mismatch: AtomicU64,
    invalid_identity: AtomicU64,
    no_response_received: AtomicU64,
    read_to_end_error_connection_lost: AtomicU64,
    read_to_end_error_illegal_ordered_read: AtomicU64,
    read_to_end_error_reset: AtomicU64,
    read_to_end_error_too_long: AtomicU64,
    read_to_end_error_unknown_stream: AtomicU64,
    read_to_end_error_zero_rtt_rejected: AtomicU64,
    read_to_end_timeout: AtomicU64,
    router_try_send_error_full: AtomicU64,
    write_error_connection_lost: AtomicU64,
    write_error_stopped: AtomicU64,
    write_error_unknown_stream: AtomicU64,
    write_error_zero_rtt_rejected: AtomicU64,
}

async fn report_metrics_task(name: &'static str, stats: Arc<RepairQuicStats>) {
    const METRICS_SUBMIT_CADENCE: Duration = Duration::from_secs(2);
    loop {
        tokio::time::sleep(METRICS_SUBMIT_CADENCE).await;
        report_metrics(name, &stats);
    }
}

fn record_error(err: &Error, stats: &RepairQuicStats) {
    match err {
        Error::CertificateError(_) => (),
        Error::ChannelSendError => (),
        Error::ConnectError(ConnectError::EndpointStopping) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectError(ConnectError::TooManyConnections) => {
            add_metric!(stats.connect_error_too_many_connections)
        }
        Error::ConnectError(ConnectError::InvalidDnsName(_)) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectError(ConnectError::InvalidRemoteAddress(_)) => {
            add_metric!(stats.connect_error_invalid_remote_address)
        }
        Error::ConnectError(ConnectError::NoDefaultClientConfig) => {
            add_metric!(stats.connect_error_other)
        }
        Error::ConnectError(ConnectError::UnsupportedVersion) => {
            add_metric!(stats.connect_error_other)
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
        Error::NoResponseReceived => add_metric!(stats.no_response_received),
        Error::ReadToEndError(ReadToEndError::Read(ReadError::Reset(_))) => {
            add_metric!(stats.read_to_end_error_reset)
        }
        Error::ReadToEndError(ReadToEndError::Read(ReadError::ConnectionLost(_))) => {
            add_metric!(stats.read_to_end_error_connection_lost)
        }
        Error::ReadToEndError(ReadToEndError::Read(ReadError::UnknownStream)) => {
            add_metric!(stats.read_to_end_error_unknown_stream)
        }
        Error::ReadToEndError(ReadToEndError::Read(ReadError::IllegalOrderedRead)) => {
            add_metric!(stats.read_to_end_error_illegal_ordered_read)
        }
        Error::ReadToEndError(ReadToEndError::Read(ReadError::ZeroRttRejected)) => {
            add_metric!(stats.read_to_end_error_zero_rtt_rejected)
        }
        Error::ReadToEndError(ReadToEndError::TooLong) => {
            add_metric!(stats.read_to_end_error_too_long)
        }
        Error::ReadToEndTimeout => add_metric!(stats.read_to_end_timeout),
        Error::TlsError(_) => (),
        Error::WriteError(WriteError::Stopped(_)) => add_metric!(stats.write_error_stopped),
        Error::WriteError(WriteError::ConnectionLost(_)) => {
            add_metric!(stats.write_error_connection_lost)
        }
        Error::WriteError(WriteError::UnknownStream) => {
            add_metric!(stats.write_error_unknown_stream)
        }
        Error::WriteError(WriteError::ZeroRttRejected) => {
            add_metric!(stats.write_error_zero_rtt_rejected)
        }
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
            "connect_error_too_many_connections",
            reset_metric!(stats.connect_error_too_many_connections),
            i64
        ),
        (
            "connection_error_application_closed",
            reset_metric!(stats.connection_error_application_closed),
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
            "no_response_received",
            reset_metric!(stats.no_response_received),
            i64
        ),
        (
            "read_to_end_error_connection_lost",
            reset_metric!(stats.read_to_end_error_connection_lost),
            i64
        ),
        (
            "read_to_end_error_illegal_ordered_read",
            reset_metric!(stats.read_to_end_error_illegal_ordered_read),
            i64
        ),
        (
            "read_to_end_error_reset",
            reset_metric!(stats.read_to_end_error_reset),
            i64
        ),
        (
            "read_to_end_error_too_long",
            reset_metric!(stats.read_to_end_error_too_long),
            i64
        ),
        (
            "read_to_end_error_unknown_stream",
            reset_metric!(stats.read_to_end_error_unknown_stream),
            i64
        ),
        (
            "read_to_end_error_zero_rtt_rejected",
            reset_metric!(stats.read_to_end_error_zero_rtt_rejected),
            i64
        ),
        (
            "read_to_end_timeout",
            reset_metric!(stats.read_to_end_timeout),
            i64
        ),
        (
            "router_try_send_error_full",
            reset_metric!(stats.router_try_send_error_full),
            i64
        ),
        (
            "write_error_connection_lost",
            reset_metric!(stats.write_error_connection_lost),
            i64
        ),
        (
            "write_error_stopped",
            reset_metric!(stats.write_error_stopped),
            i64
        ),
        (
            "write_error_unknown_stream",
            reset_metric!(stats.write_error_unknown_stream),
            i64
        ),
        (
            "write_error_zero_rtt_rejected",
            reset_metric!(stats.write_error_zero_rtt_rejected),
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
        const RECV_TIMEOUT: Duration = Duration::from_secs(30);
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
        let (remote_request_senders, remote_request_receivers): (Vec<_>, Vec<_>) =
            repeat_with(crossbeam_channel::unbounded::<RemoteRequest>)
                .take(NUM_ENDPOINTS)
                .unzip();
        let bank_forks = {
            let GenesisConfigInfo { genesis_config, .. } =
                create_genesis_config(/*mint_lamports:*/ 100_000);
            let bank = Bank::new_for_tests(&genesis_config);
            BankForks::new_rw_arc(bank)
        };
        let (endpoints, senders, tasks): (Vec<_>, Vec<_>, Vec<_>) = multiunzip(
            keypairs
                .iter()
                .zip(sockets)
                .zip(remote_request_senders)
                .map(|((keypair, socket), remote_request_sender)| {
                    new_quic_endpoint(
                        runtime.handle(),
                        keypair,
                        socket,
                        IpAddr::V4(Ipv4Addr::LOCALHOST),
                        remote_request_sender,
                        bank_forks.clone(),
                    )
                    .unwrap()
                }),
        );
        let (response_senders, response_receivers): (Vec<_>, Vec<_>) =
            repeat_with(crossbeam_channel::unbounded::<(SocketAddr, Vec<u8>)>)
                .take(NUM_ENDPOINTS)
                .unzip();
        // Send a unique request from each endpoint to every other endpoint.
        for (i, (keypair, &address, sender)) in izip!(&keypairs, &addresses, &senders).enumerate() {
            for (j, (&remote_address, response_sender)) in
                addresses.iter().zip(&response_senders).enumerate()
            {
                if i != j {
                    let mut bytes: Vec<u8> = format!("{i}=>{j}").into_bytes();
                    bytes.resize(PACKET_DATA_SIZE, 0xa5);
                    let request = LocalRequest {
                        remote_address,
                        bytes,
                        num_expected_responses: j + 1,
                        response_sender: response_sender.clone(),
                    };
                    sender.blocking_send(request).unwrap();
                }
            }
            // Verify all requests are received and respond to each.
            for (j, remote_request_receiver) in remote_request_receivers.iter().enumerate() {
                if i != j {
                    let RemoteRequest {
                        remote_pubkey,
                        remote_address,
                        bytes,
                        response_sender,
                    } = remote_request_receiver.recv_timeout(RECV_TIMEOUT).unwrap();
                    assert_eq!(remote_pubkey, Some(keypair.pubkey()));
                    assert_eq!(remote_address, address);
                    assert_eq!(bytes, {
                        let mut bytes = format!("{i}=>{j}").into_bytes();
                        bytes.resize(PACKET_DATA_SIZE, 0xa5);
                        bytes
                    });
                    let response: Vec<Vec<u8>> = (0..=j)
                        .map(|k| {
                            let mut bytes = format!("{j}=>{i}({k})").into_bytes();
                            bytes.resize(PACKET_DATA_SIZE, 0xd5);
                            bytes
                        })
                        .collect();
                    response_sender.unwrap().send(response).unwrap();
                }
            }
            // Verify responses.
            for (j, (&remote_address, response_receiver)) in
                addresses.iter().zip(&response_receivers).enumerate()
            {
                if i != j {
                    for k in 0..=j {
                        let (address, response) =
                            response_receiver.recv_timeout(RECV_TIMEOUT).unwrap();
                        assert_eq!(address, remote_address);
                        assert_eq!(response, {
                            let mut bytes = format!("{j}=>{i}({k})").into_bytes();
                            bytes.resize(PACKET_DATA_SIZE, 0xd5);
                            bytes
                        });
                    }
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
