pub use {
    crate::nonblocking::tpu_client::TpuSenderError,
    solana_tpu_client::tpu_client::{TpuClientConfig, DEFAULT_FANOUT_SLOTS, MAX_FANOUT_SLOTS},
};
use {
    crate::{
        connection_cache::ConnectionCache,
        nonblocking::tpu_client::TpuClient as NonblockingTpuClient,
    },
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        message::Message,
        signers::Signers,
        transaction::{Transaction, TransactionError},
        transport::Result as TransportResult,
    },
    solana_tpu_client::tpu_client::temporary_pub::Result,
    std::{net::UdpSocket, sync::Arc},
};

/// Client which sends transactions directly to the current leader's TPU port over UDP.
/// The client uses RPC to determine the current leader and fetch node contact info
pub struct TpuClient {
    _deprecated: UdpSocket, // TpuClient now uses the connection_cache to choose a send_socket
    //todo: get rid of this field
    rpc_client: Arc<RpcClient>,
    tpu_client: Arc<NonblockingTpuClient>,
}

impl TpuClient {
    /// Serialize and send transaction to the current and upcoming leader TPUs according to fanout
    /// size
    pub fn send_transaction(&self, transaction: &Transaction) -> bool {
        self.invoke(self.tpu_client.send_transaction(transaction))
    }

    /// Send a wire transaction to the current and upcoming leader TPUs according to fanout size
    pub fn send_wire_transaction(&self, wire_transaction: Vec<u8>) -> bool {
        self.invoke(self.tpu_client.send_wire_transaction(wire_transaction))
    }

    /// Serialize and send transaction to the current and upcoming leader TPUs according to fanout
    /// size
    /// Returns the last error if all sends fail
    pub fn try_send_transaction(&self, transaction: &Transaction) -> TransportResult<()> {
        self.invoke(self.tpu_client.try_send_transaction(transaction))
    }

    /// Serialize and send a batch of transactions to the current and upcoming leader TPUs according
    /// to fanout size
    /// Returns the last error if all sends fail
    pub fn try_send_transaction_batch(&self, transactions: &[Transaction]) -> TransportResult<()> {
        let wire_transactions = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();
        self.invoke(
            self.tpu_client
                .try_send_wire_transaction_batch(wire_transactions),
        )
    }

    /// Send a wire transaction to the current and upcoming leader TPUs according to fanout size
    /// Returns the last error if all sends fail
    pub fn try_send_wire_transaction(&self, wire_transaction: Vec<u8>) -> TransportResult<()> {
        self.invoke(self.tpu_client.try_send_wire_transaction(wire_transaction))
    }

    /// Create a new client that disconnects when dropped
    pub fn new(
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        config: TpuClientConfig,
    ) -> Result<Self> {
        let create_tpu_client =
            NonblockingTpuClient::new(rpc_client.get_inner_client().clone(), websocket_url, config);
        let tpu_client =
            tokio::task::block_in_place(|| rpc_client.runtime().block_on(create_tpu_client))?;

        Ok(Self {
            _deprecated: UdpSocket::bind("0.0.0.0:0").unwrap(),
            rpc_client,
            tpu_client: Arc::new(tpu_client),
        })
    }

    /// Create a new client that disconnects when dropped
    pub fn new_with_connection_cache(
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        config: TpuClientConfig,
        connection_cache: Arc<ConnectionCache>,
    ) -> Result<Self> {
        let create_tpu_client = NonblockingTpuClient::new_with_connection_cache(
            rpc_client.get_inner_client().clone(),
            websocket_url,
            config,
            connection_cache,
        );
        let tpu_client =
            tokio::task::block_in_place(|| rpc_client.runtime().block_on(create_tpu_client))?;

        Ok(Self {
            _deprecated: UdpSocket::bind("0.0.0.0:0").unwrap(),
            rpc_client,
            tpu_client: Arc::new(tpu_client),
        })
    }

    pub fn send_and_confirm_messages_with_spinner<T: Signers>(
        &self,
        messages: &[Message],
        signers: &T,
    ) -> Result<Vec<Option<TransactionError>>> {
        self.invoke(
            self.tpu_client
                .send_and_confirm_messages_with_spinner(messages, signers),
        )
    }

    pub fn rpc_client(&self) -> &RpcClient {
        &self.rpc_client
    }

    fn invoke<T, F: std::future::Future<Output = T>>(&self, f: F) -> T {
        // `block_on()` panics if called within an asynchronous execution context. Whereas
        // `block_in_place()` only panics if called from a current_thread runtime, which is the
        // lesser evil.
        tokio::task::block_in_place(move || self.rpc_client.runtime().block_on(f))
    }
}
