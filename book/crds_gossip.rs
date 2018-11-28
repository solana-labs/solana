//! Crds Gossip
//! This module ties together Crds and the push and pull gossip overlays.  The interface is
//! designed to run with a simulator or over a UDP network connection with messages up to a
//! packet::BLOB_DATA_SIZE size.

use bloom::Bloom;
use crds::Crds;
use crds_gossip_error::CrdsGossipError;
use crds_gossip_pull::CrdsGossipPull;
use crds_gossip_push::{CrdsGossipPush, CRDS_GOSSIP_NUM_ACTIVE};
use crds_value::CrdsValue;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;

pub struct CrdsGossip {
    pub crds: Crds,
    pub id: Pubkey,
    push: CrdsGossipPush,
    pull: CrdsGossipPull,
}

impl Default for CrdsGossip {
    fn default() -> Self {
        CrdsGossip {
            crds: Crds::default(),
            id: Pubkey::default(),
            push: CrdsGossipPush::default(),
            pull: CrdsGossipPull::default(),
        }
    }
}

impl CrdsGossip {
    pub fn set_self(&mut self, id: Pubkey) {
        self.id = id;
    }
    /// process a push message to the network
    pub fn process_push_message(&mut self, values: &[CrdsValue], now: u64) -> Vec<Pubkey> {
        let results: Vec<_> = values
            .iter()
            .map(|val| {
                self.push
                    .process_push_message(&mut self.crds, val.clone(), now)
            }).collect();
        results
            .into_iter()
            .zip(values)
            .filter_map(|(r, d)| {
                if r == Err(CrdsGossipError::PushMessagePrune) {
                    Some(d.label().pubkey())
                } else if let Ok(Some(val)) = r {
                    self.pull
                        .record_old_hash(val.value_hash, val.local_timestamp);
                    None
                } else {
                    None
                }
            }).collect()
    }

    pub fn new_push_messages(&mut self, now: u64) -> (Pubkey, Vec<Pubkey>, Vec<CrdsValue>) {
        let (peers, values) = self.push.new_push_messages(&self.crds, now);
        (self.id, peers, values)
    }

    /// add the `from` to the peer's filter of nodes
    pub fn process_prune_msg(&mut self, peer: Pubkey, origin: &[Pubkey]) {
        self.push.process_prune_msg(peer, origin)
    }

    /// refresh the push active set
    /// * ratio - number of actives to rotate
    pub fn refresh_push_active_set(&mut self) {
        self.push.refresh_push_active_set(
            &self.crds,
            self.id,
            self.pull.pull_request_time.len(),
            CRDS_GOSSIP_NUM_ACTIVE,
        )
    }

    /// generate a random request
    pub fn new_pull_request(
        &self,
        now: u64,
    ) -> Result<(Pubkey, Bloom<Hash>, CrdsValue), CrdsGossipError> {
        self.pull.new_pull_request(&self.crds, self.id, now)
    }

    /// time when a request to `from` was initiated
    /// This is used for weighted random selection durring `new_pull_request`
    /// It's important to use the local nodes request creation time as the weight
    /// instaad of the response received time otherwise failed nodes will increase their weight.
    pub fn mark_pull_request_creation_time(&mut self, from: Pubkey, now: u64) {
        self.pull.mark_pull_request_creation_time(from, now)
    }
    /// process a pull request and create a response
    pub fn process_pull_request(
        &mut self,
        caller: CrdsValue,
        filter: Bloom<Hash>,
        now: u64,
    ) -> Vec<CrdsValue> {
        self.pull
            .process_pull_request(&mut self.crds, caller, filter, now)
    }
    /// process a pull response
    pub fn process_pull_response(
        &mut self,
        from: Pubkey,
        response: Vec<CrdsValue>,
        now: u64,
    ) -> usize {
        self.pull
            .process_pull_response(&mut self.crds, from, response, now)
    }
    pub fn purge(&mut self, now: u64) {
        if now > self.push.msg_timeout {
            let min = now - self.push.msg_timeout;
            self.push.purge_old_pending_push_messages(&self.crds, min);
        }
        if now > 5 * self.push.msg_timeout {
            let min = now - 5 * self.push.msg_timeout;
            self.push.purge_old_pushed_once_messages(min);
        }
        if now > self.pull.crds_timeout {
            let min = now - self.pull.crds_timeout;
            self.pull.purge_active(&mut self.crds, self.id, min);
        }
        if now > 5 * self.pull.crds_timeout {
            let min = now - 5 * self.pull.crds_timeout;
            self.pull.purge_purged(min);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bincode::serialized_size;
    use contact_info::ContactInfo;
    use crds_gossip_push::CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS;
    use crds_value::CrdsValueLabel;
    use rayon::prelude::*;
    use signature::{Keypair, KeypairUtil};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type Node = Arc<Mutex<CrdsGossip>>;
    type Network = HashMap<Pubkey, Node>;
    fn star_network_create(num: usize) -> Network {
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        let mut network: HashMap<_, _> = (1..num)
            .map(|_| {
                let new =
                    CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
                let id = new.label().pubkey();
                let mut node = CrdsGossip::default();
                node.crds.insert(new.clone(), 0).unwrap();
                node.crds.insert(entry.clone(), 0).unwrap();
                node.set_self(id);
                (new.label().pubkey(), Arc::new(Mutex::new(node)))
            }).collect();
        let mut node = CrdsGossip::default();
        let id = entry.label().pubkey();
        node.crds.insert(entry.clone(), 0).unwrap();
        node.set_self(id);
        network.insert(id, Arc::new(Mutex::new(node)));
        network
    }

    fn rstar_network_create(num: usize) -> Network {
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        let mut origin = CrdsGossip::default();
        let id = entry.label().pubkey();
        origin.crds.insert(entry.clone(), 0).unwrap();
        origin.set_self(id);
        let mut network: HashMap<_, _> = (1..num)
            .map(|_| {
                let new =
                    CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
                let id = new.label().pubkey();
                let mut node = CrdsGossip::default();
                node.crds.insert(new.clone(), 0).unwrap();
                origin.crds.insert(new.clone(), 0).unwrap();
                node.set_self(id);
                (new.label().pubkey(), Arc::new(Mutex::new(node)))
            }).collect();
        network.insert(id, Arc::new(Mutex::new(origin)));
        network
    }

    fn ring_network_create(num: usize) -> Network {
        let mut network: HashMap<_, _> = (0..num)
            .map(|_| {
                let new =
                    CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
                let id = new.label().pubkey();
                let mut node = CrdsGossip::default();
                node.crds.insert(new.clone(), 0).unwrap();
                node.set_self(id);
                (new.label().pubkey(), Arc::new(Mutex::new(node)))
            }).collect();
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
            node.lock().unwrap().refresh_push_active_set();
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
                node.process_push_message(&[CrdsValue::ContactInfo(m.clone())], now);
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
                }).collect();
            let transfered: Vec<_> = requests
                .par_iter()
                .map(|(from, peers, msgs)| {
                    let mut bytes: usize = 0;
                    let mut delivered: usize = 0;
                    let mut num_msgs: usize = 0;
                    let mut prunes: usize = 0;
                    for to in peers {
                        bytes += serialized_size(msgs).unwrap() as usize;
                        num_msgs += 1;
                        let rsps = network
                            .get(&to)
                            .map(|node| node.lock().unwrap().process_push_message(&msgs, now))
                            .unwrap();
                        bytes += serialized_size(&rsps).unwrap() as usize;
                        prunes += rsps.len();
                        network
                            .get(&from)
                            .map(|node| node.lock().unwrap().process_prune_msg(*to, &rsps))
                            .unwrap();
                        delivered += rsps.is_empty() as usize;
                    }
                    (bytes, delivered, num_msgs, prunes)
                }).collect();
            for (b, d, m, p) in transfered {
                bytes += b;
                delivered += d;
                num_msgs += m;
                prunes += p;
            }
            if now % CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS == 0 && now > 0 {
                network_values.par_iter().for_each(|node| {
                    node.lock().unwrap().refresh_push_active_set();
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
            let mut requests: Vec<_> = {
                network_values
                    .par_iter()
                    .filter_map(|from| from.lock().unwrap().new_pull_request(now).ok())
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
                        }).unwrap();
                    bytes += serialized_size(&rsp).unwrap() as usize;
                    msgs += rsp.len();
                    network.get(&from).map(|node| {
                        node.lock()
                            .unwrap()
                            .mark_pull_request_creation_time(from, now);
                        overhead += node.lock().unwrap().process_pull_response(from, rsp, now);
                    });
                    (bytes, msgs, overhead)
                }).collect();
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
        use logger;
        logger::setup();
        let mut network = star_network_create(2000);
        network_simulator_pull_only(&mut network);
    }
    #[test]
    #[ignore]
    fn test_rstar_network_large_push() {
        use logger;
        logger::setup();
        let mut network = rstar_network_create(4000);
        network_simulator(&mut network);
    }
    #[test]
    #[ignore]
    fn test_ring_network_large_push() {
        use logger;
        logger::setup();
        let mut network = ring_network_create(4001);
        network_simulator(&mut network);
    }
    #[test]
    #[ignore]
    fn test_star_network_large_push() {
        use logger;
        logger::setup();
        let mut network = star_network_create(4002);
        network_simulator(&mut network);
    }
}
