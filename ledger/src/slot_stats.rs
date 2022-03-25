use {
    crate::blockstore_meta::SlotMeta, bitflags::bitflags, lru::LruCache, solana_sdk::clock::Slot,
};

const SLOTS_STATS_CACHE_CAPACITY: usize = 300;

macro_rules! get_mut_entry (
    ($cache:expr, $key:expr) => (
        match $cache.get_mut(&$key) {
            Some(entry) => entry,
            None => {
                $cache.put($key, SlotStats::default());
                $cache.get_mut(&$key).unwrap()
            }
        }
    );
);

#[derive(Copy, Clone, Debug)]
pub(crate) enum ShredSource {
    Turbine,
    Repaired,
    Recovered,
}

bitflags! {
    #[derive(Default)]
    struct SlotFlags: u8 {
        const DEAD   = 0b00000001;
        const FULL   = 0b00000010;
        const ROOTED = 0b00000100;
    }
}

#[derive(Default)]
struct SlotStats {
    flags: SlotFlags,
    num_repaired: usize,
    num_recovered: usize,
}

pub(crate) struct SlotsStats(LruCache<Slot, SlotStats>);

impl Default for SlotsStats {
    fn default() -> Self {
        // LruCache::unbounded because capacity is enforced manually.
        Self(LruCache::unbounded())
    }
}

impl SlotsStats {
    pub(crate) fn add_shred(&mut self, slot: Slot, source: ShredSource) {
        let entry = get_mut_entry!(self.0, slot);
        match source {
            ShredSource::Turbine => (),
            ShredSource::Repaired => entry.num_repaired += 1,
            ShredSource::Recovered => entry.num_recovered += 1,
        }
        self.maybe_evict_cache();
    }

    pub(crate) fn set_full(&mut self, slot_meta: &SlotMeta) {
        let total_time_ms =
            solana_sdk::timing::timestamp().saturating_sub(slot_meta.first_shred_timestamp);
        let last_index = slot_meta
            .last_index
            .and_then(|ix| i64::try_from(ix).ok())
            .unwrap_or(-1);
        let entry = get_mut_entry!(self.0, slot_meta.slot);
        if !entry.flags.contains(SlotFlags::FULL) {
            datapoint_info!(
                "shred_insert_is_full",
                ("total_time_ms", total_time_ms, i64),
                ("slot", slot_meta.slot, i64),
                ("last_index", last_index, i64),
                ("num_repaired", entry.num_repaired, i64),
                ("num_recovered", entry.num_recovered, i64),
            );
        }
        entry.flags |= SlotFlags::FULL;
        self.maybe_evict_cache();
    }

    fn maybe_evict_cache(&mut self) {
        while self.0.len() > SLOTS_STATS_CACHE_CAPACITY {
            let (_slot, _entry) = self.0.pop_lru().unwrap();
            // TODO: submit metrics for (slot, entry).
        }
    }
}
