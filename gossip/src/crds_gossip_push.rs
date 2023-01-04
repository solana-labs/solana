//! Crds Gossip Push overlay.
//!
//! This module is used to propagate recently created CrdsValues across the network
//! Eager push strategy is based on [Plumtree].
//!
//! [Plumtree]: http://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf
//!
//! Main differences are:
//!
//! 1. There is no `max hop`.  Messages are signed with a local wallclock.  If they are outside of
//!    the local nodes wallclock window they are dropped silently.
//! 2. The prune set is stored in a Bloom filter.

use {
    crate::{
        cluster_info::{Ping, CRDS_UNIQUE_PUBKEY_CAPACITY},
        contact_info::ContactInfo,
        crds::{Crds, CrdsError, Cursor, GossipRoute},
        crds_gossip::{self, get_stake, get_weight},
        crds_value::CrdsValue,
        ping_pong::PingCache,
        received_cache::ReceivedCache,
        weighted_shuffle::WeightedShuffle,
    },
    bincode::serialized_size,
    indexmap::map::IndexMap,
    itertools::Itertools,
    lru::LruCache,
    rand::{seq::SliceRandom, Rng},
    solana_bloom::bloom::{AtomicBloom, Bloom},
    solana_sdk::{
        packet::PACKET_DATA_SIZE,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        cmp,
        collections::{HashMap, HashSet},
        iter::repeat,
        net::SocketAddr,
        ops::{DerefMut, RangeBounds},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex, RwLock,
        },
        time::Instant,
    },
};

pub(crate) const CRDS_GOSSIP_NUM_ACTIVE: usize = 30;
const CRDS_GOSSIP_PUSH_FANOUT: usize = 6;
// With a fanout of 6, a 1000 node cluster should only take ~4 hops to converge.
// However since pushes are stake weighed, some trailing nodes
// might need more time to receive values. 30 seconds should be plenty.
pub const CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS: u64 = 30000;
const CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS: u64 = 500;
const CRDS_GOSSIP_PRUNE_STAKE_THRESHOLD_PCT: f64 = 0.15;
const CRDS_GOSSIP_PRUNE_MIN_INGRESS_NODES: usize = 3;
// Do not push to peers which have not been updated for this long.
const PUSH_ACTIVE_TIMEOUT_MS: u64 = 60_000;

pub struct CrdsGossipPush {
    /// Max bytes per message
    max_bytes: usize,
    /// Active set of validators for push
    active_set: RwLock<IndexMap<Pubkey, AtomicBloom<Pubkey>>>,
    /// Cursor into the crds table for values to push.
    crds_cursor: Mutex<Cursor>,
    /// Cache that tracks which validators a message was received from
    /// This cache represents a lagging view of which validators
    /// currently have this node in their `active_set`
    received_cache: Mutex<ReceivedCache>,
    last_pushed_to: RwLock<LruCache</*node:*/ Pubkey, /*timestamp:*/ u64>>,
    num_active: usize,
    push_fanout: usize,
    pub(crate) msg_timeout: u64,
    pub prune_timeout: u64,
    pub num_total: AtomicUsize,
    pub num_old: AtomicUsize,
    pub num_pushes: AtomicUsize,
}

impl Default for CrdsGossipPush {
    fn default() -> Self {
        Self {
            // Allow upto 64 Crds Values per PUSH
            max_bytes: PACKET_DATA_SIZE * 64,
            active_set: RwLock::default(),
            crds_cursor: Mutex::default(),
            received_cache: Mutex::new(ReceivedCache::new(2 * CRDS_UNIQUE_PUBKEY_CAPACITY)),
            last_pushed_to: RwLock::new(LruCache::new(CRDS_UNIQUE_PUBKEY_CAPACITY)),
            num_active: CRDS_GOSSIP_NUM_ACTIVE,
            push_fanout: CRDS_GOSSIP_PUSH_FANOUT,
            msg_timeout: CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS,
            prune_timeout: CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS,
            num_total: AtomicUsize::default(),
            num_old: AtomicUsize::default(),
            num_pushes: AtomicUsize::default(),
        }
    }
}
impl CrdsGossipPush {
    pub fn num_pending(&self, crds: &RwLock<Crds>) -> usize {
        let mut cursor: Cursor = *self.crds_cursor.lock().unwrap();
        crds.read().unwrap().get_entries(&mut cursor).count()
    }

    pub(crate) fn prune_received_cache<I>(
        &self,
        self_pubkey: &Pubkey,
        origins: I, // Unique pubkeys of crds values' owners.
        stakes: &HashMap<Pubkey, u64>,
    ) -> HashMap</*gossip peer:*/ Pubkey, /*origins:*/ Vec<Pubkey>>
    where
        I: IntoIterator<Item = Pubkey>,
    {
        let mut received_cache = self.received_cache.lock().unwrap();
        origins
            .into_iter()
            .flat_map(|origin| {
                received_cache
                    .prune(
                        self_pubkey,
                        origin,
                        CRDS_GOSSIP_PRUNE_STAKE_THRESHOLD_PCT,
                        CRDS_GOSSIP_PRUNE_MIN_INGRESS_NODES,
                        stakes,
                    )
                    .zip(repeat(origin))
            })
            .into_group_map()
    }

    fn wallclock_window(&self, now: u64) -> impl RangeBounds<u64> {
        now.saturating_sub(self.msg_timeout)..=now.saturating_add(self.msg_timeout)
    }

    /// Process a push message to the network.
    ///
    /// Returns origins' pubkeys of upserted values.
    pub(crate) fn process_push_message(
        &self,
        crds: &RwLock<Crds>,
        messages: Vec<(/*from:*/ Pubkey, Vec<CrdsValue>)>,
        now: u64,
    ) -> HashSet<Pubkey> {
        let mut received_cache = self.received_cache.lock().unwrap();
        let mut crds = crds.write().unwrap();
        let wallclock_window = self.wallclock_window(now);
        let mut origins = HashSet::with_capacity(messages.len());
        for (from, values) in messages {
            self.num_total.fetch_add(values.len(), Ordering::Relaxed);
            for value in values {
                if !wallclock_window.contains(&value.wallclock()) {
                    continue;
                }
                let origin = value.pubkey();
                match crds.insert(value, now, GossipRoute::PushMessage) {
                    Ok(()) => {
                        received_cache.record(origin, from, /*num_dups:*/ 0);
                        origins.insert(origin);
                    }
                    Err(CrdsError::DuplicatePush(num_dups)) => {
                        received_cache.record(origin, from, usize::from(num_dups));
                        self.num_old.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        self.num_old.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        origins
    }

    /// New push message to broadcast to peers.
    ///
    /// Returns a list of Pubkeys for the selected peers and a list of values to send to all the
    /// peers.
    /// The list of push messages is created such that all the randomly selected peers have not
    /// pruned the source addresses.
    pub(crate) fn new_push_messages(
        &self,
        crds: &RwLock<Crds>,
        now: u64,
    ) -> (
        HashMap<Pubkey, Vec<CrdsValue>>,
        usize, // number of values
        usize, // number of push messages
    ) {
        let active_set = self.active_set.read().unwrap();
        let active_set_len = active_set.len();
        let push_fanout = self.push_fanout.min(active_set_len);
        if push_fanout == 0 {
            return (HashMap::default(), 0, 0);
        }
        let mut num_pushes = 0;
        let mut num_values = 0;
        let mut total_bytes: usize = 0;
        let mut push_messages: HashMap<Pubkey, Vec<CrdsValue>> = HashMap::new();
        let wallclock_window = self.wallclock_window(now);
        let mut crds_cursor = self.crds_cursor.lock().unwrap();
        // crds should be locked last after self.{active_set,crds_cursor}.
        let crds = crds.read().unwrap();
        let entries = crds
            .get_entries(crds_cursor.deref_mut())
            .map(|entry| &entry.value)
            .filter(|value| wallclock_window.contains(&value.wallclock()));
        for value in entries {
            let serialized_size = serialized_size(&value).unwrap();
            total_bytes = total_bytes.saturating_add(serialized_size as usize);
            if total_bytes > self.max_bytes {
                break;
            }
            num_values += 1;
            let origin = value.pubkey();
            // Use a consistent index for the same origin so the active set
            // learns the MST for that origin.
            let offset = origin.as_ref()[0] as usize;
            for i in offset..offset + push_fanout {
                let index = i % active_set_len;
                let (peer, filter) = active_set.get_index(index).unwrap();
                if !filter.contains(&origin) || value.should_force_push(peer) {
                    trace!("new_push_messages insert {} {:?}", *peer, value);
                    push_messages.entry(*peer).or_default().push(value.clone());
                    num_pushes += 1;
                }
            }
        }
        drop(crds);
        drop(crds_cursor);
        drop(active_set);
        self.num_pushes.fetch_add(num_pushes, Ordering::Relaxed);
        trace!("new_push_messages {} {}", num_values, active_set_len);
        let mut last_pushed_to = self.last_pushed_to.write().unwrap();
        for target_pubkey in push_messages.keys().copied() {
            last_pushed_to.put(target_pubkey, now);
        }
        (push_messages, num_values, num_pushes)
    }

    /// Add the `from` to the peer's filter of nodes.
    pub(crate) fn process_prune_msg(
        &self,
        self_pubkey: &Pubkey,
        peer: &Pubkey,
        origins: &[Pubkey],
    ) {
        if let Some(filter) = self.active_set.read().unwrap().get(peer) {
            for origin in origins {
                if origin != self_pubkey {
                    filter.add(origin);
                }
            }
        }
    }

    fn compute_need(num_active: usize, active_set_len: usize, ratio: usize) -> usize {
        let num = active_set_len / ratio;
        cmp::min(num_active, (num_active - active_set_len) + num)
    }

    /// Refresh the push active set.
    ///
    /// # Arguments
    ///
    /// * ratio - active_set.len()/ratio is the number of actives to rotate
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refresh_push_active_set(
        &self,
        crds: &RwLock<Crds>,
        stakes: &HashMap<Pubkey, u64>,
        gossip_validators: Option<&HashSet<Pubkey>>,
        self_keypair: &Keypair,
        self_shred_version: u16,
        network_size: usize,
        ratio: usize,
        ping_cache: &Mutex<PingCache>,
        pings: &mut Vec<(SocketAddr, Ping)>,
        socket_addr_space: &SocketAddrSpace,
    ) {
        const BLOOM_FALSE_RATE: f64 = 0.1;
        const BLOOM_MAX_BITS: usize = 1024 * 8 * 4;
        #[cfg(debug_assertions)]
        const MIN_NUM_BLOOM_ITEMS: usize = 512;
        #[cfg(not(debug_assertions))]
        const MIN_NUM_BLOOM_ITEMS: usize = CRDS_UNIQUE_PUBKEY_CAPACITY;
        let mut rng = rand::thread_rng();
        let mut new_items = HashMap::new();
        // Gossip peers and respective sampling weights.
        let peers = self.push_options(
            crds,
            &self_keypair.pubkey(),
            self_shred_version,
            stakes,
            gossip_validators,
            socket_addr_space,
        );
        // Check for nodes which have responded to ping messages.
        let peers: Vec<_> = {
            let mut ping_cache = ping_cache.lock().unwrap();
            let mut pingf = move || Ping::new_rand(&mut rng, self_keypair).ok();
            let now = Instant::now();
            peers
                .into_iter()
                .filter(|(_weight, peer)| {
                    let node = (peer.id, peer.gossip);
                    let (check, ping) = ping_cache.check(now, node, &mut pingf);
                    if let Some(ping) = ping {
                        pings.push((peer.gossip, ping));
                    }
                    check
                })
                .collect()
        };
        let (weights, peers): (Vec<_>, Vec<_>) = crds_gossip::dedup_gossip_addresses(peers)
            .into_values()
            .map(|(weight, node)| (weight, node.id))
            .unzip();
        if peers.is_empty() {
            return;
        }
        let num_bloom_items = MIN_NUM_BLOOM_ITEMS.max(network_size);
        let shuffle = WeightedShuffle::new("push-options", &weights).shuffle(&mut rng);
        let mut active_set = self.active_set.write().unwrap();
        let need = Self::compute_need(self.num_active, active_set.len(), ratio);
        for peer in shuffle.map(|i| peers[i]) {
            if new_items.len() >= need {
                break;
            }
            if active_set.contains_key(&peer) || new_items.contains_key(&peer) {
                continue;
            }
            let bloom = AtomicBloom::from(Bloom::random(
                num_bloom_items,
                BLOOM_FALSE_RATE,
                BLOOM_MAX_BITS,
            ));
            bloom.add(&peer);
            new_items.insert(peer, bloom);
        }
        let mut keys: Vec<Pubkey> = active_set.keys().cloned().collect();
        keys.shuffle(&mut rng);
        let num = keys.len() / ratio;
        for k in &keys[..num] {
            active_set.swap_remove(k);
        }
        for (k, v) in new_items {
            active_set.insert(k, v);
        }
    }

    fn push_options(
        &self,
        crds: &RwLock<Crds>,
        self_id: &Pubkey,
        self_shred_version: u16,
        stakes: &HashMap<Pubkey, u64>,
        gossip_validators: Option<&HashSet<Pubkey>>,
        socket_addr_space: &SocketAddrSpace,
    ) -> Vec<(/*weight:*/ u64, /*node:*/ ContactInfo)> {
        let now = timestamp();
        let mut rng = rand::thread_rng();
        let max_weight = u16::MAX as f32 - 1.0;
        let active_cutoff = now.saturating_sub(PUSH_ACTIVE_TIMEOUT_MS);
        let last_pushed_to = self.last_pushed_to.read().unwrap();
        // crds should be locked last after self.last_pushed_to.
        let crds = crds.read().unwrap();
        crds.get_nodes()
            .filter_map(|value| {
                let info = value.value.contact_info().unwrap();
                // Stop pushing to nodes which have not been active recently.
                if value.local_timestamp < active_cutoff {
                    // In order to mitigate eclipse attack, for staked nodes
                    // continue retrying periodically.
                    let stake = stakes.get(&info.id).unwrap_or(&0);
                    if *stake == 0 || !rng.gen_ratio(1, 16) {
                        return None;
                    }
                }
                Some(info)
            })
            .filter(|info| {
                info.id != *self_id
                    && ContactInfo::is_valid_address(&info.gossip, socket_addr_space)
                    && self_shred_version == info.shred_version
                    && gossip_validators.map_or(true, |gossip_validators| {
                        gossip_validators.contains(&info.id)
                    })
            })
            .map(|info| {
                let last_pushed_to = last_pushed_to.peek(&info.id).copied().unwrap_or_default();
                let since = (now.saturating_sub(last_pushed_to).min(3600 * 1000) / 1024) as u32;
                let stake = get_stake(&info.id, stakes);
                let weight = get_weight(max_weight, since, stake);
                // Weights are bounded by max_weight defined above.
                // So this type-cast should be safe.
                ((weight * 100.0) as u64, info.clone())
            })
            .collect()
    }

    // Only for tests and simulations.
    pub(crate) fn mock_clone(&self) -> Self {
        let active_set = {
            let active_set = self.active_set.read().unwrap();
            active_set
                .iter()
                .map(|(k, v)| (*k, v.mock_clone()))
                .collect()
        };
        let last_pushed_to = {
            let last_pushed_to = self.last_pushed_to.read().unwrap();
            let mut clone = LruCache::new(last_pushed_to.cap());
            for (k, v) in last_pushed_to.iter().rev() {
                clone.put(*k, *v);
            }
            clone
        };
        let received_cache = self.received_cache.lock().unwrap().mock_clone();
        let crds_cursor = *self.crds_cursor.lock().unwrap();
        Self {
            active_set: RwLock::new(active_set),
            received_cache: Mutex::new(received_cache),
            last_pushed_to: RwLock::new(last_pushed_to),
            crds_cursor: Mutex::new(crds_cursor),
            num_total: AtomicUsize::new(self.num_total.load(Ordering::Relaxed)),
            num_old: AtomicUsize::new(self.num_old.load(Ordering::Relaxed)),
            num_pushes: AtomicUsize::new(self.num_pushes.load(Ordering::Relaxed)),
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{contact_info::ContactInfo, crds_value::CrdsData},
        std::time::Duration,
    };

    fn new_ping_cache() -> PingCache {
        PingCache::new(
            Duration::from_secs(20 * 60),      // ttl
            Duration::from_secs(20 * 60) / 64, // rate_limit_delay
            128,                               // capacity
        )
    }

    #[test]
    fn test_process_push_one() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let label = value.label();
        // push a new message
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value.clone()])], 0),
            [label.pubkey()].into_iter().collect(),
        );
        assert_eq!(crds.read().unwrap().get::<&CrdsValue>(&label), Some(&value));

        // push it again
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0)
            .is_empty());
    }
    #[test]
    fn test_process_push_old_version() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ci.wallclock = 1;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone()));

        // push a new message
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0),
            [ci.id].into_iter().collect()
        );

        // push an old version
        ci.wallclock = 0;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0)
            .is_empty());
    }
    #[test]
    fn test_process_push_timeout() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let timeout = push.msg_timeout;
        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);

        // push a version to far in the future
        ci.wallclock = timeout + 1;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone()));
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0)
            .is_empty());

        // push a version to far in the past
        ci.wallclock = 0;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], timeout + 1)
            .is_empty());
    }
    #[test]
    fn test_process_push_update() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        let origin = ci.id;
        ci.wallclock = 0;
        let value_old = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone()));

        // push a new message
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value_old])], 0),
            [origin].into_iter().collect()
        );

        // push an old version
        ci.wallclock = 1;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0),
            [origin].into_iter().collect()
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
        let now = timestamp();
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let mut ping_cache = new_ping_cache();
        let value1 = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ping_cache.mock_pong(value1.id, value1.gossip, Instant::now());
        let value1 = CrdsValue::new_unsigned(CrdsData::ContactInfo(value1));

        assert_eq!(
            crds.insert(value1.clone(), now, GossipRoute::LocalMessage),
            Ok(())
        );
        let keypair = Keypair::new();
        let crds = RwLock::new(crds);
        let ping_cache = Mutex::new(ping_cache);
        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validators
            &keypair,
            0, // self_shred_version
            1, // network_sizer
            1, // ratio
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let active_set = push.active_set.read().unwrap();
        assert!(active_set.get(&value1.label().pubkey()).is_some());
        let mut value2 = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        value2.gossip.set_port(1245);
        ping_cache
            .lock()
            .unwrap()
            .mock_pong(value2.id, value2.gossip, Instant::now());
        let value2 = CrdsValue::new_unsigned(CrdsData::ContactInfo(value2));
        assert!(active_set.get(&value2.label().pubkey()).is_none());
        drop(active_set);
        assert_eq!(
            crds.write()
                .unwrap()
                .insert(value2.clone(), now, GossipRoute::LocalMessage),
            Ok(())
        );
        for _ in 0..30 {
            push.refresh_push_active_set(
                &crds,
                &HashMap::new(), // stakes
                None,            // gossip_validators
                &keypair,
                0, // self_shred_version
                1, // network_size
                1, // ratio
                &ping_cache,
                &mut Vec::new(), // pings
                &SocketAddrSpace::Unspecified,
            );
            let active_set = push.active_set.read().unwrap();
            if active_set.get(&value2.label().pubkey()).is_some() {
                break;
            }
        }
        {
            let active_set = push.active_set.read().unwrap();
            assert!(active_set.get(&value2.label().pubkey()).is_some());
        }
        for k in 0..push.num_active {
            let mut value2 = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
            value2.gossip.set_port(1246 + k as u16);
            ping_cache
                .lock()
                .unwrap()
                .mock_pong(value2.id, value2.gossip, Instant::now());
            let value2 = CrdsValue::new_unsigned(CrdsData::ContactInfo(value2));
            assert_eq!(
                crds.write()
                    .unwrap()
                    .insert(value2.clone(), now, GossipRoute::LocalMessage),
                Ok(())
            );
        }

        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validators
            &keypair,
            0, // self_shred_version
            1, // network_size
            1, // ratio
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );
        assert_eq!(push.active_set.read().unwrap().len(), push.num_active);
    }
    #[test]
    fn test_active_set_refresh_with_bank() {
        solana_logger::setup();
        let time = timestamp() - 1024; //make sure there's at least a 1 second delay
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let mut stakes = HashMap::new();
        for i in 1..=100 {
            let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &solana_sdk::pubkey::new_rand(),
                time,
            )));
            let id = peer.label().pubkey();
            crds.insert(peer.clone(), time, GossipRoute::LocalMessage)
                .unwrap();
            stakes.insert(id, i * 100);
            push.last_pushed_to.write().unwrap().put(id, time);
        }
        let crds = RwLock::new(crds);
        let mut options = push.push_options(
            &crds,
            &Pubkey::default(),
            0,
            &stakes,
            None,
            &SocketAddrSpace::Unspecified,
        );
        assert!(!options.is_empty());
        options.sort_by(|(weight_l, _), (weight_r, _)| weight_r.partial_cmp(weight_l).unwrap());
        // check that the highest stake holder is also the heaviest weighted.
        assert_eq!(stakes[&options[0].1.id], 10_000_u64);
    }

    #[test]
    fn test_no_pushes_to_from_different_shred_versions() {
        let now = timestamp();
        let mut crds = Crds::default();
        let stakes = HashMap::new();
        let node = CrdsGossipPush::default();

        let gossip = socketaddr!("127.0.0.1:1234");

        let me = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            shred_version: 123,
            gossip,
            ..ContactInfo::default()
        }));
        let spy = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            shred_version: 0,
            gossip,
            ..ContactInfo::default()
        }));
        let node_123 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            shred_version: 123,
            gossip,
            ..ContactInfo::default()
        }));
        let node_456 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            shred_version: 456,
            gossip,
            ..ContactInfo::default()
        }));

        crds.insert(me.clone(), now, GossipRoute::LocalMessage)
            .unwrap();
        crds.insert(spy.clone(), now, GossipRoute::LocalMessage)
            .unwrap();
        crds.insert(node_123.clone(), now, GossipRoute::LocalMessage)
            .unwrap();
        crds.insert(node_456, now, GossipRoute::LocalMessage)
            .unwrap();
        let crds = RwLock::new(crds);

        // shred version 123 should ignore nodes with versions 0 and 456
        let options = node
            .push_options(
                &crds,
                &me.label().pubkey(),
                123,
                &stakes,
                None,
                &SocketAddrSpace::Unspecified,
            )
            .iter()
            .map(|(_, node)| node.id)
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 1);
        assert!(!options.contains(&spy.pubkey()));
        assert!(options.contains(&node_123.pubkey()));

        // spy nodes should not push to people on different shred versions
        let options = node.push_options(
            &crds,
            &spy.label().pubkey(),
            0,
            &stakes,
            None,
            &SocketAddrSpace::Unspecified,
        );
        assert!(options.is_empty());
    }

    #[test]
    fn test_pushes_only_to_allowed() {
        let now = timestamp();
        let mut crds = Crds::default();
        let stakes = HashMap::new();
        let node = CrdsGossipPush::default();
        let gossip = socketaddr!("127.0.0.1:1234");

        let me = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            gossip,
            ..ContactInfo::default()
        }));
        let node_123 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            gossip,
            ..ContactInfo::default()
        }));

        crds.insert(me.clone(), 0, GossipRoute::LocalMessage)
            .unwrap();
        crds.insert(node_123.clone(), now, GossipRoute::LocalMessage)
            .unwrap();
        let crds = RwLock::new(crds);

        // Unknown pubkey in gossip_validators -- will push to nobody
        let mut gossip_validators = HashSet::new();
        let options = node.push_options(
            &crds,
            &me.label().pubkey(),
            0,
            &stakes,
            Some(&gossip_validators),
            &SocketAddrSpace::Unspecified,
        );

        assert!(options.is_empty());

        // Unknown pubkey in gossip_validators -- will push to nobody
        gossip_validators.insert(solana_sdk::pubkey::new_rand());
        let options = node.push_options(
            &crds,
            &me.label().pubkey(),
            0,
            &stakes,
            Some(&gossip_validators),
            &SocketAddrSpace::Unspecified,
        );
        assert!(options.is_empty());

        // node_123 pubkey in gossip_validators -- will push to it
        gossip_validators.insert(node_123.pubkey());
        let options = node.push_options(
            &crds,
            &me.label().pubkey(),
            0,
            &stakes,
            Some(&gossip_validators),
            &SocketAddrSpace::Unspecified,
        );

        assert_eq!(options.len(), 1);
        assert_eq!(options[0].1.id, node_123.pubkey());
    }

    #[test]
    fn test_new_push_messages() {
        let now = timestamp();
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let mut ping_cache = new_ping_cache();
        let peer = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ping_cache.mock_pong(peer.id, peer.gossip, Instant::now());
        let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(peer));
        assert_eq!(
            crds.insert(peer.clone(), now, GossipRoute::LocalMessage),
            Ok(())
        );
        let crds = RwLock::new(crds);
        let ping_cache = Mutex::new(ping_cache);
        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validtors
            &Keypair::new(),
            0, // self_shred_version
            1, // network_size
            1, // ratio
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let mut expected = HashMap::new();
        expected.insert(peer.label().pubkey(), vec![new_msg.clone()]);
        let origin = new_msg.pubkey();
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![new_msg])], 0),
            [origin].into_iter().collect()
        );
        assert_eq!(push.active_set.read().unwrap().len(), 1);
        assert_eq!(push.new_push_messages(&crds, 0).0, expected);
    }
    #[test]
    fn test_personalized_push_messages() {
        let now = timestamp();
        let mut rng = rand::thread_rng();
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let mut ping_cache = new_ping_cache();
        let peers: Vec<_> = vec![0, 0, now]
            .into_iter()
            .map(|wallclock| {
                let mut peer = ContactInfo::new_rand(&mut rng, /*pubkey=*/ None);
                peer.wallclock = wallclock;
                ping_cache.mock_pong(peer.id, peer.gossip, Instant::now());
                CrdsValue::new_unsigned(CrdsData::ContactInfo(peer))
            })
            .collect();
        let origin: Vec<_> = peers.iter().map(|node| node.pubkey()).collect();
        assert_eq!(
            crds.insert(peers[0].clone(), now, GossipRoute::LocalMessage),
            Ok(())
        );
        assert_eq!(
            crds.insert(peers[1].clone(), now, GossipRoute::LocalMessage),
            Ok(())
        );
        let crds = RwLock::new(crds);
        assert_eq!(
            push.process_push_message(
                &crds,
                vec![(Pubkey::default(), vec![peers[2].clone()])],
                now
            ),
            [origin[2]].into_iter().collect()
        );
        let ping_cache = Mutex::new(ping_cache);
        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validators
            &Keypair::new(),
            0, // self_shred_version
            1, // network_size
            1, // ratio
            &ping_cache,
            &mut Vec::new(),
            &SocketAddrSpace::Unspecified,
        );

        // push 3's contact info to 1 and 2 and 3
        let expected: HashMap<_, _> = vec![
            (peers[0].pubkey(), vec![peers[2].clone()]),
            (peers[1].pubkey(), vec![peers[2].clone()]),
        ]
        .into_iter()
        .collect();
        assert_eq!(push.active_set.read().unwrap().len(), 3);
        assert_eq!(push.new_push_messages(&crds, now).0, expected);
    }
    #[test]
    fn test_process_prune() {
        let mut crds = Crds::default();
        let self_id = solana_sdk::pubkey::new_rand();
        let push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        assert_eq!(
            crds.insert(peer.clone(), 0, GossipRoute::LocalMessage),
            Ok(())
        );
        let crds = RwLock::new(crds);
        let ping_cache = Mutex::new(new_ping_cache());
        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validators
            &Keypair::new(),
            0, // self_shred_version
            1, // network_size
            1, // ratio
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let expected = HashMap::new();
        let origin = new_msg.pubkey();
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![new_msg.clone()])], 0),
            [origin].into_iter().collect()
        );
        push.process_prune_msg(
            &self_id,
            &peer.label().pubkey(),
            &[new_msg.label().pubkey()],
        );
        assert_eq!(push.new_push_messages(&crds, 0).0, expected);
    }
    #[test]
    fn test_purge_old_pending_push_messages() {
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        assert_eq!(crds.insert(peer, 0, GossipRoute::LocalMessage), Ok(()));
        let crds = RwLock::new(crds);
        let ping_cache = Mutex::new(new_ping_cache());
        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validators
            &Keypair::new(),
            0, // self_shred_version
            1, // network_size
            1, // ratio
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ci.wallclock = 1;
        let new_msg = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        let expected = HashMap::new();
        let origin = new_msg.pubkey();
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![new_msg])], 1),
            [origin].into_iter().collect()
        );
        assert_eq!(push.new_push_messages(&crds, 0).0, expected);
    }

    #[test]
    fn test_purge_old_received_cache() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ci.wallclock = 0;
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        let label = value.label();
        // push a new message
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value.clone()])], 0),
            [label.pubkey()].into_iter().collect()
        );
        assert_eq!(
            crds.write().unwrap().get::<&CrdsValue>(&label),
            Some(&value)
        );

        // push it again
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value.clone()])], 0)
            .is_empty());

        // push it again
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0)
            .is_empty());
    }
}
