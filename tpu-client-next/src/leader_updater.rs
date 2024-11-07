//! This module provides [`LeaderUpdater`] trait along with
//! `create_leader_updater` function to create an instance of this trait.
//!
//! Currently, the main purpose of [`LeaderUpdater`] is to abstract over leader
//! updates, hiding the details of how leaders are retrieved and which
//! structures are used. It contains trait implementations
//! `LeaderUpdaterService` and `PinnedLeaderUpdater`, where
//! `LeaderUpdaterService` keeps [`LeaderTpuService`] internal to this module.
//! Yet, it also allows to implement custom leader estimation.

use {
    async_trait::async_trait,
    log::*,
    solana_connection_cache::connection_cache::Protocol,
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    solana_sdk::clock::NUM_CONSECUTIVE_LEADER_SLOTS,
    solana_tpu_client::nonblocking::tpu_client::LeaderTpuService,
    std::{
        fmt,
        net::SocketAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    },
    thiserror::Error,
};

/// [`LeaderUpdater`] trait abstracts out functionality required for the
/// [`ConnectionWorkersScheduler`](crate::ConnectionWorkersScheduler) to
/// identify next leaders to send transactions to.
#[async_trait]
pub trait LeaderUpdater: Send {
    /// Returns next leaders for the next `lookahead_leaders` starting from
    /// current estimated slot.
    ///
    /// Leaders are returned per [`NUM_CONSECUTIVE_LEADER_SLOTS`] to avoid unnecessary repetition.
    ///
    /// If the current leader estimation is incorrect and transactions are sent to
    /// only one estimated leader, there is a risk of losing all the transactions,
    /// depending on the forwarding policy.
    fn next_leaders(&mut self, lookahead_leaders: usize) -> Vec<SocketAddr>;

    /// Stop [`LeaderUpdater`] and releases all associated resources.
    async fn stop(&mut self);
}

/// Error type for [`LeaderUpdater`].
#[derive(Error, PartialEq)]
pub struct LeaderUpdaterError;

impl fmt::Display for LeaderUpdaterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Leader updater encountered an error")
    }
}

impl fmt::Debug for LeaderUpdaterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LeaderUpdaterError")
    }
}

/// Creates a [`LeaderUpdater`] based on the configuration provided by the
/// caller.
///
/// If `pinned_address` is provided, it returns a `PinnedLeaderUpdater` that
/// always returns the provided address instead of checking leader schedule.
/// Otherwise, it creates a `LeaderUpdaterService` which dynamically updates the
/// leaders by connecting to the network via the [`LeaderTpuService`].
pub async fn create_leader_updater(
    rpc_client: Arc<RpcClient>,
    websocket_url: String,
    pinned_address: Option<SocketAddr>,
) -> Result<Box<dyn LeaderUpdater>, LeaderUpdaterError> {
    if let Some(pinned_address) = pinned_address {
        return Ok(Box::new(PinnedLeaderUpdater {
            address: vec![pinned_address],
        }));
    }

    let exit = Arc::new(AtomicBool::new(false));
    let leader_tpu_service =
        LeaderTpuService::new(rpc_client, &websocket_url, Protocol::QUIC, exit.clone())
            .await
            .map_err(|error| {
                error!("Failed to create a LeaderTpuService: {error}");
                LeaderUpdaterError
            })?;
    Ok(Box::new(LeaderUpdaterService {
        leader_tpu_service,
        exit,
    }))
}

/// `LeaderUpdaterService` is an implementation of the [`LeaderUpdater`] trait
/// that dynamically retrieves the current and upcoming leaders by communicating
/// with the Solana network using [`LeaderTpuService`].
struct LeaderUpdaterService {
    leader_tpu_service: LeaderTpuService,
    exit: Arc<AtomicBool>,
}

#[async_trait]
impl LeaderUpdater for LeaderUpdaterService {
    fn next_leaders(&mut self, lookahead_leaders: usize) -> Vec<SocketAddr> {
        let lookahead_slots =
            (lookahead_leaders as u64).saturating_mul(NUM_CONSECUTIVE_LEADER_SLOTS);
        self.leader_tpu_service.leader_tpu_sockets(lookahead_slots)
    }

    async fn stop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
        self.leader_tpu_service.join().await;
    }
}

/// `PinnedLeaderUpdater` is an implementation of [`LeaderUpdater`] that always
/// returns a fixed, "pinned" leader address. It is mainly used for testing.
struct PinnedLeaderUpdater {
    address: Vec<SocketAddr>,
}

#[async_trait]
impl LeaderUpdater for PinnedLeaderUpdater {
    fn next_leaders(&mut self, _lookahead_leaders: usize) -> Vec<SocketAddr> {
        self.address.clone()
    }

    async fn stop(&mut self) {}
}
