use {
    crate::blockstore_meta::SlotMeta,
    bitflags::bitflags,
    lru::LruCache,
    solana_sdk::clock::Slot,
    std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    },
};

const SLOTS_STATS_CACHE_CAPACITY: usize = 300;

#[derive(Copy, Clone, Debug)]
pub(crate) enum ShredSource {
    Turbine,
    Repaired,
    Recovered,
}

bitflags! {
    #[derive(Copy, Clone, Default)]
    struct SlotFlags: u8 {
        const DEAD   = 0b00000001;
        const FULL   = 0b00000010;
        const ROOTED = 0b00000100;
    }
}

#[derive(Clone, Default)]
pub struct SlotStats {
    turbine_fec_set_index_counts: HashMap</*fec_set_index*/ u32, /*count*/ usize>,
    num_repaired: usize,
    num_recovered: usize,
    last_index: u64,
    flags: SlotFlags,
}

impl SlotStats {
    pub fn get_min_index_count(&self) -> usize {
        self.turbine_fec_set_index_counts
            .values()
            .min()
            .copied()
            .unwrap_or_default()
    }

    fn report(&self, slot: Slot) {
        let min_fec_set_count = self.get_min_index_count();
        datapoint_info!(
            "slot_stats_tracking_complete",
            ("slot", slot, i64),
            ("last_index", self.last_index, i64),
            ("num_repaired", self.num_repaired, i64),
            ("num_recovered", self.num_recovered, i64),
            ("min_turbine_fec_set_count", min_fec_set_count, i64),
            ("is_full", self.flags.contains(SlotFlags::FULL), bool),
            ("is_rooted", self.flags.contains(SlotFlags::ROOTED), bool),
            ("is_dead", self.flags.contains(SlotFlags::DEAD), bool),
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

    pub(crate) fn record_shred(
        &self,
        slot: Slot,
        fec_set_index: u32,
        source: ShredSource,
        slot_meta: Option<&SlotMeta>,
    ) {
        let mut slot_full_reporting_info = None;
        let mut stats = self.stats.lock().unwrap();
        let (slot_stats, evicted) = Self::get_or_default_with_eviction_check(&mut stats, slot);
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
                if !slot_stats.flags.contains(SlotFlags::FULL) {
                    slot_stats.flags |= SlotFlags::FULL;
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
            evicted_stats.report(evicted_slot);
        }
    }

    fn add_flag(&self, slot: Slot, flag: SlotFlags) {
        let evicted = {
            let mut stats = self.stats.lock().unwrap();
            let (slot_stats, evicted) = Self::get_or_default_with_eviction_check(&mut stats, slot);
            slot_stats.flags |= flag;
            evicted
        };
        if let Some((evicted_slot, evicted_stats)) = evicted {
            evicted_stats.report(evicted_slot);
        }
    }

    pub fn mark_dead(&self, slot: Slot) {
        self.add_flag(slot, SlotFlags::DEAD);
    }

    pub fn mark_rooted(&self, slot: Slot) {
        self.add_flag(slot, SlotFlags::ROOTED);
    }
}
