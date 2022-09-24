use {
    crate::{
        connection_cache_stats::{ConnectionCacheStats, CONNECTION_STAT_SUBMISSION_INTERVAL},
        nonblocking::{
            quic_client::{QuicClient, QuicClientCertificate, QuicLazyInitializedEndpoint},
            tpu_connection::TpuConnection as NonblockingTpuConnection,
        },
        tpu_connection::TpuConnection as BlockingTpuConnection,
    },
    indexmap::map::{Entry, IndexMap},
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_sdk::{pubkey::Pubkey, signature::Keypair, timing::AtomicInterval},
    solana_streamer::{
        nonblocking::quic::{compute_max_allowed_uni_streams, ConnectionPeerType},
        streamer::StakedNodes,
        tls_certificates::new_self_signed_tls_certificate_chain,
    },
    std::{
        error::Error,
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::{atomic::Ordering, Arc, RwLock},
    },
    thiserror::Error,
};

// Should be non-zero
pub(crate) static MAX_CONNECTIONS: usize = 1024;

/// Used to decide whether the TPU and underlying connection cache should use
/// QUIC connections.
pub const DEFAULT_TPU_USE_QUIC: bool = true;

/// Default TPU connection pool size per remote address
pub const DEFAULT_TPU_CONNECTION_POOL_SIZE: usize = 4;

pub const DEFAULT_TPU_ENABLE_UDP: bool = false;

pub struct ConnectionCache {
    map: RwLock<IndexMap<SocketAddr, ConnectionPool>>,
    stats: Arc<ConnectionCacheStats>,
    last_stats: AtomicInterval,
    connection_pool_size: usize,
    tpu_udp_socket: Arc<UdpSocket>,
    client_certificate: Arc<QuicClientCertificate>,
    use_quic: bool,
    maybe_staked_nodes: Option<Arc<RwLock<StakedNodes>>>,
    maybe_client_pubkey: Option<Pubkey>,
}

impl ConnectionCache {
    pub fn new(connection_pool_size: usize) -> Self {
        // The minimum pool size is 1.
        let connection_pool_size = 1.max(connection_pool_size);
        Self {
            use_quic: true,
            connection_pool_size,
            ..Self::default()
        }
    }

    pub fn update_client_certificate(
        &mut self,
        keypair: &Keypair,
        ipaddr: IpAddr,
    ) -> Result<(), Box<dyn Error>> {
        let (certs, priv_key) = new_self_signed_tls_certificate_chain(keypair, ipaddr)?;
        self.client_certificate = Arc::new(QuicClientCertificate {
            certificates: certs,
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

    pub fn with_udp(connection_pool_size: usize) -> Self {
        // The minimum pool size is 1.
        let connection_pool_size = 1.max(connection_pool_size);
        Self {
            use_quic: false,
            connection_pool_size,
            ..Self::default()
        }
    }

    pub fn use_quic(&self) -> bool {
        self.use_quic
    }

    fn create_endpoint(&self, force_use_udp: bool) -> Option<Arc<QuicLazyInitializedEndpoint>> {
        if self.use_quic() && !force_use_udp {
            Some(Arc::new(QuicLazyInitializedEndpoint::new(
                self.client_certificate.clone(),
            )))
        } else {
            None
        }
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

    /// Create a lazy connection object under the exclusive lock of the cache map if there is not
    /// enough used connections in the connection pool for the specified address.
    /// Returns CreateConnectionResult.
    fn create_connection(
        &self,
        lock_timing_ms: &mut u64,
        addr: &SocketAddr,
        force_use_udp: bool,
    ) -> CreateConnectionResult {
        let mut get_connection_map_lock_measure = Measure::start("get_connection_map_lock_measure");
        let mut map = self.map.write().unwrap();
        get_connection_map_lock_measure.stop();
        *lock_timing_ms = lock_timing_ms.saturating_add(get_connection_map_lock_measure.as_ms());
        // Read again, as it is possible that between read lock dropped and the write lock acquired
        // another thread could have setup the connection.

        let (to_create_connection, endpoint) =
            map.get(addr)
                .map_or((true, self.create_endpoint(force_use_udp)), |pool| {
                    (
                        pool.need_new_connection(self.connection_pool_size),
                        pool.endpoint.clone(),
                    )
                });

        let (cache_hit, num_evictions, eviction_timing_ms) = if to_create_connection {
            let connection = if !self.use_quic() || force_use_udp {
                BaseTpuConnection::Udp(self.tpu_udp_socket.clone())
            } else {
                BaseTpuConnection::Quic(Arc::new(QuicClient::new(
                    endpoint.as_ref().unwrap().clone(),
                    *addr,
                    self.compute_max_parallel_streams(),
                )))
            };

            let connection = Arc::new(connection);

            // evict a connection if the cache is reaching upper bounds
            let mut num_evictions = 0;
            let mut get_connection_cache_eviction_measure =
                Measure::start("get_connection_cache_eviction_measure");
            while map.len() >= MAX_CONNECTIONS {
                let mut rng = thread_rng();
                let n = rng.gen_range(0, MAX_CONNECTIONS);
                map.swap_remove_index(n);
                num_evictions += 1;
            }
            get_connection_cache_eviction_measure.stop();

            match map.entry(*addr) {
                Entry::Occupied(mut entry) => {
                    let pool = entry.get_mut();
                    pool.connections.push(connection);
                }
                Entry::Vacant(entry) => {
                    entry.insert(ConnectionPool {
                        connections: vec![connection],
                        endpoint,
                    });
                }
            }
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

        let port_offset = if self.use_quic() { QUIC_PORT_OFFSET } else { 0 };

        let port = addr
            .port()
            .checked_add(port_offset)
            .unwrap_or_else(|| addr.port());
        let force_use_udp = port == addr.port();
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
                    self.create_connection(&mut lock_timing_ms, &addr, force_use_udp)
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
                self.create_connection(&mut lock_timing_ms, &addr, force_use_udp)
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
    ) -> (Arc<BaseTpuConnection>, Arc<ConnectionCacheStats>) {
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

    pub fn get_connection(&self, addr: &SocketAddr) -> BlockingConnection {
        let (connection, connection_cache_stats) = self.get_connection_and_log_stats(addr);
        connection.new_blocking_connection(*addr, connection_cache_stats)
    }

    pub fn get_nonblocking_connection(&self, addr: &SocketAddr) -> NonblockingConnection {
        let (connection, connection_cache_stats) = self.get_connection_and_log_stats(addr);
        connection.new_nonblocking_connection(*addr, connection_cache_stats)
    }
}

impl Default for ConnectionCache {
    fn default() -> Self {
        let (certs, priv_key) = new_self_signed_tls_certificate_chain(
            &Keypair::new(),
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        )
        .expect("Failed to initialize QUIC client certificates");
        Self {
            map: RwLock::new(IndexMap::with_capacity(MAX_CONNECTIONS)),
            stats: Arc::new(ConnectionCacheStats::default()),
            last_stats: AtomicInterval::default(),
            connection_pool_size: DEFAULT_TPU_CONNECTION_POOL_SIZE,
            tpu_udp_socket: Arc::new(
                solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)))
                    .expect("Unable to bind to UDP socket"),
            ),
            client_certificate: Arc::new(QuicClientCertificate {
                certificates: certs,
                key: priv_key,
            }),
            use_quic: DEFAULT_TPU_USE_QUIC,
            maybe_staked_nodes: None,
            maybe_client_pubkey: None,
        }
    }
}

#[derive(Error, Debug)]
pub enum ConnectionPoolError {
    #[error("connection index is out of range of the pool")]
    IndexOutOfRange,
}

pub trait ConnectionPool {
    type PoolTpuConnection: BaseTpuConnection;
    type TpuConfig: Default;
    const PORT_OFFSET: u16 = 0;

    /// Create a new connection pool based on protocol-specific configuration
    fn new_with_connection(config: &Self::TpuConfig, addr: &SocketAddr) -> Self;

    /// Add a connection to the pool
    fn add_connection(&mut self, config: &Self::TpuConfig, addr: &SocketAddr);

    /// Get the number of current connections in the pool
    fn num_connections(&self) -> usize;

    /// Get a connection based on its index in the pool, without checking if the
    fn get(&self, index: usize) -> Result<Arc<Self::PoolTpuConnection>, ConnectionPoolError>;

    /// Get a connection from the pool. It must have at least one connection in the pool.
    /// This randomly picks a connection in the pool.
    fn borrow_connection(&self) -> Arc<Self::PoolTpuConnection> {
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
        config: &Self::TpuConfig,
        addr: &SocketAddr,
    ) -> Self::PoolTpuConnection;
}

pub trait BaseTpuConnection {
    type BlockingConnectionType: BlockingTpuConnection;
    type NonblockingConnectionType: NonblockingTpuConnection;

    fn new_blocking_connection(
        &self,
        addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Self::BlockingConnectionType;

    fn new_nonblocking_connection(
        &self,
        addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Self::NonblockingConnectionType;
}

struct GetConnectionResult {
    connection: Arc<BaseTpuConnection>,
    cache_hit: bool,
    report_stats: bool,
    map_timing_ms: u64,
    lock_timing_ms: u64,
    connection_cache_stats: Arc<ConnectionCacheStats>,
    num_evictions: u64,
    eviction_timing_ms: u64,
}

struct CreateConnectionResult {
    connection: Arc<BaseTpuConnection>,
    cache_hit: bool,
    connection_cache_stats: Arc<ConnectionCacheStats>,
    num_evictions: u64,
    eviction_timing_ms: u64,
}
