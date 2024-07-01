//! Crds Gossip.
//!
//! This module ties together Crds and the push and pull gossip overlays.  The interface is
//! designed to run with a simulator or over a UDP network connection with messages up to a
//! packet::PACKET_DATA_SIZE size.

use {
    crate::{
        cluster_info::Ping,
        cluster_info_metrics::GossipStats,
        contact_info::ContactInfo,
        crds::{Crds, GossipRoute},
        crds_gossip_error::CrdsGossipError,
        crds_gossip_pull::{CrdsFilter, CrdsGossipPull, CrdsTimeouts, ProcessPullStats},
        crds_gossip_push::CrdsGossipPush,
        crds_value::{CrdsData, CrdsValue},
        duplicate_shred::{self, DuplicateShredIndex, MAX_DUPLICATE_SHREDS},
        ping_pong::PingCache,
    },
    itertools::Itertools,
    rand::{CryptoRng, Rng},
    rayon::ThreadPool,
    solana_ledger::shred::Shred,
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::{HashMap, HashSet},
        net::SocketAddr,
        sync::{Mutex, RwLock},
        time::{Duration, Instant},
    },
};

#[derive(Default)]
pub struct CrdsGossip {
    pub crds: RwLock<Crds>,
    pub push: CrdsGossipPush,
    pub pull: CrdsGossipPull,
}

impl CrdsGossip {
    /// Process a push message to the network.
    ///
    /// Returns unique origins' pubkeys of upserted values.
    pub fn process_push_message(
        &self,
        messages: Vec<(/*from:*/ Pubkey, Vec<CrdsValue>)>,
        now: u64,
    ) -> HashSet<Pubkey> {
        self.push.process_push_message(&self.crds, messages, now)
    }

    /// Remove redundant paths in the network.
    pub fn prune_received_cache<I>(
        &self,
        self_pubkey: &Pubkey,
        origins: I, // Unique pubkeys of crds values' owners.
        stakes: &HashMap<Pubkey, u64>,
    ) -> HashMap</*gossip peer:*/ Pubkey, /*origins:*/ Vec<Pubkey>>
    where
        I: IntoIterator<Item = Pubkey>,
    {
        self.push.prune_received_cache(self_pubkey, origins, stakes)
    }

    pub fn new_push_messages(
        &self,
        pubkey: &Pubkey, // This node.
        now: u64,
        stakes: &HashMap<Pubkey, u64>,
    ) -> (
        HashMap<Pubkey, Vec<CrdsValue>>,
        usize, // number of values
        usize, // number of push messages
    ) {
        self.push.new_push_messages(pubkey, &self.crds, now, stakes)
    }

    pub(crate) fn push_duplicate_shred<F>(
        &self,
        keypair: &Keypair,
        shred: &Shred,
        other_payload: &[u8],
        leader_schedule: Option<F>,
        // Maximum serialized size of each DuplicateShred chunk payload.
        max_payload_size: usize,
        shred_version: u16,
    ) -> Result<(), duplicate_shred::Error>
    where
        F: FnOnce(Slot) -> Option<Pubkey>,
    {
        let pubkey = keypair.pubkey();
        // Skip if there are already records of duplicate shreds for this slot.
        let shred_slot = shred.slot();
        let mut crds = self.crds.write().unwrap();
        if crds
            .get_records(&pubkey)
            .any(|value| match &value.value.data {
                CrdsData::DuplicateShred(_, value) => value.slot == shred_slot,
                _ => false,
            })
        {
            return Ok(());
        }
        let chunks = duplicate_shred::from_shred(
            shred.clone(),
            pubkey,
            Vec::from(other_payload),
            leader_schedule,
            timestamp(),
            max_payload_size,
            shred_version,
        )?;
        // Find the index of oldest duplicate shred.
        let mut num_dup_shreds = 0;
        let offset = crds
            .get_records(&pubkey)
            .filter_map(|value| match &value.value.data {
                CrdsData::DuplicateShred(ix, value) => {
                    num_dup_shreds += 1;
                    Some((value.wallclock, *ix))
                }
                _ => None,
            })
            .min() // Override the oldest records.
            .map(|(_ /*wallclock*/, ix)| ix)
            .unwrap_or(0);
        let offset = if num_dup_shreds < MAX_DUPLICATE_SHREDS {
            num_dup_shreds
        } else {
            offset
        };
        let entries = chunks.enumerate().map(|(k, chunk)| {
            let index = (offset + k as DuplicateShredIndex) % MAX_DUPLICATE_SHREDS;
            let data = CrdsData::DuplicateShred(index, chunk);
            CrdsValue::new_signed(data, keypair)
        });
        let now = timestamp();
        for entry in entries {
            if let Err(err) = crds.insert(entry, now, GossipRoute::LocalMessage) {
                error!("push_duplicate_shred failed: {:?}", err);
            }
        }
        Ok(())
    }

    /// Add the `from` to the peer's filter of nodes.
    pub fn process_prune_msg(
        &self,
        self_pubkey: &Pubkey,
        peer: &Pubkey,
        destination: &Pubkey,
        origin: &[Pubkey],
        wallclock: u64,
        now: u64,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Result<(), CrdsGossipError> {
        if now > wallclock.saturating_add(self.push.prune_timeout) {
            Err(CrdsGossipError::PruneMessageTimeout)
        } else if self_pubkey == destination {
            self.push
                .process_prune_msg(self_pubkey, peer, origin, stakes);
            Ok(())
        } else {
            Err(CrdsGossipError::BadPruneDestination)
        }
    }

    /// Refresh the push active set.
    pub fn refresh_push_active_set(
        &self,
        self_keypair: &Keypair,
        self_shred_version: u16,
        stakes: &HashMap<Pubkey, u64>,
        gossip_validators: Option<&HashSet<Pubkey>>,
        ping_cache: &Mutex<PingCache>,
        pings: &mut Vec<(SocketAddr, Ping)>,
        socket_addr_space: &SocketAddrSpace,
    ) {
        self.push.refresh_push_active_set(
            &self.crds,
            stakes,
            gossip_validators,
            self_keypair,
            self_shred_version,
            ping_cache,
            pings,
            socket_addr_space,
        )
    }

    /// Generate a random request.
    #[allow(clippy::too_many_arguments)]
    pub fn new_pull_request(
        &self,
        thread_pool: &ThreadPool,
        self_keypair: &Keypair,
        self_shred_version: u16,
        now: u64,
        gossip_validators: Option<&HashSet<Pubkey>>,
        stakes: &HashMap<Pubkey, u64>,
        bloom_size: usize,
        ping_cache: &Mutex<PingCache>,
        pings: &mut Vec<(SocketAddr, Ping)>,
        socket_addr_space: &SocketAddrSpace,
    ) -> Result<Vec<(ContactInfo, Vec<CrdsFilter>)>, CrdsGossipError> {
        self.pull.new_pull_request(
            thread_pool,
            &self.crds,
            self_keypair,
            self_shred_version,
            now,
            gossip_validators,
            stakes,
            bloom_size,
            ping_cache,
            pings,
            socket_addr_space,
        )
    }

    /// Process a pull request and create a response.
    pub fn process_pull_requests<I>(&self, callers: I, now: u64)
    where
        I: IntoIterator<Item = CrdsValue>,
    {
        CrdsGossipPull::process_pull_requests(&self.crds, callers, now);
    }

    pub fn generate_pull_responses(
        &self,
        thread_pool: &ThreadPool,
        filters: &[(CrdsValue, CrdsFilter)],
        output_size_limit: usize, // Limit number of crds values returned.
        now: u64,
        stats: &GossipStats,
    ) -> Vec<Vec<CrdsValue>> {
        CrdsGossipPull::generate_pull_responses(
            thread_pool,
            &self.crds,
            filters,
            output_size_limit,
            now,
            stats,
        )
    }

    pub fn filter_pull_responses(
        &self,
        timeouts: &CrdsTimeouts,
        response: Vec<CrdsValue>,
        now: u64,
        process_pull_stats: &mut ProcessPullStats,
    ) -> (
        Vec<CrdsValue>, // valid responses.
        Vec<CrdsValue>, // responses with expired timestamps.
        Vec<Hash>,      // hash of outdated values.
    ) {
        self.pull
            .filter_pull_responses(&self.crds, timeouts, response, now, process_pull_stats)
    }

    /// Process a pull response.
    pub fn process_pull_responses(
        &self,
        responses: Vec<CrdsValue>,
        responses_expired_timeout: Vec<CrdsValue>,
        failed_inserts: Vec<Hash>,
        now: u64,
        process_pull_stats: &mut ProcessPullStats,
    ) {
        self.pull.process_pull_responses(
            &self.crds,
            responses,
            responses_expired_timeout,
            failed_inserts,
            now,
            process_pull_stats,
        );
    }

    pub fn make_timeouts<'a>(
        &self,
        self_pubkey: Pubkey,
        stakes: &'a HashMap<Pubkey, u64>,
        epoch_duration: Duration,
    ) -> CrdsTimeouts<'a> {
        self.pull.make_timeouts(self_pubkey, stakes, epoch_duration)
    }

    pub fn purge(
        &self,
        self_pubkey: &Pubkey,
        thread_pool: &ThreadPool,
        now: u64,
        timeouts: &CrdsTimeouts,
    ) -> usize {
        let mut rv = 0;
        if now > self.pull.crds_timeout {
            debug_assert_eq!(timeouts[self_pubkey], u64::MAX);
            debug_assert_ne!(timeouts[&Pubkey::default()], 0u64);
            rv = CrdsGossipPull::purge_active(thread_pool, &self.crds, now, timeouts);
        }
        self.crds
            .write()
            .unwrap()
            .trim_purged(now.saturating_sub(5 * self.pull.crds_timeout));
        self.pull.purge_failed_inserts(now);
        rv
    }
}

// Returns active and valid cluster nodes to gossip with.
pub(crate) fn get_gossip_nodes<R: Rng>(
    rng: &mut R,
    now: u64,
    pubkey: &Pubkey, // This node.
    // By default, should only push to or pull from gossip nodes with the same
    // shred-version. Except for spy nodes (shred_version == 0u16) which can
    // pull from any node.
    verify_shred_version: impl Fn(/*shred_version:*/ u16) -> bool,
    crds: &RwLock<Crds>,
    gossip_validators: Option<&HashSet<Pubkey>>,
    stakes: &HashMap<Pubkey, u64>,
    socket_addr_space: &SocketAddrSpace,
) -> Vec<ContactInfo> {
    // Exclude nodes which have not been active for this long.
    const ACTIVE_TIMEOUT: Duration = Duration::from_secs(60);
    let active_cutoff = now.saturating_sub(ACTIVE_TIMEOUT.as_millis() as u64);
    let crds = crds.read().unwrap();
    crds.get_nodes()
        .filter_map(|value| {
            let node = value.value.contact_info().unwrap();
            // Exclude nodes which have not been active recently.
            if value.local_timestamp < active_cutoff {
                // In order to mitigate eclipse attack, for staked nodes
                // continue retrying periodically.
                let stake = stakes.get(node.pubkey()).copied().unwrap_or_default();
                if stake == 0u64 || !rng.gen_ratio(1, 16) {
                    return None;
                }
            }
            Some(node)
        })
        .filter(|node| {
            node.pubkey() != pubkey
                && verify_shred_version(node.shred_version())
                && node
                    .gossip()
                    .map(|addr| socket_addr_space.check(&addr))
                    .unwrap_or_default()
                && match gossip_validators {
                    Some(nodes) => nodes.contains(node.pubkey()),
                    None => true,
                }
        })
        .cloned()
        .collect()
}

// Dedups gossip addresses, keeping only the one with the highest stake.
pub(crate) fn dedup_gossip_addresses(
    nodes: impl IntoIterator<Item = ContactInfo>,
    stakes: &HashMap<Pubkey, u64>,
) -> HashMap</*gossip:*/ SocketAddr, (/*stake:*/ u64, ContactInfo)> {
    nodes
        .into_iter()
        .filter_map(|node| Some((node.gossip().ok()?, node)))
        .into_grouping_map()
        .aggregate(|acc, _node_gossip, node| {
            let stake = stakes.get(node.pubkey()).copied().unwrap_or_default();
            match acc {
                Some((ref s, _)) if s >= &stake => acc,
                Some(_) | None => Some((stake, node)),
            }
        })
}

// Pings gossip addresses if needed.
// Returns nodes which have recently responded to a ping message.
#[must_use]
pub(crate) fn maybe_ping_gossip_addresses<R: Rng + CryptoRng>(
    rng: &mut R,
    nodes: impl IntoIterator<Item = ContactInfo>,
    keypair: &Keypair,
    ping_cache: &Mutex<PingCache>,
    pings: &mut Vec<(SocketAddr, Ping)>,
) -> Vec<ContactInfo> {
    let mut ping_cache = ping_cache.lock().unwrap();
    let mut pingf = move || Ping::new_rand(rng, keypair).ok();
    let now = Instant::now();
    nodes
        .into_iter()
        .filter(|node| {
            let Ok(node_gossip) = node.gossip() else {
                return false;
            };
            let (check, ping) = {
                let node = (*node.pubkey(), node_gossip);
                ping_cache.check(now, node, &mut pingf)
            };
            if let Some(ping) = ping {
                pings.push((node_gossip, ping));
            }
            check
        })
        .collect()
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::crds_value::CrdsData,
        solana_sdk::{hash::hash, timing::timestamp},
    };

    #[test]
    fn test_prune_errors() {
        let crds_gossip = CrdsGossip::default();
        let keypair = Keypair::new();
        let id = keypair.pubkey();
        let ci = ContactInfo::new_localhost(&Pubkey::from([1; 32]), 0);
        let prune_pubkey = Pubkey::from([2; 32]);
        crds_gossip
            .crds
            .write()
            .unwrap()
            .insert(
                CrdsValue::new_unsigned(CrdsData::ContactInfo(ci.clone())),
                0,
                GossipRoute::LocalMessage,
            )
            .unwrap();
        let ping_cache = PingCache::new(
            Duration::from_secs(20 * 60),      // ttl
            Duration::from_secs(20 * 60) / 64, // rate_limit_delay
            128,                               // capacity
        );
        let ping_cache = Mutex::new(ping_cache);
        crds_gossip.refresh_push_active_set(
            &keypair,
            0,               // shred version
            &HashMap::new(), // stakes
            None,            // gossip validators
            &ping_cache,
            &mut Vec::new(), // pings
            &SocketAddrSpace::Unspecified,
        );
        let now = timestamp();
        //incorrect dest
        let mut res = crds_gossip.process_prune_msg(
            &id,
            ci.pubkey(),
            &Pubkey::from(hash(&[1; 32]).to_bytes()),
            &[prune_pubkey],
            now,
            now,
            &HashMap::<Pubkey, u64>::default(), // stakes
        );
        assert_eq!(res.err(), Some(CrdsGossipError::BadPruneDestination));
        //correct dest
        res = crds_gossip.process_prune_msg(
            &id,             // self_pubkey
            ci.pubkey(),     // peer
            &id,             // destination
            &[prune_pubkey], // origins
            now,
            now,
            &HashMap::<Pubkey, u64>::default(), // stakes
        );
        res.unwrap();
        //test timeout
        let timeout = now + crds_gossip.push.prune_timeout * 2;
        res = crds_gossip.process_prune_msg(
            &id,             // self_pubkey
            ci.pubkey(),     // peer
            &id,             // destination
            &[prune_pubkey], // origins
            now,
            timeout,
            &HashMap::<Pubkey, u64>::default(), // stakes
        );
        assert_eq!(res.err(), Some(CrdsGossipError::PruneMessageTimeout));
    }
}
