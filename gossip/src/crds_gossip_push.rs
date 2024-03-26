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
        crds::{Crds, CrdsError, Cursor, GossipRoute},
        crds_gossip,
        crds_value::CrdsValue,
        ping_pong::PingCache,
        push_active_set::PushActiveSet,
        received_cache::ReceivedCache,
    },
    bincode::serialized_size,
    itertools::Itertools,
    solana_sdk::{
        packet::PACKET_DATA_SIZE,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::{HashMap, HashSet},
        iter::repeat,
        net::SocketAddr,
        ops::{DerefMut, RangeBounds},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex, RwLock,
        },
    },
};

const CRDS_GOSSIP_PUSH_FANOUT: usize = 9;
// With a fanout of 9, a 2000 node cluster should only take ~3.5 hops to converge.
// However since pushes are stake weighed, some trailing nodes
// might need more time to receive values. 30 seconds should be plenty.
pub const CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS: u64 = 30000;
const CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS: u64 = 500;
const CRDS_GOSSIP_PRUNE_STAKE_THRESHOLD_PCT: f64 = 0.15;
const CRDS_GOSSIP_PRUNE_MIN_INGRESS_NODES: usize = 2;
const CRDS_GOSSIP_PUSH_ACTIVE_SET_SIZE: usize = CRDS_GOSSIP_PUSH_FANOUT + 3;

pub struct CrdsGossipPush {
    /// Max bytes per message
    max_bytes: usize,
    /// Active set of validators for push
    active_set: RwLock<PushActiveSet>,
    /// Cursor into the crds table for values to push.
    crds_cursor: Mutex<Cursor>,
    /// Cache that tracks which validators a message was received from
    /// This cache represents a lagging view of which validators
    /// currently have this node in their `active_set`
    received_cache: Mutex<ReceivedCache>,
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
        let mut origins = HashSet::new();
        for (from, values) in messages {
            self.num_total.fetch_add(values.len(), Ordering::Relaxed);
            for value in values {
                if !wallclock_window.contains(&value.wallclock()) {
                    continue;
                }
                let origin = value.pubkey();
                match crds.insert(value, now, GossipRoute::PushMessage(&from)) {
                    Ok(()) => {
                        received_cache.record(origin, from, /*num_dups:*/ 0);
                        origins.insert(origin);
                    }
                    Err(CrdsError::DuplicatePush(num_dups)) => {
                        received_cache.record(origin, from, usize::from(num_dups));
                        self.num_old.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(CrdsError::InsertFailed | CrdsError::UnknownStakes) => {
                        received_cache.record(origin, from, /*num_dups:*/ usize::MAX);
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
        pubkey: &Pubkey, // This node.
        crds: &RwLock<Crds>,
        now: u64,
        stakes: &HashMap<Pubkey, u64>,
    ) -> (
        HashMap<Pubkey, Vec<CrdsValue>>,
        usize, // number of values
        usize, // number of push messages
    ) {
        let active_set = self.active_set.read().unwrap();
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
            let nodes = active_set.get_nodes(
                pubkey,
                &origin,
                |node| value.should_force_push(node),
                stakes,
            );
            for node in nodes.take(self.push_fanout) {
                push_messages.entry(*node).or_default().push(value.clone());
                num_pushes += 1;
            }
        }
        drop(crds);
        drop(crds_cursor);
        drop(active_set);
        self.num_pushes.fetch_add(num_pushes, Ordering::Relaxed);
        (push_messages, num_values, num_pushes)
    }

    /// Add the `from` to the peer's filter of nodes.
    pub(crate) fn process_prune_msg(
        &self,
        self_pubkey: &Pubkey,
        peer: &Pubkey,
        origins: &[Pubkey],
        stakes: &HashMap<Pubkey, u64>,
    ) {
        let active_set = self.active_set.read().unwrap();
        active_set.prune(self_pubkey, peer, origins, stakes);
    }

    /// Refresh the push active set.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refresh_push_active_set(
        &self,
        crds: &RwLock<Crds>,
        stakes: &HashMap<Pubkey, u64>,
        gossip_validators: Option<&HashSet<Pubkey>>,
        self_keypair: &Keypair,
        self_shred_version: u16,
        ping_cache: &Mutex<PingCache>,
        pings: &mut Vec<(SocketAddr, Ping)>,
        socket_addr_space: &SocketAddrSpace,
    ) {
        let mut rng = rand::thread_rng();
        // Active and valid gossip nodes with matching shred-version.
        let nodes = crds_gossip::get_gossip_nodes(
            &mut rng,
            timestamp(), // now
            &self_keypair.pubkey(),
            // Only push to nodes with the same shred version.
            |shred_version| shred_version == self_shred_version,
            crds,
            gossip_validators,
            stakes,
            socket_addr_space,
        );
        // Check for nodes which have responded to ping messages.
        let nodes = crds_gossip::maybe_ping_gossip_addresses(
            &mut rng,
            nodes,
            self_keypair,
            ping_cache,
            pings,
        );
        let nodes = crds_gossip::dedup_gossip_addresses(nodes, stakes)
            .into_values()
            .map(|(_stake, node)| *node.pubkey())
            .collect::<Vec<_>>();
        if nodes.is_empty() {
            return;
        }
        let cluster_size = crds.read().unwrap().num_pubkeys().max(stakes.len());
        let mut active_set = self.active_set.write().unwrap();
        active_set.rotate(
            &mut rng,
            CRDS_GOSSIP_PUSH_ACTIVE_SET_SIZE,
            cluster_size,
            &nodes,
            stakes,
        )
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{crds_value::CrdsData, legacy_contact_info::LegacyContactInfo as ContactInfo},
        std::time::{Duration, Instant},
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
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0),
        ));
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
        ci.set_wallclock(1);
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci.clone()));

        // push a new message
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0),
            [*ci.pubkey()].into_iter().collect()
        );

        // push an old version
        ci.set_wallclock(0);
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci));
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
        ci.set_wallclock(timeout + 1);
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci.clone()));
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0)
            .is_empty());

        // push a version to far in the past
        ci.set_wallclock(0);
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci));
        assert!(push
            .process_push_message(&crds, vec![(Pubkey::default(), vec![value])], timeout + 1)
            .is_empty());
    }
    #[test]
    fn test_process_push_update() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        let origin = *ci.pubkey();
        ci.set_wallclock(0);
        let value_old = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci.clone()));

        // push a new message
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value_old])], 0),
            [origin].into_iter().collect()
        );

        // push an old version
        ci.set_wallclock(1);
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci));
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![value])], 0),
            [origin].into_iter().collect()
        );
    }

    #[test]
    fn test_new_push_messages() {
        let now = timestamp();
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let mut ping_cache = new_ping_cache();
        let peer = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ping_cache.mock_pong(*peer.pubkey(), peer.gossip().unwrap(), Instant::now());
        let peer = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(peer));
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
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let new_msg = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0),
        ));
        let mut expected = HashMap::new();
        expected.insert(peer.label().pubkey(), vec![new_msg.clone()]);
        let origin = new_msg.pubkey();
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![new_msg])], 0),
            [origin].into_iter().collect()
        );
        assert_eq!(
            push.new_push_messages(
                &Pubkey::default(),
                &crds,
                0,
                &HashMap::<Pubkey, u64>::default(), // stakes
            )
            .0,
            expected
        );
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
                peer.set_wallclock(wallclock);
                ping_cache.mock_pong(*peer.pubkey(), peer.gossip().unwrap(), Instant::now());
                CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(peer))
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
        assert_eq!(
            push.new_push_messages(
                &Pubkey::default(),
                &crds,
                now,
                &HashMap::<Pubkey, u64>::default(), // stakes
            )
            .0,
            expected
        );
    }
    #[test]
    fn test_process_prune() {
        let mut crds = Crds::default();
        let self_id = solana_sdk::pubkey::new_rand();
        let push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0),
        ));
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
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let new_msg = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0),
        ));
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
            &HashMap::<Pubkey, u64>::default(), // stakes
        );
        assert_eq!(
            push.new_push_messages(
                &self_id,
                &crds,
                0,
                &HashMap::<Pubkey, u64>::default(), // stakes
            )
            .0,
            expected
        );
    }
    #[test]
    fn test_purge_old_pending_push_messages() {
        let mut crds = Crds::default();
        let push = CrdsGossipPush::default();
        let peer = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0),
        ));
        assert_eq!(crds.insert(peer, 0, GossipRoute::LocalMessage), Ok(()));
        let crds = RwLock::new(crds);
        let ping_cache = Mutex::new(new_ping_cache());
        push.refresh_push_active_set(
            &crds,
            &HashMap::new(), // stakes
            None,            // gossip_validators
            &Keypair::new(),
            0, // self_shred_version
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );

        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ci.set_wallclock(1);
        let new_msg = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci));
        let expected = HashMap::new();
        let origin = new_msg.pubkey();
        assert_eq!(
            push.process_push_message(&crds, vec![(Pubkey::default(), vec![new_msg])], 1),
            [origin].into_iter().collect()
        );
        assert_eq!(
            push.new_push_messages(
                &Pubkey::default(),
                &crds,
                0,
                &HashMap::<Pubkey, u64>::default(), // stakes
            )
            .0,
            expected
        );
    }

    #[test]
    fn test_purge_old_received_cache() {
        let crds = RwLock::<Crds>::default();
        let push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        ci.set_wallclock(0);
        let value = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(ci));
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
