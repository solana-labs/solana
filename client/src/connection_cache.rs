use {
    crate::{
        quic_client::QuicTpuConnection,
        tpu_connection::{ClientStats, TpuConnection},
        udp_client::UdpTpuConnection,
    },
    lazy_static::lazy_static,
    lru::LruCache,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::{
        timing::AtomicInterval, transaction::VersionedTransaction, transport::TransportError,
    },
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Mutex,
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
    sent_packets: AtomicU64,
    total_batches: AtomicU64,
    batch_success: AtomicU64,
    batch_failure: AtomicU64,

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
    map: LruCache<SocketAddr, Connection>,
    stats: Arc<ConnectionCacheStats>,
    last_stats: AtomicInterval,
    use_quic: bool,
}

impl ConnectionMap {
    pub fn new() -> Self {
        Self {
            map: LruCache::new(MAX_CONNECTIONS),
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
    static ref CONNECTION_MAP: Mutex<ConnectionMap> = Mutex::new(ConnectionMap::new());
}

pub fn set_use_quic(use_quic: bool) {
    let mut map = (*CONNECTION_MAP).lock().unwrap();
    map.set_use_quic(use_quic);
}

// TODO: see https://github.com/solana-labs/solana/issues/23661
// remove lazy_static and optimize and refactor this
fn get_connection(addr: &SocketAddr) -> (Connection, Arc<ConnectionCacheStats>) {
    let mut map = (*CONNECTION_MAP).lock().unwrap();

    if map
        .last_stats
        .should_update(CONNECTION_STAT_SUBMISSION_INTERVAL)
    {
        map.stats.report();
    }

    let (connection, hit, maybe_stats) = match map.map.get(addr) {
        Some(connection) => {
            let mut stats = None;
            // update connection stats
            if let Connection::Quic(conn) = connection {
                stats = conn.stats().map(|s| (conn.base_stats(), s));
            }
            (connection.clone(), true, stats)
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

            map.map.put(*addr, connection.clone());
            (connection, false, None)
        }
    };

    if let Some((connection_stats, new_stats)) = maybe_stats {
        map.stats.total_client_stats.congestion_events.update_stat(
            &connection_stats.congestion_events,
            new_stats.path.congestion_events,
        );

        map.stats
            .total_client_stats
            .tx_streams_blocked_uni
            .update_stat(
                &connection_stats.tx_streams_blocked_uni,
                new_stats.frame_tx.streams_blocked_uni,
            );

        map.stats.total_client_stats.tx_data_blocked.update_stat(
            &connection_stats.tx_data_blocked,
            new_stats.frame_tx.data_blocked,
        );

        map.stats
            .total_client_stats
            .tx_acks
            .update_stat(&connection_stats.tx_acks, new_stats.frame_tx.acks);
    }

    if hit {
        map.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
    } else {
        map.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
    }
    (connection, map.stats.clone())
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
        let first_addr = get_addr(&mut rng);
        assert!(ip(get_connection(&first_addr).0) == first_addr.ip());
        let addrs = (0..MAX_CONNECTIONS)
            .into_iter()
            .map(|_| {
                let addr = get_addr(&mut rng);
                get_connection(&addr);
                addr
            })
            .collect::<Vec<_>>();
        {
            let map = (*CONNECTION_MAP).lock().unwrap();
            addrs.iter().for_each(|a| {
                let conn = map.map.peek(a).expect("Address not found");
                assert!(a.ip() == ip(conn.clone()));
            });

            assert!(map.map.peek(&first_addr).is_none());
        }

        // Test that get_connection updates which connection is next up for eviction
        // when an existing connection is used. Initially, addrs[0] should be next up for eviction, since
        // it was the earliest added. But we do get_connection(&addrs[0]), thereby using
        // that connection, and bumping it back to the end of the queue. So addrs[1] should be
        // the next up for eviction. So we add a new connection, and test that addrs[0] is not
        // evicted but addrs[1] is.
        get_connection(&addrs[0]);
        get_connection(&get_addr(&mut rng));

        let map = (*CONNECTION_MAP).lock().unwrap();
        assert!(map.map.peek(&addrs[0]).is_some());
        assert!(map.map.peek(&addrs[1]).is_none());
    }
}
