//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the servers flow control.

use {
    async_mutex::Mutex,
    futures::future::join_all,
    quinn::{ClientConfig, Endpoint, EndpointConfig, NewConnection, WriteError},
    rayon::iter::{
        IndexedParallelIterator, IntoParallelIterator, IntoParallelRefIterator, ParallelIterator,
    },
    solana_sdk::{transaction::Transaction, transport::Result as TransportResult},
    std::{
        net::{SocketAddr, UdpSocket},
        sync::Arc,
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

pub struct QuicClient {
    runtime: Runtime,
    endpoint: Endpoint,
    connection: Arc<Mutex<Option<Arc<NewConnection>>>>,
    addr: SocketAddr,
}

pub enum TpuConnection {
    Udp(UdpSocket, SocketAddr),
    Quic(Arc<QuicClient>),
}

impl TpuConnection {
    pub fn new_quic(client_socket: UdpSocket, tpu_addr: SocketAddr) -> TpuConnection {
        let client = Arc::new(QuicClient::new(client_socket, tpu_addr));

        Self::Quic(client)
    }

    pub fn new_udp(client_socket: UdpSocket, tpu_addr: SocketAddr) -> TpuConnection {
        Self::Udp(client_socket, tpu_addr)
    }

    pub fn tpu_addr(&self) -> &SocketAddr {
        match self {
            Self::Udp(_socket, addr) => addr,
            Self::Quic(client) => &client.addr,
        }
    }

    pub fn send_transaction(&self, tx: &Transaction) -> TransportResult<()> {
        let data = bincode::serialize(tx).expect("serialize Transaction in send_transaction");
        self.send_wire_transaction(data)
    }

    pub fn send_wire_transaction(&self, data: Vec<u8>) -> TransportResult<()> {
        match self {
            Self::Udp(socket, addr) => {
                socket.send_to(&data[..], addr)?;
                Ok(())
            }
            Self::Quic(client) => {
                let _guard = client.runtime.enter();
                let send_buffer = client.send_buffer(&data[..]);
                client.runtime.block_on(send_buffer)?;
                Ok(())
            }
        }
    }

    pub fn send_batch(&self, transactions: Vec<Transaction>) -> TransportResult<()> {
        let buffers = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();

        match self {
            Self::Udp(socket, addr) => {
                buffers
                    .into_iter()
                    .try_for_each(|buff| -> TransportResult<()> {
                        socket.send_to(&buff[..], addr)?;
                        Ok(())
                    })?;
                Ok(())
            }
            Self::Quic(client) => {
                let slices = buffers.par_iter().map(|buf| &buf[..]).collect::<Vec<_>>();
                let _guard = client.runtime.enter();
                let send_batch = client.send_batch(slices);
                client.runtime.block_on(send_batch)?;
                Ok(())
            }
        }
    }
}

impl QuicClient {
    pub fn new(client_socket: UdpSocket, addr: SocketAddr) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        let _guard = runtime.enter();

        let crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();

        let create_endpoint = QuicClient::create_endpoint(EndpointConfig::default(), client_socket);

        let mut endpoint = runtime.block_on(create_endpoint);

        endpoint.set_default_client_config(ClientConfig::new(Arc::new(crypto)));

        Self {
            runtime,
            endpoint,
            connection: Arc::new(Mutex::new(None)),
            addr,
        }
    }

    async fn create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
        quinn::Endpoint::new(config, None, client_socket).unwrap().0
    }

    async fn _send_buffer_using_conn(
        data: &[u8],
        connection: &Arc<NewConnection>,
    ) -> Result<(), WriteError> {
        let mut send_stream = connection.connection.open_uni().await?;
        send_stream.write_all(data).await?;
        send_stream.finish().await?;
        Ok(())
    }

    // Attempts to send data, connecting/reconnecting as necessary
    // On success, returns the connection used to successfully send the data
    pub async fn send_buffer(&self, data: &[u8]) -> Result<Arc<NewConnection>, WriteError> {
        let connection = {
            let mut conn_guard = self.connection.lock().await;

            let maybe_conn = (*conn_guard).clone();
            match maybe_conn {
                Some(conn) => conn.clone(),
                None => {
                    let connecting = self.endpoint.connect(self.addr, "connect").unwrap();
                    let connection = Arc::new(connecting.await?);
                    *conn_guard = Some(connection.clone());
                    connection
                }
            }
        };
        match Self::_send_buffer_using_conn(data, &connection).await {
            Ok(()) => Ok(connection),
            _ => {
                let connection = {
                    let connecting = self.endpoint.connect(self.addr, "connect").unwrap();
                    let connection = Arc::new(connecting.await?);
                    let mut conn_guard = self.connection.lock().await;
                    *conn_guard = Some(connection.clone());
                    connection
                };
                Self::_send_buffer_using_conn(data, &connection).await?;
                Ok(connection)
            }
        }
    }

    pub async fn send_batch(&self, buffers: Vec<&[u8]>) -> Result<(), WriteError> {
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
        let connection = self.send_buffer(buffers[0]).await?;

        let futures = buffers[1..buffers.len()]
            .par_iter()
            .chunks(2048)
            .map(|buffs| {
                let send_futures = buffs
                    .into_par_iter()
                    .map(|buf| Self::_send_buffer_using_conn(buf, &connection))
                    .collect::<Vec<_>>();

                join_all(send_futures)
            })
            .collect::<Vec<_>>();

        for f in futures {
            f.await.into_iter().try_for_each(|res| res)?;
        }
        Ok(())
    }
}
