use {
    crate::{banking_stage::BankingStageStats, packet_hasher::PacketHasher},
    lru::LruCache,
    solana_measure::measure::Measure,
    solana_perf::packet::PacketBatch,
    std::{
        ops::DerefMut,
        sync::{atomic::Ordering, Arc, Mutex},
    },
};

const DEFAULT_LRU_SIZE: usize = 200_000;

#[derive(Clone)]
pub struct PacketDeduper(Arc<Mutex<(LruCache<u64, ()>, PacketHasher)>>);

impl Default for PacketDeduper {
    fn default() -> Self {
        Self(Arc::new(Mutex::new((
            LruCache::new(DEFAULT_LRU_SIZE),
            PacketHasher::default(),
        ))))
    }
}

impl PacketDeduper {
    pub fn dedupe_packets(
        &self,
        packet_batch: &PacketBatch,
        packet_indexes: &mut Vec<usize>,
        banking_stage_stats: &BankingStageStats,
    ) {
        let original_packets_count = packet_indexes.len();
        let mut packet_duplicate_check_time = Measure::start("packet_duplicate_check");
        let mut duplicates = self.0.lock().unwrap();
        let (cache, hasher) = duplicates.deref_mut();
        packet_indexes.retain(|i| {
            let packet_hash = hasher.hash_packet(&packet_batch.packets[*i]);
            match cache.get_mut(&packet_hash) {
                Some(_hash) => false,
                None => {
                    cache.put(packet_hash, ());
                    true
                }
            }
        });
        packet_duplicate_check_time.stop();
        banking_stage_stats
            .packet_duplicate_check_elapsed
            .fetch_add(packet_duplicate_check_time.as_us(), Ordering::Relaxed);
        banking_stage_stats
            .dropped_duplicated_packets_count
            .fetch_add(
                original_packets_count.saturating_sub(packet_indexes.len()),
                Ordering::Relaxed,
            );
    }

    pub fn reset(&self) {
        let mut duplicates = self.0.lock().unwrap();
        duplicates.0.clear();
    }
}
