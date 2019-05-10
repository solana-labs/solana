use hashbrown::{HashMap, HashSet};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::prelude::*;
use solana::cluster_info::{compute_retransmit_peers, ClusterInfo};
use solana::contact_info::ContactInfo;
use solana_sdk::pubkey::Pubkey;
use std::sync::mpsc::channel;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
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

fn run_simulation(stakes: &[u64], fanout: usize) {
    let num_threads = num_threads();
    // set timeout to 5 minutes
    let timeout = 60 * 5;

    // describe the leader
    let leader_info = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
    let mut cluster_info = ClusterInfo::new_with_invalid_keypair(leader_info.clone());

    // setup staked nodes
    let mut staked_nodes = HashMap::new();

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
    let range: Vec<_> = (1..=stakes.len()).collect();
    let chunk_size = (stakes.len() + num_threads - 1) / num_threads;
    range.chunks(chunk_size).for_each(|chunk| {
        chunk.into_iter().for_each(|i| {
            //distribute neighbors across threads to maximize parallel compute
            let batch_ix = *i as usize % batches.len();
            let node = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
            staked_nodes.insert(node.id, stakes[*i - 1]);
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

    // create some "blobs".
    let blobs: Vec<(_, _)> = (0..100).into_par_iter().map(|i| (i as i32, true)).collect();

    // pretend to broadcast from leader - cluster_info::create_broadcast_orders
    let mut broadcast_table = cluster_info.sorted_tvu_peers(&staked_nodes);
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
                assert!(
                    now.elapsed().as_secs() < timeout,
                    "Timed out with {:?} remaining nodes",
                    remaining
                );
                cluster.gossip.set_self(&*id);
                if !mapped_peers.contains_key(id) {
                    let (neighbors, children) = compute_retransmit_peers(
                        &staked_nodes,
                        &Arc::new(RwLock::new(cluster.clone())),
                        fanout,
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

// Recommended to not run these tests in parallel (they are resource heavy and want all the compute)

//todo add tests with network failures

// Run with a single layer
#[test]
fn test_retransmit_small() {
    let stakes: Vec<_> = (0..200).map(|i| i).collect();
    run_simulation(&stakes, 200);
}

// Make sure at least 2 layers are used
#[test]
fn test_retransmit_medium() {
    let num_nodes = 2000;
    let stakes: Vec<_> = (0..num_nodes).map(|i| i).collect();
    run_simulation(&stakes, 200);
}

// Make sure at least 2 layers are used but with equal stakes
#[test]
fn test_retransmit_medium_equal_stakes() {
    let num_nodes = 2000;
    let stakes: Vec<_> = (0..num_nodes).map(|_| 10).collect();
    run_simulation(&stakes, 200);
}

// Scale down the network and make sure many layers are used
#[test]
fn test_retransmit_large() {
    let num_nodes = 4000;
    let stakes: Vec<_> = (0..num_nodes).map(|i| i).collect();
    run_simulation(&stakes, 2);
}
