use {
    crate::{tpu_connection::TpuConnection, udp_client::UdpTpuConnection},
    lazy_static::lazy_static,
    std::{
        collections::{hash_map::Entry, BTreeMap, HashMap},
        net::{SocketAddr, UdpSocket},
        sync::{Arc, Mutex},
    },
};

// Should be non-zero
static MAX_CONNECTIONS: usize = 64;

struct ConnMap {
    // Keeps track of the connection associated with an addr and the last time it was used
    map: HashMap<SocketAddr, (Arc<dyn TpuConnection + 'static + Sync + Send>, u64)>,
    // Helps to find the least recently used connection. The search and inserts are O(log(n))
    // but since we're bounding the size of the collections, this should be constant
    // (and hopefully negligible) time. In theory, we can do this in constant time
    // with a queue implemented as a doubly-linked list (and all the
    // HashMap entries holding a "pointer" to the corresponding linked-list node),
    // so we can push, pop and bump a used connection back to the end of the queue in O(1) time, but
    // that seems non-"Rust-y" and low bang/buck. This is still pretty terrible though...
    last_used_times: BTreeMap<u64, SocketAddr>,
    ticks: u64,
}

impl ConnMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            last_used_times: BTreeMap::new(),
            ticks: 0,
        }
    }
}

lazy_static! {
    static ref CONNECTION_MAP: Mutex<ConnMap> = Mutex::new(ConnMap::new());
}

#[allow(dead_code)]
// TODO: see https://github.com/solana-labs/solana/issues/23661
// remove lazy_static and optimize and refactor this
pub fn get_connection(addr: &SocketAddr) -> Arc<dyn TpuConnection + 'static + Sync + Send> {
    let mut map = (*CONNECTION_MAP).lock().unwrap();
    let ticks = map.ticks;

    let (conn, target_ticks) = match map.map.entry(*addr) {
        Entry::Occupied(mut entry) => {
            let mut pair = entry.get_mut();
            let old_ticks = pair.1;
            pair.1 = ticks;
            (pair.0.clone(), old_ticks)
        }
        Entry::Vacant(entry) => {
            let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            // TODO: see https://github.com/solana-labs/solana/issues/23659
            // make it configurable (e.g. via the command line) whether to use UDP or Quic
            let conn = Arc::new(UdpTpuConnection::new(send_socket, *addr));
            entry.insert((conn.clone(), ticks));
            (
                conn as Arc<dyn TpuConnection + 'static + Sync + Send>,
                ticks,
            )
        }
    };

    let num_connections = map.map.len();
    if num_connections > MAX_CONNECTIONS {
        let (old_ticks, target_addr) = {
            let (old_ticks, target_addr) = map.last_used_times.iter().next().unwrap();
            (*old_ticks, *target_addr)
        };
        map.map.remove(&target_addr);
        map.last_used_times.remove(&old_ticks);
    }

    if target_ticks != ticks {
        map.last_used_times.remove(&target_ticks);
    }
    map.last_used_times.insert(ticks, *addr);

    map.ticks += 1;
    conn
}

#[cfg(test)]
mod tests {
    use {
        crate::connection_cache::{get_connection, CONNECTION_MAP, MAX_CONNECTIONS},
        rand::{Rng, SeedableRng},
        rand_chacha::ChaChaRng,
        std::net::SocketAddr,
    };

    fn get_addr(rng: &mut ChaChaRng) -> SocketAddr {
        let a = rng.gen_range(1, 255);
        let b = rng.gen_range(1, 255);
        let c = rng.gen_range(1, 255);
        let d = rng.gen_range(1, 255);

        let addr_str = format!("{}.{}.{}.{}:80", a, b, c, d);

        addr_str.parse().expect("Invalid address")
    }

    #[test]
    fn test_connection_cache() {
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
        assert!(get_connection(&first_addr).tpu_addr().ip() == first_addr.ip());
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
                let conn = map.map.get(a).expect("Address not found");
                assert!(a.ip() == conn.0.tpu_addr().ip());
            });

            assert!(map.map.get(&first_addr).is_none());
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
        assert!(map.map.get(&addrs[0]).is_some());
        assert!(map.map.get(&addrs[1]).is_none());
    }
}
