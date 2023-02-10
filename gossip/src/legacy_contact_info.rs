use {
    crate::crds_value::MAX_WALLCLOCK,
    solana_sdk::{
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Structure representing a node on the network
#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, AbiExample, Deserialize, Serialize,
)]
pub struct LegacyContactInfo {
    pub id: Pubkey,
    /// gossip address
    pub gossip: SocketAddr,
    /// address to connect to for replication
    pub tvu: SocketAddr,
    /// address to forward shreds to
    pub tvu_forwards: SocketAddr,
    /// address to send repair responses to
    pub repair: SocketAddr,
    /// transactions address
    pub tpu: SocketAddr,
    /// address to forward unprocessed transactions to
    pub tpu_forwards: SocketAddr,
    /// address to which to send bank state requests
    pub tpu_vote: SocketAddr,
    /// address to which to send JSON-RPC requests
    pub rpc: SocketAddr,
    /// websocket for JSON-RPC push notifications
    pub rpc_pubsub: SocketAddr,
    /// address to send repair requests to
    pub serve_repair: SocketAddr,
    /// latest wallclock picked
    pub wallclock: u64,
    /// node shred version
    pub shred_version: u16,
}

impl Sanitize for LegacyContactInfo {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        std::net::SocketAddr::from((std::net::Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        $str.parse::<std::net::SocketAddr>().unwrap()
    }};
}

#[macro_export]
macro_rules! socketaddr_any {
    () => {
        socketaddr!(std::net::Ipv4Addr::UNSPECIFIED, 0)
    };
}

impl Default for LegacyContactInfo {
    fn default() -> Self {
        LegacyContactInfo {
            id: Pubkey::default(),
            gossip: socketaddr_any!(),
            tvu: socketaddr_any!(),
            tvu_forwards: socketaddr_any!(),
            repair: socketaddr_any!(),
            tpu: socketaddr_any!(),
            tpu_forwards: socketaddr_any!(),
            tpu_vote: socketaddr_any!(),
            rpc: socketaddr_any!(),
            rpc_pubsub: socketaddr_any!(),
            serve_repair: socketaddr_any!(),
            wallclock: 0,
            shred_version: 0,
        }
    }
}

impl LegacyContactInfo {
    pub fn new_localhost(id: &Pubkey, now: u64) -> Self {
        Self {
            id: *id,
            gossip: socketaddr!(Ipv4Addr::LOCALHOST, 1234),
            tvu: socketaddr!(Ipv4Addr::LOCALHOST, 1235),
            tvu_forwards: socketaddr!(Ipv4Addr::LOCALHOST, 1236),
            repair: socketaddr!(Ipv4Addr::LOCALHOST, 1237),
            tpu: socketaddr!(Ipv4Addr::LOCALHOST, 1238),
            tpu_forwards: socketaddr!(Ipv4Addr::LOCALHOST, 1239),
            tpu_vote: socketaddr!(Ipv4Addr::LOCALHOST, 1240),
            rpc: socketaddr!(Ipv4Addr::LOCALHOST, 1241),
            rpc_pubsub: socketaddr!(Ipv4Addr::LOCALHOST, 1242),
            serve_repair: socketaddr!(Ipv4Addr::LOCALHOST, 1243),
            wallclock: now,
            shred_version: 0,
        }
    }

    /// New random LegacyContactInfo for tests and simulations.
    pub fn new_rand<R: rand::Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        let delay = 10 * 60 * 1000; // 10 minutes
        let now = timestamp() - delay + rng.gen_range(0, 2 * delay);
        let pubkey = pubkey.unwrap_or_else(solana_sdk::pubkey::new_rand);
        let mut node = LegacyContactInfo::new_localhost(&pubkey, now);
        node.gossip.set_port(rng.gen_range(1024, u16::MAX));
        node
    }

    // Construct a LegacyContactInfo that's only usable for gossip
    pub fn new_gossip_entry_point(gossip_addr: &SocketAddr) -> Self {
        Self {
            id: Pubkey::default(),
            gossip: *gossip_addr,
            wallclock: timestamp(),
            ..LegacyContactInfo::default()
        }
    }

    fn is_valid_ip(addr: IpAddr) -> bool {
        !(addr.is_unspecified() || addr.is_multicast())
        // || (addr.is_loopback() && !cfg_test))
        // TODO: boot loopback in production networks
    }

    /// port must not be 0
    /// ip must be specified and not multicast
    /// loopback ip is only allowed in tests
    // TODO: Replace this entirely with streamer SocketAddrSpace.
    pub fn is_valid_address(addr: &SocketAddr, socket_addr_space: &SocketAddrSpace) -> bool {
        addr.port() != 0u16 && Self::is_valid_ip(addr.ip()) && socket_addr_space.check(addr)
    }

    pub fn client_facing_addr(&self) -> (SocketAddr, SocketAddr) {
        (self.rpc, self.tpu)
    }

    pub(crate) fn valid_client_facing_addr(
        &self,
        socket_addr_space: &SocketAddrSpace,
    ) -> Option<(SocketAddr, SocketAddr)> {
        if LegacyContactInfo::is_valid_address(&self.rpc, socket_addr_space)
            && LegacyContactInfo::is_valid_address(&self.tpu, socket_addr_space)
        {
            Some((self.rpc, self.tpu))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_address() {
        let bad_address_port = socketaddr!(Ipv4Addr::LOCALHOST, 0);
        assert!(!LegacyContactInfo::is_valid_address(
            &bad_address_port,
            &SocketAddrSpace::Unspecified
        ));
        let bad_address_unspecified = socketaddr!(Ipv4Addr::UNSPECIFIED, 1234);
        assert!(!LegacyContactInfo::is_valid_address(
            &bad_address_unspecified,
            &SocketAddrSpace::Unspecified
        ));
        let bad_address_multicast = socketaddr!([224, 254, 0, 0], 1234);
        assert!(!LegacyContactInfo::is_valid_address(
            &bad_address_multicast,
            &SocketAddrSpace::Unspecified
        ));
        let loopback = socketaddr!(Ipv4Addr::LOCALHOST, 1234);
        assert!(LegacyContactInfo::is_valid_address(
            &loopback,
            &SocketAddrSpace::Unspecified
        ));
        //        assert!(!LegacyContactInfo::is_valid_ip_internal(loopback.ip(), false));
    }

    #[test]
    fn test_default() {
        let ci = LegacyContactInfo::default();
        assert!(ci.gossip.ip().is_unspecified());
        assert!(ci.tvu.ip().is_unspecified());
        assert!(ci.tpu_forwards.ip().is_unspecified());
        assert!(ci.rpc.ip().is_unspecified());
        assert!(ci.rpc_pubsub.ip().is_unspecified());
        assert!(ci.tpu.ip().is_unspecified());
        assert!(ci.tpu_vote.ip().is_unspecified());
        assert!(ci.serve_repair.ip().is_unspecified());
    }

    #[test]
    fn test_entry_point() {
        let addr = socketaddr!(Ipv4Addr::LOCALHOST, 10);
        let ci = LegacyContactInfo::new_gossip_entry_point(&addr);
        assert_eq!(ci.gossip, addr);
        assert!(ci.tvu.ip().is_unspecified());
        assert!(ci.tpu_forwards.ip().is_unspecified());
        assert!(ci.rpc.ip().is_unspecified());
        assert!(ci.rpc_pubsub.ip().is_unspecified());
        assert!(ci.tpu.ip().is_unspecified());
        assert!(ci.tpu_vote.ip().is_unspecified());
        assert!(ci.serve_repair.ip().is_unspecified());
    }

    #[test]
    fn test_valid_client_facing() {
        let mut ci = LegacyContactInfo::default();
        assert_eq!(
            ci.valid_client_facing_addr(&SocketAddrSpace::Unspecified),
            None
        );
        ci.tpu = socketaddr!(Ipv4Addr::LOCALHOST, 123);
        assert_eq!(
            ci.valid_client_facing_addr(&SocketAddrSpace::Unspecified),
            None
        );
        ci.rpc = socketaddr!(Ipv4Addr::LOCALHOST, 234);
        assert!(ci
            .valid_client_facing_addr(&SocketAddrSpace::Unspecified)
            .is_some());
    }

    #[test]
    fn test_sanitize() {
        let mut ci = LegacyContactInfo::default();
        assert_eq!(ci.sanitize(), Ok(()));
        ci.wallclock = MAX_WALLCLOCK;
        assert_eq!(ci.sanitize(), Err(SanitizeError::ValueOutOfBounds));
    }
}
