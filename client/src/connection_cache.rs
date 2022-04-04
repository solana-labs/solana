use {
    crate::{
        quic_client::QuicTpuConnection, tpu_connection::TpuConnection, udp_client::UdpTpuConnection,
    },
    lazy_static::lazy_static,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::{transaction::VersionedTransaction, transport::TransportError},
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{Arc, Mutex},
    },
    lru::LruCache,
};

// Should be non-zero
static MAX_CONNECTIONS: usize = 64;

#[derive(Clone)]
enum Connection {
    Udp(Arc<UdpTpuConnection>),
    Quic(Arc<QuicTpuConnection>),
}

struct ConnMap {
    map: LruCache<SocketAddr, Connection>,
    use_quic: bool,
}

impl ConnMap {
    pub fn new() -> Self {
        Self {
            map: LruCache::new(MAX_CONNECTIONS),
            use_quic: false,
        }
    }

    pub fn set_use_quic(&mut self, use_quic: bool) {
        self.use_quic = use_quic;
    }
}

lazy_static! {
    static ref CONNECTION_MAP: Mutex<ConnMap> = Mutex::new(ConnMap::new());
}

pub fn set_use_quic(use_quic: bool) {
    let mut map = (*CONNECTION_MAP).lock().unwrap();
    map.set_use_quic(use_quic);
}

// TODO: see https://github.com/solana-labs/solana/issues/23661
// remove lazy_static and optimize and refactor this
fn get_connection(addr: &SocketAddr) -> Connection {
    let mut map = (*CONNECTION_MAP).lock().unwrap();

    match map.map.get(addr) {
        Some(connection) => {
            connection.clone()
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

            map.map.put(addr.clone(), connection.clone());
            connection
        }
    }
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
    let conn = get_connection(addr);
    match conn {
        Connection::Udp(conn) => conn.send_wire_transaction_batch(packets),
        Connection::Quic(conn) => conn.send_wire_transaction_batch(packets),
    }
}

pub fn send_wire_transaction(
    wire_transaction: &[u8],
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let conn = get_connection(addr);
    match conn {
        Connection::Udp(conn) => conn.send_wire_transaction(wire_transaction),
        Connection::Quic(conn) => conn.send_wire_transaction(wire_transaction),
    }
}

pub fn serialize_and_send_transaction(
    transaction: &VersionedTransaction,
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let conn = get_connection(addr);
    match conn {
        Connection::Udp(conn) => conn.serialize_and_send_transaction(transaction),
        Connection::Quic(conn) => conn.serialize_and_send_transaction(transaction),
    }
}

pub fn par_serialize_and_send_transaction_batch(
    transactions: &[VersionedTransaction],
    addr: &SocketAddr,
) -> Result<(), TransportError> {
    let conn = get_connection(addr);
    match conn {
        Connection::Udp(conn) => conn.par_serialize_and_send_transaction_batch(transactions),
        Connection::Quic(conn) => conn.par_serialize_and_send_transaction_batch(transactions),
    }
}
