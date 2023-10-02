pub use crate::tpu_client::Result;
use {
    crate::tpu_client::{RecentLeaderSlots, TpuClientConfig, MAX_FANOUT_SLOTS},
    bincode::serialize,
    futures_util::{
        future::{join_all, FutureExt, TryFutureExt},
        stream::StreamExt,
    },
    log::*,
    solana_connection_cache::{
        connection_cache::{
            ConnectionCache, ConnectionManager, ConnectionPool, NewConnectionConfig, Protocol,
            DEFAULT_CONNECTION_POOL_SIZE,
        },
        nonblocking::client_connection::ClientConnection,
    },
    solana_pubsub_client::nonblocking::pubsub_client::{PubsubClient, PubsubClientError},
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    solana_rpc_client_api::{
        client_error::{Error as ClientError, Result as ClientResult},
        response::{RpcContactInfo, SlotUpdate},
    },
    solana_sdk::{
        clock::Slot,
        commitment_config::CommitmentConfig,
        epoch_info::EpochInfo,
        pubkey::Pubkey,
        quic::QUIC_PORT_OFFSET,
        signature::SignerError,
        transaction::Transaction,
        transport::{Result as TransportResult, TransportError},
    },
    std::{
        collections::{HashMap, HashSet},
        future::Future,
        iter,
        net::SocketAddr,
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
    thiserror::Error,
    tokio::{
        task::JoinHandle,
        time::{sleep, timeout, Duration, Instant},
    },
};
#[cfg(feature = "spinner")]
use {
    crate::tpu_client::{SEND_TRANSACTION_INTERVAL, TRANSACTION_RESEND_INTERVAL},
    indicatif::ProgressBar,
    solana_rpc_client::spinner::{self, SendTransactionProgress},
    solana_rpc_client_api::request::MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS,
    solana_sdk::{message::Message, signers::Signers, transaction::TransactionError},
};

#[derive(Error, Debug)]
pub enum TpuSenderError {
    #[error("Pubsub error: {0:?}")]
    PubsubError(#[from] PubsubClientError),
    #[error("RPC error: {0:?}")]
    RpcError(#[from] ClientError),
    #[error("IO error: {0:?}")]
    IoError(#[from] std::io::Error),
    #[error("Signer error: {0:?}")]
    SignerError(#[from] SignerError),
    #[error("Custom error: {0}")]
    Custom(String),
}

struct LeaderTpuCacheUpdateInfo {
    pub(super) maybe_cluster_nodes: Option<ClientResult<Vec<RpcContactInfo>>>,
    pub(super) maybe_epoch_info: Option<ClientResult<EpochInfo>>,
    pub(super) maybe_slot_leaders: Option<ClientResult<Vec<Pubkey>>>,
}
impl LeaderTpuCacheUpdateInfo {
    pub fn has_some(&self) -> bool {
        self.maybe_cluster_nodes.is_some()
            || self.maybe_epoch_info.is_some()
            || self.maybe_slot_leaders.is_some()
    }
}

struct LeaderTpuCache {
    protocol: Protocol,
    first_slot: Slot,
    leaders: Vec<Pubkey>,
    leader_tpu_map: HashMap<Pubkey, SocketAddr>,
    slots_in_epoch: Slot,
    last_epoch_info_slot: Slot,
}

impl LeaderTpuCache {
    pub fn new(
        first_slot: Slot,
        slots_in_epoch: Slot,
        leaders: Vec<Pubkey>,
        cluster_nodes: Vec<RpcContactInfo>,
        protocol: Protocol,
    ) -> Self {
        let leader_tpu_map = Self::extract_cluster_tpu_sockets(protocol, cluster_nodes);
        Self {
            protocol,
            first_slot,
            leaders,
            leader_tpu_map,
            slots_in_epoch,
            last_epoch_info_slot: first_slot,
        }
    }

    // Last slot that has a cached leader pubkey
    pub fn last_slot(&self) -> Slot {
        self.first_slot + self.leaders.len().saturating_sub(1) as u64
    }

    pub fn slot_info(&self) -> (Slot, Slot, Slot) {
        (
            self.last_slot(),
            self.last_epoch_info_slot,
            self.slots_in_epoch,
        )
    }

    // Get the TPU sockets for the current leader and upcoming leaders according to fanout size
    fn get_leader_sockets(
        &self,
        estimated_current_slot: Slot,
        fanout_slots: u64,
    ) -> Vec<SocketAddr> {
        let mut leader_set = HashSet::new();
        let mut leader_sockets = Vec::new();
        // `first_slot` might have been advanced since caller last read the `estimated_current_slot`
        // value. Take the greater of the two values to ensure we are reading from the latest
        // leader schedule.
        let current_slot = std::cmp::max(estimated_current_slot, self.first_slot);
        for leader_slot in current_slot..current_slot + fanout_slots {
            if let Some(leader) = self.get_slot_leader(leader_slot) {
                if let Some(tpu_socket) = self.leader_tpu_map.get(leader) {
                    if leader_set.insert(*leader) {
                        leader_sockets.push(*tpu_socket);
                    }
                } else {
                    // The leader is probably delinquent
                    trace!("TPU not available for leader {}", leader);
                }
            } else {
                // Overran the local leader schedule cache
                warn!(
                    "Leader not known for slot {}; cache holds slots [{},{}]",
                    leader_slot,
                    self.first_slot,
                    self.last_slot()
                );
            }
        }
        leader_sockets
    }

    pub fn get_slot_leader(&self, slot: Slot) -> Option<&Pubkey> {
        if slot >= self.first_slot {
            let index = slot - self.first_slot;
            self.leaders.get(index as usize)
        } else {
            None
        }
    }

    fn extract_cluster_tpu_sockets(
        protocol: Protocol,
        cluster_contact_info: Vec<RpcContactInfo>,
    ) -> HashMap<Pubkey, SocketAddr> {
        cluster_contact_info
            .into_iter()
            .filter_map(|contact_info| {
                let pubkey = Pubkey::from_str(&contact_info.pubkey).ok()?;
                let socket = match protocol {
                    Protocol::QUIC => contact_info.tpu_quic.or_else(|| {
                        let mut socket = contact_info.tpu?;
                        let port = socket.port().checked_add(QUIC_PORT_OFFSET)?;
                        socket.set_port(port);
                        Some(socket)
                    }),
                    Protocol::UDP => contact_info.tpu,
                }?;
                Some((pubkey, socket))
            })
            .collect()
    }

    pub fn fanout(slots_in_epoch: Slot) -> Slot {
        (2 * MAX_FANOUT_SLOTS).min(slots_in_epoch)
    }

    pub fn update_all(
        &mut self,
        estimated_current_slot: Slot,
        cache_update_info: LeaderTpuCacheUpdateInfo,
    ) -> (bool, bool) {
        let mut has_error = false;
        let mut cluster_refreshed = false;
        if let Some(cluster_nodes) = cache_update_info.maybe_cluster_nodes {
            match cluster_nodes {
                Ok(cluster_nodes) => {
                    self.leader_tpu_map =
                        Self::extract_cluster_tpu_sockets(self.protocol, cluster_nodes);
                    cluster_refreshed = true;
                }
                Err(err) => {
                    warn!("Failed to fetch cluster tpu sockets: {}", err);
                    has_error = true;
                }
            }
        }

        if let Some(Ok(epoch_info)) = cache_update_info.maybe_epoch_info {
            self.slots_in_epoch = epoch_info.slots_in_epoch;
            self.last_epoch_info_slot = estimated_current_slot;
        }

        if let Some(slot_leaders) = cache_update_info.maybe_slot_leaders {
            match slot_leaders {
                Ok(slot_leaders) => {
                    self.first_slot = estimated_current_slot;
                    self.leaders = slot_leaders;
                }
                Err(err) => {
                    warn!(
                        "Failed to fetch slot leaders (current estimated slot: {}): {}",
                        estimated_current_slot, err
                    );
                    has_error = true;
                }
            }
        }
        (has_error, cluster_refreshed)
    }
}

/// Client which sends transactions directly to the current leader's TPU port over UDP.
/// The client uses RPC to determine the current leader and fetch node contact info
pub struct TpuClient<
    P, // ConnectionPool
    M, // ConnectionManager
    C, // NewConnectionConfig
> {
    fanout_slots: u64,
    leader_tpu_service: LeaderTpuService,
    exit: Arc<AtomicBool>,
    rpc_client: Arc<RpcClient>,
    connection_cache: Arc<ConnectionCache<P, M, C>>,
}

/// Helper function which generates futures to all be awaited together for maximum
/// throughput
#[cfg(feature = "spinner")]
fn send_wire_transaction_futures<'a, P, M, C>(
    progress_bar: &'a ProgressBar,
    progress: &'a SendTransactionProgress,
    index: usize,
    num_transactions: usize,
    wire_transaction: Vec<u8>,
    leaders: Vec<SocketAddr>,
    connection_cache: &'a ConnectionCache<P, M, C>,
) -> Vec<impl Future<Output = TransportResult<()>> + 'a>
where
    P: ConnectionPool<NewConnectionConfig = C>,
    M: ConnectionManager<ConnectionPool = P, NewConnectionConfig = C>,
    C: NewConnectionConfig,
{
    const SEND_TIMEOUT_INTERVAL: Duration = Duration::from_secs(5);
    let sleep_duration = SEND_TRANSACTION_INTERVAL.saturating_mul(index as u32);
    let send_timeout = SEND_TIMEOUT_INTERVAL.saturating_add(sleep_duration);
    leaders
        .into_iter()
        .map(|addr| {
            timeout_future(
                send_timeout,
                sleep_and_send_wire_transaction_to_addr(
                    sleep_duration,
                    connection_cache,
                    addr,
                    wire_transaction.clone(),
                ),
            )
            .boxed_local() // required to make types work simply
        })
        .chain(iter::once(
            timeout_future(
                send_timeout,
                sleep_and_set_message(
                    sleep_duration,
                    progress_bar,
                    progress,
                    index,
                    num_transactions,
                ),
            )
            .boxed_local(), // required to make types work simply
        ))
        .collect::<Vec<_>>()
}

// Wrap an existing future with a timeout.
//
// Useful for end-users who don't need a persistent connection to each validator,
// and want to abort more quickly.
fn timeout_future<'a, Fut: Future<Output = TransportResult<()>> + 'a>(
    timeout_duration: Duration,
    future: Fut,
) -> impl Future<Output = TransportResult<()>> + 'a {
    timeout(timeout_duration, future)
        .unwrap_or_else(|_| Err(TransportError::Custom("Timed out".to_string())))
}

#[cfg(feature = "spinner")]
async fn sleep_and_set_message(
    sleep_duration: Duration,
    progress_bar: &ProgressBar,
    progress: &SendTransactionProgress,
    index: usize,
    num_transactions: usize,
) -> TransportResult<()> {
    sleep(sleep_duration).await;
    progress.set_message_for_confirmed_transactions(
        progress_bar,
        &format!("Sending {}/{} transactions", index + 1, num_transactions,),
    );
    Ok(())
}

async fn sleep_and_send_wire_transaction_to_addr<P, M, C>(
    sleep_duration: Duration,
    connection_cache: &ConnectionCache<P, M, C>,
    addr: SocketAddr,
    wire_transaction: Vec<u8>,
) -> TransportResult<()>
where
    P: ConnectionPool<NewConnectionConfig = C>,
    M: ConnectionManager<ConnectionPool = P, NewConnectionConfig = C>,
    C: NewConnectionConfig,
{
    sleep(sleep_duration).await;
    send_wire_transaction_to_addr(connection_cache, &addr, wire_transaction).await
}

async fn send_wire_transaction_to_addr<P, M, C>(
    connection_cache: &ConnectionCache<P, M, C>,
    addr: &SocketAddr,
    wire_transaction: Vec<u8>,
) -> TransportResult<()>
where
    P: ConnectionPool<NewConnectionConfig = C>,
    M: ConnectionManager<ConnectionPool = P, NewConnectionConfig = C>,
    C: NewConnectionConfig,
{
    let conn = connection_cache.get_nonblocking_connection(addr);
    conn.send_data(&wire_transaction).await
}

async fn send_wire_transaction_batch_to_addr<P, M, C>(
    connection_cache: &ConnectionCache<P, M, C>,
    addr: &SocketAddr,
    wire_transactions: &[Vec<u8>],
) -> TransportResult<()>
where
    P: ConnectionPool<NewConnectionConfig = C>,
    M: ConnectionManager<ConnectionPool = P, NewConnectionConfig = C>,
    C: NewConnectionConfig,
{
    let conn = connection_cache.get_nonblocking_connection(addr);
    conn.send_data_batch(wire_transactions).await
}

impl<P, M, C> TpuClient<P, M, C>
where
    P: ConnectionPool<NewConnectionConfig = C>,
    M: ConnectionManager<ConnectionPool = P, NewConnectionConfig = C>,
    C: NewConnectionConfig,
{
    /// Serialize and send transaction to the current and upcoming leader TPUs according to fanout
    /// size
    pub async fn send_transaction(&self, transaction: &Transaction) -> bool {
        let wire_transaction = serialize(transaction).expect("serialization should succeed");
        self.send_wire_transaction(wire_transaction).await
    }

    /// Send a wire transaction to the current and upcoming leader TPUs according to fanout size
    pub async fn send_wire_transaction(&self, wire_transaction: Vec<u8>) -> bool {
        self.try_send_wire_transaction(wire_transaction)
            .await
            .is_ok()
    }

    /// Serialize and send transaction to the current and upcoming leader TPUs according to fanout
    /// size
    /// Returns the last error if all sends fail
    pub async fn try_send_transaction(&self, transaction: &Transaction) -> TransportResult<()> {
        let wire_transaction = serialize(transaction).expect("serialization should succeed");
        self.try_send_wire_transaction(wire_transaction).await
    }

    /// Send a wire transaction to the current and upcoming leader TPUs according to fanout size
    /// Returns the last error if all sends fail
    pub async fn try_send_wire_transaction(
        &self,
        wire_transaction: Vec<u8>,
    ) -> TransportResult<()> {
        let leaders = self
            .leader_tpu_service
            .leader_tpu_sockets(self.fanout_slots);
        let futures = leaders
            .iter()
            .map(|addr| {
                send_wire_transaction_to_addr(
                    &self.connection_cache,
                    addr,
                    wire_transaction.clone(),
                )
            })
            .collect::<Vec<_>>();
        let results: Vec<TransportResult<()>> = join_all(futures).await;

        let mut last_error: Option<TransportError> = None;
        let mut some_success = false;
        for result in results {
            if let Err(e) = result {
                if last_error.is_none() {
                    last_error = Some(e);
                }
            } else {
                some_success = true;
            }
        }
        if !some_success {
            Err(if let Some(err) = last_error {
                err
            } else {
                std::io::Error::new(std::io::ErrorKind::Other, "No sends attempted").into()
            })
        } else {
            Ok(())
        }
    }

    /// Send a batch of wire transactions to the current and upcoming leader TPUs according to
    /// fanout size
    /// Returns the last error if all sends fail
    pub async fn try_send_wire_transaction_batch(
        &self,
        wire_transactions: Vec<Vec<u8>>,
    ) -> TransportResult<()> {
        let leaders = self
            .leader_tpu_service
            .leader_tpu_sockets(self.fanout_slots);
        let futures = leaders
            .iter()
            .map(|addr| {
                send_wire_transaction_batch_to_addr(
                    &self.connection_cache,
                    addr,
                    &wire_transactions,
                )
            })
            .collect::<Vec<_>>();
        let results: Vec<TransportResult<()>> = join_all(futures).await;

        let mut last_error: Option<TransportError> = None;
        let mut some_success = false;
        for result in results {
            if let Err(e) = result {
                if last_error.is_none() {
                    last_error = Some(e);
                }
            } else {
                some_success = true;
            }
        }
        if !some_success {
            Err(if let Some(err) = last_error {
                err
            } else {
                std::io::Error::new(std::io::ErrorKind::Other, "No sends attempted").into()
            })
        } else {
            Ok(())
        }
    }

    /// Create a new client that disconnects when dropped
    pub async fn new(
        name: &'static str,
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        config: TpuClientConfig,
        connection_manager: M,
    ) -> Result<Self> {
        let connection_cache = Arc::new(
            ConnectionCache::new(name, connection_manager, DEFAULT_CONNECTION_POOL_SIZE).unwrap(),
        ); // TODO: Handle error properly, as the ConnectionCache ctor is now fallible.
        Self::new_with_connection_cache(rpc_client, websocket_url, config, connection_cache).await
    }

    /// Create a new client that disconnects when dropped
    pub async fn new_with_connection_cache(
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        config: TpuClientConfig,
        connection_cache: Arc<ConnectionCache<P, M, C>>,
    ) -> Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));
        let leader_tpu_service =
            LeaderTpuService::new(rpc_client.clone(), websocket_url, M::PROTOCOL, exit.clone())
                .await?;

        Ok(Self {
            fanout_slots: config.fanout_slots.clamp(1, MAX_FANOUT_SLOTS),
            leader_tpu_service,
            exit,
            rpc_client,
            connection_cache,
        })
    }

    #[cfg(feature = "spinner")]
    pub async fn send_and_confirm_messages_with_spinner<T: Signers + ?Sized>(
        &self,
        messages: &[Message],
        signers: &T,
    ) -> Result<Vec<Option<TransactionError>>> {
        let mut progress = SendTransactionProgress::default();
        let progress_bar = spinner::new_progress_bar();
        progress_bar.set_message("Setting up...");

        let mut transactions = messages
            .iter()
            .enumerate()
            .map(|(i, message)| (i, Transaction::new_unsigned(message.clone())))
            .collect::<Vec<_>>();
        progress.total_transactions = transactions.len();
        let mut transaction_errors = vec![None; transactions.len()];
        progress.block_height = self.rpc_client.get_block_height().await?;
        for expired_blockhash_retries in (0..5).rev() {
            let (blockhash, last_valid_block_height) = self
                .rpc_client
                .get_latest_blockhash_with_commitment(self.rpc_client.commitment())
                .await?;
            progress.last_valid_block_height = last_valid_block_height;

            let mut pending_transactions = HashMap::new();
            for (i, mut transaction) in transactions {
                transaction.try_sign(signers, blockhash)?;
                pending_transactions.insert(transaction.signatures[0], (i, transaction));
            }

            let mut last_resend = Instant::now() - TRANSACTION_RESEND_INTERVAL;
            while progress.block_height <= progress.last_valid_block_height {
                let num_transactions = pending_transactions.len();

                // Periodically re-send all pending transactions
                if Instant::now().duration_since(last_resend) > TRANSACTION_RESEND_INTERVAL {
                    // Prepare futures for all transactions
                    let mut futures = vec![];
                    for (index, (_i, transaction)) in pending_transactions.values().enumerate() {
                        let wire_transaction = serialize(transaction).unwrap();
                        let leaders = self
                            .leader_tpu_service
                            .leader_tpu_sockets(self.fanout_slots);
                        futures.extend(send_wire_transaction_futures(
                            &progress_bar,
                            &progress,
                            index,
                            num_transactions,
                            wire_transaction,
                            leaders,
                            &self.connection_cache,
                        ));
                    }

                    // Start the process of sending them all
                    let results = join_all(futures).await;

                    progress.set_message_for_confirmed_transactions(
                        &progress_bar,
                        "Checking sent transactions",
                    );
                    for (index, (tx_results, (_i, transaction))) in results
                        .chunks(self.fanout_slots as usize)
                        .zip(pending_transactions.values())
                        .enumerate()
                    {
                        // Only report an error if every future in the chunk errored
                        if tx_results.iter().all(|r| r.is_err()) {
                            progress.set_message_for_confirmed_transactions(
                                &progress_bar,
                                &format!(
                                    "Resending failed transaction {} of {}",
                                    index + 1,
                                    num_transactions,
                                ),
                            );
                            let _result = self.rpc_client.send_transaction(transaction).await.ok();
                        }
                    }
                    last_resend = Instant::now();
                }

                // Wait for the next block before checking for transaction statuses
                let mut block_height_refreshes = 10;
                progress.set_message_for_confirmed_transactions(
                    &progress_bar,
                    &format!("Waiting for next block, {num_transactions} transactions pending..."),
                );
                let mut new_block_height = progress.block_height;
                while progress.block_height == new_block_height && block_height_refreshes > 0 {
                    sleep(Duration::from_millis(500)).await;
                    new_block_height = self.rpc_client.get_block_height().await?;
                    block_height_refreshes -= 1;
                }
                progress.block_height = new_block_height;

                // Collect statuses for the transactions, drop those that are confirmed
                let pending_signatures = pending_transactions.keys().cloned().collect::<Vec<_>>();
                for pending_signatures_chunk in
                    pending_signatures.chunks(MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS)
                {
                    if let Ok(result) = self
                        .rpc_client
                        .get_signature_statuses(pending_signatures_chunk)
                        .await
                    {
                        let statuses = result.value;
                        for (signature, status) in
                            pending_signatures_chunk.iter().zip(statuses.into_iter())
                        {
                            if let Some(status) = status {
                                if status.satisfies_commitment(self.rpc_client.commitment()) {
                                    if let Some((i, _)) = pending_transactions.remove(signature) {
                                        progress.confirmed_transactions += 1;
                                        if status.err.is_some() {
                                            progress_bar
                                                .println(format!("Failed transaction: {status:?}"));
                                        }
                                        transaction_errors[i] = status.err;
                                    }
                                }
                            }
                        }
                    }
                    progress.set_message_for_confirmed_transactions(
                        &progress_bar,
                        "Checking transaction status...",
                    );
                }

                if pending_transactions.is_empty() {
                    return Ok(transaction_errors);
                }
            }

            transactions = pending_transactions.into_values().collect();
            progress_bar.println(format!(
                "Blockhash expired. {expired_blockhash_retries} retries remaining"
            ));
        }
        Err(TpuSenderError::Custom("Max retries exceeded".into()))
    }

    pub fn rpc_client(&self) -> &RpcClient {
        &self.rpc_client
    }

    pub async fn shutdown(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
        self.leader_tpu_service.join().await;
    }
}

impl<P, M, C> Drop for TpuClient<P, M, C> {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
    }
}

/// Service that tracks upcoming leaders and maintains an up-to-date mapping
/// of leader id to TPU socket address.
pub struct LeaderTpuService {
    recent_slots: RecentLeaderSlots,
    leader_tpu_cache: Arc<RwLock<LeaderTpuCache>>,
    t_leader_tpu_service: Option<JoinHandle<Result<()>>>,
}

impl LeaderTpuService {
    pub async fn new(
        rpc_client: Arc<RpcClient>,
        websocket_url: &str,
        protocol: Protocol,
        exit: Arc<AtomicBool>,
    ) -> Result<Self> {
        let start_slot = rpc_client
            .get_slot_with_commitment(CommitmentConfig::processed())
            .await?;

        let recent_slots = RecentLeaderSlots::new(start_slot);
        let slots_in_epoch = rpc_client.get_epoch_info().await?.slots_in_epoch;
        let leaders = rpc_client
            .get_slot_leaders(start_slot, LeaderTpuCache::fanout(slots_in_epoch))
            .await?;
        let cluster_nodes = rpc_client.get_cluster_nodes().await?;
        let leader_tpu_cache = Arc::new(RwLock::new(LeaderTpuCache::new(
            start_slot,
            slots_in_epoch,
            leaders,
            cluster_nodes,
            protocol,
        )));

        let pubsub_client = if !websocket_url.is_empty() {
            Some(PubsubClient::new(websocket_url).await?)
        } else {
            None
        };

        let t_leader_tpu_service = Some({
            let recent_slots = recent_slots.clone();
            let leader_tpu_cache = leader_tpu_cache.clone();
            tokio::spawn(Self::run(
                rpc_client,
                recent_slots,
                leader_tpu_cache,
                pubsub_client,
                exit,
            ))
        });

        Ok(LeaderTpuService {
            recent_slots,
            leader_tpu_cache,
            t_leader_tpu_service,
        })
    }

    pub async fn join(&mut self) {
        if let Some(t_handle) = self.t_leader_tpu_service.take() {
            t_handle.await.unwrap().unwrap();
        }
    }

    pub fn estimated_current_slot(&self) -> Slot {
        self.recent_slots.estimated_current_slot()
    }

    fn leader_tpu_sockets(&self, fanout_slots: u64) -> Vec<SocketAddr> {
        let current_slot = self.recent_slots.estimated_current_slot();
        self.leader_tpu_cache
            .read()
            .unwrap()
            .get_leader_sockets(current_slot, fanout_slots)
    }

    async fn run(
        rpc_client: Arc<RpcClient>,
        recent_slots: RecentLeaderSlots,
        leader_tpu_cache: Arc<RwLock<LeaderTpuCache>>,
        pubsub_client: Option<PubsubClient>,
        exit: Arc<AtomicBool>,
    ) -> Result<()> {
        let (mut notifications, unsubscribe) = if let Some(pubsub_client) = &pubsub_client {
            let (notifications, unsubscribe) = pubsub_client.slot_updates_subscribe().await?;
            (Some(notifications), Some(unsubscribe))
        } else {
            (None, None)
        };
        let mut last_cluster_refresh = Instant::now();
        let mut sleep_ms = 1000;
        loop {
            if exit.load(Ordering::Relaxed) {
                if let Some(unsubscribe) = unsubscribe {
                    (unsubscribe)().await;
                }
                // `notifications` requires a valid reference to `pubsub_client`
                // so `notifications` must be dropped before moving `pubsub_client`
                drop(notifications);
                if let Some(pubsub_client) = pubsub_client {
                    pubsub_client.shutdown().await.unwrap();
                };
                break;
            }

            // Sleep a slot before checking if leader cache needs to be refreshed again
            sleep(Duration::from_millis(sleep_ms)).await;
            sleep_ms = 1000;

            if let Some(notifications) = &mut notifications {
                while let Ok(Some(update)) =
                    timeout(Duration::from_millis(10), notifications.next()).await
                {
                    let current_slot = match update {
                        // This update indicates that a full slot was received by the connected
                        // node so we can stop sending transactions to the leader for that slot
                        SlotUpdate::Completed { slot, .. } => slot.saturating_add(1),
                        // This update indicates that we have just received the first shred from
                        // the leader for this slot and they are probably still accepting transactions.
                        SlotUpdate::FirstShredReceived { slot, .. } => slot,
                        _ => continue,
                    };
                    recent_slots.record_slot(current_slot);
                }
            }

            let cache_update_info = maybe_fetch_cache_info(
                &leader_tpu_cache,
                last_cluster_refresh,
                &rpc_client,
                &recent_slots,
            )
            .await;

            if cache_update_info.has_some() {
                let mut leader_tpu_cache = leader_tpu_cache.write().unwrap();
                let (has_error, cluster_refreshed) = leader_tpu_cache
                    .update_all(recent_slots.estimated_current_slot(), cache_update_info);
                if has_error {
                    sleep_ms = 100;
                }
                if cluster_refreshed {
                    last_cluster_refresh = Instant::now();
                }
            }
        }
        Ok(())
    }
}

async fn maybe_fetch_cache_info(
    leader_tpu_cache: &Arc<RwLock<LeaderTpuCache>>,
    last_cluster_refresh: Instant,
    rpc_client: &RpcClient,
    recent_slots: &RecentLeaderSlots,
) -> LeaderTpuCacheUpdateInfo {
    // Refresh cluster TPU ports every 5min in case validators restart with new port configuration
    // or new validators come online
    let maybe_cluster_nodes = if last_cluster_refresh.elapsed() > Duration::from_secs(5 * 60) {
        Some(rpc_client.get_cluster_nodes().await)
    } else {
        None
    };

    let estimated_current_slot = recent_slots.estimated_current_slot();
    let (last_slot, last_epoch_info_slot, slots_in_epoch) = {
        let leader_tpu_cache = leader_tpu_cache.read().unwrap();
        leader_tpu_cache.slot_info()
    };
    let maybe_epoch_info =
        if estimated_current_slot >= last_epoch_info_slot.saturating_sub(slots_in_epoch) {
            Some(rpc_client.get_epoch_info().await)
        } else {
            None
        };

    let maybe_slot_leaders = if estimated_current_slot >= last_slot.saturating_sub(MAX_FANOUT_SLOTS)
    {
        Some(
            rpc_client
                .get_slot_leaders(
                    estimated_current_slot,
                    LeaderTpuCache::fanout(slots_in_epoch),
                )
                .await,
        )
    } else {
        None
    };
    LeaderTpuCacheUpdateInfo {
        maybe_cluster_nodes,
        maybe_epoch_info,
        maybe_slot_leaders,
    }
}
