//! The `retransmit_stage` retransmits blobs between validators

use crate::bank::Bank;
use crate::cluster_info::{ClusterInfo, DATA_PLANE_FANOUT, GROW_LAYER_CAPACITY, NEIGHBORHOOD_SIZE};
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::leader_scheduler::LeaderScheduler;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::BlobReceiver;
use crate::window_service::window_service;
use log::Level;
use solana_metrics::{influxdb, submit};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

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

    // TODO layer 2 logic here
    // 1 - find out if I am in layer 1 first
    // 1.1 - If yes, then broadcast to all layer 1 nodes
    //      1 - using my layer 1 index, broadcast to all layer 2 nodes assuming you know neighborhood size
    // 1.2 - If no, then figure out what layer I am in and who my neighbors are and only broadcast to them
    //      1 - also check if there are nodes in lower layers and repeat the layer 1 to layer 2 logic
    let peers = cluster_info.read().unwrap().sorted_retransmit_peers(bank);
    let my_id = cluster_info.read().unwrap().id();
    //calc num_layers and num_neighborhoods using the total number of nodes
    let (num_layers, layer_indices) = ClusterInfo::describe_data_plane(
        peers.len(),
        DATA_PLANE_FANOUT,
        NEIGHBORHOOD_SIZE,
        GROW_LAYER_CAPACITY,
    );
    if num_layers <= 1 {
        /* single layer data plane */
        for b in &mut dq {
            ClusterInfo::retransmit(&cluster_info, b, sock)?;
        }
    } else {
        //find my index (my ix is the same as the first node with smaller stake)
        let my_index = peers.iter().position(|ci| {
            bank.root().get_balance_slow(&ci.id) <= bank.root().get_balance_slow(&my_id)
        });
        //find my layer
        let locality = ClusterInfo::localize(
            &layer_indices,
            NEIGHBORHOOD_SIZE,
            my_index.unwrap_or(peers.len() - 1),
        );
        let mut retransmit_peers =
            peers[locality.neighbor_bounds.0..locality.neighbor_bounds.1].to_vec();
        locality.child_layer_peers.iter().for_each(|&ix| {
            if let Some(peer) = peers.get(ix) {
                retransmit_peers.push(peer.clone());
            }
        });

        for b in &mut dq {
            ClusterInfo::retransmit_to(&cluster_info, &retransmit_peers, b, sock)?;
        }
    }
    Ok(())
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
}

impl RetransmitStage {
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new(
        bank: &Arc<Bank>,
        db_ledger: Arc<DbLedger>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        tick_height: u64,
        entry_height: u64,
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
        let done = Arc::new(AtomicBool::new(false));
        let t_window = window_service(
            db_ledger,
            cluster_info.clone(),
            tick_height,
            entry_height,
            0,
            fetch_stage_receiver,
            retransmit_sender,
            repair_socket,
            leader_scheduler,
            done,
            exit,
        );

        let thread_hdls = vec![t_retransmit, t_window];
        Self { thread_hdls }
    }
}

impl Service for RetransmitStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
