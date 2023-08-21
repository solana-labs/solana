use {
    bincode::{serialize, Error},
    lru::LruCache,
    rand::{CryptoRng, Fill, Rng},
    serde::Serialize,
    solana_sdk::{
        hash::{self, Hash},
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        signature::{Keypair, Signable, Signature, Signer},
    },
    std::{
        borrow::Cow,
        net::SocketAddr,
        time::{Duration, Instant},
    },
};

const PING_PONG_HASH_PREFIX: &[u8] = "SOLANA_PING_PONG".as_bytes();

#[derive(AbiExample, Debug, Deserialize, Serialize)]
pub struct Ping<T> {
    from: Pubkey,
    token: T,
    signature: Signature,
}

#[derive(AbiExample, Debug, Deserialize, Serialize)]
pub struct Pong {
    from: Pubkey,
    hash: Hash, // Hash of received ping token.
    signature: Signature,
}

/// Maintains records of remote nodes which have returned a valid response to a
/// ping message, and on-the-fly ping messages pending a pong response from the
/// remote node.
pub struct PingCache {
    // Time-to-live of received pong messages.
    ttl: Duration,
    // Rate limit delay to generate pings for a given address
    rate_limit_delay: Duration,
    // Timestamp of last ping message sent to a remote node.
    // Used to rate limit pings to remote nodes.
    pings: LruCache<(Pubkey, SocketAddr), Instant>,
    // Verified pong responses from remote nodes.
    pongs: LruCache<(Pubkey, SocketAddr), Instant>,
    // Hash of ping tokens sent out to remote nodes,
    // pending a pong response back.
    pending_cache: LruCache<Hash, (Pubkey, SocketAddr)>,
}

impl<T: Serialize> Ping<T> {
    pub fn new(token: T, keypair: &Keypair) -> Result<Self, Error> {
        let signature = keypair.sign_message(&serialize(&token)?);
        let ping = Ping {
            from: keypair.pubkey(),
            token,
            signature,
        };
        Ok(ping)
    }
}

impl<T> Ping<T>
where
    T: Serialize + Fill + Default,
{
    pub fn new_rand<R>(rng: &mut R, keypair: &Keypair) -> Result<Self, Error>
    where
        R: Rng + CryptoRng,
    {
        let mut token = T::default();
        rng.fill(&mut token);
        Ping::new(token, keypair)
    }
}

impl<T> Sanitize for Ping<T> {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        self.from.sanitize()?;
        // TODO Add self.token.sanitize()?; when rust's
        // specialization feature becomes stable.
        self.signature.sanitize()
    }
}

impl<T: Serialize> Signable for Ping<T> {
    fn pubkey(&self) -> Pubkey {
        self.from
    }

    fn signable_data(&self) -> Cow<[u8]> {
        Cow::Owned(serialize(&self.token).unwrap())
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature;
    }
}

impl Pong {
    pub fn new<T: Serialize>(ping: &Ping<T>, keypair: &Keypair) -> Result<Self, Error> {
        let token = serialize(&ping.token)?;
        let hash = hash::hashv(&[PING_PONG_HASH_PREFIX, &token]);
        let pong = Pong {
            from: keypair.pubkey(),
            hash,
            signature: keypair.sign_message(hash.as_ref()),
        };
        Ok(pong)
    }

    pub fn from(&self) -> &Pubkey {
        &self.from
    }
}

impl Sanitize for Pong {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        self.from.sanitize()?;
        self.hash.sanitize()?;
        self.signature.sanitize()
    }
}

impl Signable for Pong {
    fn pubkey(&self) -> Pubkey {
        self.from
    }

    fn signable_data(&self) -> Cow<[u8]> {
        Cow::Owned(self.hash.as_ref().into())
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature;
    }
}

impl PingCache {
    pub fn new(ttl: Duration, rate_limit_delay: Duration, cap: usize) -> Self {
        // Sanity check ttl/rate_limit_delay
        assert!(rate_limit_delay <= ttl / 2);
        Self {
            ttl,
            rate_limit_delay,
            pings: LruCache::new(cap),
            pongs: LruCache::new(cap),
            pending_cache: LruCache::new(cap),
        }
    }

    /// Checks if the pong hash, pubkey and socket match a ping message sent
    /// out previously. If so records current timestamp for the remote node and
    /// returns true.
    /// Note: Does not verify the signature.
    pub fn add(&mut self, pong: &Pong, socket: SocketAddr, now: Instant) -> bool {
        let node = (pong.pubkey(), socket);
        match self.pending_cache.peek(&pong.hash) {
            Some(value) if *value == node => {
                self.pings.pop(&node);
                self.pongs.put(node, now);
                self.pending_cache.pop(&pong.hash);
                true
            }
            _ => false,
        }
    }

    /// Checks if the remote node has been pinged recently. If not, calls the
    /// given function to generates a new ping message, records current
    /// timestamp and hash of ping token, and returns the ping message.
    fn maybe_ping<T, F>(
        &mut self,
        now: Instant,
        node: (Pubkey, SocketAddr),
        mut pingf: F,
    ) -> Option<Ping<T>>
    where
        T: Serialize,
        F: FnMut() -> Option<Ping<T>>,
    {
        match self.pings.peek(&node) {
            // Rate limit consecutive pings sent to a remote node.
            Some(t) if now.saturating_duration_since(*t) < self.rate_limit_delay => None,
            _ => {
                let ping = pingf()?;
                let token = serialize(&ping.token).ok()?;
                let hash = hash::hashv(&[PING_PONG_HASH_PREFIX, &token]);
                self.pending_cache.put(hash, node);
                self.pings.put(node, now);
                Some(ping)
            }
        }
    }

    /// Returns true if the remote node has responded to a ping message.
    /// Removes expired pong messages. In order to extend verifications before
    /// expiration, if the pong message is not too recent, and the node has not
    /// been pinged recently, calls the given function to generates a new ping
    /// message, records current timestamp and hash of ping token, and returns
    /// the ping message.
    /// Caller should verify if the socket address is valid. (e.g. by using
    /// ContactInfo::is_valid_address).
    pub fn check<T, F>(
        &mut self,
        now: Instant,
        node: (Pubkey, SocketAddr),
        pingf: F,
    ) -> (bool, Option<Ping<T>>)
    where
        T: Serialize,
        F: FnMut() -> Option<Ping<T>>,
    {
        let (check, should_ping) = match self.pongs.get(&node) {
            None => (false, true),
            Some(t) => {
                let age = now.saturating_duration_since(*t);
                // Pop if the pong message has expired.
                if age > self.ttl {
                    self.pongs.pop(&node);
                }
                // If the pong message is not too recent, generate a new ping
                // message to extend remote node verification.
                (true, age > self.ttl / 8)
            }
        };
        let ping = if should_ping {
            self.maybe_ping(now, node, pingf)
        } else {
            None
        };
        (check, ping)
    }

    /// Only for tests and simulations.
    pub fn mock_pong(&mut self, node: Pubkey, socket: SocketAddr, now: Instant) {
        self.pongs.put((node, socket), now);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{
            collections::HashSet,
            iter::repeat_with,
            net::{Ipv4Addr, SocketAddrV4},
        },
    };

    type Token = [u8; 32];

    #[test]
    fn test_ping_pong() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let ping = Ping::<Token>::new_rand(&mut rng, &keypair).unwrap();
        assert!(ping.verify());
        assert!(ping.sanitize().is_ok());

        let pong = Pong::new(&ping, &keypair).unwrap();
        assert!(pong.verify());
        assert!(pong.sanitize().is_ok());
        assert_eq!(
            hash::hashv(&[PING_PONG_HASH_PREFIX, &ping.token]),
            pong.hash
        );
    }

    #[test]
    fn test_ping_cache() {
        let now = Instant::now();
        let mut rng = rand::thread_rng();
        let ttl = Duration::from_millis(256);
        let delay = ttl / 64;
        let mut cache = PingCache::new(ttl, delay, /*cap=*/ 1000);
        let this_node = Keypair::new();
        let keypairs: Vec<_> = repeat_with(Keypair::new).take(8).collect();
        let sockets: Vec<_> = repeat_with(|| {
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
                rng.gen(),
            ))
        })
        .take(8)
        .collect();
        let remote_nodes: Vec<(&Keypair, SocketAddr)> = repeat_with(|| {
            let keypair = &keypairs[rng.gen_range(0..keypairs.len())];
            let socket = sockets[rng.gen_range(0..sockets.len())];
            (keypair, socket)
        })
        .take(128)
        .collect();

        // Initially all checks should fail. The first observation of each node
        // should create a ping packet.
        let mut seen_nodes = HashSet::<(Pubkey, SocketAddr)>::new();
        let pings: Vec<Option<Ping<Token>>> = remote_nodes
            .iter()
            .map(|(keypair, socket)| {
                let node = (keypair.pubkey(), *socket);
                let pingf = || Ping::<Token>::new_rand(&mut rng, &this_node).ok();
                let (check, ping) = cache.check(now, node, pingf);
                assert!(!check);
                assert_eq!(seen_nodes.insert(node), ping.is_some());
                ping
            })
            .collect();

        let now = now + Duration::from_millis(1);
        let panic_ping = || -> Option<Ping<Token>> { panic!("this should not happen!") };
        for ((keypair, socket), ping) in remote_nodes.iter().zip(&pings) {
            match ping {
                None => {
                    // Already have a recent ping packets for nodes, so no new
                    // ping packet will be generated.
                    let node = (keypair.pubkey(), *socket);
                    let (check, ping) = cache.check(now, node, panic_ping);
                    assert!(check);
                    assert!(ping.is_none());
                }
                Some(ping) => {
                    let pong = Pong::new(ping, keypair).unwrap();
                    assert!(cache.add(&pong, *socket, now));
                }
            }
        }

        let now = now + Duration::from_millis(1);
        // All nodes now have a recent pong packet.
        for (keypair, socket) in &remote_nodes {
            let node = (keypair.pubkey(), *socket);
            let (check, ping) = cache.check(now, node, panic_ping);
            assert!(check);
            assert!(ping.is_none());
        }

        let now = now + ttl / 8;
        // All nodes still have a valid pong packet, but the cache will create
        // a new ping packet to extend verification.
        seen_nodes.clear();
        for (keypair, socket) in &remote_nodes {
            let node = (keypair.pubkey(), *socket);
            let pingf = || Ping::<Token>::new_rand(&mut rng, &this_node).ok();
            let (check, ping) = cache.check(now, node, pingf);
            assert!(check);
            assert_eq!(seen_nodes.insert(node), ping.is_some());
        }

        let now = now + Duration::from_millis(1);
        // All nodes still have a valid pong packet, and a very recent ping
        // packet pending response. So no new ping packet will be created.
        for (keypair, socket) in &remote_nodes {
            let node = (keypair.pubkey(), *socket);
            let (check, ping) = cache.check(now, node, panic_ping);
            assert!(check);
            assert!(ping.is_none());
        }

        let now = now + ttl;
        // Pong packets are still valid but expired. The first observation of
        // each node will remove the pong packet from cache and create a new
        // ping packet.
        seen_nodes.clear();
        for (keypair, socket) in &remote_nodes {
            let node = (keypair.pubkey(), *socket);
            let pingf = || Ping::<Token>::new_rand(&mut rng, &this_node).ok();
            let (check, ping) = cache.check(now, node, pingf);
            if seen_nodes.insert(node) {
                assert!(check);
                assert!(ping.is_some());
            } else {
                assert!(!check);
                assert!(ping.is_none());
            }
        }

        let now = now + Duration::from_millis(1);
        // No valid pong packet in the cache. A recent ping packet already
        // created, so no new one will be created.
        for (keypair, socket) in &remote_nodes {
            let node = (keypair.pubkey(), *socket);
            let (check, ping) = cache.check(now, node, panic_ping);
            assert!(!check);
            assert!(ping.is_none());
        }

        let now = now + ttl / 64;
        // No valid pong packet in the cache. Another ping packet will be
        // created for the first observation of each node.
        seen_nodes.clear();
        for (keypair, socket) in &remote_nodes {
            let node = (keypair.pubkey(), *socket);
            let pingf = || Ping::<Token>::new_rand(&mut rng, &this_node).ok();
            let (check, ping) = cache.check(now, node, pingf);
            assert!(!check);
            assert_eq!(seen_nodes.insert(node), ping.is_some());
        }
    }
}
