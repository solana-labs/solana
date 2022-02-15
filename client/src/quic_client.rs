//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the servers flow control.

use {
    async_mutex::Mutex,
    quinn::{ClientConfig, ConnectionError, Endpoint, EndpointConfig, NewConnection, WriteError},
    rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator},
    solana_sdk::{transaction::Transaction, transport::Result as TransportResult},
    std::{
        net::{SocketAddr, UdpSocket},
        sync::Arc,
    },
    tokio::{runtime::Runtime, task::yield_now, task::JoinHandle},
};

pub struct SkipServerVerification;

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

#[derive(Debug, Clone)]
pub enum ConnectionState {
    // either we haven't attempted to connect yet or the last connection attempt
    // failed
    Disconnected,
    //todo: make this contain a future
    Connecting,
    // we have attempted to connect and the last connection attempt succeeded,
    // but since then the connection may have been lost
    Connected(Arc<NewConnection>),
}

struct QuicClient {
    runtime: Runtime,
    endpoint: Endpoint,
    // todo: we've switched away from normal mutexes to async mutexes - does this
    // have any implications for how we use them? For example, can you now await while holding
    // the lock? If so, we could get rid of some of the cumbersome polling
    // and the ConnectionState::Connecting state
    connection: Arc<Mutex<ConnectionState>>,
    tpu_addr: SocketAddr,
}

//todo: turn this into an enum with quic and udp options
pub struct QuicTpuConnection {
    client: Arc<QuicClient>,
}

impl QuicTpuConnection {
    pub fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> QuicTpuConnection {
        let client = Arc::new(QuicClient::new(client_socket, tpu_addr));

        QuicTpuConnection { client }
    }

    pub fn tpu_addr(&self) -> &SocketAddr {
        &self.client.tpu_addr
    }

    pub fn send_transaction_sync(&self, tx: &Transaction) -> Result<(), WriteError> {
        let _guard = self.client.runtime.enter();
        let send_transaction = self.client.send_transaction(tx);
        self.client.runtime.block_on(send_transaction)
    }

    pub fn send_wire_transaction_sync(&self, data: &[u8]) -> Result<(), WriteError> {
        let _guard = self.client.runtime.enter();
        let send_transaction = self.client.send_wire_transaction(data);
        self.client.runtime.block_on(send_transaction)
    }

    fn enqueue_send_transaction_async(&self, tx: Transaction) -> JoinHandle<TransportResult<()>> {
        //todo: is this a memleak? The client Arc should be dropped when the task finishes...
        let client = self.client.clone();
        self.client.runtime.spawn(async move {
            client.send_transaction(&tx).await?;
            Ok(())
        })
    }

    pub fn send_batch_sync(&self, transactions: Vec<Transaction>) -> TransportResult<()> {
        transactions
            .into_par_iter()
            .chunks(1024)
            .try_for_each(|transactions| {
                // collect this to ensure the tasks are immediately kicked off before we block on them
                let tasks = transactions
                    .into_par_iter()
                    .map(|tx| self.enqueue_send_transaction_async(tx))
                    .collect::<Vec<JoinHandle<TransportResult<()>>>>();
                tasks.into_iter().try_for_each(|task| {
                    self.client.runtime.block_on(task).unwrap()?;
                    Ok(())
                })
            })
    }
}

impl QuicClient {
    pub fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self {
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
            connection: Arc::new(Mutex::new(ConnectionState::Disconnected)),
            tpu_addr,
        }
    }

    async fn create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
        quinn::Endpoint::new(config, None, client_socket).unwrap().0
    }

    // If reconnect is false, only attempt to connect if the connection state is
    // Disconnected. This is intended to be used when we have not (recently) attempted to
    // connect to the server. If reconnect is true, only attempt to connect if the
    // connection state is Connected. This is intended to be used when we've encountered an
    // error with the current connection. In this case the connection state being set to
    // Disconnected is taken to mean we've recently tried connecting and failed.
    pub async fn connect(&self, tpu_addr: &SocketAddr, reconnect: bool) {
        let conn = {
            let mut conn_guard = self.connection.lock().await;
            let prev_conn = conn_guard.clone();
            if (!reconnect && matches!(*conn_guard, ConnectionState::Disconnected))
                || (reconnect && matches!(*conn_guard, ConnectionState::Connected(_)))
            {
                *conn_guard = ConnectionState::Connecting;
            }
            prev_conn
        };

        if (!reconnect && matches!(conn, ConnectionState::Disconnected))
            || (reconnect && matches!(conn, ConnectionState::Connected(_)))
        {
            let connecting = self.endpoint.connect(*tpu_addr, "connect").unwrap();

            match connecting.await {
                Ok(conn) => {
                    let mut conn_guard = self.connection.lock().await;

                    *conn_guard = ConnectionState::Connected(Arc::new(conn));
                }
                _ => {
                    let mut conn_guard = self.connection.lock().await;

                    *conn_guard = ConnectionState::Disconnected;
                }
            }
        }
    }

    async fn _send_transaction(&self, data: &[u8]) -> Result<(), WriteError> {
        let mut send_stream = {
            // todo: make ConnectionState::Connecting contain a future
            // and get rid of this ad-hoc polling
            let mutex = self.connection.clone();
            let connection = tokio::spawn(async move {
                loop {
                    {
                        let conn_guard = mutex.lock().await;
                        let conn = conn_guard.clone();
                        match conn {
                            ConnectionState::Connected(conn) => {
                                return Ok(conn);
                            }
                            ConnectionState::Disconnected => {
                                return Err(ConnectionError::LocallyClosed);
                            }
                            _ => {}
                        }
                    }
                    yield_now().await;
                }
            })
            .await
            .unwrap()?;
            connection.connection.open_uni().await?
        };
        send_stream.write_all(data).await?;
        send_stream.finish().await?;
        Ok(())
    }

    pub async fn send_transaction(&self, tx: &Transaction) -> Result<(), WriteError> {
        let serialized_tx =
            bincode::serialize(tx).expect("serialize Transaction in pub fn transfer_signed");
        self.send_wire_transaction(&serialized_tx).await
    }

    pub async fn send_wire_transaction(&self, data: &[u8]) -> Result<(), WriteError> {
        // if we have not attempted to connect yet, or the last connection attempt
        // failed, attempt to connect,
        // otherwise, this is a no-op. Notably, if the connection is e.g.
        // timed out, this will still do nothing, and the below call to _send_transaction
        // will fail.
        self.connect(&self.tpu_addr, false).await;
        // attempt to send the transaction. If the connection state is
        // Disconnected (i.e. if the above connection attempt failed)
        // this is a no-op and returns an error
        match self._send_transaction(data).await {
            Ok(()) => Ok(()),
            Err(_err) => {
                // if we encountered an error sending the transaction above,
                // attempt to reconnect and resend the transaction, if the connection
                // state is not Disconnected. If the connection state is Disconnected
                // we presumably recently attempted to connect to the server (above) and failed.
                // The intention is that this would attempt to reconnect and resend the transaction
                // if the above connection attempt succeeded, but for example, during sending the transaction
                // the connection was lost for some reason, or the last connection attempt succeeded (so the above connect() call did nothing)
                // but it's been a while since we used the connection, so it's timed out.
                self.connect(&self.tpu_addr, true).await;
                self._send_transaction(data).await
            }
        }
    }
}
