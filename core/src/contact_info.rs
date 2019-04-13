use bincode::serialize;
use solana_sdk::pubkey::Pubkey;
#[cfg(test)]
use solana_sdk::rpc_port;
#[cfg(test)]
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::signature::{Signable, Signature};
use solana_sdk::timing::timestamp;
use std::cmp::{Ord, Ordering, PartialEq, PartialOrd};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Structure representing a node on the network
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContactInfo {
    pub id: Pubkey,
    /// signature of this ContactInfo
    pub signature: Signature,
    /// gossip address
    pub gossip: SocketAddr,
    /// address to connect to for replication
    pub tvu: SocketAddr,
    /// transactions address
    pub tpu: SocketAddr,
    /// address to forward unprocessed transactions to
    pub tpu_via_blobs: SocketAddr,
    /// storage data address
    pub storage_addr: SocketAddr,
    /// address to which to send JSON-RPC requests
    pub rpc: SocketAddr,
    /// websocket for JSON-RPC push notifications
    pub rpc_pubsub: SocketAddr,
    /// latest wallclock picked
    pub wallclock: u64,
}

impl Ord for ContactInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

impl PartialOrd for ContactInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ContactInfo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ContactInfo {}

#[macro_export]
macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        std::net::SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: std::net::SocketAddr = $str.parse().unwrap();
        a
    }};
}
#[macro_export]
macro_rules! socketaddr_any {
    () => {
        socketaddr!(0, 0)
    };
}

impl Default for ContactInfo {
    fn default() -> Self {
        ContactInfo {
            id: Pubkey::default(),
            gossip: socketaddr_any!(),
            tvu: socketaddr_any!(),
            tpu: socketaddr_any!(),
            tpu_via_blobs: socketaddr_any!(),
            storage_addr: socketaddr_any!(),
            rpc: socketaddr_any!(),
            rpc_pubsub: socketaddr_any!(),
            wallclock: 0,
            signature: Signature::default(),
        }
    }
}

impl ContactInfo {
    pub fn new(
        id: &Pubkey,
        gossip: SocketAddr,
        tvu: SocketAddr,
        tpu: SocketAddr,
        tpu_via_blobs: SocketAddr,
        storage_addr: SocketAddr,
        rpc: SocketAddr,
        rpc_pubsub: SocketAddr,
        now: u64,
    ) -> Self {
        Self {
            id: *id,
            signature: Signature::default(),
            gossip,
            tvu,
            tpu,
            tpu_via_blobs,
            storage_addr,
            rpc,
            rpc_pubsub,
            wallclock: now,
        }
    }

    pub fn new_localhost(id: &Pubkey, now: u64) -> Self {
        Self::new(
            id,
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
            socketaddr!("127.0.0.1:1238"),
            socketaddr!("127.0.0.1:1239"),
            socketaddr!("127.0.0.1:1240"),
            now,
        )
    }

    #[cfg(test)]
    /// ContactInfo with multicast addresses for adversarial testing.
    pub fn new_multicast() -> Self {
        let addr = socketaddr!("224.0.1.255:1000");
        assert!(addr.ip().is_multicast());
        Self::new(
            &Pubkey::new_rand(),
            addr,
            addr,
            addr,
            addr,
            addr,
            addr,
            addr,
            0,
        )
    }

    #[cfg(test)]
    fn new_with_pubkey_socketaddr(pubkey: &Pubkey, bind_addr: &SocketAddr) -> Self {
        fn next_port(addr: &SocketAddr, nxt: u16) -> SocketAddr {
            let mut nxt_addr = *addr;
            nxt_addr.set_port(addr.port() + nxt);
            nxt_addr
        }

        let tpu_addr = *bind_addr;
        let gossip_addr = next_port(&bind_addr, 1);
        let tvu_addr = next_port(&bind_addr, 2);
        let tpu_via_blobs_addr = next_port(&bind_addr, 3);
        let rpc_addr = SocketAddr::new(bind_addr.ip(), rpc_port::DEFAULT_RPC_PORT);
        let rpc_pubsub_addr = SocketAddr::new(bind_addr.ip(), rpc_port::DEFAULT_RPC_PUBSUB_PORT);
        Self::new(
            pubkey,
            gossip_addr,
            tvu_addr,
            tpu_addr,
            tpu_via_blobs_addr,
            "0.0.0.0:0".parse().unwrap(),
            rpc_addr,
            rpc_pubsub_addr,
            timestamp(),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_socketaddr(bind_addr: &SocketAddr) -> Self {
        let keypair = Keypair::new();
        Self::new_with_pubkey_socketaddr(&keypair.pubkey(), bind_addr)
    }

    // Construct a ContactInfo that's only usable for gossip
    pub fn new_gossip_entry_point(gossip_addr: &SocketAddr) -> Self {
        let daddr: SocketAddr = socketaddr!("0.0.0.0:0");
        Self::new(
            &Pubkey::default(),
            *gossip_addr,
            daddr,
            daddr,
            daddr,
            daddr,
            daddr,
            daddr,
            timestamp(),
        )
    }

    fn is_valid_ip(addr: IpAddr) -> bool {
        !(addr.is_unspecified() || addr.is_multicast())
        // || (addr.is_loopback() && !cfg_test))
        // TODO: boot loopback in production networks
    }

    /// port must not be 0
    /// ip must be specified and not mulitcast
    /// loopback ip is only allowed in tests
    pub fn is_valid_address(addr: &SocketAddr) -> bool {
        (addr.port() != 0) && Self::is_valid_ip(addr.ip())
    }

    pub fn client_facing_addr(&self) -> (SocketAddr, SocketAddr) {
        (self.rpc, self.tpu)
    }
}

impl Signable for ContactInfo {
    fn pubkey(&self) -> Pubkey {
        self.id
    }

    fn signable_data(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct SignData {
            id: Pubkey,
            gossip: SocketAddr,
            tvu: SocketAddr,
            tpu: SocketAddr,
            tpu_via_blobs: SocketAddr,
            storage_addr: SocketAddr,
            rpc: SocketAddr,
            rpc_pubsub: SocketAddr,
            wallclock: u64,
        }

        let me = self;
        let data = SignData {
            id: me.id,
            gossip: me.gossip,
            tvu: me.tvu,
            tpu: me.tpu,
            storage_addr: me.storage_addr,
            tpu_via_blobs: me.tpu_via_blobs,
            rpc: me.rpc,
            rpc_pubsub: me.rpc_pubsub,
            wallclock: me.wallclock,
        };
        serialize(&data).expect("failed to serialize ContactInfo")
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_address() {
        assert!(cfg!(test));
        let bad_address_port = socketaddr!("127.0.0.1:0");
        assert!(!ContactInfo::is_valid_address(&bad_address_port));
        let bad_address_unspecified = socketaddr!(0, 1234);
        assert!(!ContactInfo::is_valid_address(&bad_address_unspecified));
        let bad_address_multicast = socketaddr!([224, 254, 0, 0], 1234);
        assert!(!ContactInfo::is_valid_address(&bad_address_multicast));
        let loopback = socketaddr!("127.0.0.1:1234");
        assert!(ContactInfo::is_valid_address(&loopback));
        //        assert!(!ContactInfo::is_valid_ip_internal(loopback.ip(), false));
    }
    #[test]
    fn test_default() {
        let ci = ContactInfo::default();
        assert!(ci.gossip.ip().is_unspecified());
        assert!(ci.tvu.ip().is_unspecified());
        assert!(ci.tpu_via_blobs.ip().is_unspecified());
        assert!(ci.rpc.ip().is_unspecified());
        assert!(ci.rpc_pubsub.ip().is_unspecified());
        assert!(ci.tpu.ip().is_unspecified());
        assert!(ci.storage_addr.ip().is_unspecified());
    }
    #[test]
    fn test_multicast() {
        let ci = ContactInfo::new_multicast();
        assert!(ci.gossip.ip().is_multicast());
        assert!(ci.tvu.ip().is_multicast());
        assert!(ci.tpu_via_blobs.ip().is_multicast());
        assert!(ci.rpc.ip().is_multicast());
        assert!(ci.rpc_pubsub.ip().is_multicast());
        assert!(ci.tpu.ip().is_multicast());
        assert!(ci.storage_addr.ip().is_multicast());
    }
    #[test]
    fn test_entry_point() {
        let addr = socketaddr!("127.0.0.1:10");
        let ci = ContactInfo::new_gossip_entry_point(&addr);
        assert_eq!(ci.gossip, addr);
        assert!(ci.tvu.ip().is_unspecified());
        assert!(ci.tpu_via_blobs.ip().is_unspecified());
        assert!(ci.rpc.ip().is_unspecified());
        assert!(ci.rpc_pubsub.ip().is_unspecified());
        assert!(ci.tpu.ip().is_unspecified());
        assert!(ci.storage_addr.ip().is_unspecified());
    }
    #[test]
    fn test_socketaddr() {
        let addr = socketaddr!("127.0.0.1:10");
        let ci = ContactInfo::new_with_socketaddr(&addr);
        assert_eq!(ci.tpu, addr);
        assert_eq!(ci.gossip.port(), 11);
        assert_eq!(ci.tvu.port(), 12);
        assert_eq!(ci.tpu_via_blobs.port(), 13);
        assert_eq!(ci.rpc.port(), 8899);
        assert_eq!(ci.rpc_pubsub.port(), 8900);
        assert!(ci.storage_addr.ip().is_unspecified());
    }
    #[test]
    fn replayed_data_new_with_socketaddr_with_pubkey() {
        let keypair = Keypair::new();
        let d1 = ContactInfo::new_with_pubkey_socketaddr(
            &keypair.pubkey(),
            &socketaddr!("127.0.0.1:1234"),
        );
        assert_eq!(d1.id, keypair.pubkey());
        assert_eq!(d1.gossip, socketaddr!("127.0.0.1:1235"));
        assert_eq!(d1.tvu, socketaddr!("127.0.0.1:1236"));
        assert_eq!(d1.tpu_via_blobs, socketaddr!("127.0.0.1:1237"));
        assert_eq!(d1.tpu, socketaddr!("127.0.0.1:1234"));
        assert_eq!(d1.rpc, socketaddr!("127.0.0.1:8899"));
        assert_eq!(d1.rpc_pubsub, socketaddr!("127.0.0.1:8900"));
    }
}
