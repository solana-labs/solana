use {
    crate::crds_gossip::CrdsGossip,
    itertools::Itertools,
    solana_measure::measure::Measure,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::HashMap,
        ops::{Deref, DerefMut},
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    },
};

#[derive(Default)]
pub(crate) struct Counter(AtomicU64);

impl Counter {
    pub(crate) fn add_measure(&self, x: &mut Measure) {
        x.stop();
        self.0.fetch_add(x.as_us(), Ordering::Relaxed);
    }
    pub(crate) fn add_relaxed(&self, x: u64) {
        self.0.fetch_add(x, Ordering::Relaxed);
    }
    fn clear(&self) -> u64 {
        self.0.swap(0, Ordering::Relaxed)
    }
}

pub(crate) struct TimedGuard<'a, T> {
    guard: T,
    timer: Measure,
    counter: &'a Counter,
}

pub(crate) struct ScopedTimer<'a> {
    clock: Instant,
    metric: &'a AtomicU64,
}

impl<'a> From<&'a Counter> for ScopedTimer<'a> {
    // Output should be assigned to a *named* variable, otherwise it is
    // immediately dropped.
    #[must_use]
    fn from(counter: &'a Counter) -> Self {
        Self {
            clock: Instant::now(),
            metric: &counter.0,
        }
    }
}

impl Drop for ScopedTimer<'_> {
    fn drop(&mut self) {
        let micros = self.clock.elapsed().as_micros();
        self.metric.fetch_add(micros as u64, Ordering::Relaxed);
    }
}

impl<'a, T> TimedGuard<'a, T> {
    pub(crate) fn new(guard: T, label: &'static str, counter: &'a Counter) -> Self {
        Self {
            guard,
            timer: Measure::start(label),
            counter,
        }
    }
}

impl<'a, T> Deref for TimedGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a, T> DerefMut for TimedGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a, T> Drop for TimedGuard<'a, T> {
    fn drop(&mut self) {
        self.counter.add_measure(&mut self.timer);
    }
}

#[derive(Default)]
pub(crate) struct GossipStats {
    pub(crate) all_tvu_peers: Counter,
    pub(crate) entrypoint2: Counter,
    pub(crate) entrypoint: Counter,
    pub(crate) epoch_slots_lookup: Counter,
    pub(crate) filter_pull_response: Counter,
    pub(crate) generate_pull_responses: Counter,
    pub(crate) get_accounts_hash: Counter,
    pub(crate) get_votes: Counter,
    pub(crate) gossip_packets_dropped_count: Counter,
    pub(crate) handle_batch_ping_messages_time: Counter,
    pub(crate) handle_batch_pong_messages_time: Counter,
    pub(crate) handle_batch_prune_messages_time: Counter,
    pub(crate) handle_batch_pull_requests_time: Counter,
    pub(crate) handle_batch_pull_responses_time: Counter,
    pub(crate) handle_batch_push_messages_time: Counter,
    pub(crate) mark_pull_request: Counter,
    pub(crate) new_pull_requests: Counter,
    pub(crate) new_pull_requests_count: Counter,
    pub(crate) new_pull_requests_pings_count: Counter,
    pub(crate) new_push_requests2: Counter,
    pub(crate) new_push_requests: Counter,
    pub(crate) new_push_requests_num: Counter,
    pub(crate) packets_received_count: Counter,
    pub(crate) packets_received_prune_messages_count: Counter,
    pub(crate) packets_received_pull_requests_count: Counter,
    pub(crate) packets_received_pull_responses_count: Counter,
    pub(crate) packets_received_push_messages_count: Counter,
    pub(crate) packets_received_verified_count: Counter,
    pub(crate) packets_sent_gossip_requests_count: Counter,
    pub(crate) packets_sent_prune_messages_count: Counter,
    pub(crate) packets_sent_pull_requests_count: Counter,
    pub(crate) packets_sent_pull_responses_count: Counter,
    pub(crate) packets_sent_push_messages_count: Counter,
    pub(crate) process_gossip_packets_time: Counter,
    pub(crate) process_prune: Counter,
    pub(crate) process_pull_requests: Counter,
    pub(crate) process_pull_response: Counter,
    pub(crate) process_pull_response_count: Counter,
    pub(crate) process_pull_response_fail_insert: Counter,
    pub(crate) process_pull_response_fail_timeout: Counter,
    pub(crate) process_pull_response_len: Counter,
    pub(crate) process_pull_response_success: Counter,
    pub(crate) process_pull_response_timeout: Counter,
    pub(crate) process_push_message: Counter,
    pub(crate) process_push_success: Counter,
    pub(crate) prune_message_count: Counter,
    pub(crate) prune_message_len: Counter,
    pub(crate) prune_received_cache: Counter,
    pub(crate) pull_from_entrypoint_count: Counter,
    pub(crate) pull_request_ping_pong_check_failed_count: Counter,
    pub(crate) pull_requests_count: Counter,
    pub(crate) purge: Counter,
    pub(crate) push_message_count: Counter,
    pub(crate) push_message_value_count: Counter,
    pub(crate) push_response_count: Counter,
    pub(crate) push_vote_read: Counter,
    pub(crate) repair_peers: Counter,
    pub(crate) require_stake_for_gossip_unknown_stakes: Counter,
    pub(crate) skip_pull_response_shred_version: Counter,
    pub(crate) skip_pull_shred_version: Counter,
    pub(crate) skip_push_message_shred_version: Counter,
    pub(crate) trim_crds_table: Counter,
    pub(crate) trim_crds_table_failed: Counter,
    pub(crate) trim_crds_table_purged_values_count: Counter,
    pub(crate) tvu_peers: Counter,
    pub(crate) verify_gossip_packets_time: Counter,
}

pub(crate) fn submit_gossip_stats(
    stats: &GossipStats,
    gossip: &CrdsGossip,
    stakes: &HashMap<Pubkey, u64>,
) {
    let (crds_stats, table_size, num_nodes, num_pubkeys, purged_values_size, failed_inserts_size) = {
        let gossip_crds = gossip.crds.read().unwrap();
        (
            gossip_crds.take_stats(),
            gossip_crds.len(),
            gossip_crds.num_nodes(),
            gossip_crds.num_pubkeys(),
            gossip_crds.num_purged(),
            gossip.pull.failed_inserts_size(),
        )
    };
    let num_nodes_staked = stakes.values().filter(|stake| **stake > 0).count();
    datapoint_info!(
        "cluster_info_stats",
        ("entrypoint", stats.entrypoint.clear(), i64),
        ("entrypoint2", stats.entrypoint2.clear(), i64),
        ("push_vote_read", stats.push_vote_read.clear(), i64),
        ("get_votes", stats.get_votes.clear(), i64),
        ("get_accounts_hash", stats.get_accounts_hash.clear(), i64),
        ("all_tvu_peers", stats.all_tvu_peers.clear(), i64),
        ("tvu_peers", stats.tvu_peers.clear(), i64),
        (
            "new_push_requests_num",
            stats.new_push_requests_num.clear(),
            i64
        ),
        ("table_size", table_size as i64, i64),
        ("purged_values_size", purged_values_size as i64, i64),
        ("failed_inserts_size", failed_inserts_size as i64, i64),
        ("num_nodes", num_nodes as i64, i64),
        ("num_nodes_staked", num_nodes_staked as i64, i64),
        ("num_pubkeys", num_pubkeys, i64),
    );
    datapoint_info!(
        "cluster_info_stats2",
        (
            "gossip_packets_dropped_count",
            stats.gossip_packets_dropped_count.clear(),
            i64
        ),
        ("repair_peers", stats.repair_peers.clear(), i64),
        ("new_push_requests", stats.new_push_requests.clear(), i64),
        ("new_push_requests2", stats.new_push_requests2.clear(), i64),
        (
            "process_push_success",
            stats.process_push_success.clear(),
            i64
        ),
        ("purge", stats.purge.clear(), i64),
        (
            "process_gossip_packets_time",
            stats.process_gossip_packets_time.clear(),
            i64
        ),
        (
            "verify_gossip_packets_time",
            stats.verify_gossip_packets_time.clear(),
            i64
        ),
        (
            "handle_batch_ping_messages_time",
            stats.handle_batch_ping_messages_time.clear(),
            i64
        ),
        (
            "handle_batch_pong_messages_time",
            stats.handle_batch_pong_messages_time.clear(),
            i64
        ),
        (
            "handle_batch_prune_messages_time",
            stats.handle_batch_prune_messages_time.clear(),
            i64
        ),
        (
            "handle_batch_pull_requests_time",
            stats.handle_batch_pull_requests_time.clear(),
            i64
        ),
        (
            "handle_batch_pull_responses_time",
            stats.handle_batch_pull_responses_time.clear(),
            i64
        ),
        (
            "handle_batch_push_messages_time",
            stats.handle_batch_push_messages_time.clear(),
            i64
        ),
        (
            "process_pull_resp",
            stats.process_pull_response.clear(),
            i64
        ),
        ("filter_pull_resp", stats.filter_pull_response.clear(), i64),
        (
            "process_pull_resp_count",
            stats.process_pull_response_count.clear(),
            i64
        ),
        (
            "pull_response_fail_insert",
            stats.process_pull_response_fail_insert.clear(),
            i64
        ),
        (
            "pull_response_fail_timeout",
            stats.process_pull_response_fail_timeout.clear(),
            i64
        ),
        (
            "pull_response_success",
            stats.process_pull_response_success.clear(),
            i64
        ),
        (
            "process_pull_resp_timeout",
            stats.process_pull_response_timeout.clear(),
            i64
        ),
        (
            "push_response_count",
            stats.push_response_count.clear(),
            i64
        ),
    );
    datapoint_info!(
        "cluster_info_stats3",
        (
            "process_pull_resp_len",
            stats.process_pull_response_len.clear(),
            i64
        ),
        (
            "process_pull_requests",
            stats.process_pull_requests.clear(),
            i64
        ),
        (
            "pull_request_ping_pong_check_failed_count",
            stats.pull_request_ping_pong_check_failed_count.clear(),
            i64
        ),
        (
            "new_pull_requests_pings_count",
            stats.new_pull_requests_pings_count.clear(),
            i64
        ),
        (
            "generate_pull_responses",
            stats.generate_pull_responses.clear(),
            i64
        ),
        ("process_prune", stats.process_prune.clear(), i64),
        (
            "process_push_message",
            stats.process_push_message.clear(),
            i64
        ),
        (
            "prune_received_cache",
            stats.prune_received_cache.clear(),
            i64
        ),
        ("epoch_slots_lookup", stats.epoch_slots_lookup.clear(), i64),
        ("new_pull_requests", stats.new_pull_requests.clear(), i64),
        ("mark_pull_request", stats.mark_pull_request.clear(), i64),
    );
    datapoint_info!(
        "cluster_info_stats4",
        (
            "skip_push_message_shred_version",
            stats.skip_push_message_shred_version.clear(),
            i64
        ),
        (
            "skip_pull_response_shred_version",
            stats.skip_pull_response_shred_version.clear(),
            i64
        ),
        (
            "skip_pull_shred_version",
            stats.skip_pull_shred_version.clear(),
            i64
        ),
        ("push_message_count", stats.push_message_count.clear(), i64),
        (
            "push_message_value_count",
            stats.push_message_value_count.clear(),
            i64
        ),
        (
            "new_pull_requests_count",
            stats.new_pull_requests_count.clear(),
            i64
        ),
        (
            "pull_from_entrypoint_count",
            stats.pull_from_entrypoint_count.clear(),
            i64
        ),
        (
            "prune_message_count",
            stats.prune_message_count.clear(),
            i64
        ),
        ("prune_message_len", stats.prune_message_len.clear(), i64),
    );
    datapoint_info!(
        "cluster_info_stats5",
        (
            "pull_requests_count",
            stats.pull_requests_count.clear(),
            i64
        ),
        (
            "packets_received_count",
            stats.packets_received_count.clear(),
            i64
        ),
        (
            "packets_received_prune_messages_count",
            stats.packets_received_prune_messages_count.clear(),
            i64
        ),
        (
            "packets_received_pull_requests_count",
            stats.packets_received_pull_requests_count.clear(),
            i64
        ),
        (
            "packets_received_pull_responses_count",
            stats.packets_received_pull_responses_count.clear(),
            i64
        ),
        (
            "packets_received_push_messages_count",
            stats.packets_received_push_messages_count.clear(),
            i64
        ),
        (
            "packets_received_verified_count",
            stats.packets_received_verified_count.clear(),
            i64
        ),
        (
            "packets_sent_gossip_requests_count",
            stats.packets_sent_gossip_requests_count.clear(),
            i64
        ),
        (
            "packets_sent_prune_messages_count",
            stats.packets_sent_prune_messages_count.clear(),
            i64
        ),
        (
            "packets_sent_pull_requests_count",
            stats.packets_sent_pull_requests_count.clear(),
            i64
        ),
        (
            "packets_sent_pull_responses_count",
            stats.packets_sent_pull_responses_count.clear(),
            i64
        ),
        (
            "packets_sent_push_messages_count",
            stats.packets_sent_push_messages_count.clear(),
            i64
        ),
        (
            "require_stake_for_gossip_unknown_stakes",
            stats.require_stake_for_gossip_unknown_stakes.clear(),
            i64
        ),
        ("trim_crds_table", stats.trim_crds_table.clear(), i64),
        (
            "trim_crds_table_failed",
            stats.trim_crds_table_failed.clear(),
            i64
        ),
        (
            "trim_crds_table_purged_values_count",
            stats.trim_crds_table_purged_values_count.clear(),
            i64
        ),
    );
    let counts: Vec<_> = crds_stats
        .pull
        .counts
        .iter()
        .zip(crds_stats.push.counts.iter())
        .map(|(a, b)| a + b)
        .collect();
    datapoint_info!(
        "cluster_info_crds_stats",
        ("ContactInfo", counts[0], i64),
        ("ContactInfo-push", crds_stats.push.counts[0], i64),
        ("ContactInfo-pull", crds_stats.pull.counts[0], i64),
        ("Vote", counts[1], i64),
        ("Vote-push", crds_stats.push.counts[1], i64),
        ("Vote-pull", crds_stats.pull.counts[1], i64),
        ("LowestSlot", counts[2], i64),
        ("LowestSlot-push", crds_stats.push.counts[2], i64),
        ("LowestSlot-pull", crds_stats.pull.counts[2], i64),
        ("SnapshotHashes", counts[3], i64),
        ("SnapshotHashes-push", crds_stats.push.counts[3], i64),
        ("SnapshotHashes-pull", crds_stats.pull.counts[3], i64),
        ("AccountsHashes", counts[4], i64),
        ("AccountsHashes-push", crds_stats.push.counts[4], i64),
        ("AccountsHashes-pull", crds_stats.pull.counts[4], i64),
        ("EpochSlots", counts[5], i64),
        ("EpochSlots-push", crds_stats.push.counts[5], i64),
        ("EpochSlots-pull", crds_stats.pull.counts[5], i64),
        ("LegacyVersion", counts[6], i64),
        ("LegacyVersion-push", crds_stats.push.counts[6], i64),
        ("LegacyVersion-pull", crds_stats.pull.counts[6], i64),
        ("Version", counts[7], i64),
        ("Version-push", crds_stats.push.counts[7], i64),
        ("Version-pull", crds_stats.pull.counts[7], i64),
        ("NodeInstance", counts[8], i64),
        ("NodeInstance-push", crds_stats.push.counts[8], i64),
        ("NodeInstance-pull", crds_stats.pull.counts[8], i64),
        ("DuplicateShred", counts[9], i64),
        ("DuplicateShred-push", crds_stats.push.counts[9], i64),
        ("DuplicateShred-pull", crds_stats.pull.counts[9], i64),
        ("IncrementalSnapshotHashes", counts[10], i64),
        (
            "IncrementalSnapshotHashes-push",
            crds_stats.push.counts[10],
            i64
        ),
        (
            "IncrementalSnapshotHashes-pull",
            crds_stats.pull.counts[10],
            i64
        ),
        ("all", counts.iter().sum::<usize>(), i64),
        (
            "all-push",
            crds_stats.push.counts.iter().sum::<usize>(),
            i64
        ),
        (
            "all-pull",
            crds_stats.pull.counts.iter().sum::<usize>(),
            i64
        ),
    );
    let fails: Vec<_> = crds_stats
        .pull
        .fails
        .iter()
        .zip(crds_stats.push.fails.iter())
        .map(|(a, b)| a + b)
        .collect();
    datapoint_info!(
        "cluster_info_crds_stats_fails",
        ("ContactInfo", fails[0], i64),
        ("ContactInfo-push", crds_stats.push.fails[0], i64),
        ("ContactInfo-pull", crds_stats.pull.fails[0], i64),
        ("Vote", fails[1], i64),
        ("Vote-push", crds_stats.push.fails[1], i64),
        ("Vote-pull", crds_stats.pull.fails[1], i64),
        ("LowestSlot", fails[2], i64),
        ("LowestSlot-push", crds_stats.push.fails[2], i64),
        ("LowestSlot-pull", crds_stats.pull.fails[2], i64),
        ("SnapshotHashes", fails[3], i64),
        ("SnapshotHashes-push", crds_stats.push.fails[3], i64),
        ("SnapshotHashes-pull", crds_stats.pull.fails[3], i64),
        ("AccountsHashes", fails[4], i64),
        ("AccountsHashes-push", crds_stats.push.fails[4], i64),
        ("AccountsHashes-pull", crds_stats.pull.fails[4], i64),
        ("EpochSlots", fails[5], i64),
        ("EpochSlots-push", crds_stats.push.fails[5], i64),
        ("EpochSlots-pull", crds_stats.pull.fails[5], i64),
        ("LegacyVersion", fails[6], i64),
        ("LegacyVersion-push", crds_stats.push.fails[6], i64),
        ("LegacyVersion-pull", crds_stats.pull.fails[6], i64),
        ("Version", fails[7], i64),
        ("Version-push", crds_stats.push.fails[7], i64),
        ("Version-pull", crds_stats.pull.fails[7], i64),
        ("NodeInstance", fails[8], i64),
        ("NodeInstance-push", crds_stats.push.fails[8], i64),
        ("NodeInstance-pull", crds_stats.pull.fails[8], i64),
        ("DuplicateShred", fails[9], i64),
        ("DuplicateShred-push", crds_stats.push.fails[9], i64),
        ("DuplicateShred-pull", crds_stats.pull.fails[9], i64),
        ("IncrementalSnapshotHashes", fails[10], i64),
        (
            "IncrementalSnapshotHashes-push",
            crds_stats.push.fails[10],
            i64
        ),
        (
            "IncrementalSnapshotHashes-pull",
            crds_stats.pull.fails[10],
            i64
        ),
        ("all", fails.iter().sum::<usize>(), i64),
        ("all-push", crds_stats.push.fails.iter().sum::<usize>(), i64),
        ("all-pull", crds_stats.pull.fails.iter().sum::<usize>(), i64),
    );
    for (slot, num_votes) in &crds_stats.pull.votes {
        datapoint_info!(
            "cluster_info_crds_stats_votes_pull",
            ("slot", *slot, i64),
            ("num_votes", *num_votes, i64),
        );
    }
    for (slot, num_votes) in &crds_stats.push.votes {
        datapoint_info!(
            "cluster_info_crds_stats_votes_push",
            ("slot", *slot, i64),
            ("num_votes", *num_votes, i64),
        );
    }
    let votes: HashMap<Slot, usize> = crds_stats
        .pull
        .votes
        .into_iter()
        .map(|(slot, num_votes)| (*slot, *num_votes))
        .chain(
            crds_stats
                .push
                .votes
                .into_iter()
                .map(|(slot, num_votes)| (*slot, *num_votes)),
        )
        .into_grouping_map()
        .aggregate(|acc, _slot, num_votes| Some(acc.unwrap_or_default() + num_votes));
    for (slot, num_votes) in votes {
        datapoint_info!(
            "cluster_info_crds_stats_votes",
            ("slot", slot, i64),
            ("num_votes", num_votes, i64),
        );
    }
}
