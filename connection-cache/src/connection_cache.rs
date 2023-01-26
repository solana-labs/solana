use {
    crate::{
        client_connection::ClientConnection as BlockingClientConnection,
        connection_cache_stats::{ConnectionCacheStats, CONNECTION_STAT_SUBMISSION_INTERVAL},
        nonblocking::client_connection::ClientConnection as NonblockingClientConnection,
    },
    indexmap::map::IndexMap,
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_sdk::timing::AtomicInterval,
    std::{
        any::Any,
        net::SocketAddr,
        sync::{atomic::Ordering, Arc, RwLock},
    },
    thiserror::Error,
};

// Should be non-zero
pub static MAX_CONNECTIONS: usize = 1024;

/// Default connection pool size per remote address
pub const DEFAULT_CONNECTION_POOL_SIZE: usize = 4;

/// Defines the protocol types of an implementation supports.
pub enum ProtocolType {
    UDP,
    QUIC,
}

pub trait ConnectionManager: Sync + Send {
    fn new_connection_pool(&self) -> Box<dyn ConnectionPool>;
    fn new_connection_config(&self) -> Box<dyn NewConnectionConfig>;
    fn get_port_offset(&self) -> u16;
    fn get_protocol_type(&self) -> ProtocolType;
}

pub struct ConnectionCache {
    pub map: RwLock<IndexMap<SocketAddr, Box<dyn ConnectionPool>>>,
    pub connection_manager: Box<dyn ConnectionManager>,
    pub stats: Arc<ConnectionCacheStats>,
    pub last_stats: AtomicInterval,
    pub connection_pool_size: usize,
    pub connection_config: Box<dyn NewConnectionConfig>,
}

impl ConnectionCache {
    pub fn new(
        connection_manager: Box<dyn ConnectionManager>,
        connection_pool_size: usize,
    ) -> Result<Self, ClientError> {
        let config = connection_manager.new_connection_config();
        Ok(Self::new_with_config(
            connection_pool_size,
            config,
            connection_manager,
        ))
    }

    pub fn new_with_config(
        connection_pool_size: usize,
        connection_config: Box<dyn NewConnectionConfig>,
        connection_manager: Box<dyn ConnectionManager>,
    ) -> Self {
        Self {
            map: RwLock::new(IndexMap::with_capacity(MAX_CONNECTIONS)),
            stats: Arc::new(ConnectionCacheStats::default()),
            connection_manager,
            last_stats: AtomicInterval::default(),
            connection_pool_size: 1.max(connection_pool_size), // The minimum pool size is 1.
            connection_config,
        }
    }

    /// Create a lazy connection object under the exclusive lock of the cache map if there is not
    /// enough used connections in the connection pool for the specified address.
    /// Returns CreateConnectionResult.
    fn create_connection(
        &self,
        lock_timing_ms: &mut u64,
        addr: &SocketAddr,
    ) -> CreateConnectionResult {
        let mut get_connection_map_lock_measure = Measure::start("get_connection_map_lock_measure");
        let mut map = self.map.write().unwrap();
        get_connection_map_lock_measure.stop();
        *lock_timing_ms = lock_timing_ms.saturating_add(get_connection_map_lock_measure.as_ms());
        // Read again, as it is possible that between read lock dropped and the write lock acquired
        // another thread could have setup the connection.

        let should_create_connection = map
            .get(addr)
            .map(|pool| pool.need_new_connection(self.connection_pool_size))
            .unwrap_or(true);

        let (cache_hit, num_evictions, eviction_timing_ms) = if should_create_connection {
            // evict a connection if the cache is reaching upper bounds
            let mut num_evictions = 0;
            let mut get_connection_cache_eviction_measure =
                Measure::start("get_connection_cache_eviction_measure");
            let existing_index = map.get_index_of(addr);
            while map.len() >= MAX_CONNECTIONS {
                let mut rng = thread_rng();
                let n = rng.gen_range(0, MAX_CONNECTIONS);
                if let Some(index) = existing_index {
                    if n == index {
                        continue;
                    }
                }
                map.swap_remove_index(n);
                num_evictions += 1;
            }
            get_connection_cache_eviction_measure.stop();

            map.entry(*addr)
                .and_modify(|pool| {
                    pool.add_connection(&*self.connection_config, addr);
                })
                .or_insert_with(|| {
                    let mut pool = self.connection_manager.new_connection_pool();
                    pool.add_connection(&*self.connection_config, addr);
                    pool
                });
            (
                false,
                num_evictions,
                get_connection_cache_eviction_measure.as_ms(),
            )
        } else {
            (true, 0, 0)
        };

        let pool = map.get(addr).unwrap();
        let connection = pool.borrow_connection();

        CreateConnectionResult {
            connection,
            cache_hit,
            connection_cache_stats: self.stats.clone(),
            num_evictions,
            eviction_timing_ms,
        }
    }

    fn get_or_add_connection(&self, addr: &SocketAddr) -> GetConnectionResult {
        let mut get_connection_map_lock_measure = Measure::start("get_connection_map_lock_measure");
        let map = self.map.read().unwrap();
        get_connection_map_lock_measure.stop();

        let port_offset = self.connection_manager.get_port_offset();

        let port = addr
            .port()
            .checked_add(port_offset)
            .unwrap_or_else(|| addr.port());
        let addr = SocketAddr::new(addr.ip(), port);

        let mut lock_timing_ms = get_connection_map_lock_measure.as_ms();

        let report_stats = self
            .last_stats
            .should_update(CONNECTION_STAT_SUBMISSION_INTERVAL);

        let mut get_connection_map_measure = Measure::start("get_connection_hit_measure");
        let CreateConnectionResult {
            connection,
            cache_hit,
            connection_cache_stats,
            num_evictions,
            eviction_timing_ms,
        } = match map.get(&addr) {
            Some(pool) => {
                if pool.need_new_connection(self.connection_pool_size) {
                    // create more connection and put it in the pool
                    drop(map);
                    self.create_connection(&mut lock_timing_ms, &addr)
                } else {
                    let connection = pool.borrow_connection();
                    CreateConnectionResult {
                        connection,
                        cache_hit: true,
                        connection_cache_stats: self.stats.clone(),
                        num_evictions: 0,
                        eviction_timing_ms: 0,
                    }
                }
            }
            None => {
                // Upgrade to write access by dropping read lock and acquire write lock
                drop(map);
                self.create_connection(&mut lock_timing_ms, &addr)
            }
        };
        get_connection_map_measure.stop();

        GetConnectionResult {
            connection,
            cache_hit,
            report_stats,
            map_timing_ms: get_connection_map_measure.as_ms(),
            lock_timing_ms,
            connection_cache_stats,
            num_evictions,
            eviction_timing_ms,
        }
    }

    fn get_connection_and_log_stats(
        &self,
        addr: &SocketAddr,
    ) -> (Arc<dyn BaseClientConnection>, Arc<ConnectionCacheStats>) {
        let mut get_connection_measure = Measure::start("get_connection_measure");
        let GetConnectionResult {
            connection,
            cache_hit,
            report_stats,
            map_timing_ms,
            lock_timing_ms,
            connection_cache_stats,
            num_evictions,
            eviction_timing_ms,
        } = self.get_or_add_connection(addr);

        if report_stats {
            connection_cache_stats.report();
        }

        if cache_hit {
            connection_cache_stats
                .cache_hits
                .fetch_add(1, Ordering::Relaxed);
            connection_cache_stats
                .get_connection_hit_ms
                .fetch_add(map_timing_ms, Ordering::Relaxed);
        } else {
            connection_cache_stats
                .cache_misses
                .fetch_add(1, Ordering::Relaxed);
            connection_cache_stats
                .get_connection_miss_ms
                .fetch_add(map_timing_ms, Ordering::Relaxed);
            connection_cache_stats
                .cache_evictions
                .fetch_add(num_evictions, Ordering::Relaxed);
            connection_cache_stats
                .eviction_time_ms
                .fetch_add(eviction_timing_ms, Ordering::Relaxed);
        }

        get_connection_measure.stop();
        connection_cache_stats
            .get_connection_lock_ms
            .fetch_add(lock_timing_ms, Ordering::Relaxed);
        connection_cache_stats
            .get_connection_ms
            .fetch_add(get_connection_measure.as_ms(), Ordering::Relaxed);

        (connection, connection_cache_stats)
    }

    pub fn get_connection(&self, addr: &SocketAddr) -> Arc<dyn BlockingClientConnection> {
        let (connection, connection_cache_stats) = self.get_connection_and_log_stats(addr);
        connection.new_blocking_connection(*addr, connection_cache_stats)
    }

    pub fn get_nonblocking_connection(
        &self,
        addr: &SocketAddr,
    ) -> Arc<dyn NonblockingClientConnection> {
        let (connection, connection_cache_stats) = self.get_connection_and_log_stats(addr);
        connection.new_nonblocking_connection(*addr, connection_cache_stats)
    }

    pub fn get_protocol_type(&self) -> ProtocolType {
        self.connection_manager.get_protocol_type()
    }
}

#[derive(Error, Debug)]
pub enum ConnectionPoolError {
    #[error("connection index is out of range of the pool")]
    IndexOutOfRange,
}

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Certificate error: {0}")]
    CertificateError(String),

    #[error("IO error: {0:?}")]
    IoError(#[from] std::io::Error),
}

pub trait NewConnectionConfig: Sync + Send {
    fn new() -> Result<Self, ClientError>
    where
        Self: Sized;
    fn as_any(&self) -> &dyn Any;

    fn as_mut_any(&mut self) -> &mut dyn Any;
}

pub trait ConnectionPool: Sync + Send {
    /// Add a connection to the pool
    fn add_connection(&mut self, config: &dyn NewConnectionConfig, addr: &SocketAddr);

    /// Get the number of current connections in the pool
    fn num_connections(&self) -> usize;

    /// Get a connection based on its index in the pool, without checking if the
    fn get(&self, index: usize) -> Result<Arc<dyn BaseClientConnection>, ConnectionPoolError>;

    /// Get a connection from the pool. It must have at least one connection in the pool.
    /// This randomly picks a connection in the pool.
    fn borrow_connection(&self) -> Arc<dyn BaseClientConnection> {
        let mut rng = thread_rng();
        let n = rng.gen_range(0, self.num_connections());
        self.get(n).expect("index is within num_connections")
    }
    /// Check if we need to create a new connection. If the count of the connections
    /// is smaller than the pool size.
    fn need_new_connection(&self, required_pool_size: usize) -> bool {
        self.num_connections() < required_pool_size
    }

    fn create_pool_entry(
        &self,
        config: &dyn NewConnectionConfig,
        addr: &SocketAddr,
    ) -> Arc<dyn BaseClientConnection>;
}

pub trait BaseClientConnection: Sync + Send {
    fn new_blocking_connection(
        &self,
        addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Arc<dyn BlockingClientConnection>;

    fn new_nonblocking_connection(
        &self,
        addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Arc<dyn NonblockingClientConnection>;
}

struct GetConnectionResult {
    connection: Arc<dyn BaseClientConnection>,
    cache_hit: bool,
    report_stats: bool,
    map_timing_ms: u64,
    lock_timing_ms: u64,
    connection_cache_stats: Arc<ConnectionCacheStats>,
    num_evictions: u64,
    eviction_timing_ms: u64,
}

struct CreateConnectionResult {
    connection: Arc<dyn BaseClientConnection>,
    cache_hit: bool,
    connection_cache_stats: Arc<ConnectionCacheStats>,
    num_evictions: u64,
    eviction_timing_ms: u64,
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            client_connection::ClientConnection as BlockingClientConnection,
            nonblocking::client_connection::ClientConnection as NonblockingClientConnection,
        },
        async_trait::async_trait,
        rand::{Rng, SeedableRng},
        rand_chacha::ChaChaRng,
        solana_sdk::transport::Result as TransportResult,
        std::{
            net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
            sync::Arc,
        },
    };

    const MOCK_PORT_OFFSET: u16 = 42;

    pub struct MockUdpPool {
        connections: Vec<Arc<dyn BaseClientConnection>>,
    }
    impl ConnectionPool for MockUdpPool {
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
            let config: &MockUdpConfig = match config.as_any().downcast_ref::<MockUdpConfig>() {
                Some(b) => b,
                None => panic!("Expecting a MockUdpConfig!"),
            };

            Arc::new(MockUdp(config.udp_socket.clone()))
        }
    }

    pub struct MockUdpConfig {
        udp_socket: Arc<UdpSocket>,
    }

    impl Default for MockUdpConfig {
        fn default() -> Self {
            Self {
                udp_socket: Arc::new(
                    solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
                        .expect("Unable to bind to UDP socket"),
                ),
            }
        }
    }

    impl NewConnectionConfig for MockUdpConfig {
        fn new() -> Result<Self, ClientError> {
            Ok(Self {
                udp_socket: Arc::new(
                    solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
                        .map_err(Into::<ClientError>::into)?,
                ),
            })
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_mut_any(&mut self) -> &mut dyn Any {
            self
        }
    }

    pub struct MockUdp(Arc<UdpSocket>);
    impl BaseClientConnection for MockUdp {
        fn new_blocking_connection(
            &self,
            addr: SocketAddr,
            _stats: Arc<ConnectionCacheStats>,
        ) -> Arc<dyn BlockingClientConnection> {
            Arc::new(MockUdpConnection {
                _socket: self.0.clone(),
                addr,
            })
        }

        fn new_nonblocking_connection(
            &self,
            addr: SocketAddr,
            _stats: Arc<ConnectionCacheStats>,
        ) -> Arc<dyn NonblockingClientConnection> {
            Arc::new(MockUdpConnection {
                _socket: self.0.clone(),
                addr,
            })
        }
    }

    pub struct MockUdpConnection {
        _socket: Arc<UdpSocket>,
        addr: SocketAddr,
    }

    #[derive(Default)]
    pub struct MockConnectionManager {}

    impl ConnectionManager for MockConnectionManager {
        fn new_connection_pool(&self) -> Box<dyn ConnectionPool> {
            Box::new(MockUdpPool {
                connections: Vec::default(),
            })
        }

        fn new_connection_config(&self) -> Box<dyn NewConnectionConfig> {
            Box::new(MockUdpConfig::new().unwrap())
        }

        fn get_port_offset(&self) -> u16 {
            MOCK_PORT_OFFSET
        }

        fn get_protocol_type(&self) -> ProtocolType {
            ProtocolType::UDP
        }
    }

    impl BlockingClientConnection for MockUdpConnection {
        fn server_addr(&self) -> &SocketAddr {
            &self.addr
        }
        fn send_data(&self, _buffer: &[u8]) -> TransportResult<()> {
            unimplemented!()
        }
        fn send_data_async(&self, _data: Vec<u8>) -> TransportResult<()> {
            unimplemented!()
        }
        fn send_data_batch(&self, _buffers: &[Vec<u8>]) -> TransportResult<()> {
            unimplemented!()
        }
        fn send_data_batch_async(&self, _buffers: Vec<Vec<u8>>) -> TransportResult<()> {
            unimplemented!()
        }
    }

    #[async_trait]
    impl NonblockingClientConnection for MockUdpConnection {
        fn server_addr(&self) -> &SocketAddr {
            &self.addr
        }
        async fn send_data(&self, _data: &[u8]) -> TransportResult<()> {
            unimplemented!()
        }
        async fn send_data_batch(&self, _buffers: &[Vec<u8>]) -> TransportResult<()> {
            unimplemented!()
        }
    }

    fn get_addr(rng: &mut ChaChaRng) -> SocketAddr {
        let a = rng.gen_range(1, 255);
        let b = rng.gen_range(1, 255);
        let c = rng.gen_range(1, 255);
        let d = rng.gen_range(1, 255);

        let addr_str = format!("{a}.{b}.{c}.{d}:80");

        addr_str.parse().expect("Invalid address")
    }

    #[test]
    fn test_connection_cache() {
        solana_logger::setup();
        // Allow the test to run deterministically
        // with the same pseudorandom sequence between runs
        // and on different platforms - the cryptographic security
        // property isn't important here but ChaChaRng provides a way
        // to get the same pseudorandom sequence on different platforms
        let mut rng = ChaChaRng::seed_from_u64(42);

        // Generate a bunch of random addresses and create connections to them
        // Since ClientConnection::new is infallible, it should't matter whether or not
        // we can actually connect to those addresses - ClientConnection implementations should either
        // be lazy and not connect until first use or handle connection errors somehow
        // (without crashing, as would be required in a real practical validator)
        let connection_manager = Box::<MockConnectionManager>::default();
        let connection_cache =
            ConnectionCache::new(connection_manager, DEFAULT_CONNECTION_POOL_SIZE).unwrap();
        let port_offset = MOCK_PORT_OFFSET;
        let addrs = (0..MAX_CONNECTIONS)
            .map(|_| {
                let addr = get_addr(&mut rng);
                connection_cache.get_connection(&addr);
                addr
            })
            .collect::<Vec<_>>();
        {
            let map = connection_cache.map.read().unwrap();
            assert!(map.len() == MAX_CONNECTIONS);
            addrs.iter().for_each(|a| {
                let port = a
                    .port()
                    .checked_add(port_offset)
                    .unwrap_or_else(|| a.port());
                let addr = &SocketAddr::new(a.ip(), port);

                let conn = &map.get(addr).expect("Address not found").get(0).unwrap();
                let conn = conn.new_blocking_connection(*addr, connection_cache.stats.clone());
                assert!(addr.ip() == conn.server_addr().ip());
            });
        }

        let addr = &get_addr(&mut rng);
        connection_cache.get_connection(addr);

        let port = addr
            .port()
            .checked_add(port_offset)
            .unwrap_or_else(|| addr.port());
        let addr_with_quic_port = SocketAddr::new(addr.ip(), port);
        let map = connection_cache.map.read().unwrap();
        assert!(map.len() == MAX_CONNECTIONS);
        let _conn = map.get(&addr_with_quic_port).expect("Address not found");
    }

    // Test that we can get_connection with a connection cache configured
    // on an address with a port that would overflow to
    // an invalid port.
    #[test]
    fn test_overflow_address() {
        let port = u16::MAX - MOCK_PORT_OFFSET + 1;
        assert!(port.checked_add(MOCK_PORT_OFFSET).is_none());
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let connection_manager = Box::<MockConnectionManager>::default();
        let connection_cache = ConnectionCache::new(connection_manager, 1).unwrap();

        let conn = connection_cache.get_connection(&addr);
        // We (intentionally) don't have an interface that allows us to distinguish between
        // UDP and Quic connections, so check instead that the port is valid (non-zero)
        // and is the same as the input port (falling back on UDP)
        assert!(conn.server_addr().port() != 0);
        assert!(conn.server_addr().port() == port);
    }
}
