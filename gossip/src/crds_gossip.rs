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
        crds_gossip_pull::{CrdsFilter, CrdsGossipPull, ProcessPullStats},
        crds_gossip_push::{CrdsGossipPush, CRDS_GOSSIP_NUM_ACTIVE},
        crds_value::{CrdsData, CrdsValue},
        duplicate_shred::{self, DuplicateShredIndex, LeaderScheduleFn, MAX_DUPLICATE_SHREDS},
        ping_pong::PingCache,
    },
    itertools::Itertools,
    rayon::ThreadPool,
    solana_ledger::shred::Shred,
    solana_sdk::{
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
        time::Duration,
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
        pending_push_messages: Vec<CrdsValue>,
        now: u64,
    ) -> (
        HashMap<Pubkey, Vec<CrdsValue>>,
        usize, // number of values
        usize, // number of push messages
    ) {
        {
            let mut crds = self.crds.write().unwrap();
            for entry in pending_push_messages {
                let _ = crds.insert(entry, now, GossipRoute::LocalMessage);
            }
        }
        self.push.new_push_messages(&self.crds, now)
    }

    pub(crate) fn push_duplicate_shred(
        &self,
        keypair: &Keypair,
        shred: &Shred,
        other_payload: &[u8],
        leader_schedule: Option<impl LeaderScheduleFn>,
        // Maximum serialized size of each DuplicateShred chunk payload.
        max_payload_size: usize,
    ) -> Result<(), duplicate_shred::Error> {
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
                error!("push_duplicate_shred faild: {:?}", err);
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
    ) -> Result<(), CrdsGossipError> {
        if now > wallclock.saturating_add(self.push.prune_timeout) {
            Err(CrdsGossipError::PruneMessageTimeout)
        } else if self_pubkey == destination {
            self.push.process_prune_msg(self_pubkey, peer, origin);
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
        let network_size = self.crds.read().unwrap().num_nodes();
        self.push.refresh_push_active_set(
            &self.crds,
            stakes,
            gossip_validators,
            self_keypair,
            self_shred_version,
            network_size,
            CRDS_GOSSIP_NUM_ACTIVE,
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
    ) -> Result<HashMap<ContactInfo, Vec<CrdsFilter>>, CrdsGossipError> {
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

    /// Time when a request to `from` was initiated.
    ///
    /// This is used for weighted random selection during `new_pull_request`
    /// It's important to use the local nodes request creation time as the weight
    /// instead of the response received time otherwise failed nodes will increase their weight.
    pub fn mark_pull_request_creation_time(&self, from: Pubkey, now: u64) {
        self.pull.mark_pull_request_creation_time(from, now)
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
        timeouts: &HashMap<Pubkey, u64>,
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
        from: &Pubkey,
        responses: Vec<CrdsValue>,
        responses_expired_timeout: Vec<CrdsValue>,
        failed_inserts: Vec<Hash>,
        now: u64,
        process_pull_stats: &mut ProcessPullStats,
    ) {
        self.pull.process_pull_responses(
            &self.crds,
            from,
            responses,
            responses_expired_timeout,
            failed_inserts,
            now,
            process_pull_stats,
        );
    }

    pub fn make_timeouts(
        &self,
        self_pubkey: Pubkey,
        stakes: &HashMap<Pubkey, u64>,
        epoch_duration: Duration,
    ) -> HashMap<Pubkey, u64> {
        self.pull.make_timeouts(self_pubkey, stakes, epoch_duration)
    }

    pub fn purge(
        &self,
        self_pubkey: &Pubkey,
        thread_pool: &ThreadPool,
        now: u64,
        timeouts: &HashMap<Pubkey, u64>,
    ) -> usize {
        let mut rv = 0;
        if now > self.pull.crds_timeout {
            //sanity check
            assert_eq!(timeouts[self_pubkey], std::u64::MAX);
            assert!(timeouts.contains_key(&Pubkey::default()));
            rv = CrdsGossipPull::purge_active(thread_pool, &self.crds, now, timeouts);
        }
        self.crds
            .write()
            .unwrap()
            .trim_purged(now.saturating_sub(5 * self.pull.crds_timeout));
        self.pull.purge_failed_inserts(now);
        rv
    }

    // Only for tests and simulations.
    pub(crate) fn mock_clone(&self) -> Self {
        let crds = self.crds.read().unwrap().mock_clone();
        Self {
            crds: RwLock::new(crds),
            push: self.push.mock_clone(),
            pull: self.pull.mock_clone(),
        }
    }
}

/// Computes a normalized (log of actual stake) stake.
pub fn get_stake<S: std::hash::BuildHasher>(id: &Pubkey, stakes: &HashMap<Pubkey, u64, S>) -> f32 {
    // cap the max balance to u32 max (it should be plenty)
    let bal = f64::from(u32::max_value()).min(*stakes.get(id).unwrap_or(&0) as f64);
    1_f32.max((bal as f32).ln())
}

/// Computes bounded weight given some max, a time since last selected, and a stake value.
///
/// The minimum stake is 1 and not 0 to allow 'time since last' picked to factor in.
pub fn get_weight(max_weight: f32, time_since_last_selected: u32, stake: f32) -> f32 {
    let mut weight = time_since_last_selected as f32 * stake;
    if weight.is_infinite() {
        weight = max_weight;
    }
    1.0_f32.max(weight.min(max_weight))
}

// Dedups gossip addresses, keeping only the one with the highest weight.
pub(crate) fn dedup_gossip_addresses<I, T: PartialOrd>(
    nodes: I,
) -> HashMap</*gossip:*/ SocketAddr, (/*weight:*/ T, ContactInfo)>
where
    I: IntoIterator<Item = (/*weight:*/ T, ContactInfo)>,
{
    nodes
        .into_iter()
        .into_grouping_map_by(|(_weight, node)| node.gossip)
        .aggregate(|acc, _node_gossip, (weight, node)| match acc {
            Some((ref w, _)) if w >= &weight => acc,
            Some(_) | None => Some((weight, node)),
        })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{contact_info::ContactInfo, crds_value::CrdsData},
        solana_sdk::{hash::hash, timing::timestamp},
    };

    #[test]
    fn test_prune_errors() {
        let crds_gossip = CrdsGossip::default();
        let keypair = Keypair::new();
        let id = keypair.pubkey();
        let ci = ContactInfo::new_localhost(&Pubkey::new(&[1; 32]), 0);
        let prune_pubkey = Pubkey::new(&[2; 32]);
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
            &ci.id,
            &Pubkey::new(hash(&[1; 32]).as_ref()),
            &[prune_pubkey],
            now,
            now,
        );
        assert_eq!(res.err(), Some(CrdsGossipError::BadPruneDestination));
        //correct dest
        res = crds_gossip.process_prune_msg(
            &id,             // self_pubkey
            &ci.id,          // peer
            &id,             // destination
            &[prune_pubkey], // origins
            now,
            now,
        );
        res.unwrap();
        //test timeout
        let timeout = now + crds_gossip.push.prune_timeout * 2;
        res = crds_gossip.process_prune_msg(
            &id,             // self_pubkey
            &ci.id,          // peer
            &id,             // destination
            &[prune_pubkey], // origins
            now,
            timeout,
        );
        assert_eq!(res.err(), Some(CrdsGossipError::PruneMessageTimeout));
    }
}
