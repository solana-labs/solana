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
use crate::crds::Crds;
use crate::crds_gossip::{get_stake, get_weight, CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS};
use crate::crds_gossip_error::CrdsGossipError;
use crate::crds_value::{CrdsValue, CrdsValueLabel};
use rand;
use rand::distributions::{Distribution, WeightedIndex};
use rand::Rng;
use solana_runtime::bloom::Bloom;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::cmp;
use std::collections::HashMap;
use std::collections::VecDeque;

pub const CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS: u64 = 15000;
pub const FALSE_RATE: f64 = 0.1f64;

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct CrdsFilter {
    pub filter: Bloom<Hash>,
    mask: u64,
    mask_bits: u32,
}

impl CrdsFilter {
    pub fn new_rand(num_items: usize, max_bytes: usize) -> Self {
        let max_bits = (max_bytes * 8) as f64;
        let num_keys = Bloom::<Hash>::num_keys(max_bits, num_items as f64);
        let max_items = Self::max_items(max_bits, FALSE_RATE, num_keys);
        let mask_bits = Self::mask_bits(num_items as f64, max_items as f64);
        let keys = (0..num_keys as u64)
            .map(|_| rand::thread_rng().gen())
            .collect();
        let filter = Bloom::new(max_bits as usize, keys);
        let seed: u64 = rand::thread_rng().gen_range(0, 2u64.pow(mask_bits));
        let mask = Self::compute_mask(seed, mask_bits);
        CrdsFilter {
            filter,
            mask,
            mask_bits,
        }
    }
    // generates a vec of filters that together hold a complete set of Hashes
    pub fn new_complete_set(num_items: usize, max_bytes: usize) -> Vec<Self> {
        let max_bits = (max_bytes * 8) as f64;
        let num_keys = Bloom::<Hash>::num_keys(max_bits, num_items as f64);
        let max_items = Self::max_items(max_bits, FALSE_RATE, num_keys);
        let mask_bits = Self::mask_bits(num_items as f64, max_items as f64);
        // for each possible mask combination, generate a new filter.
        let mut filters = vec![];
        for seed in 0..2u64.pow(mask_bits) {
            let filter = Bloom::random(max_items as usize, FALSE_RATE, max_bits as usize);
            let mask = Self::compute_mask(seed, mask_bits);
            let filter = CrdsFilter {
                filter,
                mask,
                mask_bits,
            };
            filters.push(filter)
        }
        filters
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
    fn hash_as_u64(item: &Hash) -> u64 {
        let arr = item.as_ref();
        let mut accum = 0;
        for (i, val) in arr.iter().enumerate().take(8) {
            accum |= (u64::from(*val)) << (i * 8) as u64;
        }
        accum
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
}

#[derive(Clone)]
pub struct CrdsGossipPull {
    /// timestamp of last request
    pub pull_request_time: HashMap<Pubkey, u64>,
    /// hash and insert time
    purged_values: VecDeque<(Hash, u64)>,
    pub crds_timeout: u64,
}

impl Default for CrdsGossipPull {
    fn default() -> Self {
        Self {
            purged_values: VecDeque::new(),
            pull_request_time: HashMap::new(),
            crds_timeout: CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS,
        }
    }
}
impl CrdsGossipPull {
    /// generate a random request
    pub fn new_pull_request(
        &self,
        crds: &Crds,
        self_id: &Pubkey,
        now: u64,
        stakes: &HashMap<Pubkey, u64>,
        bloom_size: usize,
    ) -> Result<(Pubkey, Vec<CrdsFilter>, CrdsValue), CrdsGossipError> {
        let options = self.pull_options(crds, &self_id, now, stakes);
        if options.is_empty() {
            return Err(CrdsGossipError::NoPeers);
        }
        let filters = self.build_crds_filters(crds, bloom_size);
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
        now: u64,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Vec<(f32, &'a ContactInfo)> {
        crds.table
            .values()
            .filter_map(|v| v.value.contact_info())
            .filter(|v| v.id != *self_id && ContactInfo::is_valid_address(&v.gossip))
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

    /// process a pull request and create a response
    pub fn process_pull_requests(
        &mut self,
        crds: &mut Crds,
        requests: Vec<(CrdsValue, CrdsFilter)>,
        now: u64,
    ) -> Vec<Vec<CrdsValue>> {
        let rv = self.filter_crds_values(crds, &requests);
        requests.into_iter().for_each(|(caller, _)| {
            let key = caller.label().pubkey();
            let old = crds.insert(caller, now);
            if let Some(val) = old.ok().and_then(|opt| opt) {
                self.purged_values
                    .push_back((val.value_hash, val.local_timestamp));
            }
            crds.update_record_timestamp(&key, now);
        });
        rv
    }
    /// process a pull response
    pub fn process_pull_response(
        &mut self,
        crds: &mut Crds,
        from: &Pubkey,
        response: Vec<CrdsValue>,
        now: u64,
    ) -> usize {
        let mut failed = 0;
        for r in response {
            let owner = r.label().pubkey();
            let old = crds.insert(r, now);
            failed += old.is_err() as usize;
            old.ok().map(|opt| {
                crds.update_record_timestamp(&owner, now);
                opt.map(|val| {
                    self.purged_values
                        .push_back((val.value_hash, val.local_timestamp))
                })
            });
        }
        crds.update_record_timestamp(from, now);
        failed
    }
    // build a set of filters of the current crds table
    // num_filters - used to increase the likely hood of a value in crds being added to some filter
    pub fn build_crds_filters(&self, crds: &Crds, bloom_size: usize) -> Vec<CrdsFilter> {
        let num = cmp::max(
            CRDS_GOSSIP_DEFAULT_BLOOM_ITEMS,
            crds.table.values().count() + self.purged_values.len(),
        );
        let mut filters = CrdsFilter::new_complete_set(num, bloom_size);
        for v in crds.table.values() {
            filters
                .iter_mut()
                .for_each(|filter| filter.add(&v.value_hash));
        }
        for (value_hash, _insert_timestamp) in &self.purged_values {
            filters.iter_mut().for_each(|filter| filter.add(value_hash));
        }
        filters
    }
    /// filter values that fail the bloom filter up to max_bytes
    fn filter_crds_values(
        &self,
        crds: &Crds,
        filters: &[(CrdsValue, CrdsFilter)],
    ) -> Vec<Vec<CrdsValue>> {
        let mut ret = vec![vec![]; filters.len()];
        for v in crds.table.values() {
            filters.iter().enumerate().for_each(|(i, (_, filter))| {
                if !filter.contains(&v.value_hash) {
                    ret[i].push(v.value.clone());
                }
            });
        }
        ret
    }
    /// Purge values from the crds that are older then `active_timeout`
    /// The value_hash of an active item is put into self.purged_values queue
    pub fn purge_active(&mut self, crds: &mut Crds, self_id: &Pubkey, min_ts: u64) {
        let old = crds.find_old_labels(min_ts);
        let mut purged: VecDeque<_> = old
            .iter()
            .filter(|label| label.pubkey() != *self_id)
            .filter_map(|label| {
                let rv = crds
                    .lookup_versioned(label)
                    .map(|val| (val.value_hash, val.local_timestamp));
                crds.remove(label);
                rv
            })
            .collect();
        self.purged_values.append(&mut purged);
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
}
#[cfg(test)]
mod test {
    use super::*;
    use crate::contact_info::ContactInfo;
    use itertools::Itertools;
    use solana_sdk::hash::hash;
    use solana_sdk::packet::PACKET_DATA_SIZE;

    #[test]
    fn test_new_pull_with_stakes() {
        let mut crds = Crds::default();
        let mut stakes = HashMap::new();
        let node = CrdsGossipPull::default();
        let me = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        crds.insert(me.clone(), 0).unwrap();
        for i in 1..=30 {
            let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
            let id = entry.label().pubkey();
            crds.insert(entry.clone(), 0).unwrap();
            stakes.insert(id, i * 100);
        }
        let now = 1024;
        let mut options = node.pull_options(&crds, &me.label().pubkey(), now, &stakes);
        assert!(!options.is_empty());
        options.sort_by(|(weight_l, _), (weight_r, _)| weight_r.partial_cmp(weight_l).unwrap());
        // check that the highest stake holder is also the heaviest weighted.
        assert_eq!(
            *stakes.get(&options.get(0).unwrap().1.id).unwrap(),
            3000_u64
        );
    }

    #[test]
    fn test_new_pull_request() {
        let mut crds = Crds::default();
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        let id = entry.label().pubkey();
        let node = CrdsGossipPull::default();
        assert_eq!(
            node.new_pull_request(&crds, &id, 0, &HashMap::new(), PACKET_DATA_SIZE),
            Err(CrdsGossipError::NoPeers)
        );

        crds.insert(entry.clone(), 0).unwrap();
        assert_eq!(
            node.new_pull_request(&crds, &id, 0, &HashMap::new(), PACKET_DATA_SIZE),
            Err(CrdsGossipError::NoPeers)
        );

        let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        crds.insert(new.clone(), 0).unwrap();
        let req = node.new_pull_request(&crds, &id, 0, &HashMap::new(), PACKET_DATA_SIZE);
        let (to, _, self_info) = req.unwrap();
        assert_eq!(to, new.label().pubkey());
        assert_eq!(self_info, entry);
    }

    #[test]
    fn test_new_mark_creation_time() {
        let mut crds = Crds::default();
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        let node_pubkey = entry.label().pubkey();
        let mut node = CrdsGossipPull::default();
        crds.insert(entry.clone(), 0).unwrap();
        let old = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        crds.insert(old.clone(), 0).unwrap();
        let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        crds.insert(new.clone(), 0).unwrap();

        // set request creation time to max_value
        node.mark_pull_request_creation_time(&new.label().pubkey(), u64::max_value());

        // odds of getting the other request should be 1 in u64::max_value()
        for _ in 0..10 {
            let req = node.new_pull_request(
                &crds,
                &node_pubkey,
                u64::max_value(),
                &HashMap::new(),
                PACKET_DATA_SIZE,
            );
            let (to, _, self_info) = req.unwrap();
            assert_eq!(to, old.label().pubkey());
            assert_eq!(self_info, entry);
        }
    }

    #[test]
    fn test_process_pull_request() {
        let mut node_crds = Crds::default();
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        let node_pubkey = entry.label().pubkey();
        let node = CrdsGossipPull::default();
        node_crds.insert(entry.clone(), 0).unwrap();
        let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        node_crds.insert(new.clone(), 0).unwrap();
        let req = node.new_pull_request(
            &node_crds,
            &node_pubkey,
            0,
            &HashMap::new(),
            PACKET_DATA_SIZE,
        );

        let mut dest_crds = Crds::default();
        let mut dest = CrdsGossipPull::default();
        let (_, filters, caller) = req.unwrap();
        let filters = filters.into_iter().map(|f| (caller.clone(), f)).collect();
        let rsp = dest.process_pull_requests(&mut dest_crds, filters, 1);
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
        let mut node_crds = Crds::default();
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        let node_pubkey = entry.label().pubkey();
        let mut node = CrdsGossipPull::default();
        node_crds.insert(entry.clone(), 0).unwrap();

        let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        node_crds.insert(new.clone(), 0).unwrap();

        let mut dest = CrdsGossipPull::default();
        let mut dest_crds = Crds::default();
        let new_id = Pubkey::new_rand();
        let new = CrdsValue::ContactInfo(ContactInfo::new_localhost(&new_id, 1));
        dest_crds.insert(new.clone(), 0).unwrap();

        // node contains a key from the dest node, but at an older local timestamp
        let same_key = CrdsValue::ContactInfo(ContactInfo::new_localhost(&new_id, 0));
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
                &node_crds,
                &node_pubkey,
                0,
                &HashMap::new(),
                PACKET_DATA_SIZE,
            );
            let (_, filters, caller) = req.unwrap();
            let filters = filters.into_iter().map(|f| (caller.clone(), f)).collect();
            let mut rsp = dest.process_pull_requests(&mut dest_crds, filters, 0);
            // if there is a false positive this is empty
            // prob should be around 0.1 per iteration
            if rsp.is_empty() {
                continue;
            }

            if rsp.is_empty() {
                continue;
            }
            assert_eq!(rsp.len(), 1);
            let failed =
                node.process_pull_response(&mut node_crds, &node_pubkey, rsp.pop().unwrap(), 1);
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
        let mut node_crds = Crds::default();
        let entry = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        let node_label = entry.label();
        let node_pubkey = node_label.pubkey();
        let mut node = CrdsGossipPull::default();
        node_crds.insert(entry.clone(), 0).unwrap();
        let old = CrdsValue::ContactInfo(ContactInfo::new_localhost(&Pubkey::new_rand(), 0));
        node_crds.insert(old.clone(), 0).unwrap();
        let value_hash = node_crds.lookup_versioned(&old.label()).unwrap().value_hash;

        //verify self is valid
        assert_eq!(node_crds.lookup(&node_label).unwrap().label(), node_label);

        // purge
        node.purge_active(&mut node_crds, &node_pubkey, 1);

        //verify self is still valid after purge
        assert_eq!(node_crds.lookup(&node_label).unwrap().label(), node_label);

        assert_eq!(node_crds.lookup_versioned(&old.label()), None);
        assert_eq!(node.purged_values.len(), 1);
        for _ in 0..30 {
            // there is a chance of a false positive with bloom filters
            // assert that purged value is still in the set
            // chance of 30 consecutive false positives is 0.1^30
            let filters = node.build_crds_filters(&node_crds, PACKET_DATA_SIZE);
            assert!(filters.iter().any(|filter| filter.contains(&value_hash)));
        }

        // purge the value
        node.purge_purged(1);
        assert_eq!(node.purged_values.len(), 0);
    }
    #[test]
    fn test_crds_filter_mask() {
        let filter = CrdsFilter::new_rand(1, 128);
        assert_eq!(filter.mask, !0x0);
        assert_eq!(CrdsFilter::max_items(80f64, 0.01, 8f64), 9f64);
        //1000/9 = 111, so 7 bits are needed to mask it
        assert_eq!(CrdsFilter::mask_bits(1000f64, 9f64), 7u32);
        let filter = CrdsFilter::new_rand(1000, 10);
        assert_eq!(filter.mask & 0x00ffffffff, 0x00ffffffff);
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
        let mut filters = CrdsFilter::new_complete_set(1000, 10);
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
            .into_iter()
            .map(|seed| CrdsFilter::compute_mask(seed, mask_bits))
            .dedup()
            .collect();
        assert_eq!(masks.len(), 2u64.pow(mask_bits) as usize)
    }
}
