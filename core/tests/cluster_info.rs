use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use rayon::iter::ParallelIterator;
use rayon::prelude::*;
use solana::cluster_info::{compute_retransmit_peers, ClusterInfo};
use solana::contact_info::ContactInfo;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::channel;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

type Nodes = HashMap<Pubkey, (bool, HashSet<i32>, Receiver<(i32, bool)>)>;

fn num_threads() -> usize {
    sys_info::cpu_num().unwrap_or(10) as usize
}

/// Search for the a node with the given balance
fn find_insert_blob(id: &Pubkey, blob: i32, batches: &mut [Nodes]) {
    batches.par_iter_mut().for_each(|batch| {
        if batch.contains_key(id) {
            let _ = batch.get_mut(id).unwrap().1.insert(blob);
        }
    });
}

fn retransmit(
    mut shuffled_nodes: Vec<ContactInfo>,
    senders: &HashMap<Pubkey, Sender<(i32, bool)>>,
    cluster: &ClusterInfo,
    fanout: usize,
    blob: i32,
    retransmit: bool,
) -> i32 {
    let mut seed = [0; 32];
    let mut my_index = 0;
    let mut index = 0;
    shuffled_nodes.retain(|c| {
        if c.id == cluster.id() {
            my_index = index;
            false
        } else {
            index += 1;
            true
        }
    });
    seed[0..4].copy_from_slice(&blob.to_le_bytes());
    let (neighbors, children) = compute_retransmit_peers(fanout, my_index, shuffled_nodes);
    children.iter().for_each(|p| {
        let s = senders.get(&p.id).unwrap();
        let _ = s.send((blob, retransmit));
    });

    if retransmit {
        neighbors.iter().for_each(|p| {
            let s = senders.get(&p.id).unwrap();
            let _ = s.send((blob, false));
        });
    }

    blob
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
        .insert(leader_info.id, (false, HashSet::new(), r));
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
                .insert(node.id, (false, HashSet::new(), r));
            senders.lock().unwrap().insert(node.id, s);
        })
    });
    let c_info = cluster_info.clone();

    let blobs_len = 100;
    let shuffled_peers: Vec<Vec<ContactInfo>> = (0..blobs_len as i32)
        .map(|i| {
            let mut seed = [0; 32];
            seed[0..4].copy_from_slice(&i.to_le_bytes());
            let (_, peers) = cluster_info
                .shuffle_peers_and_index(Some(&staked_nodes), ChaChaRng::from_seed(seed));
            peers
        })
        .collect();

    // create some "blobs".
    (0..blobs_len).into_iter().for_each(|i| {
        let broadcast_table = &shuffled_peers[i];
        find_insert_blob(&broadcast_table[0].id, i as i32, &mut batches);
    });

    assert!(!batches.is_empty());

    // start turbine simulation
    let now = Instant::now();
    batches.par_iter_mut().for_each(|batch| {
        let mut cluster = c_info.clone();
        let mut remaining = batch.len();
        let senders: HashMap<_, _> = senders.lock().unwrap().clone();
        while remaining > 0 {
            for (id, (layer1_done, recv, r)) in batch.iter_mut() {
                assert!(
                    now.elapsed().as_secs() < timeout,
                    "Timed out with {:?} remaining nodes",
                    remaining
                );
                cluster.gossip.set_self(&*id);
                if !*layer1_done {
                    recv.iter().for_each(|i| {
                        retransmit(
                            shuffled_peers[*i as usize].clone(),
                            &senders,
                            &cluster,
                            fanout,
                            *i,
                            true,
                        );
                    });
                    *layer1_done = true;
                }

                //send and recv
                if recv.len() < blobs_len {
                    loop {
                        match r.try_recv() {
                            Ok((data, retx)) => {
                                if recv.insert(data) {
                                    let _ = retransmit(
                                        shuffled_peers[data as usize].clone(),
                                        &senders,
                                        &cluster,
                                        fanout,
                                        data,
                                        retx,
                                    );
                                }
                                if recv.len() == blobs_len {
                                    remaining -= 1;
                                    break;
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
