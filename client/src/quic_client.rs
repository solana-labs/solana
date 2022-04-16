//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the server's flow control.

use {
    crate::{
        client_error::ClientErrorKind,
        tpu_connection::{ClientStats, TpuConnection},
    },
    async_mutex::Mutex,
    futures::future::join_all,
    itertools::Itertools,
    lazy_static::lazy_static,
    log::*,
    quinn::{
        ClientConfig, Endpoint, EndpointConfig, IdleTimeout, NewConnection, VarInt, WriteError,
    },
    quinn_proto::ConnectionStats,
    solana_sdk::{
        quic::{
            QUIC_KEEP_ALIVE_MS, QUIC_MAX_CONCURRENT_STREAMS, QUIC_MAX_TIMEOUT_MS, QUIC_PORT_OFFSET,
        },
        transport::Result as TransportResult,
    },
    std::{
        net::{SocketAddr, UdpSocket},
        sync::{atomic::Ordering, Arc},
        time::Duration,
    },
    tokio::runtime::Runtime,
};

struct SkipServerVerification;

impl SkipServerVerification {
    pub fn new() -> Arc<Self> {
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
lazy_static! {
    static ref RUNTIME: Runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
}

struct QuicClient {
    endpoint: Endpoint,
    connection: Arc<Mutex<Option<Arc<NewConnection>>>>,
    addr: SocketAddr,
    stats: Arc<ClientStats>,
}

pub struct QuicTpuConnection {
    client: Arc<QuicClient>,
}

impl QuicTpuConnection {
    pub fn stats(&self) -> Option<ConnectionStats> {
        self.client.stats()
    }

    pub fn base_stats(&self) -> Arc<ClientStats> {
        self.client.stats.clone()
    }
}

impl TpuConnection for QuicTpuConnection {
    fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self {
        let tpu_addr = SocketAddr::new(tpu_addr.ip(), tpu_addr.port() + QUIC_PORT_OFFSET);
        let client = Arc::new(QuicClient::new(client_socket, tpu_addr));

        Self { client }
    }

    fn tpu_addr(&self) -> &SocketAddr {
        &self.client.addr
    }

    fn send_wire_transaction<T>(
        &self,
        wire_transaction: T,
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]>,
    {
        let _guard = RUNTIME.enter();
        let send_buffer = self.client.send_buffer(wire_transaction, stats);
        RUNTIME.block_on(send_buffer)?;
        Ok(())
    }

    fn send_wire_transaction_batch<T>(
        &self,
        buffers: &[T],
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]>,
    {
        let _guard = RUNTIME.enter();
        let send_batch = self.client.send_batch(buffers, stats);
        RUNTIME.block_on(send_batch)?;
        Ok(())
    }

    fn send_wire_transaction_async(
        &self,
        wire_transaction: Vec<u8>,
        stats: Arc<ClientStats>,
    ) -> TransportResult<()> {
        let _guard = RUNTIME.enter();
        let client = self.client.clone();
        //drop and detach the task
        let _ = RUNTIME.spawn(async move {
            let send_buffer = client.send_buffer(wire_transaction, &stats);
            if let Err(e) = send_buffer.await {
                warn!("Failed to send transaction async to {:?}", e);
                datapoint_warn!("send-wire-async", ("failure", 1, i64),);
            }
        });
        Ok(())
    }

    fn send_wire_transaction_batch_async(
        &self,
        buffers: Vec<Vec<u8>>,
        stats: Arc<ClientStats>,
    ) -> TransportResult<()> {
        let _guard = RUNTIME.enter();
        let client = self.client.clone();
        //drop and detach the task
        let _ = RUNTIME.spawn(async move {
            let send_batch = client.send_batch(&buffers, &stats);
            if let Err(e) = send_batch.await {
                warn!("Failed to send transaction batch async to {:?}", e);
                datapoint_warn!("send-wire-batch-async", ("failure", 1, i64),);
            }
        });
        Ok(())
    }
}

impl QuicClient {
    pub fn new(client_socket: UdpSocket, addr: SocketAddr) -> Self {
        let _guard = RUNTIME.enter();

        let crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();

        let create_endpoint = QuicClient::create_endpoint(EndpointConfig::default(), client_socket);

        let mut endpoint = RUNTIME.block_on(create_endpoint);

        let mut config = ClientConfig::new(Arc::new(crypto));
        let transport_config = Arc::get_mut(&mut config.transport).unwrap();
        let timeout = IdleTimeout::from(VarInt::from_u32(QUIC_MAX_TIMEOUT_MS));
        transport_config.max_idle_timeout(Some(timeout));
        transport_config.keep_alive_interval(Some(Duration::from_millis(QUIC_KEEP_ALIVE_MS)));

        endpoint.set_default_client_config(config);

        Self {
            endpoint,
            connection: Arc::new(Mutex::new(None)),
            addr,
            stats: Arc::new(ClientStats::default()),
        }
    }

    pub fn stats(&self) -> Option<ConnectionStats> {
        let conn_guard = self.connection.lock();
        let x = RUNTIME.block_on(conn_guard);
        x.as_ref().map(|c| c.connection.stats())
    }

    // If this function becomes public, it should be changed to
    // not expose details of the specific Quic implementation we're using
    async fn create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
        quinn::Endpoint::new(config, None, client_socket).unwrap().0
    }

    async fn _send_buffer_using_conn(
        data: &[u8],
        connection: &NewConnection,
    ) -> Result<(), WriteError> {
        let mut send_stream = connection.connection.open_uni().await?;
        send_stream.write_all(data).await?;
        send_stream.finish().await?;
        Ok(())
    }

    async fn make_connection(&self, stats: &ClientStats) -> Result<Arc<NewConnection>, WriteError> {
        let connecting = self.endpoint.connect(self.addr, "connect").unwrap();
        stats.total_connections.fetch_add(1, Ordering::Relaxed);
        let connecting_result = connecting.await;
        if connecting_result.is_err() {
            stats.connection_errors.fetch_add(1, Ordering::Relaxed);
        }
        let connection = connecting_result?;
        Ok(Arc::new(connection))
    }

    // Attempts to send data, connecting/reconnecting as necessary
    // On success, returns the connection used to successfully send the data
    async fn _send_buffer(
        &self,
        data: &[u8],
        stats: &ClientStats,
    ) -> Result<Arc<NewConnection>, WriteError> {
        let connection = {
            let mut conn_guard = self.connection.lock().await;

            let maybe_conn = (*conn_guard).clone();
            match maybe_conn {
                Some(conn) => {
                    stats.connection_reuse.fetch_add(1, Ordering::Relaxed);
                    conn.clone()
                }
                None => {
                    let connection = self.make_connection(stats).await?;
                    *conn_guard = Some(connection.clone());
                    connection
                }
            }
        };
        match Self::_send_buffer_using_conn(data, &connection).await {
            Ok(()) => Ok(connection),
            _ => {
                let connection = {
                    let connection = self.make_connection(stats).await?;
                    let mut conn_guard = self.connection.lock().await;
                    *conn_guard = Some(connection.clone());
                    connection
                };
                Self::_send_buffer_using_conn(data, &connection).await?;
                Ok(connection)
            }
        }
    }

    pub async fn send_buffer<T>(&self, data: T, stats: &ClientStats) -> Result<(), ClientErrorKind>
    where
        T: AsRef<[u8]>,
    {
        self._send_buffer(data.as_ref(), stats).await?;
        Ok(())
    }

    pub async fn send_batch<T>(
        &self,
        buffers: &[T],
        stats: &ClientStats,
    ) -> Result<(), ClientErrorKind>
    where
        T: AsRef<[u8]>,
    {
        // Start off by "testing" the connection by sending the first transaction
        // This will also connect to the server if not already connected
        // and reconnect and retry if the first send attempt failed
        // (for example due to a timed out connection), returning an error
        // or the connection that was used to successfully send the transaction.
        // We will use the returned connection to send the rest of the transactions in the batch
        // to avoid touching the mutex in self, and not bother reconnecting if we fail along the way
        // since testing even in the ideal GCE environment has found no cases
        // where reconnecting and retrying in the middle of a batch send
        // (i.e. we encounter a connection error in the middle of a batch send, which presumably cannot
        // be due to a timed out connection) has succeeded
        if buffers.is_empty() {
            return Ok(());
        }
        let connection = self._send_buffer(buffers[0].as_ref(), stats).await?;

        // Used to avoid dereferencing the Arc multiple times below
        // by just getting a reference to the NewConnection once
        let connection_ref: &NewConnection = &connection;

        let chunks = buffers[1..buffers.len()]
            .iter()
            .chunks(QUIC_MAX_CONCURRENT_STREAMS);

        let futures: Vec<_> = chunks
            .into_iter()
            .map(|buffs| {
                join_all(
                    buffs
                        .into_iter()
                        .map(|buf| Self::_send_buffer_using_conn(buf.as_ref(), connection_ref)),
                )
            })
            .collect();

        for f in futures {
            f.await.into_iter().try_for_each(|res| res)?;
        }
        Ok(())
    }
}
