use bincode::serialize;
use rpc::RPC_PORT;
use signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::timestamp;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Structure representing a node on the network
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContactInfo {
    pub id: Pubkey,
    /// signature of this ContactInfo
    pub signature: Signature,
    /// gossip address
    pub ncp: SocketAddr,
    /// address to connect to for replication
    pub tvu: SocketAddr,
    /// transactions address
    pub tpu: SocketAddr,
    /// storage data address
    pub storage_addr: SocketAddr,
    /// address to which to send JSON-RPC requests
    pub rpc: SocketAddr,
    /// websocket for JSON-RPC push notifications
    pub rpc_pubsub: SocketAddr,
    /// latest wallclock picked
    pub wallclock: u64,
}

#[macro_export]
macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: SocketAddr = $str.parse().unwrap();
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
            ncp: socketaddr_any!(),
            tvu: socketaddr_any!(),
            tpu: socketaddr_any!(),
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
        id: Pubkey,
        ncp: SocketAddr,
        tvu: SocketAddr,
        tpu: SocketAddr,
        storage_addr: SocketAddr,
        rpc: SocketAddr,
        rpc_pubsub: SocketAddr,
        now: u64,
    ) -> Self {
        ContactInfo {
            id,
            signature: Signature::default(),
            ncp,
            tvu,
            tpu,
            storage_addr,
            rpc,
            rpc_pubsub,
            wallclock: now,
        }
    }

    pub fn new_localhost(id: Pubkey, now: u64) -> Self {
        Self::new(
            id,
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
            socketaddr!("127.0.0.1:1238"),
            socketaddr!("127.0.0.1:1239"),
            now,
        )
    }

    #[cfg(test)]
    /// ContactInfo with multicast addresses for adversarial testing.
    pub fn new_multicast() -> Self {
        let addr = socketaddr!("224.0.1.255:1000");
        assert!(addr.ip().is_multicast());
        Self::new(
            Keypair::new().pubkey(),
            addr,
            addr,
            addr,
            addr,
            addr,
            addr,
            0,
        )
    }
    fn next_port(addr: &SocketAddr, nxt: u16) -> SocketAddr {
        let mut nxt_addr = *addr;
        nxt_addr.set_port(addr.port() + nxt);
        nxt_addr
    }
    pub fn new_with_pubkey_socketaddr(pubkey: Pubkey, bind_addr: &SocketAddr) -> Self {
        let transactions_addr = *bind_addr;
        let gossip_addr = Self::next_port(&bind_addr, 1);
        let replicate_addr = Self::next_port(&bind_addr, 2);
        let rpc_addr = SocketAddr::new(bind_addr.ip(), RPC_PORT);
        let rpc_pubsub_addr = SocketAddr::new(bind_addr.ip(), RPC_PORT + 1);
        ContactInfo::new(
            pubkey,
            gossip_addr,
            replicate_addr,
            transactions_addr,
            "0.0.0.0:0".parse().unwrap(),
            rpc_addr,
            rpc_pubsub_addr,
            timestamp(),
        )
    }
    pub fn new_with_socketaddr(bind_addr: &SocketAddr) -> Self {
        let keypair = Keypair::new();
        Self::new_with_pubkey_socketaddr(keypair.pubkey(), bind_addr)
    }
    //
    pub fn new_entry_point(gossip_addr: &SocketAddr) -> Self {
        let daddr: SocketAddr = socketaddr!("0.0.0.0:0");
        ContactInfo::new(
            Pubkey::default(),
            *gossip_addr,
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
    pub fn get_sign_data(&self) -> Vec<u8> {
        let mut data = serialize(&self.id).expect("serialize id");
        let ncp = serialize(&self.ncp).expect("serialize ncp");
        data.extend_from_slice(&ncp);
        let tvu = serialize(&self.tvu).expect("serialize tvu");
        data.extend_from_slice(&tvu);
        let tpu = serialize(&self.tpu).expect("serialize tpu");
        data.extend_from_slice(&tpu);
        let storage_addr = serialize(&self.storage_addr).expect("serialize storage_addr");
        data.extend_from_slice(&storage_addr);
        let rpc = serialize(&self.rpc).expect("serialize rpc");
        data.extend_from_slice(&rpc);
        let rpc_pubsub = serialize(&self.rpc_pubsub).expect("serialize rpc_pubsub");
        data.extend_from_slice(&rpc_pubsub);
        let wallclock = serialize(&self.wallclock).expect("serialize wallclock");
        data.extend_from_slice(&wallclock);
        data
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
        assert!(ci.ncp.ip().is_unspecified());
        assert!(ci.tvu.ip().is_unspecified());
        assert!(ci.rpc.ip().is_unspecified());
        assert!(ci.rpc_pubsub.ip().is_unspecified());
        assert!(ci.tpu.ip().is_unspecified());
        assert!(ci.storage_addr.ip().is_unspecified());
    }
    #[test]
    fn test_multicast() {
        let ci = ContactInfo::new_multicast();
        assert!(ci.ncp.ip().is_multicast());
        assert!(ci.tvu.ip().is_multicast());
        assert!(ci.rpc.ip().is_multicast());
        assert!(ci.rpc_pubsub.ip().is_multicast());
        assert!(ci.tpu.ip().is_multicast());
        assert!(ci.storage_addr.ip().is_multicast());
    }
    #[test]
    fn test_entry_point() {
        let addr = socketaddr!("127.0.0.1:10");
        let ci = ContactInfo::new_entry_point(&addr);
        assert_eq!(ci.ncp, addr);
        assert!(ci.tvu.ip().is_unspecified());
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
        assert_eq!(ci.ncp.port(), 11);
        assert_eq!(ci.tvu.port(), 12);
        assert_eq!(ci.rpc.port(), 8899);
        assert_eq!(ci.rpc_pubsub.port(), 8900);
        assert!(ci.storage_addr.ip().is_unspecified());
    }
    #[test]
    fn replicated_data_new_with_socketaddr_with_pubkey() {
        let keypair = Keypair::new();
        let d1 = ContactInfo::new_with_pubkey_socketaddr(
            keypair.pubkey().clone(),
            &socketaddr!("127.0.0.1:1234"),
        );
        assert_eq!(d1.id, keypair.pubkey());
        assert_eq!(d1.ncp, socketaddr!("127.0.0.1:1235"));
        assert_eq!(d1.tvu, socketaddr!("127.0.0.1:1236"));
        assert_eq!(d1.tpu, socketaddr!("127.0.0.1:1234"));
        assert_eq!(d1.rpc, socketaddr!("127.0.0.1:8899"));
        assert_eq!(d1.rpc_pubsub, socketaddr!("127.0.0.1:8900"));
    }
}
