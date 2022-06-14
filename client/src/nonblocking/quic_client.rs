//! Simple nonblocking client that connects to a given UDP port with the QUIC protocol
//! and provides an interface for sending transactions which is restricted by the
//! server's flow control.
use {
    crate::{
        client_error::ClientErrorKind, connection_cache::ConnectionCacheStats,
        tpu_connection::ClientStats,
    },
    async_mutex::Mutex,
    futures::future::join_all,
    itertools::Itertools,
    quinn::{
        ClientConfig, Endpoint, EndpointConfig, IdleTimeout, NewConnection, VarInt, WriteError,
    },
    solana_measure::measure::Measure,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::quic::{QUIC_KEEP_ALIVE_MS, QUIC_MAX_CONCURRENT_STREAMS, QUIC_MAX_TIMEOUT_MS},
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::{atomic::Ordering, Arc},
        time::Duration,
    },
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

/// A wrapper over NewConnection with additional capability to create the endpoint as part
/// of creating a new connection.
#[derive(Clone)]
struct QuicNewConnection {
    endpoint: Endpoint,
    connection: Arc<NewConnection>,
}

impl QuicNewConnection {
    /// Create a QuicNewConnection given the remote address 'addr'.
    async fn make_connection(addr: SocketAddr, stats: &ClientStats) -> Result<Self, WriteError> {
        let mut make_connection_measure = Measure::start("make_connection_measure");
        let (_, client_socket) = solana_net_utils::bind_in_range(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            VALIDATOR_PORT_RANGE,
        )
        .unwrap();

        let mut crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();
        crypto.enable_early_data = true;

        let mut endpoint =
            QuicNewConnection::create_endpoint(EndpointConfig::default(), client_socket);

        let mut config = ClientConfig::new(Arc::new(crypto));
        let transport_config = Arc::get_mut(&mut config.transport).unwrap();
        let timeout = IdleTimeout::from(VarInt::from_u32(QUIC_MAX_TIMEOUT_MS));
        transport_config.max_idle_timeout(Some(timeout));
        transport_config.keep_alive_interval(Some(Duration::from_millis(QUIC_KEEP_ALIVE_MS)));

        endpoint.set_default_client_config(config);

        let connecting = endpoint.connect(addr, "connect").unwrap();
        stats.total_connections.fetch_add(1, Ordering::Relaxed);
        let connecting_result = connecting.await;
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
    }

    fn create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
        quinn::Endpoint::new(config, None, client_socket).unwrap().0
    }

    // Attempts to make a faster connection by taking advantage of pre-existing key material.
    // Only works if connection to this endpoint was previously established.
    async fn make_connection_0rtt(
        &mut self,
        addr: SocketAddr,
        stats: &ClientStats,
    ) -> Result<Arc<NewConnection>, WriteError> {
        let connecting = self.endpoint.connect(addr, "connect").unwrap();
        stats.total_connections.fetch_add(1, Ordering::Relaxed);
        let connection = match connecting.into_0rtt() {
            Ok((connection, zero_rtt)) => {
                if zero_rtt.await {
                    stats.zero_rtt_accepts.fetch_add(1, Ordering::Relaxed);
                } else {
                    stats.zero_rtt_rejects.fetch_add(1, Ordering::Relaxed);
                }
                connection
            }
            Err(connecting) => {
                stats.connection_errors.fetch_add(1, Ordering::Relaxed);
                let connecting = connecting.await;
                connecting?
            }
        };
        self.connection = Arc::new(connection);
        Ok(self.connection.clone())
    }
}

pub struct QuicClient {
    connection: Arc<Mutex<Option<QuicNewConnection>>>,
    addr: SocketAddr,
    stats: Arc<ClientStats>,
}

impl QuicClient {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            connection: Arc::new(Mutex::new(None)),
            addr,
            stats: Arc::new(ClientStats::default()),
        }
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

    // Attempts to send data, connecting/reconnecting as necessary
    // On success, returns the connection used to successfully send the data
    async fn _send_buffer(
        &self,
        data: &[u8],
        stats: &ClientStats,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Result<Arc<NewConnection>, WriteError> {
        let connection = {
            let mut conn_guard = self.connection.lock().await;

            let maybe_conn = conn_guard.clone();
            match maybe_conn {
                Some(conn) => {
                    stats.connection_reuse.fetch_add(1, Ordering::Relaxed);
                    conn.connection.clone()
                }
                None => {
                    let conn = QuicNewConnection::make_connection(self.addr, stats).await?;
                    *conn_guard = Some(conn.clone());
                    conn.connection.clone()
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

        match Self::_send_buffer_using_conn(data, &connection).await {
            Ok(()) => Ok(connection),
            _ => {
                let connection = {
                    let mut conn_guard = self.connection.lock().await;
                    let conn = conn_guard.as_mut().unwrap();
                    conn.make_connection_0rtt(self.addr, stats).await?
                };
                Self::_send_buffer_using_conn(data, &connection).await?;
                Ok(connection)
            }
        }
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
            .await?;
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
            .await?;

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

    pub fn tpu_addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn stats(&self) -> Arc<ClientStats> {
        self.stats.clone()
    }
}
