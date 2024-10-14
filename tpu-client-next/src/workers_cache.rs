//! This module defines `WorkersCache` along with aux struct `WorkerInfo`. These
//! structures provide mechanisms for caching workers, sending transaction
//! batches, and gathering send transaction statistics.

use {
    super::SendTransactionStats,
    crate::transaction_batch::TransactionBatch,
    log::*,
    lru::LruCache,
    std::{
        collections::HashMap,
        net::{IpAddr, SocketAddr},
    },
    thiserror::Error,
    tokio::{sync::mpsc, task::JoinHandle},
    tokio_util::sync::CancellationToken,
};

/// [`WorkerInfo`] holds information about a worker responsible for sending
/// transaction batches.
pub(crate) struct WorkerInfo {
    pub sender: mpsc::Sender<TransactionBatch>,
    pub handle: JoinHandle<SendTransactionStats>,
    pub cancel: CancellationToken,
}

impl WorkerInfo {
    pub fn new(
        sender: mpsc::Sender<TransactionBatch>,
        handle: JoinHandle<SendTransactionStats>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            sender,
            handle,
            cancel,
        }
    }

    async fn send_transactions(
        &self,
        txs_batch: TransactionBatch,
    ) -> Result<(), WorkersCacheError> {
        self.sender
            .send(txs_batch)
            .await
            .map_err(|_| WorkersCacheError::ReceiverDropped)?;
        Ok(())
    }

    /// Closes the worker by dropping the sender and awaiting the worker's
    /// statistics.
    async fn shutdown(self) -> Result<SendTransactionStats, WorkersCacheError> {
        self.cancel.cancel();
        drop(self.sender);
        let stats = self
            .handle
            .await
            .map_err(|_| WorkersCacheError::TaskJoinFailure)?;
        Ok(stats)
    }
}

/// [`WorkersCache`] manages and caches workers. It uses an LRU cache to store and
/// manage workers. It also tracks transaction statistics for each peer.
pub(crate) struct WorkersCache {
    workers: LruCache<SocketAddr, WorkerInfo>,
    send_stats_per_addr: HashMap<IpAddr, SendTransactionStats>,

    /// Indicates that the `WorkersCache` is been `shutdown()`, interrupting any outstanding
    /// `send_txs()` invocations.
    cancel: CancellationToken,
}

#[derive(Debug, Error, PartialEq)]
pub enum WorkersCacheError {
    /// typically happens when the client could not establish the connection.
    #[error("Work receiver has been dropped unexpectedly.")]
    ReceiverDropped,

    #[error("Task failed to join.")]
    TaskJoinFailure,

    #[error("The WorkersCache is being shutdown.")]
    ShutdownError,
}

impl WorkersCache {
    pub fn new(capacity: usize, cancel: CancellationToken) -> Self {
        Self {
            workers: LruCache::new(capacity),
            send_stats_per_addr: HashMap::new(),
            cancel,
        }
    }

    pub fn contains(&self, peer: &SocketAddr) -> bool {
        self.workers.contains(peer)
    }

    pub async fn push(&mut self, peer: SocketAddr, peer_worker: WorkerInfo) {
        // Although there might be concerns about the performance implications
        // of waiting for the worker to be closed when trying to add a new
        // worker, the idea is that these workers are almost always created in
        // advance so the latency is hidden.
        if let Some((leader, popped_worker)) = self.workers.push(peer, peer_worker) {
            self.shutdown_worker(leader, popped_worker).await;
        }
    }

    /// Sends a batch of transactions to the worker for a given peer. If the
    /// worker for the peer is disconnected or fails, it is removed from the
    /// cache.
    pub async fn send_transactions_to_address(
        &mut self,
        peer: &SocketAddr,
        txs_batch: TransactionBatch,
    ) -> Result<(), WorkersCacheError> {
        let Self {
            workers, cancel, ..
        } = self;

        let body = async move {
            let current_worker = workers.get(peer).expect(
                "Failed to fetch worker for peer {peer}.\n\
             Peer existence must be checked before this call using `contains` method.",
            );
            let send_res = current_worker.send_transactions(txs_batch).await;

            if let Err(WorkersCacheError::ReceiverDropped) = send_res {
                // Remove the worker from the cache, if the peer has disconnected.
                if let Some(current_worker) = workers.pop(peer) {
                    // To avoid obscuring the error from send, ignore a possible
                    // `TaskJoinFailure`.
                    let close_result = current_worker.shutdown().await;
                    if let Err(error) = close_result {
                        error!("Error while closing worker: {error}.");
                    }
                }
            }

            send_res
        };

        tokio::select! {
            send_res = body => send_res,
            () = cancel.cancelled() => Err(WorkersCacheError::ShutdownError),
        }
    }

    pub fn transaction_stats(&self) -> &HashMap<IpAddr, SendTransactionStats> {
        &self.send_stats_per_addr
    }

    /// Closes and removes all workers in the cache. This is typically done when
    /// shutting down the system.
    pub async fn shutdown(&mut self) {
        // Interrupt any outstanding `send_txs()` calls.
        self.cancel.cancel();

        while let Some((leader, worker)) = self.workers.pop_lru() {
            self.shutdown_worker(leader, worker).await;
        }
    }

    /// Shuts down a worker for a given peer by closing the worker and gathering
    /// its transaction statistics.
    async fn shutdown_worker(&mut self, leader: SocketAddr, worker: WorkerInfo) {
        let res = worker.shutdown().await;

        let stats = match res {
            Ok(stats) => stats,
            Err(err) => {
                debug!("Error while shutting down worker for {leader}: {err}");
                return;
            }
        };

        self.send_stats_per_addr
            .entry(leader.ip())
            .and_modify(|e| e.add(&stats))
            .or_insert(stats);
    }
}
