#![allow(clippy::integer_arithmetic)]

pub mod nonblocking;

use {
    solana_tpu_client::{
        connection_cache::ConnectionCacheStats,
        nonblocking::udp_client::UdpTpuConnection as NonblockingUdpTpuConnection,
        tpu_connection_cache::BaseTpuConnection,
        udp_client::UdpTpuConnection as BlockingUdpTpuConnection,
    },
    std::{
        net::{SocketAddr, UdpSocket},
        sync::Arc,
    },
};

pub struct Udp(Arc<UdpSocket>);
impl BaseTpuConnection for Udp {
    type BlockingConnectionType = BlockingUdpTpuConnection;
    type NonblockingConnectionType = NonblockingUdpTpuConnection;

    fn new_blocking_connection(
        &self,
        addr: SocketAddr,
        _stats: Arc<ConnectionCacheStats>,
    ) -> BlockingUdpTpuConnection {
        BlockingUdpTpuConnection::new_from_addr(self.0.clone(), addr)
    }

    fn new_nonblocking_connection(
        &self,
        addr: SocketAddr,
        _stats: Arc<ConnectionCacheStats>,
    ) -> NonblockingUdpTpuConnection {
        NonblockingUdpTpuConnection::new_from_addr(self.0.try_clone().unwrap(), addr)
    }
}
