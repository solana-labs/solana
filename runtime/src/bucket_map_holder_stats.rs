use crate::accounts_index::IndexValue;
use crate::bucket_map_holder::BucketMapHolder;
use solana_sdk::timing::{timestamp, AtomicInterval};
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

// stats logged every 10 s
const STATS_INTERVAL_MS: u64 = 10_000;

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    pub held_in_mem_slot_list_len: AtomicU64,
    pub held_in_mem_slot_list_cached: AtomicU64,
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
    count: AtomicU64,
    pub bg_waiting_us: AtomicU64,
    pub bg_throttling_wait_us: AtomicU64,
    pub count_in_mem: AtomicU64,
    pub per_bucket_count: Vec<AtomicU64>,
    pub flush_entries_updated_on_disk: AtomicU64,
    pub flush_entries_removed_from_mem: AtomicU64,
    pub active_threads: AtomicU64,
    pub get_range_us: AtomicU64,
    last_age: AtomicU8,
    last_age_time: AtomicU64,
    pub flush_scan_update_us: AtomicU64,
    pub flush_remove_us: AtomicU64,
    pub flush_grow_us: AtomicU64,
    last_was_startup: AtomicBool,
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

    pub fn insert_or_delete(&self, insert: bool, _bin: usize) {
        if insert {
            self.inserts.fetch_add(1, Ordering::Relaxed);
            self.count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.deletes.fetch_add(1, Ordering::Relaxed);
            self.count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn insert_or_delete_mem(&self, insert: bool, bin: usize) {
        self.insert_or_delete_mem_count(insert, bin, 1)
    }

    pub fn insert_or_delete_mem_count(&self, insert: bool, bin: usize, count: u64) {
        let per_bucket = self.per_bucket_count.get(bin);
        if insert {
            self.count_in_mem.fetch_add(count, Ordering::Relaxed);
            per_bucket.map(|stat| stat.fetch_add(count, Ordering::Relaxed));
        } else {
            self.count_in_mem.fetch_sub(count, Ordering::Relaxed);
            per_bucket.map(|stat| stat.fetch_sub(count, Ordering::Relaxed));
        }
    }

    pub fn get_elapsed_ms_and_reset(&self) -> u64 {
        let now = timestamp();
        let last = self.last_age_time.swap(now, Ordering::Relaxed);
        now.saturating_sub(last) // could saturate to 0. That is ok.
    }

    fn ms_per_age<T: IndexValue>(&self, storage: &BucketMapHolder<T>, elapsed_ms: u64) -> u64 {
        if !storage.get_startup() {
            let age_now = storage.current_age();
            let last_age = self.last_age.swap(age_now, Ordering::Relaxed) as u64;
            let mut age_now = age_now as u64;
            if last_age > age_now {
                // age wrapped
                age_now += u8::MAX as u64 + 1;
            }
            let age_delta = age_now.saturating_sub(last_age) as u64;
            if age_delta > 0 {
                return elapsed_ms / age_delta;
            }
        }
        0 // avoid crazy numbers
    }

    pub fn remaining_until_next_interval(&self) -> u64 {
        self.last_time
            .remaining_until_next_interval(STATS_INTERVAL_MS)
    }

    /// return min, max, sum, median of data
    fn get_stats(mut data: Vec<u64>) -> (u64, u64, u64, u64) {
        if data.is_empty() {
            (0, 0, 0, 0)
        } else {
            data.sort_unstable();
            (
                *data.first().unwrap(),
                *data.last().unwrap(),
                data.iter().sum(),
                data[data.len() / 2],
            )
        }
    }

    fn calc_percent(&self, ms: u64, elapsed_ms: u64) -> f32 {
        if elapsed_ms == 0 {
            0.0
        } else {
            ms as f32 / elapsed_ms as f32
        }
    }

    pub fn report_stats<T: IndexValue>(&self, storage: &BucketMapHolder<T>) {
        if !self.last_time.should_update(STATS_INTERVAL_MS) {
            return;
        }

        let elapsed_ms = self.get_elapsed_ms_and_reset();

        let ms_per_age = self.ms_per_age(storage, elapsed_ms);

        let in_mem_per_bucket_counts = self
            .per_bucket_count
            .iter()
            .map(|count| count.load(Ordering::Relaxed))
            .collect::<Vec<_>>();
        let disk = storage.disk.as_ref();
        let disk_per_bucket_counts = disk
            .map(|disk| {
                disk.stats
                    .per_bucket_count
                    .iter()
                    .map(|count| count.load(Ordering::Relaxed))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let in_mem_stats = Self::get_stats(in_mem_per_bucket_counts);
        let disk_stats = Self::get_stats(disk_per_bucket_counts);

        const US_PER_MS: u64 = 1_000;

        // all metrics during startup are written to a different data point
        let startup = storage.get_startup();
        let was_startup = self.last_was_startup.swap(startup, Ordering::Relaxed);

        // sum of elapsed time in each thread
        let mut thread_time_elapsed_ms = elapsed_ms * storage.threads as u64;
        datapoint_info!(
            if startup || was_startup {
                thread_time_elapsed_ms *= 2; // more threads are allocated during startup
                "accounts_index_startup"
            } else {
                "accounts_index"
            },
            (
                "count_in_mem",
                self.count_in_mem.load(Ordering::Relaxed),
                i64
            ),
            ("count", self.count.load(Ordering::Relaxed), i64),
            (
                "bg_waiting_percent",
                self.calc_percent(
                    self.bg_waiting_us.swap(0, Ordering::Relaxed) / US_PER_MS,
                    thread_time_elapsed_ms
                ),
                f64
            ),
            (
                "bg_throttling_wait_percent",
                self.calc_percent(
                    self.bg_throttling_wait_us.swap(0, Ordering::Relaxed) / US_PER_MS,
                    thread_time_elapsed_ms
                ),
                f64
            ),
            (
                "held_in_mem_slot_list_len",
                self.held_in_mem_slot_list_len.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "held_in_mem_slot_list_cached",
                self.held_in_mem_slot_list_cached.swap(0, Ordering::Relaxed),
                i64
            ),
            ("min_in_bin_mem", in_mem_stats.0, i64),
            ("max_in_bin_mem", in_mem_stats.1, i64),
            ("count_from_bins_mem", in_mem_stats.2, i64),
            ("median_from_bins_mem", in_mem_stats.3, i64),
            ("min_in_bin_disk", disk_stats.0, i64),
            ("max_in_bin_disk", disk_stats.1, i64),
            ("count_from_bins_disk", disk_stats.2, i64),
            ("median_from_bins_disk", disk_stats.3, i64),
            (
                "gets_from_mem",
                self.gets_from_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_mem_us",
                self.get_mem_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "gets_missing",
                self.gets_missing.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_missing_us",
                self.get_missing_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entries_from_mem",
                self.entries_from_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entry_mem_us",
                self.entry_mem_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_disk_found_count",
                self.load_disk_found_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_disk_found_us",
                self.load_disk_found_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_disk_missing_count",
                self.load_disk_missing_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_disk_missing_us",
                self.load_disk_missing_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entries_missing",
                self.entries_missing.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entry_missing_us",
                self.entry_missing_us.swap(0, Ordering::Relaxed),
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
                "flush_scan_update_us",
                self.flush_scan_update_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_grow_us",
                self.flush_remove_us.swap(0, Ordering::Relaxed),
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
            (
                "flush_entries_removed_from_mem",
                self.flush_entries_removed_from_mem
                    .swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}
