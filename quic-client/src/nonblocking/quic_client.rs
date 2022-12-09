//! Simple nonblocking client that connects to a given UDP port with the QUIC protocol
//! and provides an interface for sending transactions which is restricted by the
//! server's flow control.
use {
    async_mutex::Mutex,
    async_trait::async_trait,
    futures::future::join_all,
    itertools::Itertools,
    log::*,
    quinn::{
        ClientConfig, ConnectError, ConnectionError, Endpoint, EndpointConfig, IdleTimeout,
        NewConnection, VarInt, WriteError,
    },
    solana_measure::measure::Measure,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_rpc_client_api::client_error::ErrorKind as ClientErrorKind,
    solana_sdk::{
        quic::{
            QUIC_CONNECTION_HANDSHAKE_TIMEOUT_MS, QUIC_KEEP_ALIVE_MS, QUIC_MAX_TIMEOUT_MS,
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS,
        },
        signature::Keypair,
        transport::Result as TransportResult,
    },
    solana_streamer::{
        nonblocking::quic::ALPN_TPU_PROTOCOL_ID,
        tls_certificates::new_self_signed_tls_certificate_chain,
    },
    solana_tpu_client::{
        connection_cache_stats::ConnectionCacheStats, nonblocking::tpu_connection::TpuConnection,
        tpu_connection::ClientStats,
    },
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::{atomic::Ordering, Arc},
        thread,
        time::Duration,
    },
    thiserror::Error,
    tokio::{sync::RwLock, time::timeout},
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

pub struct QuicClientCertificate {
    pub certificates: Vec<rustls::Certificate>,
    pub key: rustls::PrivateKey,
}

/// A lazy-initialized Quic Endpoint
pub struct QuicLazyInitializedEndpoint {
    endpoint: RwLock<Option<Arc<Endpoint>>>,
    client_certificate: Arc<QuicClientCertificate>,
    client_endpoint: Option<Endpoint>,
}

#[derive(Error, Debug)]
pub enum QuicError {
    #[error(transparent)]
    WriteError(#[from] WriteError),
    #[error(transparent)]
    ConnectionError(#[from] ConnectionError),
    #[error(transparent)]
    ConnectError(#[from] ConnectError),
}

impl From<QuicError> for ClientErrorKind {
    fn from(quic_error: QuicError) -> Self {
        Self::Custom(format!("{quic_error:?}"))
    }
}

impl QuicLazyInitializedEndpoint {
    pub fn new(
        client_certificate: Arc<QuicClientCertificate>,
        client_endpoint: Option<Endpoint>,
    ) -> Self {
        Self {
            endpoint: RwLock::new(None),
            client_certificate,
            client_endpoint,
        }
    }

    fn create_endpoint(&self) -> Endpoint {
        let mut endpoint = if let Some(endpoint) = &self.client_endpoint {
            endpoint.clone()
        } else {
            let client_socket = solana_net_utils::bind_in_range(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                VALIDATOR_PORT_RANGE,
            )
            .expect("QuicLazyInitializedEndpoint::create_endpoint bind_in_range")
            .1;

            QuicNewConnection::create_endpoint(EndpointConfig::default(), client_socket)
        };

        let mut crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_single_cert(
                self.client_certificate.certificates.clone(),
                self.client_certificate.key.clone(),
            )
            .expect("Failed to set QUIC client certificates");
        crypto.enable_early_data = true;
        crypto.alpn_protocols = vec![ALPN_TPU_PROTOCOL_ID.to_vec()];

        let mut config = ClientConfig::new(Arc::new(crypto));
        let transport_config = Arc::get_mut(&mut config.transport)
            .expect("QuicLazyInitializedEndpoint::create_endpoint Arc::get_mut");
        let timeout = IdleTimeout::from(VarInt::from_u32(QUIC_MAX_TIMEOUT_MS));
        transport_config.max_idle_timeout(Some(timeout));
        transport_config.keep_alive_interval(Some(Duration::from_millis(QUIC_KEEP_ALIVE_MS)));

        endpoint.set_default_client_config(config);

        endpoint
    }

    async fn get_endpoint(&self) -> Arc<Endpoint> {
        let lock = self.endpoint.read().await;
        let endpoint = lock.as_ref();

        match endpoint {
            Some(endpoint) => endpoint.clone(),
            None => {
                drop(lock);
                let mut lock = self.endpoint.write().await;
                let endpoint = lock.as_ref();

                match endpoint {
                    Some(endpoint) => endpoint.clone(),
                    None => {
                        let connection = Arc::new(self.create_endpoint());
                        *lock = Some(connection.clone());
                        connection
                    }
                }
            }
        }
    }
}

impl Default for QuicLazyInitializedEndpoint {
    fn default() -> Self {
        let (certs, priv_key) = new_self_signed_tls_certificate_chain(
            &Keypair::new(),
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        )
        .expect("Failed to create QUIC client certificate");
        Self::new(
            Arc::new(QuicClientCertificate {
                certificates: certs,
                key: priv_key,
            }),
            None,
        )
    }
}

/// A wrapper over NewConnection with additional capability to create the endpoint as part
/// of creating a new connection.
#[derive(Clone)]
struct QuicNewConnection {
    endpoint: Arc<Endpoint>,
    connection: Arc<NewConnection>,
}

impl QuicNewConnection {
    /// Create a QuicNewConnection given the remote address 'addr'.
    async fn make_connection(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        addr: SocketAddr,
        stats: &ClientStats,
    ) -> Result<Self, QuicError> {
        let mut make_connection_measure = Measure::start("make_connection_measure");
        let endpoint = endpoint.get_endpoint().await;

        let connecting = endpoint.connect(addr, "connect")?;
        stats.total_connections.fetch_add(1, Ordering::Relaxed);
        if let Ok(connecting_result) = timeout(
            Duration::from_millis(QUIC_CONNECTION_HANDSHAKE_TIMEOUT_MS),
            connecting,
        )
        .await
        {
            if connecting_result.is_err() {
                stats.connection_errors.fetch_add(1, Ordering::Relaxed);
            }
            make_connection_measure.stop();
            stats
                .make_connection_ms
                .fetch_add(make_connection_measure.as_ms(), Ordering::Relaxed);

            let connection = connecting_result?;

            Ok(Self {
                endpoint,
                connection: Arc::new(connection),
            })
        } else {
            Err(ConnectionError::TimedOut.into())
        }
    }

    fn create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
        quinn::Endpoint::new(config, None, client_socket)
            .expect("QuicNewConnection::create_endpoint quinn::Endpoint::new")
            .0
    }

    // Attempts to make a faster connection by taking advantage of pre-existing key material.
    // Only works if connection to this endpoint was previously established.
    async fn make_connection_0rtt(
        &mut self,
        addr: SocketAddr,
        stats: &ClientStats,
    ) -> Result<Arc<NewConnection>, QuicError> {
        let connecting = self.endpoint.connect(addr, "connect")?;
        stats.total_connections.fetch_add(1, Ordering::Relaxed);
        let connection = match connecting.into_0rtt() {
            Ok((connection, zero_rtt)) => {
                if let Ok(zero_rtt) = timeout(
                    Duration::from_millis(QUIC_CONNECTION_HANDSHAKE_TIMEOUT_MS),
                    zero_rtt,
                )
                .await
                {
                    if zero_rtt {
                        stats.zero_rtt_accepts.fetch_add(1, Ordering::Relaxed);
                    } else {
                        stats.zero_rtt_rejects.fetch_add(1, Ordering::Relaxed);
                    }
                    connection
                } else {
                    return Err(ConnectionError::TimedOut.into());
                }
            }
            Err(connecting) => {
                stats.connection_errors.fetch_add(1, Ordering::Relaxed);

                if let Ok(connecting_result) = timeout(
                    Duration::from_millis(QUIC_CONNECTION_HANDSHAKE_TIMEOUT_MS),
                    connecting,
                )
                .await
                {
                    connecting_result?
                } else {
                    return Err(ConnectionError::TimedOut.into());
                }
            }
        };
        self.connection = Arc::new(connection);
        Ok(self.connection.clone())
    }
}

pub struct QuicClient {
    endpoint: Arc<QuicLazyInitializedEndpoint>,
    connection: Arc<Mutex<Option<QuicNewConnection>>>,
    addr: SocketAddr,
    stats: Arc<ClientStats>,
    chunk_size: usize,
}

impl QuicClient {
    pub fn new(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        addr: SocketAddr,
        chunk_size: usize,
    ) -> Self {
        Self {
            endpoint,
            connection: Arc::new(Mutex::new(None)),
            addr,
            stats: Arc::new(ClientStats::default()),
            chunk_size,
        }
    }

    async fn _send_buffer_using_conn(
        data: &[u8],
        connection: &NewConnection,
    ) -> Result<(), QuicError> {
        let mut send_stream = connection.connection.open_uni().await?;

        send_stream.write_all(data).await?;
        send_stream.finish().await?;
        Ok(())
    }

    // Attempts to send data, connecting/reconnecting as necessary
    // On success, returns the connection used to successfully send the data
    async fn _send_buffer(
        &self,
        data: &[u8],
        stats: &ClientStats,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Result<Arc<NewConnection>, QuicError> {
        let mut connection_try_count = 0;
        let mut last_connection_id = 0;
        let mut last_error = None;

        while connection_try_count < 2 {
            let connection = {
                let mut conn_guard = self.connection.lock().await;

                let maybe_conn = conn_guard.as_mut();
                match maybe_conn {
                    Some(conn) => {
                        if conn.connection.connection.stable_id() == last_connection_id {
                            // this is the problematic connection we had used before, create a new one
                            let conn = conn.make_connection_0rtt(self.addr, stats).await;
                            match conn {
                                Ok(conn) => {
                                    info!(
                                        "Made 0rtt connection to {} with id {} try_count {}, last_connection_id: {}, last_error: {:?}",
                                        self.addr,
                                        conn.connection.stable_id(),
                                        connection_try_count,
                                        last_connection_id,
                                        last_error,
                                    );
                                    connection_try_count += 1;
                                    conn
                                }
                                Err(err) => {
                                    info!(
                                        "Cannot make 0rtt connection to {}, error {:}",
                                        self.addr, err
                                    );
                                    return Err(err);
                                }
                            }
                        } else {
                            stats.connection_reuse.fetch_add(1, Ordering::Relaxed);
                            conn.connection.clone()
                        }
                    }
                    None => {
                        let conn = QuicNewConnection::make_connection(
                            self.endpoint.clone(),
                            self.addr,
                            stats,
                        )
                        .await;
                        match conn {
                            Ok(conn) => {
                                *conn_guard = Some(conn.clone());
                                info!(
                                    "Made connection to {} id {} try_count {}",
                                    self.addr,
                                    conn.connection.connection.stable_id(),
                                    connection_try_count
                                );
                                connection_try_count += 1;
                                conn.connection.clone()
                            }
                            Err(err) => {
                                info!("Cannot make connection to {}, error {:}", self.addr, err);
                                return Err(err);
                            }
                        }
                    }
                }
            };

            let new_stats = connection.connection.stats();

            connection_stats
                .total_client_stats
                .congestion_events
                .update_stat(
                    &self.stats.congestion_events,
                    new_stats.path.congestion_events,
                );

            connection_stats
                .total_client_stats
                .tx_streams_blocked_uni
                .update_stat(
                    &self.stats.tx_streams_blocked_uni,
                    new_stats.frame_tx.streams_blocked_uni,
                );

            connection_stats
                .total_client_stats
                .tx_data_blocked
                .update_stat(&self.stats.tx_data_blocked, new_stats.frame_tx.data_blocked);

            connection_stats
                .total_client_stats
                .tx_acks
                .update_stat(&self.stats.tx_acks, new_stats.frame_tx.acks);

            last_connection_id = connection.connection.stable_id();
            match Self::_send_buffer_using_conn(data, &connection).await {
                Ok(()) => {
                    return Ok(connection);
                }
                Err(err) => match err {
                    QuicError::ConnectionError(_) => {
                        last_error = Some(err);
                    }
                    _ => {
                        info!(
                            "Error sending to {} with id {}, error {:?} thread: {:?}",
                            self.addr,
                            connection.connection.stable_id(),
                            err,
                            thread::current().id(),
                        );
                        return Err(err);
                    }
                },
            }
        }

        // if we come here, that means we have exhausted maximum retries, return the error
        info!(
            "Ran into an error sending transactions {:?}, exhausted retries to {}",
            last_error, self.addr
        );
        // If we get here but last_error is None, then we have a logic error
        // in this function, so panic here with an expect to help debugging
        Err(last_error.expect("QuicClient::_send_buffer last_error.expect"))
    }

    pub async fn send_buffer<T>(
        &self,
        data: T,
        stats: &ClientStats,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Result<(), ClientErrorKind>
    where
        T: AsRef<[u8]>,
    {
        self._send_buffer(data.as_ref(), stats, connection_stats)
            .await
            .map_err(Into::<ClientErrorKind>::into)?;
        Ok(())
    }

    pub async fn send_batch<T>(
        &self,
        buffers: &[T],
        stats: &ClientStats,
        connection_stats: Arc<ConnectionCacheStats>,
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
        let connection = self
            ._send_buffer(buffers[0].as_ref(), stats, connection_stats)
            .await
            .map_err(Into::<ClientErrorKind>::into)?;

        // Used to avoid dereferencing the Arc multiple times below
        // by just getting a reference to the NewConnection once
        let connection_ref: &NewConnection = &connection;

        let chunks = buffers[1..buffers.len()].iter().chunks(self.chunk_size);

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
            f.await
                .into_iter()
                .try_for_each(|res| res)
                .map_err(Into::<ClientErrorKind>::into)?;
        }
        Ok(())
    }

    pub fn tpu_addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn stats(&self) -> Arc<ClientStats> {
        self.stats.clone()
    }
}

pub struct QuicTpuConnection {
    pub client: Arc<QuicClient>,
    pub connection_stats: Arc<ConnectionCacheStats>,
}

impl QuicTpuConnection {
    pub fn base_stats(&self) -> Arc<ClientStats> {
        self.client.stats()
    }

    pub fn connection_stats(&self) -> Arc<ConnectionCacheStats> {
        self.connection_stats.clone()
    }

    pub fn new(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        addr: SocketAddr,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let client = Arc::new(QuicClient::new(
            endpoint,
            addr,
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS,
        ));
        Self::new_with_client(client, connection_stats)
    }

    pub fn new_with_client(
        client: Arc<QuicClient>,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        Self {
            client,
            connection_stats,
        }
    }
}

#[async_trait]
impl TpuConnection for QuicTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        self.client.tpu_addr()
    }

    async fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let stats = ClientStats::default();
        let len = buffers.len();
        let res = self
            .client
            .send_batch(buffers, &stats, self.connection_stats.clone())
            .await;
        self.connection_stats
            .add_client_stats(&stats, len, res.is_ok());
        res?;
        Ok(())
    }

    async fn send_wire_transaction<T>(&self, wire_transaction: T) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let stats = Arc::new(ClientStats::default());
        let send_buffer =
            self.client
                .send_buffer(wire_transaction, &stats, self.connection_stats.clone());
        if let Err(e) = send_buffer.await {
            warn!(
                "Failed to send transaction async to {}, error: {:?} ",
                self.tpu_addr(),
                e
            );
            datapoint_warn!("send-wire-async", ("failure", 1, i64),);
            self.connection_stats.add_client_stats(&stats, 1, false);
        } else {
            self.connection_stats.add_client_stats(&stats, 1, true);
        }
        Ok(())
    }
}
