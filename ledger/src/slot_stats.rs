use {
    solana_sdk::clock::Slot,
    std::{
        collections::{BTreeMap, HashMap},
        time::Instant,
    }
};

pub struct SlotsStats {
    pub last_cleanup_ts: Instant,
    pub stats: BTreeMap<Slot, SlotStats>,
}

impl Default for SlotsStats {
    fn default() -> Self {
        SlotsStats {
            last_cleanup_ts: Instant::now(),
            stats: BTreeMap::new(),
        }
    }
}

#[derive(Default)]
pub struct SlotStats {
    pub num_repaired: usize,
    pub num_recovered: usize,
}

#[derive(Default)]
pub struct TurbineFecSetStats(HashMap<Slot, HashMap</*fec_set_index*/ u32, /*count*/ usize>>);

impl TurbineFecSetStats {
    pub fn inc_index_count(&mut self, slot: Slot, fec_set_index: u32) {
        *self
            .0
            .entry(slot)
            .or_default()
            .entry(fec_set_index)
            .or_default() += 1;
    }

    pub fn get_min_index_count(&self, slot: &Slot) -> (usize, usize) {
        let fec_index_counts = match self.0.get(slot) {
            Some(counts) => counts,
            None => return (0, 0),
        };
        let last_idx = match fec_index_counts.iter().map(|(idx, _)| *idx).max() {
            Some(last) => last,
            None => return (0, 0),
        };
        let min_fec_set_cnt = fec_index_counts
            .iter()
            .filter(|(idx, _)| **idx != last_idx)
            .map(|(_, cnt)| *cnt)
            .min()
            .unwrap_or(0);
        let last_fec_set_cnt = *fec_index_counts.get(&last_idx).unwrap_or(&0);

        (min_fec_set_cnt, last_fec_set_cnt)
    }

    pub fn remove(&mut self, slot: &Slot) -> bool {
        self.0.remove(slot).is_some()
    }

    pub fn prune(&mut self, slot: &Slot) {
        for (s, _) in self.0.iter() {
            if *s <= *slot {
                info!("pruning fec set count data slot={}", s);
            }
        }
        self.0.retain(|s, _| *s > *slot);
    }
}
