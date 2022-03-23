use {
    crate::{blockstore::ShredSource, shred::MAX_DATA_SHREDS_PER_FEC_BLOCK},
    lru::LruCache,
    solana_sdk::clock::Slot,
    std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    },
};

const SLOTS_STATS_CACHE_CAPACITY: usize = 1024;

#[derive(Clone, Default)]
pub struct SlotStats {
    pub num_repaired: usize,
    pub num_recovered: usize,
    turbine_fec_set_index_counts: HashMap</*fec_set_index*/ u32, /*count*/ usize>,
    pub last_index: u64,
    pub is_full: bool,
}

impl SlotStats {
    pub fn get_min_index_count(&self) -> (usize, usize) {
        let last_idx = match self
            .turbine_fec_set_index_counts
            .iter()
            .map(|(idx, _)| *idx)
            .max()
        {
            Some(last) => last,
            None => return (0, 0),
        };
        let min_fec_set_cnt = self
            .turbine_fec_set_index_counts
            .iter()
            .filter(|(idx, _)| **idx != last_idx)
            .map(|(_, cnt)| *cnt)
            .min()
            .unwrap_or(0);
        let last_fec_set_cnt = *self
            .turbine_fec_set_index_counts
            .get(&last_idx)
            .unwrap_or(&0);

        (min_fec_set_cnt, last_fec_set_cnt)
    }

    fn report_complete(&self, slot: Slot, reason: SlotStatsReportingReason) {
        let (min_fec_set_count, last_fec_set_count) = self.get_min_index_count();
        let last_fec_set_size = self.last_index % MAX_DATA_SHREDS_PER_FEC_BLOCK as u64;
        datapoint_info!(
            "slot_stats_tracking_complete",
            ("slot", slot, i64),
            ("last_index", self.last_index, i64),
            ("num_repaired", self.num_repaired, i64),
            ("num_recovered", self.num_recovered, i64),
            ("min_turbine_fec_set_count", min_fec_set_count, i64),
            ("last_turbine_fec_set_count", last_fec_set_count, i64),
            ("last_turbine_fec_set_size", last_fec_set_size, i64),
            ("is_full", self.is_full, bool),
            (
                "is_rooted",
                reason == SlotStatsReportingReason::Rooted,
                bool
            ),
            ("is_dead", reason == SlotStatsReportingReason::Dead, bool),
            (
                "is_evicted",
                reason == SlotStatsReportingReason::Evicted,
                bool
            ),
            (
                "is_pruned",
                reason == SlotStatsReportingReason::Pruned,
                bool
            ),
        );
    }
}

pub struct SlotsStats {
    pub stats: Mutex<LruCache<Slot, SlotStats>>,
}

impl Default for SlotsStats {
    fn default() -> Self {
        Self {
            stats: Mutex::new(LruCache::new(SLOTS_STATS_CACHE_CAPACITY)),
        }
    }
}

#[derive(PartialEq)]
pub enum SlotStatsReportingReason {
    Rooted,
    Dead,
    Evicted,
    Pruned,
}

impl SlotsStats {
    fn get_or_default_with_eviction_check<'a>(
        stats: &'a mut MutexGuard<LruCache<Slot, SlotStats>>,
        slot: Slot,
    ) -> (&'a mut SlotStats, Option<(Slot, SlotStats)>) {
        let mut evicted = None;
        if stats.len() == stats.cap() {
            match stats.peek_lru() {
                Some((s, _)) if *s == slot => (),
                _ => {
                    evicted = Some(stats.pop_lru().expect("cache capacity should not be 0"));
                }
            }
        }
        stats.get_or_insert(slot, SlotStats::default);
        (stats.get_mut(&slot).unwrap(), evicted)
    }

    pub fn inc_index_count(&self, slot: Slot, fec_set_index: u32, source: ShredSource) {
        let mut stats = self.stats.lock().unwrap();
        let (mut slot_stats, evicted) = Self::get_or_default_with_eviction_check(&mut stats, slot);
        match source {
            ShredSource::Recovered => slot_stats.num_recovered += 1,
            ShredSource::Repaired => slot_stats.num_repaired += 1,
            ShredSource::Turbine => {
                *slot_stats
                    .turbine_fec_set_index_counts
                    .entry(fec_set_index)
                    .or_default() += 1
            }
        }
        drop(stats);
        if let Some((evicted_slot, evicted_stats)) = evicted {
            evicted_stats.report_complete(evicted_slot, SlotStatsReportingReason::Evicted);
        }
    }

    pub fn set_slot_opts(&self, slot: Slot, is_full: bool, last_index: u64) {
        let mut stats = self.stats.lock().unwrap();
        let (mut slot_stats, evicted) = Self::get_or_default_with_eviction_check(&mut stats, slot);
        slot_stats.is_full = is_full;
        slot_stats.last_index = last_index;
        drop(stats);
        if let Some((evicted_slot, evicted_stats)) = evicted {
            evicted_stats.report_complete(evicted_slot, SlotStatsReportingReason::Evicted);
        }
    }

    pub fn get_min_index_count(&self, slot: &Slot) -> (usize, usize) {
        self.stats
            .lock()
            .unwrap()
            .get(slot)
            .map(|stats| stats.get_min_index_count())
            .unwrap_or((0, 0))
    }

    pub fn get_clone(&self, slot: Slot) -> Option<SlotStats> {
        self.stats.lock().unwrap().get(&slot).cloned()
    }

    pub fn remove(&self, slot: &Slot, reason: SlotStatsReportingReason) {
        let slot_stats = self.stats.lock().unwrap().pop(slot);
        if let Some(slot_stats) = slot_stats {
            slot_stats.report_complete(*slot, reason);
        }
    }

    pub fn prune(&self, slot: &Slot) {
        let mut pruned_stats = Vec::default();
        {
            let mut prune_list = Vec::default();
            let mut stats = self.stats.lock().unwrap();
            for (s, _) in stats.iter() {
                if *s <= *slot {
                    prune_list.push(*s);
                }
            }
            for s in prune_list.iter() {
                if let Some(stats) = stats.pop(s) {
                    pruned_stats.push((*s, stats));
                }
            }
        }
        for (s, stats) in pruned_stats.iter() {
            stats.report_complete(*s, SlotStatsReportingReason::Pruned);
        }
    }
}
