use {
    crate::blockstore_meta::SlotMeta,
    lru::LruCache,
    solana_sdk::clock::Slot,
    std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    },
};

const SLOTS_STATS_CACHE_CAPACITY: usize = 1_000;

#[derive(Copy, Clone, Debug)]
pub(crate) enum ShredSource {
    Turbine,
    Repaired,
    Recovered,
}

#[derive(Clone, Default)]
pub struct SlotStats {
    turbine_fec_set_index_counts: HashMap</*fec_set_index*/ u32, /*count*/ usize>,
    num_repaired: usize,
    num_recovered: usize,
    last_index: u64,
    is_full: bool,
}

impl SlotStats {
    pub fn get_min_index_count(&self) -> usize {
        self.turbine_fec_set_index_counts
            .iter()
            .map(|(_, cnt)| *cnt)
            .min()
            .unwrap_or(0)
    }

    fn report_complete(&self, slot: Slot, reason: SlotStatsReportingReason) {
        let min_fec_set_count = self.get_min_index_count();
        datapoint_info!(
            "slot_stats_tracking_complete",
            ("slot", slot, i64),
            ("last_index", self.last_index, i64),
            ("num_repaired", self.num_repaired, i64),
            ("num_recovered", self.num_recovered, i64),
            ("min_turbine_fec_set_count", min_fec_set_count, i64),
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
        let evicted = if stats.len() == stats.cap() {
            match stats.peek_lru() {
                Some((s, _)) if *s == slot => None,
                _ => stats.pop_lru(),
            }
        } else {
            None
        };
        stats.get_or_insert(slot, SlotStats::default);
        (stats.get_mut(&slot).unwrap(), evicted)
    }

    fn prune<'a>(
        stats: &'a mut MutexGuard<LruCache<Slot, SlotStats>>,
        slot: &Slot,
    ) -> Vec<(Slot, SlotStats)> {
        let prune_slots: Vec<_> = stats
            .iter()
            .filter_map(|(s, _)| if *s <= *slot { Some(*s) } else { None })
            .collect();
        let mut pruned_entries = Vec::default();
        for s in prune_slots.iter() {
            if let Some(stats) = stats.pop(s) {
                pruned_entries.push((*s, stats));
            }
        }
        pruned_entries
    }

    pub(crate) fn record_shred(
        &self,
        slot: Slot,
        fec_set_index: u32,
        source: ShredSource,
        slot_meta: Option<&SlotMeta>,
    ) {
        let mut slot_full_reporting_info = None;
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
        if let Some(meta) = slot_meta {
            if meta.is_full() {
                slot_stats.last_index = meta.last_index.unwrap_or_default();
                if !slot_stats.is_full {
                    slot_stats.is_full = true;
                    slot_full_reporting_info =
                        Some((slot_stats.num_repaired, slot_stats.num_recovered));
                }
            }
        }
        drop(stats);
        if let Some((num_repaired, num_recovered)) = slot_full_reporting_info {
            let slot_meta = slot_meta.unwrap();
            let total_time_ms =
                solana_sdk::timing::timestamp().saturating_sub(slot_meta.first_shred_timestamp);
            let last_index = slot_meta
                .last_index
                .and_then(|ix| i64::try_from(ix).ok())
                .unwrap_or(-1);
            datapoint_info!(
                "shred_insert_is_full",
                ("total_time_ms", total_time_ms, i64),
                ("slot", slot, i64),
                ("last_index", last_index, i64),
                ("num_repaired", num_repaired, i64),
                ("num_recovered", num_recovered, i64),
            );
        }
        if let Some((evicted_slot, evicted_stats)) = evicted {
            evicted_stats.report_complete(evicted_slot, SlotStatsReportingReason::Evicted);
        }
    }

    pub fn remove(&self, slot: &Slot, reason: SlotStatsReportingReason) {
        let (removed_entry, pruned_entries) = {
            let mut stats = self.stats.lock().unwrap();
            let removed_entry = stats.pop(slot);
            let pruned_entries = if reason == SlotStatsReportingReason::Rooted {
                Self::prune(&mut stats, slot)
            } else {
                Vec::default()
            };
            (removed_entry, pruned_entries)
        };
        if let Some(slot_stats) = removed_entry {
            slot_stats.report_complete(*slot, reason);
        }
        for (pruned_slot, pruned_stats) in pruned_entries {
            pruned_stats.report_complete(pruned_slot, SlotStatsReportingReason::Pruned);
        }
    }
}
