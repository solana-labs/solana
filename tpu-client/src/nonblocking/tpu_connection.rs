//! Trait defining async send functions, to be used for UDP or QUIC sending

use {
    async_trait::async_trait,
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    std::net::SocketAddr,
};

#[async_trait]
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
