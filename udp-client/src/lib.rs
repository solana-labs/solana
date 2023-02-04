#![allow(clippy::integer_arithmetic)]

pub mod nonblocking;
pub mod udp_client;

use {
    crate::{
        nonblocking::udp_client::UdpClientConnection as NonblockingUdpConnection,
        udp_client::UdpClientConnection as BlockingUdpConnection,
    },
    solana_connection_cache::{
        client_connection::ClientConnection as BlockingClientConnection,
        connection_cache::{
            BaseClientConnection, ClientError, ConnectionManager, ConnectionPool,
            ConnectionPoolError, NewConnectionConfig, ProtocolType,
        },
        connection_cache_stats::ConnectionCacheStats,
        nonblocking::client_connection::ClientConnection as NonblockingClientConnection,
    },
    std::{
        any::Any,
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::Arc,
    },
};

pub struct UdpPool {
    connections: Vec<Arc<dyn BaseClientConnection>>,
}
impl ConnectionPool for UdpPool {
    fn add_connection(&mut self, config: &dyn NewConnectionConfig, addr: &SocketAddr) {
        let connection = self.create_pool_entry(config, addr);
        self.connections.push(connection);
    }

    fn num_connections(&self) -> usize {
        self.connections.len()
    }

    fn get(&self, index: usize) -> Result<Arc<dyn BaseClientConnection>, ConnectionPoolError> {
        self.connections
            .get(index)
            .cloned()
            .ok_or(ConnectionPoolError::IndexOutOfRange)
    }

    fn create_pool_entry(
        &self,
        config: &dyn NewConnectionConfig,
        _addr: &SocketAddr,
    ) -> Arc<dyn BaseClientConnection> {
        let config: &UdpConfig = match config.as_any().downcast_ref::<UdpConfig>() {
            Some(b) => b,
            None => panic!("Expecting a UdpConfig!"),
        };
        Arc::new(Udp(config.udp_socket.clone()))
    }
}

pub struct UdpConfig {
    udp_socket: Arc<UdpSocket>,
}

impl NewConnectionConfig for UdpConfig {
    fn new() -> Result<Self, ClientError> {
        let socket = solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
            .map_err(Into::<ClientError>::into)?;
        Ok(Self {
            udp_socket: Arc::new(socket),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct Udp(Arc<UdpSocket>);
impl BaseClientConnection for Udp {
    fn new_blocking_connection(
        &self,
        addr: SocketAddr,
        _stats: Arc<ConnectionCacheStats>,
    ) -> Arc<dyn BlockingClientConnection> {
        Arc::new(BlockingUdpConnection::new_from_addr(self.0.clone(), addr))
    }

    fn new_nonblocking_connection(
        &self,
        addr: SocketAddr,
        _stats: Arc<ConnectionCacheStats>,
    ) -> Arc<dyn NonblockingClientConnection> {
        Arc::new(NonblockingUdpConnection::new_from_addr(
            self.0.try_clone().unwrap(),
            addr,
        ))
    }
}

#[derive(Default)]
pub struct UdpConnectionManager {}

impl ConnectionManager for UdpConnectionManager {
    fn new_connection_pool(&self) -> Box<dyn ConnectionPool> {
        Box::new(UdpPool {
            connections: Vec::default(),
        })
    }

    fn new_connection_config(&self) -> Box<dyn NewConnectionConfig> {
        Box::new(UdpConfig::new().unwrap())
    }

    fn get_port_offset(&self) -> u16 {
        0
    }

    fn get_protocol_type(&self) -> ProtocolType {
        ProtocolType::UDP
    }
}
