use {
    crate::{
        connection_cache::Connection, quic_client::QuicTpuConnection, udp_client::UdpTpuConnection,
    },
    enum_dispatch::enum_dispatch,
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_metrics::MovingStat,
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    std::{net::SocketAddr, sync::atomic::AtomicU64},
};

#[derive(Default)]
pub struct ClientStats {
    pub total_connections: AtomicU64,
    pub connection_reuse: AtomicU64,
    pub connection_errors: AtomicU64,
    pub zero_rtt_accepts: AtomicU64,
    pub zero_rtt_rejects: AtomicU64,

    // these will be the last values of these stats
    pub congestion_events: MovingStat,
    pub tx_streams_blocked_uni: MovingStat,
    pub tx_data_blocked: MovingStat,
    pub tx_acks: MovingStat,
    pub make_connection_ms: AtomicU64,
}

#[enum_dispatch]
pub trait TpuConnection {
    fn tpu_addr(&self) -> &SocketAddr;

    fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction_async(wire_transaction)
    }

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>) -> TransportResult<()>;

    fn par_serialize_and_send_transaction_batch(
        &self,
        transactions: &[VersionedTransaction],
    ) -> TransportResult<()> {
        let buffers = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();

        self.send_wire_transaction_batch_async(buffers)
    }

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()>;

    fn forward_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        self.send_wire_transaction_batch_async(buffers)
    }
}
