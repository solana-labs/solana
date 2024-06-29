#[cfg(test)]
use crate::contact_info::{get_quic_socket, sanitize_socket};
use {
    crate::{
        contact_info::{
            sanitize_quic_offset, ContactInfo, Error, Protocol, SOCKET_ADDR_UNSPECIFIED,
        },
        crds_value::MAX_WALLCLOCK,
    },
    solana_sanitize::{Sanitize, SanitizeError},
    solana_sdk::pubkey::Pubkey,
    solana_streamer::socket::SocketAddrSpace,
    std::net::{IpAddr, SocketAddr},
};

/// Structure representing a node on the network
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct LegacyContactInfo {
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
        #[cfg(test)]
        pub(crate) fn $name(&self) -> Result<SocketAddr, Error> {
            let socket = &self.$name;
            sanitize_socket(socket)?;
            Ok(socket).copied()
        }
    };
    ($name:ident, $quic:ident) => {
        #[cfg(test)]
        pub(crate) fn $name(&self, protocol: Protocol) -> Result<SocketAddr, Error> {
            let socket = match protocol {
                Protocol::QUIC => &self.$quic,
                Protocol::UDP => &self.$name,
            };
            sanitize_socket(socket)?;
            Ok(socket).copied()
        }
    };
    (@quic $name:ident) => {
        #[cfg(test)]
        pub(crate) fn $name(&self, protocol: Protocol) -> Result<SocketAddr, Error> {
            let socket = &self.$name;
            sanitize_socket(socket)?;
            match protocol {
                Protocol::QUIC => get_quic_socket(socket),
                Protocol::UDP => Ok(socket).copied(),
            }
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

#[cfg(test)]
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
    #[inline]
    pub(crate) fn pubkey(&self) -> &Pubkey {
        &self.id
    }

    #[inline]
    pub(crate) fn wallclock(&self) -> u64 {
        self.wallclock
    }

    #[inline]
    pub(crate) fn shred_version(&self) -> u16 {
        self.shred_version
    }

    pub(crate) fn gossip(&self) -> Result<SocketAddr, Error> {
        let socket = &self.gossip;
        crate::contact_info::sanitize_socket(socket)?;
        Ok(socket).copied()
    }

    get_socket!(tvu, tvu_quic);
    get_socket!(@quic tpu);
    get_socket!(@quic tpu_forwards);
    get_socket!(tpu_vote);
    get_socket!(rpc);
    get_socket!(rpc_pubsub);
    get_socket!(serve_repair, serve_repair_quic);

    fn is_valid_ip(addr: IpAddr) -> bool {
        !(addr.is_unspecified() || addr.is_multicast())
        // || (addr.is_loopback() && !cfg_test))
        // TODO: boot loopback in production networks
    }

    /// port must not be 0
    /// ip must be specified and not multicast
    /// loopback ip is only allowed in tests
    // TODO: Replace this entirely with streamer SocketAddrSpace.
    pub(crate) fn is_valid_address(addr: &SocketAddr, socket_addr_space: &SocketAddrSpace) -> bool {
        addr.port() != 0u16 && Self::is_valid_ip(addr.ip()) && socket_addr_space.check(addr)
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
    use {super::*, std::net::Ipv4Addr};

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
    fn test_sanitize() {
        let mut ci = LegacyContactInfo::default();
        assert_eq!(ci.sanitize(), Ok(()));
        ci.wallclock = MAX_WALLCLOCK;
        assert_eq!(ci.sanitize(), Err(SanitizeError::ValueOutOfBounds));
    }
}
