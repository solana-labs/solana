use {
    crate::{
        quic_client::QuicTpuConnection,
        tpu_connection::{ClientStats, TpuConnection},
        udp_client::UdpTpuConnection,
    },
    indexmap::map::IndexMap,
    lazy_static::lazy_static,
    quinn_proto::ConnectionStats,
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::{
        timing::AtomicInterval, transaction::VersionedTransaction, transport::TransportError,
    },
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, RwLock,
        },
    },
};

// Should be non-zero
static MAX_CONNECTIONS: usize = 1024;

#[derive(Clone)]
pub enum Connection {
    Udp(Arc<UdpTpuConnection>),
    Quic(Arc<QuicTpuConnection>),
}

#[derive(Default)]
struct ConnectionCacheStats {
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    cache_evictions: AtomicU64,
    eviction_time_ms: AtomicU64,
    sent_packets: AtomicU64,
    total_batches: AtomicU64,
    batch_success: AtomicU64,
    batch_failure: AtomicU64,
    get_connection_ms: AtomicU64,
    get_connection_lock_ms: AtomicU64,
    get_connection_hit_ms: AtomicU64,
    get_connection_miss_ms: AtomicU64,

    // Need to track these separately per-connection
    // because we need to track the base stat value from quinn
    total_client_stats: ClientStats,
}

const CONNECTION_STAT_SUBMISSION_INTERVAL: u64 = 2000;

impl ConnectionCacheStats {
    fn add_client_stats(&self, client_stats: &ClientStats, num_packets: usize, is_success: bool) {
        self.total_client_stats.total_connections.fetch_add(
            client_stats.total_connections.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.connection_reuse.fetch_add(
            client_stats.connection_reuse.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.sent_packets
            .fetch_add(num_packets as u64, Ordering::Relaxed);
        self.total_batches.fetch_add(1, Ordering::Relaxed);
        if is_success {
            self.batch_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.batch_failure.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn report(&self) {
        datapoint_info!(
            "quic-client-connection-stats",
            (
                "cache_hits",
                self.cache_hits.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cache_misses",
                self.cache_misses.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cache_evictions",
                self.cache_evictions.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "eviction_time_ms",
                self.eviction_time_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_ms",
                self.get_connection_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_lock_ms",
                self.get_connection_lock_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_hit_ms",
                self.get_connection_hit_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_miss_ms",
                self.get_connection_miss_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "total_connections",
                self.total_client_stats
                    .total_connections
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_reuse",
                self.total_client_stats
                    .connection_reuse
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "congestion_events",
                self.total_client_stats.congestion_events.load_and_reset(),
                i64
            ),
            (
                "tx_streams_blocked_uni",
                self.total_client_stats
                    .tx_streams_blocked_uni
                    .load_and_reset(),
                i64
            ),
            (
                "tx_data_blocked",
                self.total_client_stats.tx_data_blocked.load_and_reset(),
                i64
            ),
            (
                "tx_acks",
                self.total_client_stats.tx_acks.load_and_reset(),
                i64
            ),
            (
                "num_packets",
                self.sent_packets.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "total_batches",
                self.total_batches.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "batch_failure",
                self.batch_failure.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}

struct ConnectionMap {
    map: IndexMap<SocketAddr, Connection>,
    stats: Arc<ConnectionCacheStats>,
    last_stats: AtomicInterval,
    use_quic: bool,
}

impl ConnectionMap {
    pub fn new() -> Self {
        Self {
            map: IndexMap::with_capacity(MAX_CONNECTIONS),
            stats: Arc::new(ConnectionCacheStats::default()),
            last_stats: AtomicInterval::default(),
            use_quic: false,
        }
    }

    pub fn set_use_quic(&mut self, use_quic: bool) {
        self.use_quic = use_quic;
    }
}

lazy_static! {
    static ref CONNECTION_MAP: RwLock<ConnectionMap> = RwLock::new(ConnectionMap::new());
}

pub fn set_use_quic(use_quic: bool) {
    let mut map = (*CONNECTION_MAP).write().unwrap();
    map.set_use_quic(use_quic);
}

struct GetConnectionResult {
    connection: Connection,
    cache_hit: bool,
    report_stats: bool,
    map_timing_ms: u64,
    lock_timing_ms: u64,
    connection_cache_stats: Arc<ConnectionCacheStats>,
    other_stats: Option<(Arc<ClientStats>, ConnectionStats)>,
    num_evictions: u64,
    eviction_timing_ms: u64,
}

fn get_or_add_connection(addr: &SocketAddr) -> GetConnectionResult {
    let mut get_connection_map_lock_measure = Measure::start("get_connection_map_lock_measure");
    let map = (*CONNECTION_MAP).read().unwrap();
    get_connection_map_lock_measure.stop();

    let mut lock_timing_ms = get_connection_map_lock_measure.as_ms();

    let report_stats = map
        .last_stats
        .should_update(CONNECTION_STAT_SUBMISSION_INTERVAL);

    let mut get_connection_map_measure = Measure::start("get_connection_hit_measure");
    let (
        connection,
        cache_hit,
        connection_cache_stats,
        maybe_stats,
        num_evictions,
        eviction_timing_ms,
    ) = match map.map.get(addr) {
        Some(connection) => {
            let mut stats = None;
            // update connection stats
            if let Connection::Quic(conn) = connection {
                stats = conn.stats().map(|s| (conn.base_stats(), s));
            }
            (connection.clone(), true, map.stats.clone(), stats, 0, 0)
        }
        None => {
            let (_, send_socket) = solana_net_utils::bind_in_range(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                VALIDATOR_PORT_RANGE,
            )
            .unwrap();

            let connection = if map.use_quic {
                Connection::Quic(Arc::new(QuicTpuConnection::new(send_socket, *addr)))
            } else {
                Connection::Udp(Arc::new(UdpTpuConnection::new(send_socket, *addr)))
            };

            // Upgrade to write access by dropping read lock and acquire write lock
            drop(map);
            let mut get_connection_map_lock_measure =
                Measure::start("get_connection_map_lock_measure");
            let mut map = (*CONNECTION_MAP).write().unwrap();
            get_connection_map_lock_measure.stop();

            lock_timing_ms = lock_timing_ms.saturating_add(get_connection_map_lock_measure.as_ms());

            // evict a connection if the cache is reaching upper bounds
            let mut num_evictions = 0;
            let mut get_connection_cache_eviction_measure =
                Measure::start("get_connection_cache_eviction_measure");
            while map.map.len() >= MAX_CONNECTIONS {
                let mut rng = thread_rng();
                let n = rng.gen_range(0, MAX_CONNECTIONS);
                map.map.swap_remove_index(n);
                num_evictions += 1;
            }
            get_connection_cache_eviction_measure.stop();

            map.map.insert(*addr, connection.clone());
            (
                connection,
                false,
                map.stats.clone(),
                None,
                num_evictions,
                get_connection_cache_eviction_measure.as_ms(),
            )
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
        other_stats: maybe_stats,
        num_evictions,
        eviction_timing_ms,
    }
}

// TODO: see https://github.com/solana-labs/solana/issues/23661
// remove lazy_static and optimize and refactor this
fn get_connection(addr: &SocketAddr) -> (Connection, Arc<ConnectionCacheStats>) {
    let mut get_connection_measure = Measure::start("get_connection_measure");
    let GetConnectionResult {
        connection,
        cache_hit,
        report_stats,
        map_timing_ms,
        lock_timing_ms,
        connection_cache_stats,
        other_stats,
        num_evictions,
        eviction_timing_ms,
    } = get_or_add_connection(addr);

    if report_stats {
        connection_cache_stats.report();
    }

    if let Some((connection_stats, new_stats)) = other_stats {
        connection_cache_stats
            .total_client_stats
            .congestion_events
            .update_stat(
                &connection_stats.congestion_events,
                new_stats.path.congestion_events,
            );

        connection_cache_stats
            .total_client_stats
            .tx_streams_blocked_uni
            .update_stat(
                &connection_stats.tx_streams_blocked_uni,
                new_stats.frame_tx.streams_blocked_uni,
            );

        connection_cache_stats
            .total_client_stats
            .tx_data_blocked
            .update_stat(
                &connection_stats.tx_data_blocked,
                new_stats.frame_tx.data_blocked,
            );

        connection_cache_stats
            .total_client_stats
            .tx_acks
            .update_stat(&connection_stats.tx_acks, new_stats.frame_tx.acks);
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

// TODO: see https://github.com/solana-labs/solana/issues/23851
// use enum_dispatch and get rid of this tedious code.
// The main blocker to using enum_dispatch right now is that
// the it doesn't work with static methods like TpuConnection::new
// which is used by thin_client. This will be eliminated soon
// once thin_client is moved to using this connection cache.
// Once that is done, we will migrate to using enum_dispatch
// This will be done in a followup to
// https://github.com/solana-labs/solana/pull/23817
pub fn send_wire_transaction_batch(
    packets: &[&[u8]],
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let (conn, stats) = get_connection(addr);
    let client_stats = ClientStats::default();
    let r = match conn {
        Connection::Udp(conn) => conn.send_wire_transaction_batch(packets, &client_stats),
        Connection::Quic(conn) => conn.send_wire_transaction_batch(packets, &client_stats),
    };
    stats.add_client_stats(&client_stats, packets.len(), r.is_ok());
    r
}

pub fn send_wire_transaction_async(
    packets: Vec<u8>,
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let (conn, stats) = get_connection(addr);
    let client_stats = Arc::new(ClientStats::default());
    let r = match conn {
        Connection::Udp(conn) => conn.send_wire_transaction_async(packets, client_stats.clone()),
        Connection::Quic(conn) => conn.send_wire_transaction_async(packets, client_stats.clone()),
    };
    stats.add_client_stats(&client_stats, 1, r.is_ok());
    r
}

pub fn send_wire_transaction_batch_async(
    packets: Vec<Vec<u8>>,
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let (conn, stats) = get_connection(addr);
    let client_stats = Arc::new(ClientStats::default());
    let len = packets.len();
    let r = match conn {
        Connection::Udp(conn) => {
            conn.send_wire_transaction_batch_async(packets, client_stats.clone())
        }
        Connection::Quic(conn) => {
            conn.send_wire_transaction_batch_async(packets, client_stats.clone())
        }
    };
    stats.add_client_stats(&client_stats, len, r.is_ok());
    r
}

pub fn send_wire_transaction(
    wire_transaction: &[u8],
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    send_wire_transaction_batch(&[wire_transaction], addr)
}

pub fn serialize_and_send_transaction(
    transaction: &VersionedTransaction,
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let (conn, stats) = get_connection(addr);
    let client_stats = ClientStats::default();
    let r = match conn {
        Connection::Udp(conn) => conn.serialize_and_send_transaction(transaction, &client_stats),
        Connection::Quic(conn) => conn.serialize_and_send_transaction(transaction, &client_stats),
    };
    stats.add_client_stats(&client_stats, 1, r.is_ok());
    r
}

pub fn par_serialize_and_send_transaction_batch(
    transactions: &[VersionedTransaction],
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let (conn, stats) = get_connection(addr);
    let client_stats = ClientStats::default();
    let r = match conn {
        Connection::Udp(conn) => {
            conn.par_serialize_and_send_transaction_batch(transactions, &client_stats)
        }
        Connection::Quic(conn) => {
            conn.par_serialize_and_send_transaction_batch(transactions, &client_stats)
        }
    };
    stats.add_client_stats(&client_stats, transactions.len(), r.is_ok());
    r
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            connection_cache::{get_connection, Connection, CONNECTION_MAP, MAX_CONNECTIONS},
            tpu_connection::TpuConnection,
        },
        rand::{Rng, SeedableRng},
        rand_chacha::ChaChaRng,
        std::net::{IpAddr, SocketAddr},
    };

    fn get_addr(rng: &mut ChaChaRng) -> SocketAddr {
        let a = rng.gen_range(1, 255);
        let b = rng.gen_range(1, 255);
        let c = rng.gen_range(1, 255);
        let d = rng.gen_range(1, 255);

        let addr_str = format!("{}.{}.{}.{}:80", a, b, c, d);

        addr_str.parse().expect("Invalid address")
    }

    fn ip(conn: Connection) -> IpAddr {
        match conn {
            Connection::Udp(conn) => conn.tpu_addr().ip(),
            Connection::Quic(conn) => conn.tpu_addr().ip(),
        }
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

        // Generate a bunch of random addresses and create TPUConnections to them
        // Since TPUConnection::new is infallible, it should't matter whether or not
        // we can actually connect to those addresses - TPUConnection implementations should either
        // be lazy and not connect until first use or handle connection errors somehow
        // (without crashing, as would be required in a real practical validator)
        let addrs = (0..MAX_CONNECTIONS)
            .into_iter()
            .map(|_| {
                let addr = get_addr(&mut rng);
                get_connection(&addr);
                addr
            })
            .collect::<Vec<_>>();
        {
            let map = (*CONNECTION_MAP).read().unwrap();
            assert!(map.map.len() == MAX_CONNECTIONS);
            addrs.iter().for_each(|a| {
                let conn = map.map.get(a).expect("Address not found");
                assert!(a.ip() == ip(conn.clone()));
            });
        }

        let addr = get_addr(&mut rng);
        get_connection(&addr);

        let map = (*CONNECTION_MAP).read().unwrap();
        assert!(map.map.len() == MAX_CONNECTIONS);
        let _conn = map.map.get(&addr).expect("Address not found");
    }
}
