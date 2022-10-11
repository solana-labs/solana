#![allow(clippy::integer_arithmetic)]

pub mod nonblocking;
pub mod udp_client;

use {
    crate::{
        nonblocking::udp_client::UdpTpuConnection as NonblockingUdpTpuConnection,
        udp_client::UdpTpuConnection as BlockingUdpTpuConnection,
    },
    solana_tpu_client::{
        connection_cache_stats::ConnectionCacheStats,
        tpu_connection_cache::{BaseTpuConnection, ConnectionPool, ConnectionPoolError},
    },
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::Arc,
    },
};

pub struct UdpPool {
    connections: Vec<Arc<Udp>>,
}
impl ConnectionPool for UdpPool {
    type PoolTpuConnection = Udp;
    type TpuConfig = UdpConfig;

    fn new_with_connection(config: &Self::TpuConfig, addr: &SocketAddr) -> Self {
        let mut pool = Self {
            connections: vec![],
        };
        pool.add_connection(config, addr);
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
        _addr: &SocketAddr,
    ) -> Self::PoolTpuConnection {
        Udp(config.tpu_udp_socket.clone())
    }
}

pub struct UdpConfig {
    tpu_udp_socket: Arc<UdpSocket>,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            tpu_udp_socket: Arc::new(
                solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)))
                    .expect("Unable to bind to UDP socket"),
            ),
        }
    }
}

pub struct Udp(Arc<UdpSocket>);
impl BaseTpuConnection for Udp {
    type BlockingConnectionType = BlockingUdpTpuConnection;
    type NonblockingConnectionType = NonblockingUdpTpuConnection;

    fn new_blocking_connection(
        &self,
        addr: SocketAddr,
        _stats: Arc<ConnectionCacheStats>,
    ) -> BlockingUdpTpuConnection {
        BlockingUdpTpuConnection::new_from_addr(self.0.clone(), addr)
    }

    fn new_nonblocking_connection(
        &self,
        addr: SocketAddr,
        _stats: Arc<ConnectionCacheStats>,
    ) -> NonblockingUdpTpuConnection {
        NonblockingUdpTpuConnection::new_from_addr(self.0.try_clone().unwrap(), addr)
    }
}
