#![allow(clippy::integer_arithmetic)]

pub mod nonblocking;
pub mod quic_client;

#[macro_use]
extern crate solana_metrics;

use {
    crate::{
        nonblocking::quic_client::{
            QuicClient, QuicClientCertificate,
            QuicClientConnection as NonblockingQuicClientConnection, QuicLazyInitializedEndpoint,
        },
        quic_client::QuicClientConnection as BlockingQuicClientConnection,
    },
    quinn::Endpoint,
    solana_connection_cache::{
        connection_cache::{
            BaseClientConnection, ClientError, ConnectionManager, ConnectionPool,
            ConnectionPoolError, NewConnectionConfig,
        },
        connection_cache_stats::ConnectionCacheStats,
    },
    solana_sdk::{pubkey::Pubkey, quic::QUIC_PORT_OFFSET, signature::Keypair},
    solana_streamer::{
        nonblocking::quic::{compute_max_allowed_uni_streams, ConnectionPeerType},
        streamer::StakedNodes,
        tls_certificates::new_self_signed_tls_certificate,
    },
    std::{
        error::Error,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{Arc, RwLock},
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum QuicClientError {
    #[error("Certificate error: {0}")]
    CertificateError(String),
}

pub struct QuicPool {
    connections: Vec<Arc<Quic>>,
    endpoint: Arc<QuicLazyInitializedEndpoint>,
}
impl ConnectionPool for QuicPool {
    type BaseClientConnection = Quic;
    type NewConnectionConfig = QuicConfig;

    fn add_connection(&mut self, config: &Self::NewConnectionConfig, addr: &SocketAddr) {
        let connection = self.create_pool_entry(config, addr);
        self.connections.push(connection);
    }

    fn num_connections(&self) -> usize {
        self.connections.len()
    }

    fn get(&self, index: usize) -> Result<Arc<Self::BaseClientConnection>, ConnectionPoolError> {
        self.connections
            .get(index)
            .cloned()
            .ok_or(ConnectionPoolError::IndexOutOfRange)
    }

    fn create_pool_entry(
        &self,
        config: &Self::NewConnectionConfig,
        addr: &SocketAddr,
    ) -> Arc<Self::BaseClientConnection> {
        Arc::new(Quic(Arc::new(QuicClient::new(
            self.endpoint.clone(),
            *addr,
            config.compute_max_parallel_streams(),
        ))))
    }
}

pub struct QuicConfig {
    client_certificate: Arc<QuicClientCertificate>,
    maybe_staked_nodes: Option<Arc<RwLock<StakedNodes>>>,
    maybe_client_pubkey: Option<Pubkey>,

    // The optional specified endpoint for the quic based client connections
    // If not specified, the connection cache will create as needed.
    client_endpoint: Option<Endpoint>,
}

impl NewConnectionConfig for QuicConfig {
    fn new() -> Result<Self, ClientError> {
        let (cert, priv_key) =
            new_self_signed_tls_certificate(&Keypair::new(), IpAddr::V4(Ipv4Addr::UNSPECIFIED))
                .map_err(|err| ClientError::CertificateError(err.to_string()))?;
        Ok(Self {
            client_certificate: Arc::new(QuicClientCertificate {
                certificate: cert,
                key: priv_key,
            }),
            maybe_staked_nodes: None,
            maybe_client_pubkey: None,
            client_endpoint: None,
        })
    }
}

impl QuicConfig {
    fn create_endpoint(&self) -> QuicLazyInitializedEndpoint {
        QuicLazyInitializedEndpoint::new(
            self.client_certificate.clone(),
            self.client_endpoint.as_ref().cloned(),
        )
    }

    fn compute_max_parallel_streams(&self) -> usize {
        let (client_type, stake, total_stake) =
            self.maybe_client_pubkey
                .map_or((ConnectionPeerType::Unstaked, 0, 0), |pubkey| {
                    self.maybe_staked_nodes.as_ref().map_or(
                        (ConnectionPeerType::Unstaked, 0, 0),
                        |stakes| {
                            let rstakes = stakes.read().unwrap();
                            rstakes.pubkey_stake_map.get(&pubkey).map_or(
                                (ConnectionPeerType::Unstaked, 0, rstakes.total_stake),
                                |stake| (ConnectionPeerType::Staked, *stake, rstakes.total_stake),
                            )
                        },
                    )
                });
        compute_max_allowed_uni_streams(client_type, stake, total_stake)
    }

    pub fn update_client_certificate(
        &mut self,
        keypair: &Keypair,
        ipaddr: IpAddr,
    ) -> Result<(), Box<dyn Error>> {
        let (cert, priv_key) = new_self_signed_tls_certificate(keypair, ipaddr)?;
        self.client_certificate = Arc::new(QuicClientCertificate {
            certificate: cert,
            key: priv_key,
        });
        Ok(())
    }

    pub fn set_staked_nodes(
        &mut self,
        staked_nodes: &Arc<RwLock<StakedNodes>>,
        client_pubkey: &Pubkey,
    ) {
        self.maybe_staked_nodes = Some(staked_nodes.clone());
        self.maybe_client_pubkey = Some(*client_pubkey);
    }

    pub fn update_client_endpoint(&mut self, client_endpoint: Endpoint) {
        self.client_endpoint = Some(client_endpoint);
    }
}

pub struct Quic(Arc<QuicClient>);
impl BaseClientConnection for Quic {
    type BlockingClientConnection = BlockingQuicClientConnection;
    type NonblockingClientConnection = NonblockingQuicClientConnection;

    fn new_blocking_connection(
        &self,
        _addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Arc<Self::BlockingClientConnection> {
        Arc::new(BlockingQuicClientConnection::new_with_client(
            self.0.clone(),
            stats,
        ))
    }

    fn new_nonblocking_connection(
        &self,
        _addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Arc<Self::NonblockingClientConnection> {
        Arc::new(NonblockingQuicClientConnection::new_with_client(
            self.0.clone(),
            stats,
        ))
    }
}

#[derive(Default)]
pub struct QuicConnectionManager {
    connection_config: Option<QuicConfig>,
}

impl ConnectionManager for QuicConnectionManager {
    type ConnectionPool = QuicPool;
    type NewConnectionConfig = QuicConfig;

    fn new_connection_pool(&self) -> Self::ConnectionPool {
        QuicPool {
            connections: Vec::default(),
            endpoint: Arc::new(
                self.connection_config
                    .as_ref()
                    .map_or(QuicLazyInitializedEndpoint::default(), |config| {
                        config.create_endpoint()
                    }),
            ),
        }
    }

    fn new_connection_config(&self) -> QuicConfig {
        QuicConfig::new().unwrap()
    }

    fn get_port_offset(&self) -> u16 {
        QUIC_PORT_OFFSET
    }
}

impl QuicConnectionManager {
    pub fn new_with_connection_config(config: QuicConfig) -> Self {
        Self {
            connection_config: Some(config),
        }
    }
}
#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::quic::{
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS, QUIC_MIN_STAKED_CONCURRENT_STREAMS,
            QUIC_TOTAL_STAKED_CONCURRENT_STREAMS,
        },
    };

    #[test]
    fn test_connection_cache_max_parallel_chunks() {
        solana_logger::setup();

        let mut connection_config = QuicConfig::new().unwrap();
        assert_eq!(
            connection_config.compute_max_parallel_streams(),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );

        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let pubkey = Pubkey::new_unique();
        connection_config.set_staked_nodes(&staked_nodes, &pubkey);
        assert_eq!(
            connection_config.compute_max_parallel_streams(),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );

        staked_nodes.write().unwrap().total_stake = 10000;
        assert_eq!(
            connection_config.compute_max_parallel_streams(),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );

        staked_nodes
            .write()
            .unwrap()
            .pubkey_stake_map
            .insert(pubkey, 1);

        let delta =
            (QUIC_TOTAL_STAKED_CONCURRENT_STREAMS - QUIC_MIN_STAKED_CONCURRENT_STREAMS) as f64;

        assert_eq!(
            connection_config.compute_max_parallel_streams(),
            (QUIC_MIN_STAKED_CONCURRENT_STREAMS as f64 + (1f64 / 10000f64) * delta) as usize
        );

        staked_nodes
            .write()
            .unwrap()
            .pubkey_stake_map
            .remove(&pubkey);
        staked_nodes
            .write()
            .unwrap()
            .pubkey_stake_map
            .insert(pubkey, 1000);
        assert_ne!(
            connection_config.compute_max_parallel_streams(),
            QUIC_MIN_STAKED_CONCURRENT_STREAMS
        );
    }
}
