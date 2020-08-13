//! Crds Gossip
//! This module ties together Crds and the push and pull gossip overlays.  The interface is
//! designed to run with a simulator or over a UDP network connection with messages up to a
//! packet::PACKET_DATA_SIZE size.

use crate::{
    crds::{Crds, VersionedCrdsValue},
    crds_gossip_error::CrdsGossipError,
    crds_gossip_pull::{CrdsFilter, CrdsGossipPull, ProcessPullStats},
    crds_gossip_push::{CrdsGossipPush, CRDS_GOSSIP_NUM_ACTIVE},
    crds_value::{CrdsValue, CrdsValueLabel},
};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

///The min size for bloom filters
pub const CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS: usize = 500;

pub struct CrdsGossip {
    pub crds: Crds,
    pub id: RwLock<Pubkey>,
    pub shred_version: RwLock<u16>,
    pub push: RwLock<CrdsGossipPush>,
    pub pull: RwLock<CrdsGossipPull>,
}

impl Default for CrdsGossip {
    fn default() -> Self {
        CrdsGossip {
            crds: Crds::default(),
            id: RwLock::new(Pubkey::default()),
            shred_version: RwLock::new(0),
            push: RwLock::new(CrdsGossipPush::default()),
            pull: RwLock::new(CrdsGossipPull::default()),
        }
    }
}

impl CrdsGossip {
    pub fn set_self(&self, id: &Pubkey) {
        *self.id.write().unwrap() = *id;
    }
    pub fn set_shred_version(&self, shred_version: u16) {
        *self.shred_version.write().unwrap() = shred_version;
    }

    /// process a push message to the network
    pub fn process_push_message(
        &self,
        from: &Pubkey,
        values: Vec<CrdsValue>,
        now: u64,
    ) -> Vec<VersionedCrdsValue> {
        values
            .into_iter()
            .filter_map(|val| {
                let res = self
                    .push
                    .write()
                    .unwrap()
                    .process_push_message(&self.crds, from, val, now);
                if let Ok(Some(val)) = res {
                    self.pull
                        .write()
                        .unwrap()
                        .record_old_hash(val.value_hash, val.local_timestamp);
                    Some(val)
                } else {
                    None
                }
            })
            .collect()
    }

    /// remove redundant paths in the network
    pub fn prune_received_cache(
        &self,
        labels: Vec<CrdsValueLabel>,
        stakes: &HashMap<Pubkey, u64>,
    ) -> HashMap<Pubkey, HashSet<Pubkey>> {
        let id = self.id.read().unwrap();
        let push = &mut self.push;
        let mut prune_map: HashMap<Pubkey, HashSet<_>> = HashMap::new();
        for origin in labels.iter().map(|k| k.pubkey()) {
            let peers = push
                .write()
                .unwrap()
                .prune_received_cache(&id, &origin, stakes);
            for from in peers {
                prune_map.entry(from).or_default().insert(origin);
            }
        }
        prune_map
    }

    pub fn new_push_messages(&mut self, now: u64) -> (Pubkey, HashMap<Pubkey, Vec<CrdsValue>>) {
        let push_messages = self
            .push
            .write()
            .unwrap()
            .new_push_messages(&self.crds, now);
        (*self.id.read().unwrap(), push_messages)
    }

    /// add the `from` to the peer's filter of nodes
    pub fn process_prune_msg(
        &mut self,
        peer: &Pubkey,
        destination: &Pubkey,
        origin: &[Pubkey],
        wallclock: u64,
        now: u64,
    ) -> Result<(), CrdsGossipError> {
        let expired = now > wallclock + self.push.read().unwrap().prune_timeout;
        if expired {
            return Err(CrdsGossipError::PruneMessageTimeout);
        }
        let id = *self.id.read().unwrap();
        if id == *destination {
            self.push
                .write()
                .unwrap()
                .process_prune_msg(&id, peer, origin);
            Ok(())
        } else {
            Err(CrdsGossipError::BadPruneDestination)
        }
    }

    /// refresh the push active set
    /// * ratio - number of actives to rotate
    pub fn refresh_push_active_set(&self, stakes: &HashMap<Pubkey, u64>) {
        let id = *self.id.read().unwrap();
        let shred_version = *self.shred_version.read().unwrap();
        self.push.write().unwrap().refresh_push_active_set(
            &self.crds,
            stakes,
            &id,
            shred_version,
            self.pull.read().unwrap().pull_request_time.len(),
            CRDS_GOSSIP_NUM_ACTIVE,
        )
    }

    /// generate a random request
    pub fn new_pull_request(
        &self,
        now: u64,
        stakes: &HashMap<Pubkey, u64>,
        bloom_size: usize,
    ) -> Result<(Pubkey, Vec<CrdsFilter>, CrdsValue), CrdsGossipError> {
        let id = *self.id.read().unwrap();
        let shred_version = *self.shred_version.read().unwrap();
        self.pull.read().unwrap().new_pull_request(
            &self.crds,
            &id,
            shred_version,
            now,
            stakes,
            bloom_size,
        )
    }

    /// time when a request to `from` was initiated
    /// This is used for weighted random selection during `new_pull_request`
    /// It's important to use the local nodes request creation time as the weight
    /// instead of the response received time otherwise failed nodes will increase their weight.
    pub fn mark_pull_request_creation_time(&self, from: &Pubkey, now: u64) {
        self.pull
            .write()
            .unwrap()
            .mark_pull_request_creation_time(from, now)
    }
    /// process a pull request and create a response
    pub fn process_pull_requests(&self, filters: Vec<(CrdsValue, CrdsFilter)>, now: u64) {
        self.pull
            .write()
            .unwrap()
            .process_pull_requests(&self.crds, filters, now);
    }

    pub fn generate_pull_responses(
        &self,
        filters: &[(CrdsValue, CrdsFilter)],
        now: u64,
    ) -> Vec<Vec<CrdsValue>> {
        self.pull
            .write()
            .unwrap()
            .generate_pull_responses(&self.crds, filters, now)
    }

    pub fn filter_pull_responses(
        &self,
        timeouts: &HashMap<Pubkey, u64>,
        response: Vec<CrdsValue>,
        now: u64,
        process_pull_stats: &mut ProcessPullStats,
    ) -> (Vec<VersionedCrdsValue>, Vec<VersionedCrdsValue>) {
        self.pull
            .read().unwrap().filter_pull_responses(&self.crds, timeouts, response, now, process_pull_stats)
    }

    /// process a pull response
    pub fn process_pull_responses(
        &mut self,
        from: &Pubkey,
        responses: Vec<VersionedCrdsValue>,
        responses_expired_timeout: Vec<VersionedCrdsValue>,
        now: u64,
        process_pull_stats: &mut ProcessPullStats,
    ) {
        let success = self.pull.write().unwrap().process_pull_responses(
            &self.crds,
            from,
            responses,
            responses_expired_timeout,
            now,
            process_pull_stats,
        );
        self.push.write().unwrap().push_pull_responses(success, now);
    }

    pub fn make_timeouts_test(&self) -> HashMap<Pubkey, u64> {
        self.make_timeouts(&HashMap::new(), self.pull.read().unwrap().crds_timeout)
    }

    pub fn make_timeouts(
        &self,
        stakes: &HashMap<Pubkey, u64>,
        epoch_ms: u64,
    ) -> HashMap<Pubkey, u64> {
        let id = *self.id.read().unwrap();
        self.pull.read().unwrap().make_timeouts(&id, stakes, epoch_ms)
    }

    pub fn purge(&mut self, now: u64, timeouts: &HashMap<Pubkey, u64>) -> usize {
        let mut rv = 0;
        let push_msg_timeout = self.push.read().unwrap().msg_timeout;
        if now > push_msg_timeout {
            let min = now - push_msg_timeout;
            self.push.write().unwrap().purge_old_pending_push_messages(&self.crds, min);
        }
        if now > 5 * push_msg_timeout {
            let min = now - 5 * push_msg_timeout;
            self.push.write().unwrap().purge_old_received_cache(min);
        }
        let pull_crds_timeout = self.pull.read().unwrap().crds_timeout;
        if now > pull_crds_timeout {
            //sanity check
            let id = *self.id.read().unwrap();
            let min = pull_crds_timeout;
            assert_eq!(timeouts[&id], std::u64::MAX);
            assert_eq!(timeouts[&Pubkey::default()], min);
            rv = self.pull.write().unwrap().purge_active(&mut self.crds, now, &timeouts);
        }
        if now > 5 * pull_crds_timeout {
            let min = now - 5 * pull_crds_timeout;
            self.pull.write().unwrap().purge_purged(min);
        }
        rv
    }
}

/// Computes a normalized(log of actual stake) stake
pub fn get_stake<S: std::hash::BuildHasher>(id: &Pubkey, stakes: &HashMap<Pubkey, u64, S>) -> f32 {
    // cap the max balance to u32 max (it should be plenty)
    let bal = f64::from(u32::max_value()).min(*stakes.get(id).unwrap_or(&0) as f64);
    1_f32.max((bal as f32).ln())
}

/// Computes bounded weight given some max, a time since last selected, and a stake value
/// The minimum stake is 1 and not 0 to allow 'time since last' picked to factor in.
pub fn get_weight(max_weight: f32, time_since_last_selected: u32, stake: f32) -> f32 {
    let mut weight = time_since_last_selected as f32 * stake;
    if weight.is_infinite() {
        weight = max_weight;
    }
    1.0_f32.max(weight.min(max_weight))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contact_info::ContactInfo;
    use crate::crds_value::CrdsData;
    use solana_sdk::hash::hash;
    use solana_sdk::timing::timestamp;

    #[test]
    fn test_prune_errors() {
        let mut crds_gossip = CrdsGossip::default();
        crds_gossip.set_self(&Pubkey::new(&[0; 32]));
        let id = *crds_gossip.id.read().unwrap();
        let ci = ContactInfo::new_localhost(&Pubkey::new(&[1; 32]), 0);
        let prune_pubkey = Pubkey::new(&[2; 32]);
        crds_gossip
            .crds
            .insert(
                CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone())),
                0,
            )
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
        let timeout = now + crds_gossip.push.read().unwrap().prune_timeout * 2;
        res = crds_gossip.process_prune_msg(&ci.id, &id, &[prune_pubkey], now, timeout);
        assert_eq!(res.err(), Some(CrdsGossipError::PruneMessageTimeout));
    }
}
