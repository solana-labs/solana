//! Cached data for hashing accounts
#[derive(Default, Debug)]
pub struct CacheHashDataStats {
    pub cache_file_size: usize,
    pub cache_file_count: usize,
    pub total_entries: usize,
    pub loaded_from_cache: usize,
    pub entries_loaded_from_cache: usize,
    pub save_us: u64,
    pub saved_to_cache: usize,
    pub write_to_mmap_us: u64,
    pub create_save_us: u64,
    pub load_us: u64,
    pub read_us: u64,
    pub decode_us: u64,
    pub merge_us: u64,
    pub unused_cache_files: usize,
}

impl CacheHashDataStats {
    pub fn accumulate(&mut self, other: &CacheHashDataStats) {
        self.cache_file_size += other.cache_file_size;
        self.total_entries += other.total_entries;
        self.loaded_from_cache += other.loaded_from_cache;
        self.entries_loaded_from_cache += other.entries_loaded_from_cache;
        self.load_us += other.load_us;
        self.read_us += other.read_us;
        self.decode_us += other.decode_us;
        self.save_us += other.save_us;
        self.saved_to_cache += other.saved_to_cache;
        self.create_save_us += other.create_save_us;
        self.cache_file_count += other.cache_file_count;
        self.write_to_mmap_us += other.write_to_mmap_us;
        self.unused_cache_files += other.unused_cache_files;
    }

    pub fn report(&self) {
        datapoint_info!(
            "cache_hash_data_stats",
            ("cache_file_size", self.cache_file_size, i64),
            ("cache_file_count", self.cache_file_count, i64),
            ("total_entries", self.total_entries, i64),
            ("loaded_from_cache", self.loaded_from_cache, i64),
            ("saved_to_cache", self.saved_to_cache, i64),
            (
                "entries_loaded_from_cache",
                self.entries_loaded_from_cache,
                i64
            ),
            ("save_us", self.save_us, i64),
            ("write_to_mmap_us", self.write_to_mmap_us, i64),
            ("create_save_us", self.create_save_us, i64),
            ("load_us", self.load_us, i64),
            ("read_us", self.read_us, i64),
            ("decode_us", self.decode_us, i64),
            ("unused_cache_files", self.unused_cache_files, i64),
        );
    }
}
