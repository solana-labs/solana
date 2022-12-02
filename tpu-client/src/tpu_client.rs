pub use crate::nonblocking::tpu_client::TpuSenderError;
use {
    crate::{
        nonblocking::tpu_client::TpuClient as NonblockingTpuClient,
        tpu_connection_cache::{ConnectionPool, TpuConnectionCache},
    },
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{clock::Slot, transaction::Transaction, transport::Result as TransportResult},
    std::{
        collections::VecDeque,
        net::UdpSocket,
        sync::{Arc, RwLock},
    },
};
#[cfg(feature = "spinner")]
use {
    solana_sdk::{message::Message, signers::Signers, transaction::TransactionError},
    tokio::time::Duration,
};

pub mod temporary_pub {
    use super::*;

    pub type Result<T> = std::result::Result<T, TpuSenderError>;

    /// Send at ~100 TPS
    #[cfg(feature = "spinner")]
    pub const SEND_TRANSACTION_INTERVAL: Duration = Duration::from_millis(10);
    /// Retry batch send after 4 seconds
    #[cfg(feature = "spinner")]
    pub const TRANSACTION_RESEND_INTERVAL: Duration = Duration::from_secs(4);
}
use temporary_pub::*;

/// Default number of slots used to build TPU socket fanout set
pub const DEFAULT_FANOUT_SLOTS: u64 = 12;

/// Maximum number of slots used to build TPU socket fanout set
pub const MAX_FANOUT_SLOTS: u64 = 100;

/// Config params for `TpuClient`
#[derive(Clone, Debug)]
pub struct TpuClientConfig {
    /// The range of upcoming slots to include when determining which
    /// leaders to send transactions to (min: 1, max: `MAX_FANOUT_SLOTS`)
    pub fanout_slots: u64,
}

impl Default for TpuClientConfig {
    fn default() -> Self {
        Self {
            fanout_slots: DEFAULT_FANOUT_SLOTS,
        }
    }
}

/// Client which sends transactions directly to the current leader's TPU port over UDP.
/// The client uses RPC to determine the current leader and fetch node contact info
pub struct TpuClient<P: ConnectionPool> {
    _deprecated: UdpSocket, // TpuClient now uses the connection_cache to choose a send_socket
    //todo: get rid of this field
    rpc_client: Arc<RpcClient>,
    tpu_client: Arc<NonblockingTpuClient<P>>,
}

impl<P: ConnectionPool> TpuClient<P> {
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
        connection_cache: Arc<TpuConnectionCache<P>>,
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

    #[cfg(feature = "spinner")]
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

// 48 chosen because it's unlikely that 12 leaders in a row will miss their slots
const MAX_SLOT_SKIP_DISTANCE: u64 = 48;

#[derive(Clone, Debug)]
pub(crate) struct RecentLeaderSlots(Arc<RwLock<VecDeque<Slot>>>);
impl RecentLeaderSlots {
    pub(crate) fn new(current_slot: Slot) -> Self {
        let mut recent_slots = VecDeque::new();
        recent_slots.push_back(current_slot);
        Self(Arc::new(RwLock::new(recent_slots)))
    }

    pub(crate) fn record_slot(&self, current_slot: Slot) {
        let mut recent_slots = self.0.write().unwrap();
        recent_slots.push_back(current_slot);
        // 12 recent slots should be large enough to avoid a misbehaving
        // validator from affecting the median recent slot
        while recent_slots.len() > 12 {
            recent_slots.pop_front();
        }
    }

    // Estimate the current slot from recent slot notifications.
    pub(crate) fn estimated_current_slot(&self) -> Slot {
        let mut recent_slots: Vec<Slot> = self.0.read().unwrap().iter().cloned().collect();
        assert!(!recent_slots.is_empty());
        recent_slots.sort_unstable();

        // Validators can broadcast invalid blocks that are far in the future
        // so check if the current slot is in line with the recent progression.
        let max_index = recent_slots.len() - 1;
        let median_index = max_index / 2;
        let median_recent_slot = recent_slots[median_index];
        let expected_current_slot = median_recent_slot + (max_index - median_index) as u64;
        let max_reasonable_current_slot = expected_current_slot + MAX_SLOT_SKIP_DISTANCE;

        // Return the highest slot that doesn't exceed what we believe is a
        // reasonable slot.
        recent_slots
            .into_iter()
            .rev()
            .find(|slot| *slot <= max_reasonable_current_slot)
            .unwrap()
    }
}

#[cfg(test)]
impl From<Vec<Slot>> for RecentLeaderSlots {
    fn from(recent_slots: Vec<Slot>) -> Self {
        assert!(!recent_slots.is_empty());
        Self(Arc::new(RwLock::new(recent_slots.into_iter().collect())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_slot(recent_slots: RecentLeaderSlots, expected_slot: Slot) {
        assert_eq!(recent_slots.estimated_current_slot(), expected_slot);
    }

    #[test]
    fn test_recent_leader_slots() {
        assert_slot(RecentLeaderSlots::new(0), 0);

        let mut recent_slots: Vec<Slot> = (1..=12).collect();
        assert_slot(RecentLeaderSlots::from(recent_slots.clone()), 12);

        recent_slots.reverse();
        assert_slot(RecentLeaderSlots::from(recent_slots), 12);

        assert_slot(
            RecentLeaderSlots::from(vec![0, 1 + MAX_SLOT_SKIP_DISTANCE]),
            1 + MAX_SLOT_SKIP_DISTANCE,
        );
        assert_slot(
            RecentLeaderSlots::from(vec![0, 2 + MAX_SLOT_SKIP_DISTANCE]),
            0,
        );

        assert_slot(RecentLeaderSlots::from(vec![1]), 1);
        assert_slot(RecentLeaderSlots::from(vec![1, 100]), 1);
        assert_slot(RecentLeaderSlots::from(vec![1, 2, 100]), 2);
        assert_slot(RecentLeaderSlots::from(vec![1, 2, 3, 100]), 3);
        assert_slot(RecentLeaderSlots::from(vec![1, 2, 3, 99, 100]), 3);
    }
}
