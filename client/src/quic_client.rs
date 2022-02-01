//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the servers flow control.

use {
    quinn::{ClientConfig, Endpoint, EndpointConfig, NewConnection},
    solana_sdk::transaction::Transaction,
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
}

pub struct QuicTpuConnection {
    client: Arc<QuicClient>,
    connection: NewConnection,
    tpu_addr: SocketAddr,
}

impl QuicTpuConnection {
    pub fn connect_to_tpu(client: Arc<QuicClient>, tpu_addr: SocketAddr) -> QuicTpuConnection {
        let connecting = client
            .endpoint
            .connect(tpu_addr.clone(), "connect")
            .unwrap();
        let connection = client.runtime.block_on(connecting).unwrap();

        QuicTpuConnection {
            client,
            connection,
            tpu_addr,
        }
    }

    pub fn tpu_addr(&self) -> &SocketAddr {
        &self.tpu_addr
    }

    pub fn reconnect(&mut self) {
        let connecting = self
            .client
            .endpoint
            .connect(self.tpu_addr.clone(), "connect")
            .unwrap();
        self.connection = self.client.runtime.block_on(connecting).unwrap();
    }

    pub fn send_transaction_sync(&self, tx: &Transaction) {
        let send_transaction = self.send_transaction(tx);
        self.client.runtime.block_on(send_transaction);
    }

    pub async fn send_transaction(&self, tx: &Transaction) {
        let mut send_stream = self.connection.connection.open_uni().await.unwrap();
        let serialized_tx = bincode::serialize(tx).unwrap();
        let _write_res = send_stream.write_all(&serialized_tx).await;
        let _send_res = send_stream.finish().await;
    }
}

impl QuicClient {
    pub fn new(client_socket: UdpSocket) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();

        let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, client_socket)
            .unwrap()
            .0;

        endpoint.set_default_client_config(ClientConfig::new(Arc::new(crypto)));

        Self { runtime, endpoint }
    }
}
