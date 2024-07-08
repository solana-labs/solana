pub use solana_connection_cache::connection_cache::Protocol;
use {
    quinn::Endpoint,
    solana_connection_cache::{
        client_connection::ClientConnection,
        connection_cache::{
            BaseClientConnection, ConnectionCache as BackendConnectionCache, ConnectionPool,
            NewConnectionConfig,
        },
    },
    solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool},
    solana_sdk::{
        pubkey::Pubkey, quic::NotifyKeyUpdate, signature::Keypair,
        transport::Result as TransportResult,
    },
    solana_streamer::streamer::StakedNodes,
    solana_udp_client::{UdpConfig, UdpConnectionManager, UdpPool},
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{Arc, RwLock},
    },
};

const DEFAULT_CONNECTION_POOL_SIZE: usize = 4;
const DEFAULT_CONNECTION_CACHE_USE_QUIC: bool = true;

/// A thin wrapper over connection-cache/ConnectionCache to ease
/// construction of the ConnectionCache for code dealing both with udp and quic.
/// For the scenario only using udp or quic, use connection-cache/ConnectionCache directly.
pub enum ConnectionCache {
    Quic(Arc<BackendConnectionCache<QuicPool, QuicConnectionManager, QuicConfig>>),
    Udp(Arc<BackendConnectionCache<UdpPool, UdpConnectionManager, UdpConfig>>),
}

type QuicBaseClientConnection = <QuicPool as ConnectionPool>::BaseClientConnection;
type UdpBaseClientConnection = <UdpPool as ConnectionPool>::BaseClientConnection;

pub enum BlockingClientConnection {
    Quic(Arc<<QuicBaseClientConnection as BaseClientConnection>::BlockingClientConnection>),
    Udp(Arc<<UdpBaseClientConnection as BaseClientConnection>::BlockingClientConnection>),
}

pub enum NonblockingClientConnection {
    Quic(Arc<<QuicBaseClientConnection as BaseClientConnection>::NonblockingClientConnection>),
    Udp(Arc<<UdpBaseClientConnection as BaseClientConnection>::NonblockingClientConnection>),
}

impl NotifyKeyUpdate for ConnectionCache {
    fn update_key(&self, key: &Keypair) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Udp(_) => Ok(()),
            Self::Quic(backend) => backend.update_key(key),
        }
    }
}

impl ConnectionCache {
    pub fn new(name: &'static str) -> Self {
        if DEFAULT_CONNECTION_CACHE_USE_QUIC {
            let cert_info = (&Keypair::new(), IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
            ConnectionCache::new_with_client_options(
                name,
                DEFAULT_CONNECTION_POOL_SIZE,
                None, // client_endpoint
                Some(cert_info),
                None, // stake_info
            )
        } else {
            ConnectionCache::with_udp(name, DEFAULT_CONNECTION_POOL_SIZE)
        }
    }

    /// Create a quic connection_cache
    pub fn new_quic(name: &'static str, connection_pool_size: usize) -> Self {
        Self::new_with_client_options(name, connection_pool_size, None, None, None)
    }

    /// Create a quic connection_cache with more client options
    pub fn new_with_client_options(
        name: &'static str,
        connection_pool_size: usize,
        client_endpoint: Option<Endpoint>,
        cert_info: Option<(&Keypair, IpAddr)>,
        stake_info: Option<(&Arc<RwLock<StakedNodes>>, &Pubkey)>,
    ) -> Self {
        // The minimum pool size is 1.
        let connection_pool_size = 1.max(connection_pool_size);
        let mut config = QuicConfig::new().unwrap();
        if let Some(client_endpoint) = client_endpoint {
            config.update_client_endpoint(client_endpoint);
        }
        if let Some(cert_info) = cert_info {
            config.update_client_certificate(cert_info.0, cert_info.1);
        }
        if let Some(stake_info) = stake_info {
            config.set_staked_nodes(stake_info.0, stake_info.1);
        }
        let connection_manager = QuicConnectionManager::new_with_connection_config(config);
        let cache =
            BackendConnectionCache::new(name, connection_manager, connection_pool_size).unwrap();
        Self::Quic(Arc::new(cache))
    }

    #[inline]
    pub fn protocol(&self) -> Protocol {
        match self {
            Self::Quic(_) => Protocol::QUIC,
            Self::Udp(_) => Protocol::UDP,
        }
    }

    pub fn with_udp(name: &'static str, connection_pool_size: usize) -> Self {
        // The minimum pool size is 1.
        let connection_pool_size = 1.max(connection_pool_size);
        let connection_manager = UdpConnectionManager::default();
        let cache =
            BackendConnectionCache::new(name, connection_manager, connection_pool_size).unwrap();
        Self::Udp(Arc::new(cache))
    }

    pub fn use_quic(&self) -> bool {
        matches!(self, Self::Quic(_))
    }

    pub fn get_connection(&self, addr: &SocketAddr) -> BlockingClientConnection {
        match self {
            Self::Quic(cache) => BlockingClientConnection::Quic(cache.get_connection(addr)),
            Self::Udp(cache) => BlockingClientConnection::Udp(cache.get_connection(addr)),
        }
    }

    pub fn get_nonblocking_connection(&self, addr: &SocketAddr) -> NonblockingClientConnection {
        match self {
            Self::Quic(cache) => {
                NonblockingClientConnection::Quic(cache.get_nonblocking_connection(addr))
            }
            Self::Udp(cache) => {
                NonblockingClientConnection::Udp(cache.get_nonblocking_connection(addr))
            }
        }
    }
}

macro_rules! dispatch {
    ($(#[$meta:meta])* $vis:vis fn $name:ident$(<$($t:ident: $cons:ident + ?Sized),*>)?(&self $(, $arg:ident: $ty:ty)*) $(-> $out:ty)?) => {
        #[inline]
        $(#[$meta])*
        $vis fn $name$(<$($t: $cons + ?Sized),*>)?(&self $(, $arg:$ty)*) $(-> $out)? {
            match self {
                Self::Quic(this) => this.$name($($arg, )*),
                Self::Udp(this) => this.$name($($arg, )*),
            }
        }
    };
    ($(#[$meta:meta])* $vis:vis fn $name:ident$(<$($t:ident: $cons:ident + ?Sized),*>)?(&mut self $(, $arg:ident: $ty:ty)*) $(-> $out:ty)?) => {
        #[inline]
        $(#[$meta])*
        $vis fn $name$(<$($t: $cons + ?Sized),*>)?(&mut self $(, $arg:$ty)*) $(-> $out)? {
            match self {
                Self::Quic(this) => this.$name($($arg, )*),
                Self::Udp(this) => this.$name($($arg, )*),
            }
        }
    };
}

pub(crate) use dispatch;

impl ClientConnection for BlockingClientConnection {
    dispatch!(fn server_addr(&self) -> &SocketAddr);
    dispatch!(fn send_data(&self, buffer: &[u8]) -> TransportResult<()>);
    dispatch!(fn send_data_async(&self, buffer: Vec<u8>) -> TransportResult<()>);
    dispatch!(fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()>);
    dispatch!(fn send_data_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()>);
}

#[async_trait::async_trait]
impl solana_connection_cache::nonblocking::client_connection::ClientConnection
    for NonblockingClientConnection
{
    dispatch!(fn server_addr(&self) -> &SocketAddr);

    async fn send_data(&self, buffer: &[u8]) -> TransportResult<()> {
        match self {
            Self::Quic(cache) => Ok(cache.send_data(buffer).await?),
            Self::Udp(cache) => Ok(cache.send_data(buffer).await?),
        }
    }

    async fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()> {
        match self {
            Self::Quic(cache) => Ok(cache.send_data_batch(buffers).await?),
            Self::Udp(cache) => Ok(cache.send_data_batch(buffers).await?),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::connection_cache::ConnectionCache,
        crossbeam_channel::unbounded,
        solana_sdk::{net::DEFAULT_TPU_COALESCE, signature::Keypair},
        solana_streamer::{
            nonblocking::quic::{
                DEFAULT_MAX_CONNECTIONS_PER_IPADDR_PER_MINUTE, DEFAULT_MAX_STREAMS_PER_MS,
                DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            },
            quic::SpawnServerResult,
            streamer::StakedNodes,
        },
        std::{
            net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, RwLock,
            },
        },
    };

    fn server_args() -> (UdpSocket, Arc<AtomicBool>, Keypair) {
        (
            UdpSocket::bind("127.0.0.1:0").unwrap(),
            Arc::new(AtomicBool::new(false)),
            Keypair::new(),
        )
    }

    #[test]
    fn test_connection_with_specified_client_endpoint() {
        // Start a response receiver:
        let (response_recv_socket, response_recv_exit, keypair2) = server_args();
        let (sender2, _receiver2) = unbounded();

        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));

        let SpawnServerResult {
            endpoints: mut response_recv_endpoints,
            thread: response_recv_thread,
            key_updater: _,
        } = solana_streamer::quic::spawn_server(
            "solQuicTest",
            "quic_streamer_test",
            response_recv_socket,
            &keypair2,
            sender2,
            response_recv_exit.clone(),
            1,
            staked_nodes,
            10,
            10,
            DEFAULT_MAX_STREAMS_PER_MS,
            DEFAULT_MAX_CONNECTIONS_PER_IPADDR_PER_MINUTE,
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            DEFAULT_TPU_COALESCE,
        )
        .unwrap();

        let response_recv_endpoint = response_recv_endpoints
            .pop()
            .expect("at least one endpoint");
        let connection_cache = ConnectionCache::new_with_client_options(
            "connection_cache_test",
            1,                            // connection_pool_size
            Some(response_recv_endpoint), // client_endpoint
            None,                         // cert_info
            None,                         // stake_info
        );

        // server port 1:
        let port1 = 9001;
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port1);
        let conn = connection_cache.get_connection(&addr);
        assert_eq!(conn.server_addr().port(), port1);

        // server port 2:
        let port2 = 9002;
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port2);
        let conn = connection_cache.get_connection(&addr);
        assert_eq!(conn.server_addr().port(), port2);

        response_recv_exit.store(true, Ordering::Relaxed);
        response_recv_thread.join().unwrap();
    }
}
