#![allow(clippy::arithmetic_side_effects)]

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
            BaseClientConnection, ClientError, ConnectionCache, ConnectionManager, ConnectionPool,
            ConnectionPoolError, NewConnectionConfig, Protocol,
        },
        connection_cache_stats::ConnectionCacheStats,
    },
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::{
        nonblocking::quic::{compute_max_allowed_uni_streams, ConnectionPeerType},
        streamer::StakedNodes,
        tls_certificates::new_dummy_x509_certificate,
    },
    std::{
        net::{IpAddr, SocketAddr},
        sync::{Arc, RwLock},
    },
};

pub struct QuicPool {
    connections: Vec<Arc<Quic>>,
    endpoint: Arc<QuicLazyInitializedEndpoint>,
}
impl ConnectionPool for QuicPool {
    type BaseClientConnection = Quic;
    type NewConnectionConfig = QuicConfig;

    fn add_connection(&mut self, config: &Self::NewConnectionConfig, addr: &SocketAddr) -> usize {
        let connection = self.create_pool_entry(config, addr);
        let idx = self.connections.len();
        self.connections.push(connection);
        idx
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
    // Arc to prevent having to copy the struct
    client_certificate: RwLock<Arc<QuicClientCertificate>>,
    maybe_staked_nodes: Option<Arc<RwLock<StakedNodes>>>,
    maybe_client_pubkey: Option<Pubkey>,

    // The optional specified endpoint for the quic based client connections
    // If not specified, the connection cache will create as needed.
    client_endpoint: Option<Endpoint>,
}

impl Clone for QuicConfig {
    fn clone(&self) -> Self {
        let cert_guard = self.client_certificate.read().unwrap();
        QuicConfig {
            client_certificate: RwLock::new(cert_guard.clone()),
            maybe_staked_nodes: self.maybe_staked_nodes.clone(),
            maybe_client_pubkey: self.maybe_client_pubkey,
            client_endpoint: self.client_endpoint.clone(),
        }
    }
}

impl NewConnectionConfig for QuicConfig {
    fn new() -> Result<Self, ClientError> {
        let (cert, priv_key) = new_dummy_x509_certificate(&Keypair::new());
        Ok(Self {
            client_certificate: RwLock::new(Arc::new(QuicClientCertificate {
                certificate: cert,
                key: priv_key,
            })),
            maybe_staked_nodes: None,
            maybe_client_pubkey: None,
            client_endpoint: None,
        })
    }
}

impl QuicConfig {
    fn create_endpoint(&self) -> QuicLazyInitializedEndpoint {
        let cert_guard = self.client_certificate.read().unwrap();
        QuicLazyInitializedEndpoint::new(cert_guard.clone(), self.client_endpoint.as_ref().cloned())
    }

    fn compute_max_parallel_streams(&self) -> usize {
        let (client_type, total_stake) =
            self.maybe_client_pubkey
                .map_or((ConnectionPeerType::Unstaked, 0), |pubkey| {
                    self.maybe_staked_nodes.as_ref().map_or(
                        (ConnectionPeerType::Unstaked, 0),
                        |stakes| {
                            let rstakes = stakes.read().unwrap();
                            rstakes.get_node_stake(&pubkey).map_or(
                                (ConnectionPeerType::Unstaked, rstakes.total_stake()),
                                |stake| (ConnectionPeerType::Staked(stake), rstakes.total_stake()),
                            )
                        },
                    )
                });
        compute_max_allowed_uni_streams(client_type, total_stake)
    }

    pub fn update_client_certificate(&mut self, keypair: &Keypair, _ipaddr: IpAddr) {
        let (cert, priv_key) = new_dummy_x509_certificate(keypair);

        let mut cert_guard = self.client_certificate.write().unwrap();

        *cert_guard = Arc::new(QuicClientCertificate {
            certificate: cert,
            key: priv_key,
        });
    }

    pub fn update_keypair(&self, keypair: &Keypair) {
        let (cert, priv_key) = new_dummy_x509_certificate(keypair);

        let mut cert_guard = self.client_certificate.write().unwrap();

        *cert_guard = Arc::new(QuicClientCertificate {
            certificate: cert,
            key: priv_key,
        });
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

pub struct QuicConnectionManager {
    connection_config: QuicConfig,
}

impl ConnectionManager for QuicConnectionManager {
    type ConnectionPool = QuicPool;
    type NewConnectionConfig = QuicConfig;

    const PROTOCOL: Protocol = Protocol::QUIC;

    fn new_connection_pool(&self) -> Self::ConnectionPool {
        QuicPool {
            connections: Vec::default(),
            endpoint: Arc::new(self.connection_config.create_endpoint()),
        }
    }

    fn new_connection_config(&self) -> QuicConfig {
        self.connection_config.clone()
    }

    fn update_key(&self, key: &Keypair) -> Result<(), Box<dyn std::error::Error>> {
        self.connection_config.update_keypair(key);
        Ok(())
    }
}

impl QuicConnectionManager {
    pub fn new_with_connection_config(connection_config: QuicConfig) -> Self {
        Self { connection_config }
    }
}

pub type QuicConnectionCache = ConnectionCache<QuicPool, QuicConnectionManager, QuicConfig>;

pub fn new_quic_connection_cache(
    name: &'static str,
    keypair: &Keypair,
    ipaddr: IpAddr,
    staked_nodes: &Arc<RwLock<StakedNodes>>,
    connection_pool_size: usize,
) -> Result<QuicConnectionCache, ClientError> {
    let mut config = QuicConfig::new()?;
    config.update_client_certificate(keypair, ipaddr);
    config.set_staked_nodes(staked_nodes, &keypair.pubkey());
    let connection_manager = QuicConnectionManager::new_with_connection_config(config);
    ConnectionCache::new(name, connection_manager, connection_pool_size)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::quic::{
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS, QUIC_MIN_STAKED_CONCURRENT_STREAMS,
            QUIC_TOTAL_STAKED_CONCURRENT_STREAMS,
        },
        std::collections::HashMap,
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
        let overrides = HashMap::<Pubkey, u64>::default();
        let mut stakes = HashMap::from([(Pubkey::new_unique(), 10_000)]);
        *staked_nodes.write().unwrap() =
            StakedNodes::new(Arc::new(stakes.clone()), overrides.clone());
        assert_eq!(
            connection_config.compute_max_parallel_streams(),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );

        stakes.insert(pubkey, 1);
        *staked_nodes.write().unwrap() =
            StakedNodes::new(Arc::new(stakes.clone()), overrides.clone());
        let delta =
            (QUIC_TOTAL_STAKED_CONCURRENT_STREAMS - QUIC_MIN_STAKED_CONCURRENT_STREAMS) as f64;

        assert_eq!(
            connection_config.compute_max_parallel_streams(),
            (QUIC_MIN_STAKED_CONCURRENT_STREAMS as f64 + (1f64 / 10000f64) * delta) as usize
        );
        stakes.insert(pubkey, 1_000);
        *staked_nodes.write().unwrap() = StakedNodes::new(Arc::new(stakes.clone()), overrides);
        assert_ne!(
            connection_config.compute_max_parallel_streams(),
            QUIC_MIN_STAKED_CONCURRENT_STREAMS
        );
    }
}
