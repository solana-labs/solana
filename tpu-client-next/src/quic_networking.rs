//! Utility code to handle quic networking.

use {
    quinn::{
        crypto::rustls::QuicClientConfig, ClientConfig, Connection, Endpoint, IdleTimeout,
        TransportConfig,
    },
    skip_server_verification::SkipServerVerification,
    solana_sdk::quic::{QUIC_KEEP_ALIVE, QUIC_MAX_TIMEOUT},
    solana_streamer::nonblocking::quic::ALPN_TPU_PROTOCOL_ID,
    std::{net::SocketAddr, sync::Arc},
};

pub mod error;
pub mod quic_client_certificate;
pub mod skip_server_verification;

pub use {
    error::{IoErrorWithPartialEq, QuicError},
    quic_client_certificate::QuicClientCertificate,
};

pub(crate) fn create_client_config(client_certificate: Arc<QuicClientCertificate>) -> ClientConfig {
    // adapted from QuicLazyInitializedEndpoint::create_endpoint
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_client_auth_cert(
            vec![client_certificate.certificate.clone()],
            client_certificate.key.clone_key(),
        )
        .expect("Failed to set QUIC client certificates");
    crypto.enable_early_data = true;
    crypto.alpn_protocols = vec![ALPN_TPU_PROTOCOL_ID.to_vec()];

    let transport_config = {
        let mut res = TransportConfig::default();

        let timeout = IdleTimeout::try_from(QUIC_MAX_TIMEOUT).unwrap();
        res.max_idle_timeout(Some(timeout));
        res.keep_alive_interval(Some(QUIC_KEEP_ALIVE));

        res
    };

    let mut config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto).unwrap()));
    config.transport_config(Arc::new(transport_config));

    config
}

pub(crate) fn create_client_endpoint(
    bind_addr: SocketAddr,
    client_config: ClientConfig,
) -> Result<Endpoint, QuicError> {
    let mut endpoint = Endpoint::client(bind_addr).map_err(IoErrorWithPartialEq::from)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

pub(crate) async fn send_data_over_stream(
    connection: &Connection,
    data: &[u8],
) -> Result<(), QuicError> {
    let mut send_stream = connection.open_uni().await?;
    send_stream.write_all(data).await.map_err(QuicError::from)?;

    // Stream will be finished when dropped. Finishing here explicitly is a noop.
    Ok(())
}
