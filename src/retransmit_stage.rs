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
use std::sync::atomic::{AtomicBool, AtomicUsize};
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
fn compute_retransmit_peers(
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
            ClusterInfo::retransmit_to(&cluster_info, &neighbors, &for_neighbors(b), sock)?;
        }
        // Always send blobs to children
        ClusterInfo::retransmit_to(&cluster_info, &children, b, sock)?;
    }
    Ok(())
}

/// Modifies a blob for neighbors nodes
#[inline]
fn for_neighbors(b: &SharedBlob) -> SharedBlob {
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

// Recommended to not run these tests in parallel (they are resource heavy and want all the compute)
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_info::ClusterInfo;
    use crate::contact_info::ContactInfo;
    use crate::genesis_block::GenesisBlock;
    use rayon::iter::{IntoParallelIterator, ParallelIterator};
    use rayon::prelude::*;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::collections::{HashMap, HashSet};
    use std::sync::mpsc::TryRecvError;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::Mutex;
    use std::time::Instant;

    type Nodes = HashMap<Pubkey, (HashSet<i32>, Receiver<(i32, bool)>)>;

    fn num_threads() -> usize {
        sys_info::cpu_num().unwrap_or(10) as usize
    }

    /// Search for the a node with the given balance
    fn find_insert_blob(id: &Pubkey, blob: i32, batches: &mut [Nodes]) {
        batches.par_iter_mut().for_each(|batch| {
            if batch.contains_key(id) {
                let _ = batch.get_mut(id).unwrap().0.insert(blob);
            }
        });
    }

    fn run_simulation(num_nodes: u64, fanout: usize, hood_size: usize) {
        let num_threads = num_threads();
        // set timeout to 5 minutes
        let timeout = 60 * 5;

        // math yo
        let required_balance = num_nodes * (num_nodes + 1) / 2;

        // create a genesis block
        let (genesis_block, mint_keypair) = GenesisBlock::new(required_balance + 2);

        // describe the leader
        let leader_info = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);
        let mut cluster_info = ClusterInfo::new(leader_info.clone());
        cluster_info.set_leader(leader_info.id);

        // create a bank
        let bank = Arc::new(Bank::new(&genesis_block));

        // setup accounts for all nodes (leader has 0 bal)
        let (s, r) = channel();
        let senders: Arc<Mutex<HashMap<Pubkey, Sender<(i32, bool)>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        senders.lock().unwrap().insert(leader_info.id, s);
        let mut batches: Vec<Nodes> = Vec::with_capacity(num_threads);
        (0..num_threads).for_each(|_| batches.push(HashMap::new()));
        batches
            .get_mut(0)
            .unwrap()
            .insert(leader_info.id, (HashSet::new(), r));
        let range: Vec<_> = (1..=num_nodes).collect();
        let chunk_size = (num_nodes as usize + num_threads - 1) / num_threads;
        range.chunks(chunk_size).for_each(|chunk| {
            chunk.into_iter().for_each(|i| {
                //distribute neighbors across threads to maximize parallel compute
                let batch_ix = *i as usize % batches.len();
                let node = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);
                bank.transfer(*i, &mint_keypair, node.id, bank.last_id())
                    .unwrap();
                cluster_info.insert_info(node.clone());
                let (s, r) = channel();
                batches
                    .get_mut(batch_ix)
                    .unwrap()
                    .insert(node.id, (HashSet::new(), r));
                senders.lock().unwrap().insert(node.id, s);
            })
        });
        let c_info = cluster_info.clone();

        // check that all tokens have been exhausted
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);

        // create some "blobs".
        let blobs: Vec<(_, _)> = (0..100).into_par_iter().map(|i| (i as i32, true)).collect();

        // pretend to broadcast from leader - cluster_info::create_broadcast_orders
        let mut broadcast_table = cluster_info.sorted_tvu_peers(&bank);
        broadcast_table.truncate(fanout);
        let orders = ClusterInfo::create_broadcast_orders(false, &blobs, &broadcast_table);

        // send blobs to layer 1 nodes
        orders.iter().for_each(|(b, vc)| {
            vc.iter().for_each(|c| {
                find_insert_blob(&c.id, b.0, &mut batches);
            })
        });
        assert!(!batches.is_empty());

        // start avalanche simulation
        let now = Instant::now();
        batches.par_iter_mut().for_each(|batch| {
            let mut cluster = c_info.clone();
            let batch_size = batch.len();
            let mut remaining = batch_size;
            let senders: HashMap<_, _> = senders.lock().unwrap().clone();
            // A map that holds neighbors and children senders for a given node
            let mut mapped_peers: HashMap<
                Pubkey,
                (Vec<Sender<(i32, bool)>>, Vec<Sender<(i32, bool)>>),
            > = HashMap::new();
            while remaining > 0 {
                for (id, (recv, r)) in batch.iter_mut() {
                    assert!(now.elapsed().as_secs() < timeout, "Timed out");
                    cluster.gossip.set_self(*id);
                    if !mapped_peers.contains_key(id) {
                        let (neighbors, children) = compute_retransmit_peers(
                            &bank,
                            &Arc::new(RwLock::new(cluster.clone())),
                            fanout,
                            hood_size,
                            GROW_LAYER_CAPACITY,
                        );
                        let vec_children: Vec<_> = children
                            .iter()
                            .map(|p| {
                                let s = senders.get(&p.id).unwrap();
                                recv.iter().for_each(|i| {
                                    let _ = s.send((*i, true));
                                });
                                s.clone()
                            })
                            .collect();

                        let vec_neighbors: Vec<_> = neighbors
                            .iter()
                            .map(|p| {
                                let s = senders.get(&p.id).unwrap();
                                recv.iter().for_each(|i| {
                                    let _ = s.send((*i, false));
                                });
                                s.clone()
                            })
                            .collect();
                        mapped_peers.insert(*id, (vec_neighbors, vec_children));
                    }
                    let (vec_neighbors, vec_children) = mapped_peers.get(id).unwrap();

                    //send and recv
                    if recv.len() < blobs.len() {
                        loop {
                            match r.try_recv() {
                                Ok((data, retransmit)) => {
                                    if recv.insert(data) {
                                        vec_children.iter().for_each(|s| {
                                            let _ = s.send((data, retransmit));
                                        });
                                        if retransmit {
                                            vec_neighbors.iter().for_each(|s| {
                                                let _ = s.send((data, false));
                                            })
                                        }
                                        if recv.len() == blobs.len() {
                                            remaining -= 1;
                                            break;
                                        }
                                    }
                                }
                                Err(TryRecvError::Disconnected) => break,
                                Err(TryRecvError::Empty) => break,
                            };
                        }
                    }
                }
            }
        });
    }

    //todo add tests with network failures

    // Run with a single layer
    #[test]
    fn test_retransmit_small() {
        run_simulation(
            DATA_PLANE_FANOUT as u64,
            DATA_PLANE_FANOUT,
            NEIGHBORHOOD_SIZE,
        );
    }

    // Make sure at least 2 layers are used
    #[test]
    fn test_retransmit_medium() {
        let num_nodes = DATA_PLANE_FANOUT as u64 * 10;
        run_simulation(num_nodes, DATA_PLANE_FANOUT, NEIGHBORHOOD_SIZE);
    }

    // Scale down the network and make sure at least 3 layers are used
    #[test]
    fn test_retransmit_large() {
        let num_nodes = DATA_PLANE_FANOUT as u64 * 20;
        run_simulation(num_nodes, DATA_PLANE_FANOUT / 10, NEIGHBORHOOD_SIZE / 10);
    }

    // Test that blobs always come out with forward unset for neighbors
    #[test]
    fn test_blob_for_neighbors() {
        let blob = SharedBlob::default();
        blob.write().unwrap().forward(true);
        let for_hoodies = for_neighbors(&blob);
        assert!(!for_hoodies.read().unwrap().should_forward());
    }

}
