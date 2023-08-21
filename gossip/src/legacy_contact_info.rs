use {
    crate::{
        contact_info::{
            get_quic_socket, sanitize_quic_offset, sanitize_socket, ContactInfo, Error, Protocol,
            SOCKET_ADDR_UNSPECIFIED,
        },
        crds_value::MAX_WALLCLOCK,
    },
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
    id: Pubkey,
    /// gossip address
    gossip: SocketAddr,
    /// address to connect to for replication
    tvu: SocketAddr,
    /// TVU over QUIC protocol.
    tvu_quic: SocketAddr,
    /// repair service over QUIC protocol.
    serve_repair_quic: SocketAddr,
    /// transactions address
    tpu: SocketAddr,
    /// address to forward unprocessed transactions to
    tpu_forwards: SocketAddr,
    /// address to which to send bank state requests
    tpu_vote: SocketAddr,
    /// address to which to send JSON-RPC requests
    rpc: SocketAddr,
    /// websocket for JSON-RPC push notifications
    rpc_pubsub: SocketAddr,
    /// address to send repair requests to
    serve_repair: SocketAddr,
    /// latest wallclock picked
    wallclock: u64,
    /// node shred version
    shred_version: u16,
}

impl Sanitize for LegacyContactInfo {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}

macro_rules! get_socket {
    ($name:ident) => {
        pub fn $name(&self) -> Result<SocketAddr, Error> {
            let socket = &self.$name;
            sanitize_socket(socket)?;
            Ok(socket).copied()
        }
    };
    ($name:ident, $quic:ident) => {
        pub fn $name(&self, protocol: Protocol) -> Result<SocketAddr, Error> {
            let socket = match protocol {
                Protocol::QUIC => &self.$quic,
                Protocol::UDP => &self.$name,
            };
            sanitize_socket(socket)?;
            Ok(socket).copied()
        }
    };
    (@quic $name:ident) => {
        pub fn $name(&self, protocol: Protocol) -> Result<SocketAddr, Error> {
            let socket = &self.$name;
            sanitize_socket(socket)?;
            match protocol {
                Protocol::QUIC => get_quic_socket(socket),
                Protocol::UDP => Ok(socket).copied(),
            }
        }
    };
}

macro_rules! set_socket {
    ($name:ident, $key:ident) => {
        pub fn $name<T>(&mut self, socket: T) -> Result<(), Error>
        where
            SocketAddr: From<T>,
        {
            let socket = SocketAddr::from(socket);
            sanitize_socket(&socket)?;
            self.$key = socket;
            Ok(())
        }
    };
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
            tvu_quic: socketaddr_any!(),
            serve_repair_quic: socketaddr_any!(),
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
            tvu_quic: socketaddr!(Ipv4Addr::LOCALHOST, 1236),
            serve_repair_quic: socketaddr!(Ipv4Addr::LOCALHOST, 1237),
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
        let now = timestamp() - delay + rng.gen_range(0..2 * delay);
        let pubkey = pubkey.unwrap_or_else(solana_sdk::pubkey::new_rand);
        let mut node = LegacyContactInfo::new_localhost(&pubkey, now);
        node.gossip.set_port(rng.gen_range(1024..u16::MAX));
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

    #[inline]
    pub fn pubkey(&self) -> &Pubkey {
        &self.id
    }

    #[inline]
    pub fn wallclock(&self) -> u64 {
        self.wallclock
    }

    #[inline]
    pub fn shred_version(&self) -> u16 {
        self.shred_version
    }

    pub fn set_pubkey(&mut self, pubkey: Pubkey) {
        self.id = pubkey
    }

    pub fn set_wallclock(&mut self, wallclock: u64) {
        self.wallclock = wallclock;
    }

    #[cfg(test)]
    pub(crate) fn set_shred_version(&mut self, shred_version: u16) {
        self.shred_version = shred_version
    }

    get_socket!(gossip);
    get_socket!(tvu, tvu_quic);
    get_socket!(@quic tpu);
    get_socket!(@quic tpu_forwards);
    get_socket!(tpu_vote);
    get_socket!(rpc);
    get_socket!(rpc_pubsub);
    get_socket!(serve_repair, serve_repair_quic);

    set_socket!(set_gossip, gossip);
    set_socket!(set_rpc, rpc);

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

    pub(crate) fn valid_client_facing_addr(
        &self,
        protocol: Protocol,
        socket_addr_space: &SocketAddrSpace,
    ) -> Option<(SocketAddr, SocketAddr)> {
        Some((
            self.rpc()
                .ok()
                .filter(|addr| socket_addr_space.check(addr))?,
            self.tpu(protocol)
                .ok()
                .filter(|addr| socket_addr_space.check(addr))?,
        ))
    }
}

impl TryFrom<&ContactInfo> for LegacyContactInfo {
    type Error = Error;

    fn try_from(node: &ContactInfo) -> Result<Self, Self::Error> {
        macro_rules! unwrap_socket {
            ($name:ident) => {
                node.$name().ok().unwrap_or(SOCKET_ADDR_UNSPECIFIED)
            };
            ($name:ident, $protocol:expr) => {
                node.$name($protocol)
                    .ok()
                    .unwrap_or(SOCKET_ADDR_UNSPECIFIED)
            };
        }
        sanitize_quic_offset(
            &node.tpu(Protocol::UDP).ok(),
            &node.tpu(Protocol::QUIC).ok(),
        )?;
        sanitize_quic_offset(
            &node.tpu_forwards(Protocol::UDP).ok(),
            &node.tpu_forwards(Protocol::QUIC).ok(),
        )?;
        Ok(Self {
            id: *node.pubkey(),
            gossip: unwrap_socket!(gossip),
            tvu: unwrap_socket!(tvu, Protocol::UDP),
            tvu_quic: unwrap_socket!(tvu, Protocol::QUIC),
            serve_repair_quic: unwrap_socket!(serve_repair, Protocol::QUIC),
            tpu: unwrap_socket!(tpu, Protocol::UDP),
            tpu_forwards: unwrap_socket!(tpu_forwards, Protocol::UDP),
            tpu_vote: unwrap_socket!(tpu_vote),
            rpc: unwrap_socket!(rpc),
            rpc_pubsub: unwrap_socket!(rpc_pubsub),
            serve_repair: unwrap_socket!(serve_repair, Protocol::UDP),
            wallclock: node.wallclock(),
            shred_version: node.shred_version(),
        })
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
            ci.valid_client_facing_addr(Protocol::QUIC, &SocketAddrSpace::Unspecified),
            None
        );
        ci.tpu = socketaddr!(Ipv4Addr::LOCALHOST, 123);
        assert_eq!(
            ci.valid_client_facing_addr(Protocol::QUIC, &SocketAddrSpace::Unspecified),
            None
        );
        ci.rpc = socketaddr!(Ipv4Addr::LOCALHOST, 234);
        assert!(ci
            .valid_client_facing_addr(Protocol::QUIC, &SocketAddrSpace::Unspecified)
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
