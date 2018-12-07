//! The `broadcast_service` broadcasts data from a leader node to validators
//!
use crate::cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo};
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::entry::Entry;
#[cfg(feature = "erasure")]
use crate::erasure;
use crate::leader_scheduler::LeaderScheduler;
use crate::ledger::Block;
use crate::packet::{index_blobs, SharedBlob};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::window::{SharedWindow, WindowIndex, WindowUtil};
use log::Level;
use rayon::prelude::*;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastServiceReturnType {
    LeaderRotation,
    ChannelDisconnected,
}

#[allow(clippy::too_many_arguments)]
fn broadcast(
    db_ledger: &Arc<RwLock<DbLedger>>,
    max_tick_height: Option<u64>,
    tick_height: &mut u64,
    leader_id: Pubkey,
    node_info: &NodeInfo,
    broadcast_table: &[NodeInfo],
    window: &SharedWindow,
    receiver: &Receiver<Vec<Entry>>,
    sock: &UdpSocket,
    transmit_index: &mut WindowIndex,
    receive_index: &mut u64,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<()> {
    let id = node_info.id;
    let timer = Duration::new(1, 0);
    let entries = receiver.recv_timeout(timer)?;
    let now = Instant::now();
    let mut num_entries = entries.len();
    let mut ventries = Vec::new();
    ventries.push(entries);
    while let Ok(entries) = receiver.try_recv() {
        num_entries += entries.len();
        ventries.push(entries);
    }
    inc_new_counter_info!("broadcast_service-entries_received", num_entries);

    let to_blobs_start = Instant::now();

    // Generate the tick heights for all the entries inside ventries
    let tick_heights: Vec<u64> = ventries
        .iter()
        .flat_map(|p| {
            let tick_heights: Vec<u64> = p
                .iter()
                .map(|e| {
                    if e.is_tick() {
                        *tick_height += e.is_tick() as u64;
                        *tick_height
                    } else {
                        *tick_height + 1
                    }
                })
                .collect();

            tick_heights
        })
        .collect();

    let blobs_vec: Vec<_> = ventries
        .into_par_iter()
        .flat_map(|p| p.to_blobs())
        .collect();

    let blobs_tick_heights: Vec<(SharedBlob, u64)> =
        blobs_vec.into_iter().zip(tick_heights).collect();

    let to_blobs_elapsed = duration_as_ms(&to_blobs_start.elapsed());

    let blobs_chunking = Instant::now();
    // We could receive more blobs than window slots so
    // break them up into window-sized chunks to process
    let window_size = window.read().unwrap().window_size();
    let blobs_chunked = blobs_tick_heights
        .chunks(window_size as usize)
        .map(|x| x.to_vec());
    let chunking_elapsed = duration_as_ms(&blobs_chunking.elapsed());

    let broadcast_start = Instant::now();
    for blobs in blobs_chunked {
        let blobs_len = blobs.len();
        trace!("{}: broadcast blobs.len: {}", id, blobs_len);

        index_blobs(
            blobs.iter(),
            &node_info.id,
            *receive_index,
            &leader_scheduler,
        );

        // keep the cache of blobs that are broadcast
        inc_new_counter_info!("streamer-broadcast-sent", blobs.len());
        {
            let mut win = window.write().unwrap();
            assert!(blobs.len() <= win.len());
            for (b, _) in &blobs {
                let ix = b.read().unwrap().index().expect("blob index");
                let pos = (ix % window_size) as usize;
                if let Some(x) = win[pos].data.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().index().unwrap(),
                        pos
                    );
                }
                if let Some(x) = win[pos].coding.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().index().unwrap(),
                        pos
                    );
                }

                trace!("{} null {}", id, pos);
            }
            for (b, slot) in &blobs {
                {
                    let ix = b.read().unwrap().index().expect("blob index");
                    let pos = (ix % window_size) as usize;
                    trace!("{} caching {} at {}", id, ix, pos);
                    assert!(win[pos].data.is_none());
                    win[pos].data = Some(b.clone());
                }
                db_ledger
                    .write()
                    .unwrap()
                    .write_shared_blobs(*slot, vec![b])?;
            }
        }

        // Fill in the coding blob data from the window data blobs
        #[cfg(feature = "erasure")]
        {
            erasure::generate_coding(
                &id,
                &mut window.write().unwrap(),
                *receive_index,
                blobs_len,
                &mut transmit_index.coding,
            )?;
        }

        *receive_index += blobs_len as u64;

        // Send blobs out from the window
        ClusterInfo::broadcast(
            Some(*tick_height) == max_tick_height,
            leader_id,
            &node_info,
            &broadcast_table,
            &window,
            &sock,
            transmit_index,
            *receive_index,
        )?;
    }
    let broadcast_elapsed = duration_as_ms(&broadcast_start.elapsed());

    inc_new_counter_info!(
        "broadcast_service-time_ms",
        duration_as_ms(&now.elapsed()) as usize
    );
    info!(
        "broadcast: {} entries, blob time {} chunking time {} broadcast time {}",
        num_entries, to_blobs_elapsed, chunking_elapsed, broadcast_elapsed
    );

    submit(
        influxdb::Point::new("broadcast-service")
            .add_field(
                "transmit-index",
                influxdb::Value::Integer(transmit_index.data as i64),
            )
            .to_owned(),
    );

    Ok(())
}

// Implement a destructor for the BroadcastService3 thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct BroadcastService {
    thread_hdl: JoinHandle<BroadcastServiceReturnType>,
}

impl BroadcastService {
    fn run(
        db_ledger: &Arc<RwLock<DbLedger>>,
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window: &SharedWindow,
        entry_height: u64,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
        receiver: &Receiver<Vec<Entry>>,
        max_tick_height: Option<u64>,
        tick_height: u64,
    ) -> BroadcastServiceReturnType {
        let mut transmit_index = WindowIndex {
            data: entry_height,
            coding: entry_height,
        };
        let mut receive_index = entry_height;
        let me = cluster_info.read().unwrap().my_data().clone();
        let mut tick_height_ = tick_height;
        loop {
            let broadcast_table = cluster_info.read().unwrap().tvu_peers();
            inc_new_counter_info!("broadcast_service-num_peers", broadcast_table.len() + 1);
            let leader_id = cluster_info.read().unwrap().leader_id();
            if let Err(e) = broadcast(
                db_ledger,
                max_tick_height,
                &mut tick_height_,
                leader_id,
                &me,
                &broadcast_table,
                &window,
                &receiver,
                &sock,
                &mut transmit_index,
                &mut receive_index,
                leader_scheduler,
            ) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                        return BroadcastServiceReturnType::ChannelDisconnected
                    }
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    Error::ClusterInfoError(ClusterInfoError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                    _ => {
                        inc_new_counter_info!("streamer-broadcaster-error", 1, 1);
                        error!("broadcaster error: {:?}", e);
                    }
                }
            }
        }
    }

    /// Service to broadcast messages from the leader to layer 1 nodes.
    /// See `cluster_info` for network layer definitions.
    /// # Arguments
    /// * `sock` - Socket to send from.
    /// * `exit` - Boolean to signal system exit.
    /// * `cluster_info` - ClusterInfo structure
    /// * `window` - Cache of blobs that we have broadcast
    /// * `receiver` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
    /// * `exit_sender` - Set to true when this service exits, allows rest of Tpu to exit cleanly.
    /// Otherwise, when a Tpu closes, it only closes the stages that come after it. The stages
    /// that come before could be blocked on a receive, and never notice that they need to
    /// exit. Now, if any stage of the Tpu closes, it will lead to closing the WriteStage (b/c
    /// WriteStage is the last stage in the pipeline), which will then close Broadcast service,
    /// which will then close FetchStage in the Tpu, and then the rest of the Tpu,
    /// completing the cycle.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db_ledger: Arc<RwLock<DbLedger>>,
        sock: UdpSocket,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window: SharedWindow,
        entry_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        receiver: Receiver<Vec<Entry>>,
        max_tick_height: Option<u64>,
        tick_height: u64,
        exit_sender: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_sender);
                Self::run(
                    &db_ledger,
                    &sock,
                    &cluster_info,
                    &window,
                    entry_height,
                    &leader_scheduler,
                    &receiver,
                    max_tick_height,
                    tick_height,
                )
            })
            .unwrap();

        Self { thread_hdl }
    }
}

impl Service for BroadcastService {
    type JoinReturnType = BroadcastServiceReturnType;

    fn join(self) -> thread::Result<BroadcastServiceReturnType> {
        self.thread_hdl.join()
    }
}
