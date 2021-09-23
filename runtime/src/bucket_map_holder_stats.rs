use crate::accounts_index::IndexValue;
use crate::bucket_map_holder::BucketMapHolder;
use solana_sdk::timing::{timestamp, AtomicInterval};
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    pub get_mem_us: AtomicU64,
    pub gets_from_mem: AtomicU64,
    pub get_missing_us: AtomicU64,
    pub gets_missing: AtomicU64,
    pub entry_mem_us: AtomicU64,
    pub entries_from_mem: AtomicU64,
    pub entry_missing_us: AtomicU64,
    pub entries_missing: AtomicU64,
    pub load_disk_found_count: AtomicU64,
    pub load_disk_found_us: AtomicU64,
    pub load_disk_missing_count: AtomicU64,
    pub load_disk_missing_us: AtomicU64,
    pub updates_in_mem: AtomicU64,
    pub items: AtomicU64,
    pub keys: AtomicU64,
    pub deletes: AtomicU64,
    pub inserts: AtomicU64,
    pub count_in_mem: AtomicU64,
    pub per_bucket_count: Vec<AtomicU64>,
    pub flush_entries_updated_on_disk: AtomicU64,
    pub active_threads: AtomicU64,
    pub get_range_us: AtomicU64,
    last_age: AtomicU8,
    last_age_time: AtomicU64,
    pub flush_scan_us: AtomicU64,
    pub flush_update_us: AtomicU64,
    pub flush_remove_us: AtomicU64,
    last_time: AtomicInterval,
}

impl BucketMapHolderStats {
    pub fn new(bins: usize) -> BucketMapHolderStats {
        BucketMapHolderStats {
            per_bucket_count: (0..bins)
                .into_iter()
                .map(|_| AtomicU64::default())
                .collect(),
            ..BucketMapHolderStats::default()
        }
    }

    pub fn insert_or_delete(&self, insert: bool, bin: usize) {
        let per_bucket = self.per_bucket_count.get(bin);
        if insert {
            self.inserts.fetch_add(1, Ordering::Relaxed);
            self.count_in_mem.fetch_add(1, Ordering::Relaxed);
            per_bucket.map(|count| count.fetch_add(1, Ordering::Relaxed));
        } else {
            self.deletes.fetch_add(1, Ordering::Relaxed);
            self.count_in_mem.fetch_sub(1, Ordering::Relaxed);
            per_bucket.map(|count| count.fetch_sub(1, Ordering::Relaxed));
        }
    }

    pub fn get_elapsed_ms_and_reset(&self) -> u64 {
        let now = timestamp();
        let last = self.last_age_time.swap(now, Ordering::Relaxed);
        now.saturating_sub(last) // could saturate to 0. That is ok.
    }

    fn ms_per_age<T: IndexValue>(&self, storage: &BucketMapHolder<T>) -> u64 {
        let elapsed_ms = self.get_elapsed_ms_and_reset();
        let mut age_now = storage.current_age();
        let last_age = self.last_age.swap(age_now, Ordering::Relaxed);
        if last_age > age_now {
            // age may have wrapped
            age_now += u8::MAX;
        }
        let age_delta = age_now.saturating_sub(last_age) as u64;
        if age_delta > 0 {
            elapsed_ms / age_delta
        } else {
            0
        }
    }

    pub fn report_stats<T: IndexValue>(&self, storage: &BucketMapHolder<T>) {
        // account index stats every 10 s
        if !self.last_time.should_update(10_000) {
            return;
        }

        let ms_per_age = self.ms_per_age(storage);

        let mut ct = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        for d in &self.per_bucket_count {
            let d = d.load(Ordering::Relaxed) as usize;
            ct += d;
            min = std::cmp::min(min, d);
            max = std::cmp::max(max, d);
        }

        let disk = storage.disk.as_ref();

        datapoint_info!(
            "accounts_index",
            (
                "count_in_mem",
                self.count_in_mem.load(Ordering::Relaxed),
                i64
            ),
            ("min_in_bin", min, i64),
            ("max_in_bin", max, i64),
            ("count_from_bins", ct, i64),
            (
                "gets_from_mem",
                self.gets_from_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_mem_us",
                self.get_mem_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "gets_missing",
                self.gets_missing.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_missing_us",
                self.get_missing_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "entries_from_mem",
                self.entries_from_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entry_mem_us",
                self.entry_mem_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "load_disk_found_count",
                self.load_disk_found_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_disk_found_us",
                self.load_disk_found_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "load_disk_missing_count",
                self.load_disk_missing_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_disk_missing_us",
                self.load_disk_missing_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "entries_missing",
                self.entries_missing.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entry_missing_us",
                self.entry_missing_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "updates_in_mem",
                self.updates_in_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_range_us",
                self.get_range_us.swap(0, Ordering::Relaxed),
                i64
            ),
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            (
                "active_threads",
                self.active_threads.load(Ordering::Relaxed),
                i64
            ),
            ("items", self.items.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
            ("ms_per_age", ms_per_age, i64),
            (
                "flush_scan_us",
                self.flush_scan_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_update_us",
                self.flush_update_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_remove_us",
                self.flush_remove_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "disk_index_resizes",
                disk.map(|disk| disk.stats.index.resizes.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_index_max_size",
                disk.map(|disk| {
                    let mut lock = disk.stats.index.max_size.lock().unwrap();
                    let value = *lock;
                    *lock = 0;
                    value
                })
                .unwrap_or_default(),
                i64
            ),
            (
                "disk_index_new_file_us",
                disk.map(|disk| disk.stats.index.new_file_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_index_resize_us",
                disk.map(|disk| disk.stats.index.resize_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_index_flush_file_us",
                disk.map(|disk| disk.stats.index.flush_file_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_index_flush_mmap_us",
                disk.map(|disk| disk.stats.index.mmap_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_data_resizes",
                disk.map(|disk| disk.stats.data.resizes.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_data_max_size",
                disk.map(|disk| {
                    let mut lock = disk.stats.data.max_size.lock().unwrap();
                    let value = *lock;
                    *lock = 0;
                    value
                })
                .unwrap_or_default(),
                i64
            ),
            (
                "disk_data_new_file_us",
                disk.map(|disk| disk.stats.data.new_file_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_data_resize_us",
                disk.map(|disk| disk.stats.data.resize_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_data_flush_file_us",
                disk.map(|disk| disk.stats.data.flush_file_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "disk_data_flush_mmap_us",
                disk.map(|disk| disk.stats.data.mmap_us.swap(0, Ordering::Relaxed))
                    .unwrap_or_default(),
                i64
            ),
            (
                "flush_entries_updated_on_disk",
                self.flush_entries_updated_on_disk
                    .swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}
