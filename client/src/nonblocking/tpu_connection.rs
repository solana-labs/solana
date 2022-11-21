//! Trait defining async send functions, to be used for UDP or QUIC sending

use {
    async_trait::async_trait,
    enum_dispatch::enum_dispatch,
    solana_quic_client::nonblocking::quic_client::QuicTpuConnection,
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    solana_udp_client::nonblocking::udp_client::UdpTpuConnection,
    std::net::SocketAddr,
};

// Due to the existence of `crate::connection_cache::Connection`, if this is named
// `Connection`, enum_dispatch gets confused between the two and throws errors when
// trying to convert later.
#[enum_dispatch]
pub enum NonblockingConnection {
    QuicTpuConnection,
    UdpTpuConnection,
}

#[async_trait]
#[enum_dispatch(NonblockingConnection)]
pub trait TpuConnection {
    fn tpu_addr(&self) -> &SocketAddr;

    async fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction(&wire_transaction).await
    }

    async fn send_wire_transaction<T>(&self, wire_transaction: T) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync;

    async fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync;
}
