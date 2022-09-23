#![allow(clippy::integer_arithmetic)]

pub mod nonblocking;

use {
    solana_sdk::{pubkey::Pubkey, quic::QUIC_PORT_OFFSET, signature::Keypair},
    solana_streamer::{
        streamer::StakedNodes, tls_certificates::new_self_signed_tls_certificate_chain,
    },
    solana_tpu_client::{
        connection_cache::ConnectionCacheStats,
        nonblocking::quic_client::{
            QuicClient, QuicClientCertificate, QuicLazyInitializedEndpoint,
            QuicTpuConnection as NonblockingQuicTpuConnection,
        },
        quic_client::QuicTpuConnection as BlockingQuicTpuConnection,
        tpu_connection_cache::{BaseTpuConnection, ConnectionPool, ConnectionPoolError},
    },
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{Arc, RwLock},
    },
};

pub struct QuicPool {
    connections: Vec<Arc<Quic>>,
    endpoint: Arc<QuicLazyInitializedEndpoint>,
}
impl ConnectionPool for QuicPool {
    type PoolTpuConnection = Quic;
    type TpuConfig = QuicConfig;
    const PORT_OFFSET: u16 = QUIC_PORT_OFFSET;

    fn new_with_connection(config: &Self::TpuConfig, addr: &SocketAddr) -> Self {
        let mut pool = Self {
            connections: vec![],
            endpoint: config.create_endpoint(),
        };
        let connection = Arc::new(pool.create_pool_entry(config, addr));
        pool.connections.push(connection);
        pool
    }

    fn add_connection(&mut self, config: &Self::TpuConfig, addr: &SocketAddr) {
        let connection = Arc::new(self.create_pool_entry(config, addr));
        self.connections.push(connection);
    }

    fn num_connections(&self) -> usize {
        self.connections.len()
    }

    fn get(&self, index: usize) -> Result<Arc<Self::PoolTpuConnection>, ConnectionPoolError> {
        self.connections
            .get(index)
            .cloned()
            .ok_or(ConnectionPoolError::IndexOutOfRange)
    }

    fn create_pool_entry(
        &self,
        config: &Self::TpuConfig,
        addr: &SocketAddr,
    ) -> Self::PoolTpuConnection {
        Quic(Arc::new(QuicClient::new(
            self.endpoint.clone(),
            *addr,
            config.compute_max_parallel_streams(),
        )))
    }
}

pub struct QuicConfig {
    client_certificate: Arc<QuicClientCertificate>,
    maybe_staked_nodes: Option<Arc<RwLock<StakedNodes>>>,
    maybe_client_pubkey: Option<Pubkey>,
}

impl Default for QuicConfig {
    fn default() -> Self {
        let (certs, priv_key) = new_self_signed_tls_certificate_chain(
            &Keypair::new(),
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        )
        .expect("Failed to initialize QUIC client certificates");
        Self {
            client_certificate: Arc::new(QuicClientCertificate {
                certificates: certs,
                key: priv_key,
            }),
            maybe_staked_nodes: None,
            maybe_client_pubkey: None,
        }
    }
}

pub struct Quic(Arc<QuicClient>);
impl BaseTpuConnection for Quic {
    type BlockingConnectionType = BlockingQuicTpuConnection;
    type NonblockingConnectionType = NonblockingQuicTpuConnection;

    fn new_blocking_connection(
        &self,
        _addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> BlockingQuicTpuConnection {
        BlockingQuicTpuConnection::new_with_client(self.0.clone(), stats)
    }

    fn new_nonblocking_connection(
        &self,
        _addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> NonblockingQuicTpuConnection {
        NonblockingQuicTpuConnection::new_with_client(self.0.clone(), stats)
    }
}
