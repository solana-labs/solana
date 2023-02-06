pub use solana_tpu_client::nonblocking::tpu_client::{LeaderTpuService, TpuSenderError};
use {
    crate::{connection_cache::ConnectionCache, tpu_client::TpuClientConfig},
    solana_connection_cache::connection_cache::ConnectionCache as BackendConnectionCache,
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    solana_sdk::{
        message::Message,
        signers::Signers,
        transaction::{Transaction, TransactionError},
        transport::Result as TransportResult,
    },
    solana_tpu_client::nonblocking::tpu_client::{temporary_pub::*, TpuClient as BackendTpuClient},
    std::sync::Arc,
};

/// Client which sends transactions directly to the current leader's TPU port over UDP.
/// The client uses RPC to determine the current leader and fetch node contact info
pub struct TpuClient {
    tpu_client: BackendTpuClient,
}

impl TpuClient {
    /// Serialize and send transaction to the current and upcoming leader TPUs according to fanout
    /// size
    pub async fn send_transaction(&self, transaction: &Transaction) -> bool {
        self.tpu_client.send_transaction(transaction).await
    }

    /// Send a wire transaction to the current and upcoming leader TPUs according to fanout size
    pub async fn send_wire_transaction(&self, wire_transaction: Vec<u8>) -> bool {
        self.tpu_client
            .send_wire_transaction(wire_transaction)
            .await
    }

    /// Serialize and send transaction to the current and upcoming leader TPUs according to fanout
    /// size
    /// Returns the last error if all sends fail
    pub async fn try_send_transaction(&self, transaction: &Transaction) -> TransportResult<()> {
        self.tpu_client.try_send_transaction(transaction).await
    }

    /// Send a wire transaction to the current and upcoming leader TPUs according to fanout size
    /// Returns the last error if all sends fail
    pub async fn try_send_wire_transaction(
        &self,
        wire_transaction: Vec<u8>,
    ) -> TransportResult<()> {
        self.tpu_client
            .try_send_wire_transaction(wire_transaction)
            .await
    }

    /// Send a batch of wire transactions to the current and upcoming leader TPUs according to
    /// fanout size
    /// Returns the last error if all sends fail
    pub async fn try_send_wire_transaction_batch(
        &self,
        wire_transactions: Vec<Vec<u8>>,
    ) -> TransportResult<()> {
        self.tpu_client
            .try_send_wire_transaction_batch(wire_transactions)
            .await
    }

    /// Create a new client that disconnects when dropped
    pub async fn new(
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        config: TpuClientConfig,
    ) -> Result<Self> {
        let connection_cache = Arc::new(ConnectionCache::default().into());
        Self::new_with_connection_cache(rpc_client, websocket_url, config, connection_cache).await
    }

    /// Create a new client that disconnects when dropped
    pub async fn new_with_connection_cache(
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        config: TpuClientConfig,
        connection_cache: Arc<BackendConnectionCache>,
    ) -> Result<Self> {
        Ok(Self {
            tpu_client: BackendTpuClient::new_with_connection_cache(
                rpc_client,
                websocket_url,
                config,
                connection_cache,
            )
            .await?,
        })
    }

    pub async fn send_and_confirm_messages_with_spinner<T: Signers>(
        &self,
        messages: &[Message],
        signers: &T,
    ) -> Result<Vec<Option<TransactionError>>> {
        self.tpu_client
            .send_and_confirm_messages_with_spinner(messages, signers)
            .await
    }

    pub fn rpc_client(&self) -> &RpcClient {
        self.tpu_client.rpc_client()
    }

    pub async fn shutdown(&mut self) {
        self.tpu_client.shutdown().await
    }
}
