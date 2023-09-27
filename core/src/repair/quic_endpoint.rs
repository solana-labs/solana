use {
    bincode::Options,
    crossbeam_channel::Sender,
    futures::future::TryJoin,
    itertools::Itertools,
    log::error,
    quinn::{
        ClientConfig, ConnectError, Connecting, Connection, ConnectionError, Endpoint,
        EndpointConfig, ReadToEndError, RecvStream, SendStream, ServerConfig, TokioRuntime,
        TransportConfig, VarInt, WriteError,
    },
    rcgen::RcgenError,
    rustls::{Certificate, PrivateKey},
    serde_bytes::ByteBuf,
    solana_quic_client::nonblocking::quic_client::SkipServerVerification,
    solana_sdk::{packet::PACKET_DATA_SIZE, pubkey::Pubkey, signature::Keypair},
    solana_streamer::{
        quic::SkipClientVerification, tls_certificates::new_self_signed_tls_certificate,
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        io::{Cursor, Error as IoError},
        net::{IpAddr, SocketAddr, UdpSocket},
        ops::Deref,
        sync::Arc,
        time::Duration,
    },
    thiserror::Error,
    tokio::{
        sync::{
            mpsc::{Receiver as AsyncReceiver, Sender as AsyncSender},
            oneshot::Sender as OneShotSender,
            RwLock,
        },
        task::JoinHandle,
    },
};

const ALPN_REPAIR_PROTOCOL_ID: &[u8] = b"solana-repair";
const CONNECT_SERVER_NAME: &str = "solana-repair";

const CLIENT_CHANNEL_CAPACITY: usize = 1 << 14;
const CONNECTION_CACHE_CAPACITY: usize = 4096;
const MAX_CONCURRENT_BIDI_STREAMS: VarInt = VarInt::from_u32(512);

const CONNECTION_CLOSE_ERROR_CODE_SHUTDOWN: VarInt = VarInt::from_u32(1);
const CONNECTION_CLOSE_ERROR_CODE_DROPPED: VarInt = VarInt::from_u32(2);
const CONNECTION_CLOSE_ERROR_CODE_INVALID_IDENTITY: VarInt = VarInt::from_u32(3);
const CONNECTION_CLOSE_ERROR_CODE_REPLACED: VarInt = VarInt::from_u32(4);

const CONNECTION_CLOSE_REASON_SHUTDOWN: &[u8] = b"SHUTDOWN";
const CONNECTION_CLOSE_REASON_DROPPED: &[u8] = b"DROPPED";
const CONNECTION_CLOSE_REASON_INVALID_IDENTITY: &[u8] = b"INVALID_IDENTITY";
const CONNECTION_CLOSE_REASON_REPLACED: &[u8] = b"REPLACED";

pub(crate) type AsyncTryJoinHandle = TryJoin<JoinHandle<()>, JoinHandle<()>>;
type ConnectionCache = HashMap<(SocketAddr, Option<Pubkey>), Arc<RwLock<Option<Connection>>>>;

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
    BincodeError(#[from] bincode::Error),
    #[error(transparent)]
    CertificateError(#[from] RcgenError),
    #[error(transparent)]
    ConnectError(#[from] ConnectError),
    #[error(transparent)]
    ConnectionError(#[from] ConnectionError),
    #[error("Channel Send Error")]
    ChannelSendError,
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
    WriteError(#[from] WriteError),
    #[error(transparent)]
    TlsError(#[from] rustls::Error),
}

#[allow(clippy::type_complexity)]
pub(crate) fn new_quic_endpoint(
    runtime: &tokio::runtime::Handle,
    keypair: &Keypair,
    socket: UdpSocket,
    address: IpAddr,
    remote_request_sender: Sender<RemoteRequest>,
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
    let cache = Arc::<RwLock<ConnectionCache>>::default();
    let (client_sender, client_receiver) = tokio::sync::mpsc::channel(CLIENT_CHANNEL_CAPACITY);
    let server_task = runtime.spawn(run_server(
        endpoint.clone(),
        remote_request_sender.clone(),
        cache.clone(),
    ));
    let client_task = runtime.spawn(run_client(
        endpoint.clone(),
        client_receiver,
        remote_request_sender,
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
    let mut config = TransportConfig::default();
    config
        .max_concurrent_bidi_streams(MAX_CONCURRENT_BIDI_STREAMS)
        .max_concurrent_uni_streams(VarInt::from(0u8))
        .datagram_receive_buffer_size(None);
    config
}

async fn run_server(
    endpoint: Endpoint,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) {
    while let Some(connecting) = endpoint.accept().await {
        tokio::task::spawn(handle_connecting_error(
            endpoint.clone(),
            connecting,
            remote_request_sender.clone(),
            cache.clone(),
        ));
    }
}

async fn run_client(
    endpoint: Endpoint,
    mut receiver: AsyncReceiver<LocalRequest>,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) {
    while let Some(request) = receiver.recv().await {
        tokio::task::spawn(send_request_task(
            endpoint.clone(),
            request,
            remote_request_sender.clone(),
            cache.clone(),
        ));
    }
    close_quic_endpoint(&endpoint);
}

async fn handle_connecting_error(
    endpoint: Endpoint,
    connecting: Connecting,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) {
    if let Err(err) = handle_connecting(endpoint, connecting, remote_request_sender, cache).await {
        error!("handle_connecting: {err:?}");
    }
}

async fn handle_connecting(
    endpoint: Endpoint,
    connecting: Connecting,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) -> Result<(), Error> {
    let connection = connecting.await?;
    let remote_address = connection.remote_address();
    let remote_pubkey = get_remote_pubkey(&connection)?;
    handle_connection_error(
        endpoint,
        remote_address,
        remote_pubkey,
        connection,
        remote_request_sender,
        cache,
    )
    .await;
    Ok(())
}

async fn handle_connection_error(
    endpoint: Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: Connection,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) {
    cache_connection(remote_address, remote_pubkey, connection.clone(), &cache).await;
    if let Err(err) = handle_connection(
        &endpoint,
        remote_address,
        remote_pubkey,
        &connection,
        &remote_request_sender,
    )
    .await
    {
        drop_connection(remote_address, remote_pubkey, &connection, &cache).await;
        error!("handle_connection: {remote_pubkey}, {remote_address}, {err:?}");
    }
}

async fn handle_connection(
    endpoint: &Endpoint,
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: &Connection,
    remote_request_sender: &Sender<RemoteRequest>,
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
        error!("handle_stream: {remote_address}, {remote_pubkey}, {err:?}");
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

async fn send_request_task(
    endpoint: Endpoint,
    request: LocalRequest,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) {
    if let Err(err) = send_request(&endpoint, request, remote_request_sender, cache).await {
        error!("send_request_task: {err:?}");
    }
}

async fn send_request(
    endpoint: &Endpoint,
    LocalRequest {
        remote_address,
        bytes,
        num_expected_responses,
        response_sender,
    }: LocalRequest,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) -> Result<(), Error> {
    // Assert that send won't block.
    debug_assert_eq!(response_sender.capacity(), None);
    const READ_TIMEOUT_DURATION: Duration = Duration::from_secs(10);
    let connection = get_connection(endpoint, remote_address, remote_request_sender, cache).await?;
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
                close_quic_endpoint(endpoint);
                Error::from(err)
            })
    })
}

async fn get_connection(
    endpoint: &Endpoint,
    remote_address: SocketAddr,
    remote_request_sender: Sender<RemoteRequest>,
    cache: Arc<RwLock<ConnectionCache>>,
) -> Result<Connection, Error> {
    let entry = get_cache_entry(remote_address, &cache).await;
    {
        let connection: Option<Connection> = entry.read().await.clone();
        if let Some(connection) = connection {
            if connection.close_reason().is_none() {
                return Ok(connection);
            }
        }
    }
    let connection = {
        // Need to write lock here so that only one task initiates
        // a new connection to the same remote_address.
        let mut entry = entry.write().await;
        if let Some(connection) = entry.deref() {
            if connection.close_reason().is_none() {
                return Ok(connection.clone());
            }
        }
        let connection = endpoint
            .connect(remote_address, CONNECT_SERVER_NAME)?
            .await?;
        entry.insert(connection).clone()
    };
    tokio::task::spawn(handle_connection_error(
        endpoint.clone(),
        connection.remote_address(),
        get_remote_pubkey(&connection)?,
        connection.clone(),
        remote_request_sender,
        cache,
    ));
    Ok(connection)
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

async fn get_cache_entry(
    remote_address: SocketAddr,
    cache: &RwLock<ConnectionCache>,
) -> Arc<RwLock<Option<Connection>>> {
    let key = (remote_address, /*remote_pubkey:*/ None);
    if let Some(entry) = cache.read().await.get(&key) {
        return entry.clone();
    }
    cache.write().await.entry(key).or_default().clone()
}

async fn cache_connection(
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: Connection,
    cache: &RwLock<ConnectionCache>,
) {
    // The 2nd cache entry with remote_pubkey == None allows to lookup an entry
    // only by SocketAddr when establishing outgoing connections.
    let entries: [Arc<RwLock<Option<Connection>>>; 2] = {
        let mut cache = cache.write().await;
        if cache.len() >= CONNECTION_CACHE_CAPACITY {
            connection.close(
                CONNECTION_CLOSE_ERROR_CODE_DROPPED,
                CONNECTION_CLOSE_REASON_DROPPED,
            );
            return;
        }
        [Some(remote_pubkey), None].map(|remote_pubkey| {
            let key = (remote_address, remote_pubkey);
            cache.entry(key).or_default().clone()
        })
    };
    let mut entry = entries[0].write().await;
    *entries[1].write().await = Some(connection.clone());
    if let Some(old) = entry.replace(connection) {
        drop(entry);
        old.close(
            CONNECTION_CLOSE_ERROR_CODE_REPLACED,
            CONNECTION_CLOSE_REASON_REPLACED,
        );
    }
}

async fn drop_connection(
    remote_address: SocketAddr,
    remote_pubkey: Pubkey,
    connection: &Connection,
    cache: &RwLock<ConnectionCache>,
) {
    if connection.close_reason().is_none() {
        connection.close(
            CONNECTION_CLOSE_ERROR_CODE_DROPPED,
            CONNECTION_CLOSE_REASON_DROPPED,
        );
    }
    let key = (remote_address, Some(remote_pubkey));
    if let Entry::Occupied(entry) = cache.write().await.entry(key) {
        if matches!(entry.get().read().await.deref(),
                    Some(entry) if entry.stable_id() == connection.stable_id())
        {
            entry.remove();
        }
    }
    // Cache entry for (remote_address, None) will be lazily evicted.
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        Error::ChannelSendError
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        itertools::{izip, multiunzip},
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
