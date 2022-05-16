use {
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_metrics::MovingStat,
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    std::{
        net::SocketAddr,
        sync::{atomic::AtomicU64, Arc},
    },
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
}

pub trait TpuConnection {
    fn new(tpu_addr: SocketAddr) -> Self;

    fn tpu_addr(&self) -> &SocketAddr;

    fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
        stats: &ClientStats,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction(&wire_transaction, stats)
    }

    fn send_wire_transaction<T>(
        &self,
        wire_transaction: T,
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]>;

    fn send_wire_transaction_async(
        &self,
        wire_transaction: Vec<u8>,
        stats: Arc<ClientStats>,
    ) -> TransportResult<()>;

    fn par_serialize_and_send_transaction_batch(
        &self,
        transactions: &[VersionedTransaction],
        stats: &ClientStats,
    ) -> TransportResult<()> {
        let buffers = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();

        self.send_wire_transaction_batch(&buffers, stats)
    }

    fn send_wire_transaction_batch<T>(
        &self,
        buffers: &[T],
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]>;

    fn send_wire_transaction_batch_async(
        &self,
        buffers: Vec<Vec<u8>>,
        stats: Arc<ClientStats>,
    ) -> TransportResult<()>;
}
