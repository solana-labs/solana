use bincode::serialized_size;
use hashbrown::HashMap;
use log::*;
use rayon::prelude::*;
use solana::contact_info::ContactInfo;
use solana::crds_gossip::*;
use solana::crds_gossip_error::CrdsGossipError;
use solana::crds_gossip_push::CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS;
use solana::crds_value::CrdsValue;
use solana::crds_value::CrdsValueLabel;
use solana_sdk::hash::hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::timestamp;
use std::sync::{Arc, Mutex};

type Node = Arc<Mutex<CrdsGossip>>;
type Network = HashMap<Pubkey, Node>;
fn star_network_create(num: usize) -> Network {
    let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
    let mut network: HashMap<_, _> = (1..num)
        .map(|_| {
            let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
            let id = new.label().pubkey();
            let mut node = CrdsGossip::default();
            node.crds.insert(new.clone(), 0).unwrap();
            node.crds.insert(entry.clone(), 0).unwrap();
            node.set_self(&id);
            (new.label().pubkey(), Arc::new(Mutex::new(node)))
        })
        .collect();
    let mut node = CrdsGossip::default();
    let id = entry.label().pubkey();
    node.crds.insert(entry.clone(), 0).unwrap();
    node.set_self(&id);
    network.insert(id, Arc::new(Mutex::new(node)));
    network
}

fn rstar_network_create(num: usize) -> Network {
    let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
    let mut origin = CrdsGossip::default();
    let id = entry.label().pubkey();
    origin.crds.insert(entry.clone(), 0).unwrap();
    origin.set_self(&id);
    let mut network: HashMap<_, _> = (1..num)
        .map(|_| {
            let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
            let id = new.label().pubkey();
            let mut node = CrdsGossip::default();
            node.crds.insert(new.clone(), 0).unwrap();
            origin.crds.insert(new.clone(), 0).unwrap();
            node.set_self(&id);
            (new.label().pubkey(), Arc::new(Mutex::new(node)))
        })
        .collect();
    network.insert(id, Arc::new(Mutex::new(origin)));
    network
}

fn ring_network_create(num: usize) -> Network {
    let mut network: HashMap<_, _> = (0..num)
        .map(|_| {
            let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
            let id = new.label().pubkey();
            let mut node = CrdsGossip::default();
            node.crds.insert(new.clone(), 0).unwrap();
            node.set_self(&id);
            (new.label().pubkey(), Arc::new(Mutex::new(node)))
        })
        .collect();
    let keys: Vec<Pubkey> = network.keys().cloned().collect();
    for k in 0..keys.len() {
        let start_info = {
            let start = &network[&keys[k]];
            let start_id = start.lock().unwrap().id.clone();
            start
                .lock()
                .unwrap()
                .crds
                .lookup(&CrdsValueLabel::ContactInfo(start_id))
                .unwrap()
                .clone()
        };
        let end = network.get_mut(&keys[(k + 1) % keys.len()]).unwrap();
        end.lock().unwrap().crds.insert(start_info, 0).unwrap();
    }
    network
}

fn network_simulator_pull_only(network: &mut Network) {
    let num = network.len();
    let (converged, bytes_tx) = network_run_pull(network, 0, num * 2, 0.9);
    trace!(
        "network_simulator_pull_{}: converged: {} total_bytes: {}",
        num,
        converged,
        bytes_tx
    );
    assert!(converged >= 0.9);
}

fn network_simulator(network: &mut Network) {
    let num = network.len();
    // run for a small amount of time
    let (converged, bytes_tx) = network_run_pull(network, 0, 10, 1.0);
    trace!("network_simulator_push_{}: converged: {}", num, converged);
    // make sure there is someone in the active set
    let network_values: Vec<Node> = network.values().cloned().collect();
    network_values.par_iter().for_each(|node| {
        node.lock()
            .unwrap()
            .refresh_push_active_set(&HashMap::new());
    });
    let mut total_bytes = bytes_tx;
    for second in 1..num {
        let start = second * 10;
        let end = (second + 1) * 10;
        let now = (start * 100) as u64;
        // push a message to the network
        network_values.par_iter().for_each(|locked_node| {
            let node = &mut locked_node.lock().unwrap();
            let mut m = node
                .crds
                .lookup(&CrdsValueLabel::ContactInfo(node.id))
                .and_then(|v| v.contact_info().cloned())
                .unwrap();
            m.wallclock = now;
            node.process_push_message(vec![CrdsValue::ContactInfo(m)], now);
        });
        // push for a bit
        let (queue_size, bytes_tx) = network_run_push(network, start, end);
        total_bytes += bytes_tx;
        trace!(
            "network_simulator_push_{}: queue_size: {} bytes: {}",
            num,
            queue_size,
            bytes_tx
        );
        // pull for a bit
        let (converged, bytes_tx) = network_run_pull(network, start, end, 1.0);
        total_bytes += bytes_tx;
        trace!(
            "network_simulator_push_{}: converged: {} bytes: {} total_bytes: {}",
            num,
            converged,
            bytes_tx,
            total_bytes
        );
        if converged > 0.9 {
            break;
        }
    }
}

fn network_run_push(network: &mut Network, start: usize, end: usize) -> (usize, usize) {
    let mut bytes: usize = 0;
    let mut num_msgs: usize = 0;
    let mut total: usize = 0;
    let num = network.len();
    let mut prunes: usize = 0;
    let mut delivered: usize = 0;
    let network_values: Vec<Node> = network.values().cloned().collect();
    for t in start..end {
        let now = t as u64 * 100;
        let requests: Vec<_> = network_values
            .par_iter()
            .map(|node| {
                node.lock().unwrap().purge(now);
                node.lock().unwrap().new_push_messages(now)
            })
            .collect();
        let transfered: Vec<_> = requests
            .into_par_iter()
            .map(|(from, peers, msgs)| {
                let mut bytes: usize = 0;
                let mut delivered: usize = 0;
                let mut num_msgs: usize = 0;
                let mut prunes: usize = 0;
                for to in peers {
                    bytes += serialized_size(&msgs).unwrap() as usize;
                    num_msgs += 1;
                    let rsps = network
                        .get(&to)
                        .map(|node| node.lock().unwrap().process_push_message(msgs.clone(), now))
                        .unwrap();
                    bytes += serialized_size(&rsps).unwrap() as usize;
                    prunes += rsps.len();
                    network
                        .get(&from)
                        .map(|node| {
                            let mut node = node.lock().unwrap();
                            let destination = node.id;
                            let now = timestamp();
                            node.process_prune_msg(&to, &destination, &rsps, now, now)
                                .unwrap()
                        })
                        .unwrap();
                    delivered += rsps.is_empty() as usize;
                }
                (bytes, delivered, num_msgs, prunes)
            })
            .collect();
        for (b, d, m, p) in transfered {
            bytes += b;
            delivered += d;
            num_msgs += m;
            prunes += p;
        }
        if now % CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS == 0 && now > 0 {
            network_values.par_iter().for_each(|node| {
                node.lock()
                    .unwrap()
                    .refresh_push_active_set(&HashMap::new());
            });
        }
        total = network_values
            .par_iter()
            .map(|v| v.lock().unwrap().push.num_pending())
            .sum();
        trace!(
                "network_run_push_{}: now: {} queue: {} bytes: {} num_msgs: {} prunes: {} delivered: {}",
                num,
                now,
                total,
                bytes,
                num_msgs,
                prunes,
                delivered,
            );
    }
    (total, bytes)
}

fn network_run_pull(
    network: &mut Network,
    start: usize,
    end: usize,
    max_convergance: f64,
) -> (f64, usize) {
    let mut bytes: usize = 0;
    let mut msgs: usize = 0;
    let mut overhead: usize = 0;
    let mut convergance = 0f64;
    let num = network.len();
    let network_values: Vec<Node> = network.values().cloned().collect();
    for t in start..end {
        let now = t as u64 * 100;
        let requests: Vec<_> = {
            network_values
                .par_iter()
                .filter_map(|from| {
                    from.lock()
                        .unwrap()
                        .new_pull_request(now, &HashMap::new())
                        .ok()
                })
                .collect()
        };
        let transfered: Vec<_> = requests
            .into_par_iter()
            .map(|(to, request, caller_info)| {
                let mut bytes: usize = 0;
                let mut msgs: usize = 0;
                let mut overhead: usize = 0;
                let from = caller_info.label().pubkey();
                bytes += request.keys.len();
                bytes += (request.bits.len() / 8) as usize;
                bytes += serialized_size(&caller_info).unwrap() as usize;
                let rsp = network
                    .get(&to)
                    .map(|node| {
                        node.lock()
                            .unwrap()
                            .process_pull_request(caller_info, request, now)
                    })
                    .unwrap();
                bytes += serialized_size(&rsp).unwrap() as usize;
                msgs += rsp.len();
                network.get(&from).map(|node| {
                    node.lock()
                        .unwrap()
                        .mark_pull_request_creation_time(&from, now);
                    overhead += node.lock().unwrap().process_pull_response(&from, rsp, now);
                });
                (bytes, msgs, overhead)
            })
            .collect();
        for (b, m, o) in transfered {
            bytes += b;
            msgs += m;
            overhead += o;
        }
        let total: usize = network_values
            .par_iter()
            .map(|v| v.lock().unwrap().crds.table.len())
            .sum();
        convergance = total as f64 / ((num * num) as f64);
        if convergance > max_convergance {
            break;
        }
        trace!(
                "network_run_pull_{}: now: {} connections: {} convergance: {} bytes: {} msgs: {} overhead: {}",
                num,
                now,
                total,
                convergance,
                bytes,
                msgs,
                overhead
            );
    }
    (convergance, bytes)
}

#[test]
fn test_star_network_pull_50() {
    let mut network = star_network_create(50);
    network_simulator_pull_only(&mut network);
}
#[test]
fn test_star_network_pull_100() {
    let mut network = star_network_create(100);
    network_simulator_pull_only(&mut network);
}
#[test]
fn test_star_network_push_star_200() {
    let mut network = star_network_create(200);
    network_simulator(&mut network);
}
#[test]
fn test_star_network_push_rstar_200() {
    let mut network = rstar_network_create(200);
    network_simulator(&mut network);
}
#[test]
fn test_star_network_push_ring_200() {
    let mut network = ring_network_create(200);
    network_simulator(&mut network);
}
#[test]
#[ignore]
fn test_star_network_large_pull() {
    solana_logger::setup();
    let mut network = star_network_create(2000);
    network_simulator_pull_only(&mut network);
}
#[test]
#[ignore]
fn test_rstar_network_large_push() {
    solana_logger::setup();
    let mut network = rstar_network_create(4000);
    network_simulator(&mut network);
}
#[test]
#[ignore]
fn test_ring_network_large_push() {
    solana_logger::setup();
    let mut network = ring_network_create(4001);
    network_simulator(&mut network);
}
#[test]
#[ignore]
fn test_star_network_large_push() {
    solana_logger::setup();
    let mut network = star_network_create(4002);
    network_simulator(&mut network);
}
#[test]
fn test_prune_errors() {
    let mut crds_gossip = CrdsGossip::default();
    crds_gossip.id = Pubkey::new(&[0; 32]);
    let id = crds_gossip.id;
    let ci = ContactInfo::new_localhost(&Pubkey::new(&[1; 32]), 0);
    let prune_pubkey = Pubkey::new(&[2; 32]);
    crds_gossip
        .crds
        .insert(CrdsValue::ContactInfo(ci.clone()), 0)
        .unwrap();
    crds_gossip.refresh_push_active_set(&HashMap::new());
    let now = timestamp();
    //incorrect dest
    let mut res = crds_gossip.process_prune_msg(
        &ci.id,
        &Pubkey::new(hash(&[1; 32]).as_ref()),
        &[prune_pubkey],
        now,
        now,
    );
    assert_eq!(res.err(), Some(CrdsGossipError::BadPruneDestination));
    //correct dest
    res = crds_gossip.process_prune_msg(&ci.id, &id, &[prune_pubkey], now, now);
    res.unwrap();
    //test timeout
    let timeout = now + crds_gossip.push.prune_timeout * 2;
    res = crds_gossip.process_prune_msg(&ci.id, &id, &[prune_pubkey], now, timeout);
    assert_eq!(res.err(), Some(CrdsGossipError::PruneMessageTimeout));
}
