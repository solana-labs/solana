//! Crds Gossip Pull overlay
//! This module implements the anti-entropy protocol for the network.
//!
//! The basic strategy is as follows:
//! 1. Construct a bloom filter of the local data set
//! 2. Randomly ask a node on the network for data that is not contained in the bloom filter.
//!
//! Bloom filters have a false positive rate.  Each requests uses a different bloom filter
//! with random hash functions.  So each subsequent request will have a different distribution
//! of false positives.

use crate::contact_info::ContactInfo;
use crate::crds::{Crds, VersionedCrdsValue};
use crate::crds_gossip::{get_stake, get_weight, CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS};
use crate::crds_gossip_error::CrdsGossipError;
use crate::crds_value::{CrdsValue, CrdsValueLabel};
use rand::distributions::{Distribution, WeightedIndex};
use rand::Rng;
use rayon::{prelude::*, ThreadPool};
use solana_runtime::bloom::{AtomicBloom, Bloom};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::cmp;
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::ops::Index;

pub const CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS: u64 = 15000;
// The maximum age of a value received over pull responses
pub const CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS: u64 = 60000;
// Retention period of hashes of received outdated values.
const FAILED_INSERTS_RETENTION_MS: u64 = 20_000;
pub const FALSE_RATE: f64 = 0.1f64;
pub const KEYS: f64 = 8f64;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, AbiExample)]
pub struct CrdsFilter {
    pub filter: Bloom<Hash>,
    mask: u64,
    mask_bits: u32,
}

impl Default for CrdsFilter {
    fn default() -> Self {
        CrdsFilter {
            filter: Bloom::default(),
            mask: !0u64,
            mask_bits: 0u32,
        }
    }
}

impl solana_sdk::sanitize::Sanitize for CrdsFilter {
    fn sanitize(&self) -> std::result::Result<(), solana_sdk::sanitize::SanitizeError> {
        self.filter.sanitize()?;
        Ok(())
    }
}

impl CrdsFilter {
    pub fn new_rand(num_items: usize, max_bytes: usize) -> Self {
        let max_bits = (max_bytes * 8) as f64;
        let max_items = Self::max_items(max_bits, FALSE_RATE, KEYS);
        let mask_bits = Self::mask_bits(num_items as f64, max_items as f64);
        let filter = Bloom::random(max_items as usize, FALSE_RATE, max_bits as usize);
        let seed: u64 = rand::thread_rng().gen_range(0, 2u64.pow(mask_bits));
        let mask = Self::compute_mask(seed, mask_bits);
        CrdsFilter {
            filter,
            mask,
            mask_bits,
        }
    }

    fn compute_mask(seed: u64, mask_bits: u32) -> u64 {
        assert!(seed <= 2u64.pow(mask_bits));
        let seed: u64 = seed.checked_shl(64 - mask_bits).unwrap_or(0x0);
        seed | (!0u64).checked_shr(mask_bits).unwrap_or(!0x0) as u64
    }
    pub fn max_items(max_bits: f64, false_rate: f64, num_keys: f64) -> f64 {
        let m = max_bits;
        let p = false_rate;
        let k = num_keys;
        (m / (-k / (1f64 - (p.ln() / k).exp()).ln())).ceil()
    }
    fn mask_bits(num_items: f64, max_items: f64) -> u32 {
        // for small ratios this can result in a negative number, ensure it returns 0 instead
        ((num_items / max_items).log2().ceil()).max(0.0) as u32
    }
    pub fn hash_as_u64(item: &Hash) -> u64 {
        let buf = item.as_ref()[..8].try_into().unwrap();
        u64::from_le_bytes(buf)
    }
    pub fn test_mask_u64(&self, item: u64, ones: u64) -> bool {
        let bits = item | ones;
        bits == self.mask
    }
    pub fn test_mask(&self, item: &Hash) -> bool {
        // only consider the highest mask_bits bits from the hash and set the rest to 1.
        let ones = (!0u64).checked_shr(self.mask_bits).unwrap_or(!0u64);
        let bits = Self::hash_as_u64(item) | ones;
        bits == self.mask
    }
    pub fn add(&mut self, item: &Hash) {
        if self.test_mask(item) {
            self.filter.add(item);
        }
    }
    pub fn contains(&self, item: &Hash) -> bool {
        if !self.test_mask(item) {
            return true;
        }
        self.filter.contains(item)
    }
    pub fn filter_contains(&self, item: &Hash) -> bool {
        self.filter.contains(item)
    }
}

/// A vector of crds filters that together hold a complete set of Hashes.
struct CrdsFilterSet {
    filters: Vec<AtomicBloom<Hash>>,
    mask_bits: u32,
}

impl CrdsFilterSet {
    fn new(num_items: usize, max_bytes: usize) -> Self {
        let max_bits = (max_bytes * 8) as f64;
        let max_items = CrdsFilter::max_items(max_bits, FALSE_RATE, KEYS);
        let mask_bits = CrdsFilter::mask_bits(num_items as f64, max_items as f64);
        let filters = std::iter::repeat_with(|| {
            Bloom::random(max_items as usize, FALSE_RATE, max_bits as usize).into()
        })
        .take(1 << mask_bits)
        .collect();
        Self { filters, mask_bits }
    }

    fn add(&self, hash_value: Hash) {
        let index = CrdsFilter::hash_as_u64(&hash_value)
            .checked_shr(64 - self.mask_bits)
            .unwrap_or(0);
        self.filters[index as usize].add(&hash_value);
    }
}

impl Into<Vec<CrdsFilter>> for CrdsFilterSet {
    fn into(self) -> Vec<CrdsFilter> {
        let mask_bits = self.mask_bits;
        self.filters
            .into_iter()
            .enumerate()
            .map(|(seed, filter)| CrdsFilter {
                filter: filter.into(),
                mask: CrdsFilter::compute_mask(seed as u64, mask_bits),
                mask_bits,
            })
            .collect()
    }
}

#[derive(Default)]
pub struct ProcessPullStats {
    pub success: usize,
    pub failed_insert: usize,
    pub failed_timeout: usize,
    pub timeout_count: usize,
}

#[derive(Clone)]
pub struct CrdsGossipPull {
    /// timestamp of last request
    pub pull_request_time: HashMap<Pubkey, u64>,
    /// hash and insert time
    pub purged_values: VecDeque<(Hash, u64)>,
    // Hash value and record time (ms) of the pull responses which failed to be
    // inserted in crds table; Preserved to stop the sender to send back the
    // same outdated payload again by adding them to the filter for the next
    // pull request.
    pub failed_inserts: VecDeque<(Hash, u64)>,
    pub crds_timeout: u64,
    pub msg_timeout: u64,
    pub num_pulls: usize,
}

impl Default for CrdsGossipPull {
    fn default() -> Self {
        Self {
            purged_values: VecDeque::new(),
            pull_request_time: HashMap::new(),
            failed_inserts: VecDeque::new(),
            crds_timeout: CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS,
            msg_timeout: CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS,
            num_pulls: 0,
        }
    }
}
impl CrdsGossipPull {
    /// generate a random request
    pub fn new_pull_request(
        &self,
        thread_pool: &ThreadPool,
        crds: &Crds,
        self_id: &Pubkey,
        self_shred_version: u16,
        now: u64,
        gossip_validators: Option<&HashSet<Pubkey>>,
        stakes: &HashMap<Pubkey, u64>,
        bloom_size: usize,
    ) -> Result<(Pubkey, Vec<CrdsFilter>, CrdsValue), CrdsGossipError> {
        let options = self.pull_options(
            crds,
            &self_id,
            self_shred_version,
            now,
            gossip_validators,
            stakes,
        );
        if options.is_empty() {
            return Err(CrdsGossipError::NoPeers);
        }
        let filters = self.build_crds_filters(thread_pool, crds, bloom_size);
        let index = WeightedIndex::new(options.iter().map(|weighted| weighted.0)).unwrap();
        let random = index.sample(&mut rand::thread_rng());
        let self_info = crds
            .lookup(&CrdsValueLabel::ContactInfo(*self_id))
            .unwrap_or_else(|| panic!("self_id invalid {}", self_id));
        Ok((options[random].1.id, filters, self_info.clone()))
    }

    fn pull_options<'a>(
        &self,
        crds: &'a Crds,
        self_id: &Pubkey,
        self_shred_version: u16,
        now: u64,
        gossip_validators: Option<&HashSet<Pubkey>>,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Vec<(f32, &'a ContactInfo)> {
        crds.table
            .values()
            .filter_map(|v| v.value.contact_info())
            .filter(|v| {
                v.id != *self_id
                    && ContactInfo::is_valid_address(&v.gossip)
                    && (self_shred_version == 0 || self_shred_version == v.shred_version)
                    && gossip_validators
                        .map_or(true, |gossip_validators| gossip_validators.contains(&v.id))
            })
            .map(|item| {
                let max_weight = f32::from(u16::max_value()) - 1.0;
                let req_time: u64 = *self.pull_request_time.get(&item.id).unwrap_or(&0);
                let since = ((now - req_time) / 1024) as u32;
                let stake = get_stake(&item.id, stakes);
                let weight = get_weight(max_weight, since, stake);
                (weight, item)
            })
            .collect()
    }

    /// time when a request to `from` was initiated
    /// This is used for weighted random selection during `new_pull_request`
    /// It's important to use the local nodes request creation time as the weight
    /// instead of the response received time otherwise failed nodes will increase their weight.
    pub fn mark_pull_request_creation_time(&mut self, from: &Pubkey, now: u64) {
        self.pull_request_time.insert(*from, now);
    }

    /// Store an old hash in the purged values set
    pub fn record_old_hash(&mut self, hash: Hash, timestamp: u64) {
        self.purged_values.push_back((hash, timestamp))
    }

    /// process a pull request
    pub fn process_pull_requests(
        &mut self,
        crds: &mut Crds,
        requests: Vec<(CrdsValue, CrdsFilter)>,
        now: u64,
    ) {
        requests.into_iter().for_each(|(caller, _)| {
            let key = caller.label().pubkey();
            if let Ok(Some(val)) = crds.insert(caller, now) {
                self.purged_values
                    .push_back((val.value_hash, val.local_timestamp));
            }
            crds.update_record_timestamp(&key, now);
        });
    }

    /// Create gossip responses to pull requests
    pub fn generate_pull_responses(
        &self,
        crds: &Crds,
        requests: &[(CrdsValue, CrdsFilter)],
        now: u64,
    ) -> Vec<Vec<CrdsValue>> {
        self.filter_crds_values(crds, requests, now)
    }

    // Checks if responses should be inserted and
    // returns those responses converted to VersionedCrdsValue
    // Separated in three vecs as:
    //  .0 => responses that update the owner timestamp
    //  .1 => responses that do not update the owner timestamp
    //  .2 => hash value of outdated values which will fail to insert.
    pub fn filter_pull_responses(
        &self,
        crds: &Crds,
        timeouts: &HashMap<Pubkey, u64>,
        responses: Vec<CrdsValue>,
        now: u64,
        stats: &mut ProcessPullStats,
    ) -> (Vec<VersionedCrdsValue>, Vec<VersionedCrdsValue>, Vec<Hash>) {
        let mut versioned = vec![];
        let mut versioned_expired_timestamp = vec![];
        let mut failed_inserts = vec![];
        let mut maybe_push = |response, values: &mut Vec<VersionedCrdsValue>| {
            let (push, value) = crds.would_insert(response, now);
            if push {
                values.push(value);
            } else {
                failed_inserts.push(value.value_hash)
            }
        };
        for r in responses {
            let owner = r.label().pubkey();
            // Check if the crds value is older than the msg_timeout
            if now
                > r.wallclock()
                    .checked_add(self.msg_timeout)
                    .unwrap_or_else(|| 0)
                || now + self.msg_timeout < r.wallclock()
            {
                match &r.label() {
                    CrdsValueLabel::ContactInfo(_) => {
                        // Check if this ContactInfo is actually too old, it's possible that it has
                        // stake and so might have a longer effective timeout
                        let timeout = *timeouts
                            .get(&owner)
                            .unwrap_or_else(|| timeouts.get(&Pubkey::default()).unwrap());
                        if now > r.wallclock().checked_add(timeout).unwrap_or_else(|| 0)
                            || now + timeout < r.wallclock()
                        {
                            stats.timeout_count += 1;
                            stats.failed_timeout += 1;
                            continue;
                        }
                    }
                    _ => {
                        // Before discarding this value, check if a ContactInfo for the owner
                        // exists in the table. If it doesn't, that implies that this value can be discarded
                        if crds.lookup(&CrdsValueLabel::ContactInfo(owner)).is_none() {
                            stats.timeout_count += 1;
                            stats.failed_timeout += 1;
                        } else {
                            // Silently insert this old value without bumping record timestamps
                            maybe_push(r, &mut versioned_expired_timestamp);
                        }
                        continue;
                    }
                }
            }
            maybe_push(r, &mut versioned);
        }
        (versioned, versioned_expired_timestamp, failed_inserts)
    }

    /// process a vec of pull responses
    pub fn process_pull_responses(
        &mut self,
        crds: &mut Crds,
        from: &Pubkey,
        responses: Vec<VersionedCrdsValue>,
        responses_expired_timeout: Vec<VersionedCrdsValue>,
        mut failed_inserts: Vec<Hash>,
        now: u64,
        stats: &mut ProcessPullStats,
    ) -> Vec<(CrdsValueLabel, Hash, u64)> {
        let mut success = vec![];
        let mut owners = HashSet::new();
        for r in responses_expired_timeout {
            let value_hash = r.value_hash;
            if crds.insert_versioned(r).is_err() {
                failed_inserts.push(value_hash);
            }
        }
        for r in responses {
            let label = r.value.label();
            let wc = r.value.wallclock();
            let hash = r.value_hash;
            match crds.insert_versioned(r) {
                Err(_) => failed_inserts.push(hash),
                Ok(old) => {
                    stats.success += 1;
                    self.num_pulls += 1;
                    owners.insert(label.pubkey());
                    success.push((label, hash, wc));
                    if let Some(val) = old {
                        self.purged_values
                            .push_back((val.value_hash, val.local_timestamp))
                    }
                }
            }
        }
        owners.insert(*from);
        for owner in owners {
            crds.update_record_timestamp(&owner, now);
        }
        stats.failed_insert += failed_inserts.len();
        self.purge_failed_inserts(now);
        self.failed_inserts
            .extend(failed_inserts.into_iter().zip(std::iter::repeat(now)));
        success
    }

    pub fn purge_failed_inserts(&mut self, now: u64) {
        if FAILED_INSERTS_RETENTION_MS < now {
            let cutoff = now - FAILED_INSERTS_RETENTION_MS;
            let outdated = self
                .failed_inserts
                .iter()
                .take_while(|(_, ts)| *ts < cutoff)
                .count();
            self.failed_inserts.drain(..outdated);
        }
    }

    // build a set of filters of the current crds table
    // num_filters - used to increase the likelyhood of a value in crds being added to some filter
    pub fn build_crds_filters(
        &self,
        thread_pool: &ThreadPool,
        crds: &Crds,
        bloom_size: usize,
    ) -> Vec<CrdsFilter> {
        const PAR_MIN_LENGTH: usize = 512;
        let num = cmp::max(
            CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS,
            crds.table.len() + self.purged_values.len() + self.failed_inserts.len(),
        );
        let filters = CrdsFilterSet::new(num, bloom_size);
        thread_pool.install(|| {
            crds.table
                .par_values()
                .with_min_len(PAR_MIN_LENGTH)
                .map(|v| v.value_hash)
                .chain(
                    self.purged_values
                        .par_iter()
                        .with_min_len(PAR_MIN_LENGTH)
                        .map(|(v, _)| *v),
                )
                .chain(
                    self.failed_inserts
                        .par_iter()
                        .with_min_len(PAR_MIN_LENGTH)
                        .map(|(v, _)| *v),
                )
                .for_each(|v| filters.add(v));
        });
        filters.into()
    }

    /// filter values that fail the bloom filter up to max_bytes
    fn filter_crds_values(
        &self,
        crds: &Crds,
        filters: &[(CrdsValue, CrdsFilter)],
        now: u64,
    ) -> Vec<Vec<CrdsValue>> {
        let msg_timeout = CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
        let jitter = rand::thread_rng().gen_range(0, msg_timeout / 4);
        //skip filters from callers that are too old
        let future = now.saturating_add(msg_timeout);
        let past = now.saturating_sub(msg_timeout);
        let mut dropped_requests = 0;
        let mut total_skipped = 0;
        let ret = filters
            .iter()
            .map(|(caller, filter)| {
                let caller_wallclock = caller.wallclock();
                if caller_wallclock >= future || caller_wallclock < past {
                    dropped_requests += 1;
                    return vec![];
                }
                let caller_wallclock = caller_wallclock.checked_add(jitter).unwrap_or(0);
                crds.shards
                    .find(filter.mask, filter.mask_bits)
                    .filter_map(|index| {
                        let item = crds.table.index(index);
                        debug_assert!(filter.test_mask(&item.value_hash));
                        //skip values that are too new
                        if item.value.wallclock() > caller_wallclock {
                            total_skipped += 1;
                            None
                        } else if filter.filter_contains(&item.value_hash) {
                            None
                        } else {
                            Some(item.value.clone())
                        }
                    })
                    .collect()
            })
            .collect();
        inc_new_counter_info!(
            "gossip_filter_crds_values-dropped_requests",
            dropped_requests
        );
        inc_new_counter_info!("gossip_filter_crds_values-dropped_values", total_skipped);
        ret
    }
    pub fn make_timeouts_def(
        &self,
        self_id: &Pubkey,
        stakes: &HashMap<Pubkey, u64>,
        epoch_ms: u64,
        min_ts: u64,
    ) -> HashMap<Pubkey, u64> {
        let mut timeouts: HashMap<Pubkey, u64> = stakes.keys().map(|s| (*s, epoch_ms)).collect();
        timeouts.insert(*self_id, std::u64::MAX);
        timeouts.insert(Pubkey::default(), min_ts);
        timeouts
    }

    pub fn make_timeouts(
        &self,
        self_id: &Pubkey,
        stakes: &HashMap<Pubkey, u64>,
        epoch_ms: u64,
    ) -> HashMap<Pubkey, u64> {
        self.make_timeouts_def(self_id, stakes, epoch_ms, self.crds_timeout)
    }

    /// Purge values from the crds that are older then `active_timeout`
    /// The value_hash of an active item is put into self.purged_values queue
    pub fn purge_active(
        &mut self,
        crds: &mut Crds,
        now: u64,
        timeouts: &HashMap<Pubkey, u64>,
    ) -> usize {
        let old = crds.find_old_labels(now, timeouts);
        let mut purged: VecDeque<_> = old
            .iter()
            .filter_map(|label| {
                let rv = crds
                    .lookup_versioned(label)
                    .map(|val| (val.value_hash, val.local_timestamp));
                crds.remove(label);
                rv
            })
            .collect();
        let ret = purged.len();
        self.purged_values.append(&mut purged);
        ret
    }
    /// Purge values from the `self.purged_values` queue that are older then purge_timeout
    pub fn purge_purged(&mut self, min_ts: u64) {
        let cnt = self
            .purged_values
            .iter()
            .take_while(|v| v.1 < min_ts)
            .count();
        self.purged_values.drain(..cnt);
    }

    /// For legacy tests
    #[cfg(test)]
    pub fn process_pull_response(
        &mut self,
        crds: &mut Crds,
        from: &Pubkey,
        timeouts: &HashMap<Pubkey, u64>,
        response: Vec<CrdsValue>,
        now: u64,
    ) -> (usize, usize, usize) {
        let mut stats = ProcessPullStats::default();
        let (versioned, versioned_expired_timeout, failed_inserts) =
            self.filter_pull_responses(crds, timeouts, response, now, &mut stats);
        self.process_pull_responses(
            crds,
            from,
            versioned,
            versioned_expired_timeout,
            failed_inserts,
            now,
            &mut stats,
        );
        (
            stats.failed_timeout + stats.failed_insert,
            stats.timeout_count,
            stats.success,
        )
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use crate::cluster_info::MAX_BLOOM_SIZE;
    use crate::contact_info::ContactInfo;
    use crate::crds_value::{CrdsData, Vote};
    use itertools::Itertools;
    use rand::thread_rng;
    use rayon::ThreadPoolBuilder;
    use solana_perf::test_tx::test_tx;
    use solana_sdk::hash::{hash, HASH_BYTES};
    use solana_sdk::packet::PACKET_DATA_SIZE;

    #[test]
    fn test_hash_as_u64() {
        let arr: Vec<u8> = (0..HASH_BYTES).map(|i| i as u8 + 1).collect();
        let hash = Hash::new(&arr);
        assert_eq!(CrdsFilter::hash_as_u64(&hash), 0x807060504030201);
    }

    #[test]
    fn test_hash_as_u64_random() {
        fn hash_as_u64_bitops(hash: &Hash) -> u64 {
            let mut out = 0;
            for (i, val) in hash.as_ref().iter().enumerate().take(8) {
                out |= (u64::from(*val)) << (i * 8) as u64;
            }
            out
        }
        let mut rng = thread_rng();
        for _ in 0..100 {
            let hash = Hash::new_rand(&mut rng);
            assert_eq!(CrdsFilter::hash_as_u64(&hash), hash_as_u64_bitops(&hash));
        }
    }

    #[test]
    fn test_crds_filter_default() {
        let filter = CrdsFilter::default();
        let mask = CrdsFilter::compute_mask(0, filter.mask_bits);
        assert_eq!(filter.mask, mask);
        let mut rng = thread_rng();
        for _ in 0..10 {
            let hash = Hash::new_rand(&mut rng);
            assert!(filter.test_mask(&hash));
        }
    }

    #[test]
    fn test_new_pull_with_stakes() {
        let mut crds = Crds::default();
        let mut stakes = HashMap::new();
        let node = CrdsGossipPull::default();
        let me = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        crds.insert(me.clone(), 0).unwrap();
        for i in 1..=30 {
            let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &solana_sdk::pubkey::new_rand(),
                0,
            )));
            let id = entry.label().pubkey();
            crds.insert(entry.clone(), 0).unwrap();
            stakes.insert(id, i * 100);
        }
        let now = 1024;
        let mut options = node.pull_options(&crds, &me.label().pubkey(), 0, now, None, &stakes);
        assert!(!options.is_empty());
        options.sort_by(|(weight_l, _), (weight_r, _)| weight_r.partial_cmp(weight_l).unwrap());
        // check that the highest stake holder is also the heaviest weighted.
        assert_eq!(
            *stakes.get(&options.get(0).unwrap().1.id).unwrap(),
            3000_u64
        );
    }

    #[test]
    fn test_no_pulls_from_different_shred_versions() {
        let mut crds = Crds::default();
        let stakes = HashMap::new();
        let node = CrdsGossipPull::default();

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

        crds.insert(me.clone(), 0).unwrap();
        crds.insert(spy.clone(), 0).unwrap();
        crds.insert(node_123.clone(), 0).unwrap();
        crds.insert(node_456.clone(), 0).unwrap();

        // shred version 123 should ignore nodes with versions 0 and 456
        let options = node
            .pull_options(&crds, &me.label().pubkey(), 123, 0, None, &stakes)
            .iter()
            .map(|(_, c)| c.id)
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 1);
        assert!(!options.contains(&spy.pubkey()));
        assert!(options.contains(&node_123.pubkey()));

        // spy nodes will see all
        let options = node
            .pull_options(&crds, &spy.label().pubkey(), 0, 0, None, &stakes)
            .iter()
            .map(|(_, c)| c.id)
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 3);
        assert!(options.contains(&me.pubkey()));
        assert!(options.contains(&node_123.pubkey()));
        assert!(options.contains(&node_456.pubkey()));
    }

    #[test]
    fn test_pulls_only_from_allowed() {
        let mut crds = Crds::default();
        let stakes = HashMap::new();
        let node = CrdsGossipPull::default();
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

        crds.insert(me.clone(), 0).unwrap();
        crds.insert(node_123.clone(), 0).unwrap();

        // Empty gossip_validators -- will pull from nobody
        let mut gossip_validators = HashSet::new();
        let options = node.pull_options(
            &crds,
            &me.label().pubkey(),
            0,
            0,
            Some(&gossip_validators),
            &stakes,
        );
        assert!(options.is_empty());

        // Unknown pubkey in gossip_validators -- will pull from nobody
        gossip_validators.insert(solana_sdk::pubkey::new_rand());
        let options = node.pull_options(
            &crds,
            &me.label().pubkey(),
            0,
            0,
            Some(&gossip_validators),
            &stakes,
        );
        assert!(options.is_empty());

        // node_123 pubkey in gossip_validators -- will pull from it
        gossip_validators.insert(node_123.pubkey());
        let options = node.pull_options(
            &crds,
            &me.label().pubkey(),
            0,
            0,
            Some(&gossip_validators),
            &stakes,
        );
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].1.id, node_123.pubkey());
    }

    #[test]
    fn test_crds_filter_set_add() {
        let mut rng = thread_rng();
        let crds_filter_set =
            CrdsFilterSet::new(/*num_items=*/ 9672788, /*max_bytes=*/ 8196);
        let hash_values: Vec<_> = std::iter::repeat_with(|| Hash::new_rand(&mut rng))
            .take(1024)
            .collect();
        for hash_value in &hash_values {
            crds_filter_set.add(*hash_value);
        }
        let filters: Vec<CrdsFilter> = crds_filter_set.into();
        assert_eq!(filters.len(), 1024);
        for hash_value in hash_values {
            let mut num_hits = 0;
            let mut false_positives = 0;
            for filter in &filters {
                if filter.test_mask(&hash_value) {
                    num_hits += 1;
                    assert!(filter.contains(&hash_value));
                    assert!(filter.filter.contains(&hash_value));
                } else if filter.filter.contains(&hash_value) {
                    false_positives += 1;
                }
            }
            assert_eq!(num_hits, 1);
            assert!(false_positives < 5);
        }
    }

    #[test]
    fn test_crds_filter_set_new() {
        // Validates invariances required by CrdsFilterSet::get in the
        // vector of filters generated by CrdsFilterSet::new.
        let filters: Vec<CrdsFilter> =
            CrdsFilterSet::new(/*num_items=*/ 55345017, /*max_bytes=*/ 4098).into();
        assert_eq!(filters.len(), 16384);
        let mask_bits = filters[0].mask_bits;
        let right_shift = 64 - mask_bits;
        let ones = !0u64 >> mask_bits;
        for (i, filter) in filters.iter().enumerate() {
            // Check that all mask_bits are equal.
            assert_eq!(mask_bits, filter.mask_bits);
            assert_eq!(i as u64, filter.mask >> right_shift);
            assert_eq!(ones, ones & filter.mask);
        }
    }

    #[test]
    fn test_build_crds_filter() {
        let mut rng = thread_rng();
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds_gossip_pull = CrdsGossipPull::default();
        let mut crds = Crds::default();
        for _ in 0..10_000 {
            crds_gossip_pull
                .purged_values
                .push_back((Hash::new_rand(&mut rng), rng.gen()));
        }
        let mut num_inserts = 0;
        for _ in 0..20_000 {
            if crds
                .insert(CrdsValue::new_rand(&mut rng), rng.gen())
                .is_ok()
            {
                num_inserts += 1;
            }
        }
        assert_eq!(num_inserts, 20_000);
        let filters = crds_gossip_pull.build_crds_filters(&thread_pool, &crds, MAX_BLOOM_SIZE);
        assert_eq!(filters.len(), 32);
        let hash_values: Vec<_> = crds
            .table
            .values()
            .map(|v| v.value_hash)
            .chain(
                crds_gossip_pull
                    .purged_values
                    .iter()
                    .map(|(value_hash, _)| value_hash)
                    .cloned(),
            )
            .collect();
        assert_eq!(hash_values.len(), 10_000 + 20_000);
        let mut false_positives = 0;
        for hash_value in hash_values {
            let mut num_hits = 0;
            for filter in &filters {
                if filter.test_mask(&hash_value) {
                    num_hits += 1;
                    assert!(filter.contains(&hash_value));
                    assert!(filter.filter.contains(&hash_value));
                } else if filter.filter.contains(&hash_value) {
                    false_positives += 1;
                }
            }
            assert_eq!(num_hits, 1);
        }
        assert!(false_positives < 50_000, "fp: {}", false_positives);
    }

    #[test]
    fn test_new_pull_request() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds = Crds::default();
        let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let id = entry.label().pubkey();
        let node = CrdsGossipPull::default();
        assert_eq!(
            node.new_pull_request(
                &thread_pool,
                &crds,
                &id,
                0,
                0,
                None,
                &HashMap::new(),
                PACKET_DATA_SIZE
            ),
            Err(CrdsGossipError::NoPeers)
        );

        crds.insert(entry.clone(), 0).unwrap();
        assert_eq!(
            node.new_pull_request(
                &thread_pool,
                &crds,
                &id,
                0,
                0,
                None,
                &HashMap::new(),
                PACKET_DATA_SIZE
            ),
            Err(CrdsGossipError::NoPeers)
        );

        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        crds.insert(new.clone(), 0).unwrap();
        let req = node.new_pull_request(
            &thread_pool,
            &crds,
            &id,
            0,
            0,
            None,
            &HashMap::new(),
            PACKET_DATA_SIZE,
        );
        let (to, _, self_info) = req.unwrap();
        assert_eq!(to, new.label().pubkey());
        assert_eq!(self_info, entry);
    }

    #[test]
    fn test_new_mark_creation_time() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds = Crds::default();
        let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let node_pubkey = entry.label().pubkey();
        let mut node = CrdsGossipPull::default();
        crds.insert(entry.clone(), 0).unwrap();
        let old = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        crds.insert(old.clone(), 0).unwrap();
        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        crds.insert(new.clone(), 0).unwrap();

        // set request creation time to max_value
        node.mark_pull_request_creation_time(&new.label().pubkey(), u64::max_value());

        // odds of getting the other request should be 1 in u64::max_value()
        for _ in 0..10 {
            let req = node.new_pull_request(
                &thread_pool,
                &crds,
                &node_pubkey,
                0,
                u64::max_value(),
                None,
                &HashMap::new(),
                PACKET_DATA_SIZE,
            );
            let (to, _, self_info) = req.unwrap();
            assert_eq!(to, old.label().pubkey());
            assert_eq!(self_info, entry);
        }
    }

    #[test]
    fn test_generate_pull_responses() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut node_crds = Crds::default();
        let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let node_pubkey = entry.label().pubkey();
        let node = CrdsGossipPull::default();
        node_crds.insert(entry, 0).unwrap();
        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        node_crds.insert(new, 0).unwrap();
        let req = node.new_pull_request(
            &thread_pool,
            &node_crds,
            &node_pubkey,
            0,
            0,
            None,
            &HashMap::new(),
            PACKET_DATA_SIZE,
        );

        let mut dest_crds = Crds::default();
        let dest = CrdsGossipPull::default();
        let (_, filters, caller) = req.unwrap();
        let mut filters: Vec<_> = filters.into_iter().map(|f| (caller.clone(), f)).collect();
        let rsp = dest.generate_pull_responses(&dest_crds, &filters, 0);

        assert_eq!(rsp[0].len(), 0);

        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS,
        )));
        dest_crds
            .insert(new, CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS)
            .unwrap();

        //should skip new value since caller is to old
        let rsp =
            dest.generate_pull_responses(&dest_crds, &filters, CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS);
        assert_eq!(rsp[0].len(), 0);

        assert_eq!(filters.len(), 1);
        filters.push(filters[0].clone());
        //should return new value since caller is new
        filters[1].0 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS + 1,
        )));

        let rsp =
            dest.generate_pull_responses(&dest_crds, &filters, CRDS_GOSSIP_PULL_MSG_TIMEOUT_MS);
        assert_eq!(rsp.len(), 2);
        assert_eq!(rsp[0].len(), 0);
        assert_eq!(rsp[1].len(), 1); // Orders are also preserved.
    }

    #[test]
    fn test_process_pull_request() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut node_crds = Crds::default();
        let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let node_pubkey = entry.label().pubkey();
        let node = CrdsGossipPull::default();
        node_crds.insert(entry, 0).unwrap();
        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        node_crds.insert(new, 0).unwrap();
        let req = node.new_pull_request(
            &thread_pool,
            &node_crds,
            &node_pubkey,
            0,
            0,
            None,
            &HashMap::new(),
            PACKET_DATA_SIZE,
        );

        let mut dest_crds = Crds::default();
        let mut dest = CrdsGossipPull::default();
        let (_, filters, caller) = req.unwrap();
        let filters: Vec<_> = filters.into_iter().map(|f| (caller.clone(), f)).collect();
        let rsp = dest.generate_pull_responses(&dest_crds, &filters, 0);
        dest.process_pull_requests(&mut dest_crds, filters, 1);
        assert!(rsp.iter().all(|rsp| rsp.is_empty()));
        assert!(dest_crds.lookup(&caller.label()).is_some());
        assert_eq!(
            dest_crds
                .lookup_versioned(&caller.label())
                .unwrap()
                .insert_timestamp,
            1
        );
        assert_eq!(
            dest_crds
                .lookup_versioned(&caller.label())
                .unwrap()
                .local_timestamp,
            1
        );
    }
    #[test]
    fn test_process_pull_request_response() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut node_crds = Crds::default();
        let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            1,
        )));
        let node_pubkey = entry.label().pubkey();
        let mut node = CrdsGossipPull::default();
        node_crds.insert(entry, 0).unwrap();

        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            1,
        )));
        node_crds.insert(new, 0).unwrap();

        let mut dest = CrdsGossipPull::default();
        let mut dest_crds = Crds::default();
        let new_id = solana_sdk::pubkey::new_rand();
        let new = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &new_id, 1,
        )));
        dest_crds.insert(new.clone(), 0).unwrap();

        // node contains a key from the dest node, but at an older local timestamp
        let same_key = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &new_id, 0,
        )));
        assert_eq!(same_key.label(), new.label());
        assert!(same_key.wallclock() < new.wallclock());
        node_crds.insert(same_key.clone(), 0).unwrap();
        assert_eq!(
            node_crds
                .lookup_versioned(&same_key.label())
                .unwrap()
                .local_timestamp,
            0
        );
        let mut done = false;
        for _ in 0..30 {
            // there is a chance of a false positive with bloom filters
            let req = node.new_pull_request(
                &thread_pool,
                &node_crds,
                &node_pubkey,
                0,
                0,
                None,
                &HashMap::new(),
                PACKET_DATA_SIZE,
            );
            let (_, filters, caller) = req.unwrap();
            let filters: Vec<_> = filters.into_iter().map(|f| (caller.clone(), f)).collect();
            let mut rsp = dest.generate_pull_responses(&dest_crds, &filters, 0);
            dest.process_pull_requests(&mut dest_crds, filters, 0);
            // if there is a false positive this is empty
            // prob should be around 0.1 per iteration
            if rsp.is_empty() {
                continue;
            }

            if rsp.is_empty() {
                continue;
            }
            assert_eq!(rsp.len(), 1);
            let failed = node
                .process_pull_response(
                    &mut node_crds,
                    &node_pubkey,
                    &node.make_timeouts_def(&node_pubkey, &HashMap::new(), 0, 1),
                    rsp.pop().unwrap(),
                    1,
                )
                .0;
            assert_eq!(failed, 0);
            assert_eq!(
                node_crds
                    .lookup_versioned(&new.label())
                    .unwrap()
                    .local_timestamp,
                1
            );
            // verify that the whole record was updated for dest since this is a response from dest
            assert_eq!(
                node_crds
                    .lookup_versioned(&same_key.label())
                    .unwrap()
                    .local_timestamp,
                1
            );
            done = true;
            break;
        }
        assert!(done);
    }
    #[test]
    fn test_gossip_purge() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut node_crds = Crds::default();
        let entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        let node_label = entry.label();
        let node_pubkey = node_label.pubkey();
        let mut node = CrdsGossipPull::default();
        node_crds.insert(entry, 0).unwrap();
        let old = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            0,
        )));
        node_crds.insert(old.clone(), 0).unwrap();
        let value_hash = node_crds.lookup_versioned(&old.label()).unwrap().value_hash;

        //verify self is valid
        assert_eq!(node_crds.lookup(&node_label).unwrap().label(), node_label);

        // purge
        let timeouts = node.make_timeouts_def(&node_pubkey, &HashMap::new(), 0, 1);
        node.purge_active(&mut node_crds, 2, &timeouts);

        //verify self is still valid after purge
        assert_eq!(node_crds.lookup(&node_label).unwrap().label(), node_label);

        assert_eq!(node_crds.lookup_versioned(&old.label()), None);
        assert_eq!(node.purged_values.len(), 1);
        for _ in 0..30 {
            // there is a chance of a false positive with bloom filters
            // assert that purged value is still in the set
            // chance of 30 consecutive false positives is 0.1^30
            let filters = node.build_crds_filters(&thread_pool, &node_crds, PACKET_DATA_SIZE);
            assert!(filters.iter().any(|filter| filter.contains(&value_hash)));
        }

        // purge the value
        node.purge_purged(1);
        assert_eq!(node.purged_values.len(), 0);
    }
    #[test]
    #[allow(clippy::float_cmp)]
    fn test_crds_filter_mask() {
        let filter = CrdsFilter::new_rand(1, 128);
        assert_eq!(filter.mask, !0x0);
        assert_eq!(CrdsFilter::max_items(80f64, 0.01, 8f64), 9f64);
        //1000/9 = 111, so 7 bits are needed to mask it
        assert_eq!(CrdsFilter::mask_bits(1000f64, 9f64), 7u32);
        let filter = CrdsFilter::new_rand(1000, 10);
        assert_eq!(filter.mask & 0x00_ffff_ffff, 0x00_ffff_ffff);
    }
    #[test]
    fn test_crds_filter_add_no_mask() {
        let mut filter = CrdsFilter::new_rand(1, 128);
        let h: Hash = hash(Hash::default().as_ref());
        assert!(!filter.contains(&h));
        filter.add(&h);
        assert!(filter.contains(&h));
        let h: Hash = hash(h.as_ref());
        assert!(!filter.contains(&h));
    }
    #[test]
    fn test_crds_filter_add_mask() {
        let mut filter = CrdsFilter::new_rand(1000, 10);
        let mut h: Hash = Hash::default();
        while !filter.test_mask(&h) {
            h = hash(h.as_ref());
        }
        assert!(filter.test_mask(&h));
        //if the mask succeeds, we want the guaranteed negative
        assert!(!filter.contains(&h));
        filter.add(&h);
        assert!(filter.contains(&h));
    }
    #[test]
    fn test_crds_filter_complete_set_add_mask() {
        let mut filters: Vec<CrdsFilter> = CrdsFilterSet::new(1000, 10).into();
        assert!(filters.iter().all(|f| f.mask_bits > 0));
        let mut h: Hash = Hash::default();
        // rev to make the hash::default() miss on the first few test_masks
        while !filters.iter().rev().any(|f| f.test_mask(&h)) {
            h = hash(h.as_ref());
        }
        let filter = filters.iter_mut().find(|f| f.test_mask(&h)).unwrap();
        assert!(filter.test_mask(&h));
        //if the mask succeeds, we want the guaranteed negative
        assert!(!filter.contains(&h));
        filter.add(&h);
        assert!(filter.contains(&h));
    }
    #[test]
    fn test_crds_filter_contains_mask() {
        let filter = CrdsFilter::new_rand(1000, 10);
        assert!(filter.mask_bits > 0);
        let mut h: Hash = Hash::default();
        while filter.test_mask(&h) {
            h = hash(h.as_ref());
        }
        assert!(!filter.test_mask(&h));
        //if the mask fails, the hash is contained in the set, and can be treated as a false
        //positive
        assert!(filter.contains(&h));
    }
    #[test]
    fn test_mask() {
        for i in 0..16 {
            run_test_mask(i);
        }
    }
    fn run_test_mask(mask_bits: u32) {
        let masks: Vec<_> = (0..2u64.pow(mask_bits))
            .map(|seed| CrdsFilter::compute_mask(seed, mask_bits))
            .dedup()
            .collect();
        assert_eq!(masks.len(), 2u64.pow(mask_bits) as usize)
    }

    #[test]
    fn test_process_pull_response() {
        let mut node_crds = Crds::default();
        let mut node = CrdsGossipPull::default();

        let peer_pubkey = solana_sdk::pubkey::new_rand();
        let peer_entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(
            ContactInfo::new_localhost(&peer_pubkey, 0),
        ));
        let mut timeouts = HashMap::new();
        timeouts.insert(Pubkey::default(), node.crds_timeout);
        timeouts.insert(peer_pubkey, node.msg_timeout + 1);
        // inserting a fresh value should be fine.
        assert_eq!(
            node.process_pull_response(
                &mut node_crds,
                &peer_pubkey,
                &timeouts,
                vec![peer_entry.clone()],
                1,
            )
            .0,
            0
        );

        let mut node_crds = Crds::default();
        let unstaked_peer_entry = CrdsValue::new_unsigned(CrdsData::ContactInfo(
            ContactInfo::new_localhost(&peer_pubkey, 0),
        ));
        // check that old contact infos fail if they are too old, regardless of "timeouts"
        assert_eq!(
            node.process_pull_response(
                &mut node_crds,
                &peer_pubkey,
                &timeouts,
                vec![peer_entry.clone(), unstaked_peer_entry],
                node.msg_timeout + 100,
            )
            .0,
            2
        );

        let mut node_crds = Crds::default();
        // check that old contact infos can still land as long as they have a "timeouts" entry
        assert_eq!(
            node.process_pull_response(
                &mut node_crds,
                &peer_pubkey,
                &timeouts,
                vec![peer_entry],
                node.msg_timeout + 1,
            )
            .0,
            0
        );

        // construct something that's not a contact info
        let peer_vote =
            CrdsValue::new_unsigned(CrdsData::Vote(0, Vote::new(&peer_pubkey, test_tx(), 0)));
        // check that older CrdsValues (non-ContactInfos) infos pass even if are too old,
        // but a recent contact info (inserted above) exists
        assert_eq!(
            node.process_pull_response(
                &mut node_crds,
                &peer_pubkey,
                &timeouts,
                vec![peer_vote.clone()],
                node.msg_timeout + 1,
            )
            .0,
            0
        );

        let mut node_crds = Crds::default();
        // without a contact info, inserting an old value should fail
        assert_eq!(
            node.process_pull_response(
                &mut node_crds,
                &peer_pubkey,
                &timeouts,
                vec![peer_vote],
                node.msg_timeout + 1,
            )
            .0,
            1
        );
    }
}
