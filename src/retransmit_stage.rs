//! The `retransmit_stage` retransmits blobs between validators

use crate::bank::Bank;
use crate::blocktree::Blocktree;
use crate::cluster_info::{
    ClusterInfo, NodeInfo, DATA_PLANE_FANOUT, GROW_LAYER_CAPACITY, NEIGHBORHOOD_SIZE,
};
use crate::counter::Counter;
use crate::leader_scheduler::LeaderScheduler;
use crate::packet::SharedBlob;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::BlobReceiver;
use crate::window_service::WindowService;
use core::cmp;
use log::Level;
use solana_metrics::{influxdb, submit};
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

/// Avalanche logic
/// 1 - For the current node find out if it is in layer 1
/// 1.1 - If yes, then broadcast to all layer 1 nodes
///      1 - using the layer 1 index, broadcast to all layer 2 nodes assuming you know neighborhood size
/// 1.2 - If no, then figure out what layer the node is in and who the neighbors are and only broadcast to them
///      1 - also check if there are nodes in lower layers and repeat the layer 1 to layer 2 logic

/// Returns Neighbor Nodes and Children Nodes `(neighbors, children)` for a given node based on its stake (Bank Balance)
pub fn compute_retransmit_peers(
    bank: &Arc<Bank>,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    fanout: usize,
    hood_size: usize,
    grow: bool,
) -> (Vec<NodeInfo>, Vec<NodeInfo>) {
    let peers = cluster_info.read().unwrap().sorted_retransmit_peers(bank);
    let my_id = cluster_info.read().unwrap().id();
    //calc num_layers and num_neighborhoods using the total number of nodes
    let (num_layers, layer_indices) =
        ClusterInfo::describe_data_plane(peers.len(), fanout, hood_size, grow);

    if num_layers <= 1 {
        /* single layer data plane */
        (peers, vec![])
    } else {
        //find my index (my ix is the same as the first node with smaller stake)
        let my_index = peers
            .iter()
            .position(|ci| bank.get_balance(&ci.id) <= bank.get_balance(&my_id));
        //find my layer
        let locality = ClusterInfo::localize(
            &layer_indices,
            hood_size,
            my_index.unwrap_or(peers.len() - 1),
        );
        let upper_bound = cmp::min(locality.neighbor_bounds.1, peers.len());
        let neighbors = peers[locality.neighbor_bounds.0..upper_bound].to_vec();
        let mut children = Vec::new();
        for ix in locality.child_layer_peers {
            if let Some(peer) = peers.get(ix) {
                children.push(peer.clone());
                continue;
            }
            break;
        }
        (neighbors, children)
    }
}

fn retransmit(
    bank: &Arc<Bank>,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    r: &BlobReceiver,
    sock: &UdpSocket,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq);
    }

    submit(
        influxdb::Point::new("retransmit-stage")
            .add_field("count", influxdb::Value::Integer(dq.len() as i64))
            .to_owned(),
    );
    let (neighbors, children) = compute_retransmit_peers(
        &bank,
        cluster_info,
        DATA_PLANE_FANOUT,
        NEIGHBORHOOD_SIZE,
        GROW_LAYER_CAPACITY,
    );
    for b in &dq {
        if b.read().unwrap().should_forward() {
            ClusterInfo::retransmit_to(&cluster_info, &neighbors, &copy_for_neighbors(b), sock)?;
        }
        // Always send blobs to children
        ClusterInfo::retransmit_to(&cluster_info, &children, b, sock)?;
    }
    Ok(())
}

/// Modifies a blob for neighbors nodes
#[inline]
fn copy_for_neighbors(b: &SharedBlob) -> SharedBlob {
    let mut blob = b.read().unwrap().clone();
    // Disable blob forwarding for neighbors
    blob.forward(false);
    Arc::new(RwLock::new(blob))
}

/// Service to retransmit messages from the leader or layer 1 to relevant peer nodes.
/// See `cluster_info` for network layer definitions.
/// # Arguments
/// * `sock` - Socket to read from.  Read timeout is set to 1.
/// * `exit` - Boolean to signal system exit.
/// * `cluster_info` - This structure needs to be updated and populated by the bank and via gossip.
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
fn retransmitter(
    sock: Arc<UdpSocket>,
    bank: Arc<Bank>,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    r: BlobReceiver,
) -> JoinHandle<()> {
    Builder::new()
        .name("solana-retransmitter".to_string())
        .spawn(move || {
            trace!("retransmitter started");
            loop {
                if let Err(e) = retransmit(&bank, &cluster_info, &r, &sock) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter_info!("streamer-retransmit-error", 1, 1);
                        }
                    }
                }
            }
            trace!("exiting retransmitter");
        })
        .unwrap()
}

pub struct RetransmitStage {
    thread_hdls: Vec<JoinHandle<()>>,
    window_service: WindowService,
}

impl RetransmitStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        bank: &Arc<Bank>,
        blocktree: Arc<Blocktree>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        retransmit_socket: Arc<UdpSocket>,
        repair_socket: Arc<UdpSocket>,
        fetch_stage_receiver: BlobReceiver,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = retransmitter(
            retransmit_socket,
            bank.clone(),
            cluster_info.clone(),
            retransmit_receiver,
        );
        let window_service = WindowService::new(
            blocktree,
            cluster_info.clone(),
            fetch_stage_receiver,
            retransmit_sender,
            repair_socket,
            leader_scheduler,
            exit,
        );

        let thread_hdls = vec![t_retransmit];
        Self {
            thread_hdls,
            window_service,
        }
    }
}

impl Service for RetransmitStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        self.window_service.join()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that blobs always come out with forward unset for neighbors
    #[test]
    fn test_blob_for_neighbors() {
        let blob = SharedBlob::default();
        blob.write().unwrap().forward(true);
        let for_hoodies = copy_for_neighbors(&blob);
        assert!(!for_hoodies.read().unwrap().should_forward());
    }
}
