//! Crds Gossip Push overlay
//! This module is used to propagate recently created CrdsValues across the network
//! Eager push strategy is based on Plumtree
//! http://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf
//!
//! Main differences are:
//! 1. There is no `max hop`.  Messages are signed with a local wallclock.  If they are outside of
//!    the local nodes wallclock window they are drooped silently.
//! 2. The prune set is stored in a Bloom filter.

use crate::{
    contact_info::ContactInfo,
    crds::{Crds, VersionedCrdsValue},
    crds_gossip::{get_stake, get_weight, CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS},
    crds_gossip_error::CrdsGossipError,
    crds_value::{CrdsValue, CrdsValueLabel},
    weighted_shuffle::weighted_shuffle,
};
use bincode::serialized_size;
use indexmap::map::IndexMap;
use itertools::Itertools;
use rand::{self, seq::SliceRandom, thread_rng, RngCore};
use solana_runtime::bloom::Bloom;
use solana_sdk::{hash::Hash, packet::PACKET_DATA_SIZE, pubkey::Pubkey, timing::timestamp};
use std::{
    cmp,
    collections::{HashMap, HashSet},
};

pub const CRDS_GOSSIP_NUM_ACTIVE: usize = 30;
pub const CRDS_GOSSIP_PUSH_FANOUT: usize = 6;
// With a fanout of 6, a 1000 node cluster should only take ~4 hops to converge.
// However since pushes are stake weighed, some trailing nodes
// might need more time to receive values. 30 seconds should be plenty.
pub const CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS: u64 = 30000;
pub const CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS: u64 = 500;
pub const CRDS_GOSSIP_PRUNE_STAKE_THRESHOLD_PCT: f64 = 0.15;

#[derive(Clone)]
pub struct CrdsGossipPush {
    /// max bytes per message
    pub max_bytes: usize,
    /// active set of validators for push
    active_set: IndexMap<Pubkey, Bloom<Pubkey>>,
    /// push message queue
    push_messages: HashMap<CrdsValueLabel, Hash>,
    /// cache that tracks which validators a message was received from
    received_cache: HashMap<Hash, (u64, HashSet<Pubkey>)>,
    pub num_active: usize,
    pub push_fanout: usize,
    pub msg_timeout: u64,
    pub prune_timeout: u64,
}

impl Default for CrdsGossipPush {
    fn default() -> Self {
        Self {
            // Allow upto 64 Crds Values per PUSH
            max_bytes: PACKET_DATA_SIZE * 64,
            active_set: IndexMap::new(),
            push_messages: HashMap::new(),
            received_cache: HashMap::new(),
            num_active: CRDS_GOSSIP_NUM_ACTIVE,
            push_fanout: CRDS_GOSSIP_PUSH_FANOUT,
            msg_timeout: CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS,
            prune_timeout: CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS,
        }
    }
}
impl CrdsGossipPush {
    pub fn num_pending(&self) -> usize {
        self.push_messages.len()
    }

    fn prune_stake_threshold(self_stake: u64, origin_stake: u64) -> u64 {
        let min_path_stake = self_stake.min(origin_stake);
        ((CRDS_GOSSIP_PRUNE_STAKE_THRESHOLD_PCT * min_path_stake as f64).round() as u64).max(1)
    }

    pub fn prune_received_cache(
        &mut self,
        self_pubkey: &Pubkey,
        origin: &Pubkey,
        hash: Hash,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Vec<Pubkey> {
        let origin_stake = stakes.get(origin).unwrap_or(&0);
        let self_stake = stakes.get(self_pubkey).unwrap_or(&0);
        let cache = self.received_cache.get(&hash);
        if cache.is_none() {
            return Vec::new();
        }

        let peers = &cache.unwrap().1;
        let peer_stake_total: u64 = peers.iter().map(|p| stakes.get(p).unwrap_or(&0)).sum();
        let prune_stake_threshold = Self::prune_stake_threshold(*self_stake, *origin_stake);
        if peer_stake_total < prune_stake_threshold {
            return Vec::new();
        }

        let staked_peers: Vec<(Pubkey, u64)> = peers
            .iter()
            .filter_map(|p| stakes.get(p).map(|s| (*p, *s)))
            .filter(|(_, s)| *s > 0)
            .collect();

        let mut seed = [0; 32];
        seed[0..8].copy_from_slice(&thread_rng().next_u64().to_le_bytes());
        let shuffle = weighted_shuffle(
            staked_peers.iter().map(|(_, stake)| *stake).collect_vec(),
            seed,
        );

        let mut keep = HashSet::new();
        let mut peer_stake_sum = 0;
        for next in shuffle {
            let (next_peer, next_stake) = staked_peers[next];
            keep.insert(next_peer);
            peer_stake_sum += next_stake;
            if peer_stake_sum >= prune_stake_threshold {
                break;
            }
        }

        peers
            .iter()
            .filter(|p| !keep.contains(p))
            .cloned()
            .collect()
    }

    /// process a push message to the network
    pub fn process_push_message(
        &mut self,
        crds: &mut Crds,
        from: &Pubkey,
        value: CrdsValue,
        now: u64,
    ) -> Result<Option<VersionedCrdsValue>, CrdsGossipError> {
        if now
            > value
                .wallclock()
                .checked_add(self.msg_timeout)
                .unwrap_or_else(|| 0)
        {
            return Err(CrdsGossipError::PushMessageTimeout);
        }
        if now + self.msg_timeout < value.wallclock() {
            return Err(CrdsGossipError::PushMessageTimeout);
        }
        let label = value.label();
        let new_value = crds.new_versioned(now, value);
        let value_hash = new_value.value_hash;
        if let Some((_, ref mut received_set)) = self.received_cache.get_mut(&value_hash) {
            received_set.insert(from.clone());
            return Err(CrdsGossipError::PushMessageAlreadyReceived);
        }
        let old = crds.insert_versioned(new_value);
        if old.is_err() {
            return Err(CrdsGossipError::PushMessageOldVersion);
        }
        let mut received_set = HashSet::new();
        received_set.insert(from.clone());
        self.push_messages.insert(label, value_hash);
        self.received_cache.insert(value_hash, (now, received_set));
        Ok(old.ok().and_then(|opt| opt))
    }

    /// New push message to broadcast to peers.
    /// Returns a list of Pubkeys for the selected peers and a list of values to send to all the
    /// peers.
    /// The list of push messages is created such that all the randomly selected peers have not
    /// pruned the source addresses.
    pub fn new_push_messages(&mut self, crds: &Crds, now: u64) -> HashMap<Pubkey, Vec<CrdsValue>> {
        let max = self.active_set.len();
        let mut nodes: Vec<_> = (0..max).collect();
        nodes.shuffle(&mut rand::thread_rng());
        let peers: Vec<Pubkey> = nodes
            .into_iter()
            .filter_map(|n| self.active_set.get_index(n))
            .take(self.push_fanout)
            .map(|n| *n.0)
            .collect();
        let mut total_bytes: usize = 0;
        let mut values = vec![];
        let mut push_messages: HashMap<Pubkey, Vec<CrdsValue>> = HashMap::new();
        for (label, hash) in &self.push_messages {
            let res = crds.lookup_versioned(label);
            if res.is_none() {
                continue;
            }
            let version = res.unwrap();
            if version.value_hash != *hash {
                continue;
            }
            let value = &version.value;
            if value.wallclock() > now || value.wallclock() + self.msg_timeout < now {
                continue;
            }
            total_bytes += serialized_size(value).unwrap() as usize;
            if total_bytes > self.max_bytes {
                break;
            }
            values.push(value.clone());
        }
        for v in values {
            for p in peers.iter() {
                let filter = self.active_set.get_mut(p);
                if filter.is_some() && !filter.unwrap().contains(&v.label().pubkey()) {
                    push_messages.entry(*p).or_default().push(v.clone());
                }
            }
            self.push_messages.remove(&v.label());
        }
        push_messages
    }

    /// add the `from` to the peer's filter of nodes
    pub fn process_prune_msg(&mut self, peer: &Pubkey, origins: &[Pubkey]) {
        for origin in origins {
            if let Some(p) = self.active_set.get_mut(peer) {
                p.add(origin)
            }
        }
    }

    fn compute_need(num_active: usize, active_set_len: usize, ratio: usize) -> usize {
        let num = active_set_len / ratio;
        cmp::min(num_active, (num_active - active_set_len) + num)
    }

    /// refresh the push active set
    /// * ratio - active_set.len()/ratio is the number of actives to rotate
    pub fn refresh_push_active_set(
        &mut self,
        crds: &Crds,
        stakes: &HashMap<Pubkey, u64>,
        self_id: &Pubkey,
        self_shred_version: u16,
        network_size: usize,
        ratio: usize,
    ) {
        let need = Self::compute_need(self.num_active, self.active_set.len(), ratio);
        let mut new_items = HashMap::new();

        let options: Vec<_> = self.push_options(crds, &self_id, self_shred_version, stakes);
        if options.is_empty() {
            return;
        }

        let mut seed = [0; 32];
        seed[0..8].copy_from_slice(&thread_rng().next_u64().to_le_bytes());
        let mut shuffle = weighted_shuffle(
            options.iter().map(|weighted| weighted.0).collect_vec(),
            seed,
        )
        .into_iter();

        while new_items.len() < need {
            match shuffle.next() {
                Some(index) => {
                    let item = options[index].1;
                    if self.active_set.get(&item.id).is_some() {
                        continue;
                    }
                    if new_items.get(&item.id).is_some() {
                        continue;
                    }
                    let size = cmp::max(CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS, network_size);
                    let mut bloom = Bloom::random(size, 0.1, 1024 * 8 * 4);
                    bloom.add(&item.id);
                    new_items.insert(item.id, bloom);
                }
                _ => break,
            }
        }
        let mut keys: Vec<Pubkey> = self.active_set.keys().cloned().collect();
        keys.shuffle(&mut rand::thread_rng());
        let num = keys.len() / ratio;
        for k in &keys[..num] {
            self.active_set.swap_remove(k);
        }
        for (k, v) in new_items {
            self.active_set.insert(k, v);
        }
    }

    fn push_options<'a>(
        &self,
        crds: &'a Crds,
        self_id: &Pubkey,
        self_shred_version: u16,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Vec<(f32, &'a ContactInfo)> {
        crds.table
            .values()
            .filter(|v| v.value.contact_info().is_some())
            .map(|v| (v.value.contact_info().unwrap(), v))
            .filter(|(info, _)| {
                info.id != *self_id
                    && ContactInfo::is_valid_address(&info.gossip)
                    && (self_shred_version == 0
                        || info.shred_version == 0
                        || self_shred_version == info.shred_version)
            })
            .map(|(info, value)| {
                let max_weight = f32::from(u16::max_value()) - 1.0;
                let last_updated: u64 = value.local_timestamp;
                let since = ((timestamp() - last_updated) / 1024) as u32;
                let stake = get_stake(&info.id, stakes);
                let weight = get_weight(max_weight, since, stake);
                (weight, info)
            })
            .collect()
    }

    /// purge old pending push messages
    pub fn purge_old_pending_push_messages(&mut self, crds: &Crds, min_time: u64) {
        let old_msgs: Vec<CrdsValueLabel> = self
            .push_messages
            .iter()
            .filter_map(|(k, hash)| {
                if let Some(versioned) = crds.lookup_versioned(k) {
                    if versioned.value.wallclock() < min_time || versioned.value_hash != *hash {
                        Some(k)
                    } else {
                        None
                    }
                } else {
                    Some(k)
                }
            })
            .cloned()
            .collect();
        for k in old_msgs {
            self.push_messages.remove(&k);
        }
    }

    /// purge received push message cache
    pub fn purge_old_received_cache(&mut self, min_time: u64) {
        let old_msgs: Vec<Hash> = self
            .received_cache
            .iter()
            .filter_map(|(k, (rcvd_time, _))| if *rcvd_time < min_time { Some(k) } else { None })
            .cloned()
            .collect();
        for k in old_msgs {
            self.received_cache.remove(&k);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contact_info::ContactInfo;
    use crate::crds_value::CrdsData;

    #[test]
    fn test_prune() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut stakes = HashMap::new();

        let self_id = Pubkey::new_rand();
        let origin = Pubkey::new_rand();
        stakes.insert(self_id, 100);
        stakes.insert(origin, 100);

        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &origin, 0,
        )));
        let label = value.label();
        let low_staked_peers = (0..10).map(|_| Pubkey::new_rand());
        let mut low_staked_set = HashSet::new();
        low_staked_peers.for_each(|p| {
            let _ = push.process_push_message(&mut crds, &p, value.clone(), 0);
            low_staked_set.insert(p);
            stakes.insert(p, 1);
        });

        let versioned = crds
            .lookup_versioned(&label)
            .expect("versioned value should exist");
        let hash = versioned.value_hash;
        let pruned = push.prune_received_cache(&self_id, &origin, hash, &stakes);
        assert!(
            pruned.is_empty(),
            "should not prune if min threshold has not been reached"
        );

        let high_staked_peer = Pubkey::new_rand();
        let high_stake = CrdsGossipPush::prune_stake_threshold(100, 100) + 10;
        stakes.insert(high_staked_peer, high_stake);
        let _ = push.process_push_message(&mut crds, &high_staked_peer, value, 0);

        let pruned = push.prune_received_cache(&self_id, &origin, hash, &stakes);
        assert!(
            pruned.len() < low_staked_set.len() + 1,
            "should not prune all peers"
        );
        pruned.iter().for_each(|p| {
            assert!(
                low_staked_set.contains(p),
                "only low staked peers should be pruned"
            );
        });
    }

    #[test]
    fn test_process_push() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        let label = value.label();
        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value.clone(), 0),
            Ok(None)
        );
        assert_eq!(crds.lookup(&label), Some(&value));

        // push it again
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, 0),
            Err(CrdsGossipError::PushMessageAlreadyReceived)
        );
    }
    #[test]
    fn test_process_push_old_version() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
        ci.wallclock = 1;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone()));

        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, 0),
            Ok(None)
        );

        // push an old version
        ci.wallclock = 0;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, 0),
            Err(CrdsGossipError::PushMessageOldVersion)
        );
    }
    #[test]
    fn test_process_push_timeout() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let timeout = push.msg_timeout;
        let mut ci = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);

        // push a version to far in the future
        ci.wallclock = timeout + 1;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone()));
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, 0),
            Err(CrdsGossipError::PushMessageTimeout)
        );

        // push a version to far in the past
        ci.wallclock = 0;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, timeout + 1),
            Err(CrdsGossipError::PushMessageTimeout)
        );
    }
    #[test]
    fn test_process_push_update() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
        ci.wallclock = 0;
        let value_old = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone()));

        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value_old.clone(), 0),
            Ok(None)
        );

        // push an old version
        ci.wallclock = 1;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, 0)
                .unwrap()
                .unwrap()
                .value,
            value_old
        );
    }
    #[test]
    fn test_compute_need() {
        assert_eq!(CrdsGossipPush::compute_need(30, 0, 10), 30);
        assert_eq!(CrdsGossipPush::compute_need(30, 1, 10), 29);
        assert_eq!(CrdsGossipPush::compute_need(30, 30, 10), 3);
        assert_eq!(CrdsGossipPush::compute_need(30, 29, 10), 3);
    }
    #[test]
    fn test_refresh_active_set() {
        solana_logger::setup();
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let value1 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));

        assert_eq!(crds.insert(value1.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);

        assert!(push.active_set.get(&value1.label().pubkey()).is_some());
        let value2 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert!(push.active_set.get(&value2.label().pubkey()).is_none());
        assert_eq!(crds.insert(value2.clone(), 0), Ok(None));
        for _ in 0..30 {
            push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);
            if push.active_set.get(&value2.label().pubkey()).is_some() {
                break;
            }
        }
        assert!(push.active_set.get(&value2.label().pubkey()).is_some());

        for _ in 0..push.num_active {
            let value2 = CrdsValue::new_unsigned(CrdsData::ContactInfo(
                ContactInfo::new_localhost(&Pubkey::new_rand(), 0),
            ));
            assert_eq!(crds.insert(value2.clone(), 0), Ok(None));
        }
        push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);
        assert_eq!(push.active_set.len(), push.num_active);
    }
    #[test]
    fn test_active_set_refresh_with_bank() {
        let time = timestamp() - 1024; //make sure there's at least a 1 second delay
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let mut stakes = HashMap::new();
        for i in 1..=100 {
            let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &Pubkey::new_rand(),
                time,
            )));
            let id = peer.label().pubkey();
            crds.insert(peer.clone(), time).unwrap();
            stakes.insert(id, i * 100);
        }
        let mut options = push.push_options(&crds, &Pubkey::default(), 0, &stakes);
        assert!(!options.is_empty());
        options.sort_by(|(weight_l, _), (weight_r, _)| weight_r.partial_cmp(weight_l).unwrap());
        // check that the highest stake holder is also the heaviest weighted.
        assert_eq!(
            *stakes.get(&options.get(0).unwrap().1.id).unwrap(),
            10_000_u64
        );
    }

    #[test]
    fn test_no_pushes_to_from_different_shred_versions() {
        let mut crds = Crds::default();
        let stakes = HashMap::new();
        let node = CrdsGossipPush::default();

        let gossip = socketaddr!("127.0.0.1:1234");

        let me = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: Pubkey::new_rand(),
            shred_version: 123,
            gossip,
            ..ContactInfo::default()
        }));
        let spy = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: Pubkey::new_rand(),
            shred_version: 0,
            gossip,
            ..ContactInfo::default()
        }));
        let node_123 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: Pubkey::new_rand(),
            shred_version: 123,
            gossip,
            ..ContactInfo::default()
        }));
        let node_456 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: Pubkey::new_rand(),
            shred_version: 456,
            gossip,
            ..ContactInfo::default()
        }));

        crds.insert(me.clone(), 0).unwrap();
        crds.insert(spy.clone(), 0).unwrap();
        crds.insert(node_123.clone(), 0).unwrap();
        crds.insert(node_456.clone(), 0).unwrap();

        // shred version 123 should ignore 456 nodes
        let options = node
            .push_options(&crds, &me.label().pubkey(), 123, &stakes)
            .iter()
            .map(|(_, c)| c.id)
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 2);
        assert!(options.contains(&spy.pubkey()));
        assert!(options.contains(&node_123.pubkey()));

        // spy nodes will see all
        let options = node
            .push_options(&crds, &spy.label().pubkey(), 0, &stakes)
            .iter()
            .map(|(_, c)| c.id)
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 3);
        assert!(options.contains(&me.pubkey()));
        assert!(options.contains(&node_123.pubkey()));
        assert!(options.contains(&node_456.pubkey()));
    }
    #[test]
    fn test_new_push_messages() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert_eq!(crds.insert(peer.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);

        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        let mut expected = HashMap::new();
        expected.insert(peer.label().pubkey(), vec![new_msg.clone()]);
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), new_msg, 0),
            Ok(None)
        );
        assert_eq!(push.active_set.len(), 1);
        assert_eq!(push.new_push_messages(&crds, 0), expected);
    }
    #[test]
    fn test_personalized_push_messages() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer_1 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert_eq!(crds.insert(peer_1.clone(), 0), Ok(None));
        let peer_2 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert_eq!(crds.insert(peer_2.clone(), 0), Ok(None));
        let peer_3 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), peer_3.clone(), 0),
            Ok(None)
        );
        push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);

        // push 3's contact info to 1 and 2 and 3
        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &peer_3.pubkey(),
            0,
        )));
        let mut expected = HashMap::new();
        expected.insert(peer_1.pubkey(), vec![new_msg.clone()]);
        expected.insert(peer_2.pubkey(), vec![new_msg]);
        assert_eq!(push.active_set.len(), 3);
        assert_eq!(push.new_push_messages(&crds, 0), expected);
    }
    #[test]
    fn test_process_prune() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert_eq!(crds.insert(peer.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);

        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        let expected = HashMap::new();
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), new_msg.clone(), 0),
            Ok(None)
        );
        push.process_prune_msg(&peer.label().pubkey(), &[new_msg.label().pubkey()]);
        assert_eq!(push.new_push_messages(&crds, 0), expected);
    }
    #[test]
    fn test_purge_old_pending_push_messages() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        )));
        assert_eq!(crds.insert(peer, 0), Ok(None));
        push.refresh_push_active_set(&crds, &HashMap::new(), &Pubkey::default(), 0, 1, 1);

        let mut ci = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
        ci.wallclock = 1;
        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        let expected = HashMap::new();
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), new_msg, 1),
            Ok(None)
        );
        push.purge_old_pending_push_messages(&crds, 0);
        assert_eq!(push.new_push_messages(&crds, 0), expected);
    }

    #[test]
    fn test_purge_old_received_cache() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&Pubkey::new_rand(), 0);
        ci.wallclock = 0;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        let label = value.label();
        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value.clone(), 0),
            Ok(None)
        );
        assert_eq!(crds.lookup(&label), Some(&value));

        // push it again
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value.clone(), 0),
            Err(CrdsGossipError::PushMessageAlreadyReceived)
        );

        // purge the old pushed
        push.purge_old_received_cache(1);

        // push it again
        assert_eq!(
            push.process_push_message(&mut crds, &Pubkey::default(), value, 0),
            Err(CrdsGossipError::PushMessageOldVersion)
        );
    }
}
