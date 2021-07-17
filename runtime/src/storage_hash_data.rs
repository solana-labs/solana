//! Cached data for hashing accounts
use crate::accounts_hash::CalculateHashIntermediate;
use crate::pubkey_bins::PubkeyBinCalculator16;
use log::*;
use memmap2::MmapMut;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use solana_measure::measure::Measure;
use solana_sdk::clock::Slot;
use std::collections::HashSet;
use std::fs::{self, DirEntry};
use std::fs::{remove_file, OpenOptions};
use std::io;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;
use std::{io::Read, ops::Range, path::Path};

use crate::accounts_db::{num_scan_passes, BINS_PER_PASS, PUBKEY_BINS_FOR_CALCULATING_HASHES};

pub type SavedType = Vec<Vec<CalculateHashIntermediate>>;

#[repr(C)]
pub struct Header {
    lock: AtomicU64,
    count: usize,
    //bin_sizes: [u64; BINS_PER_PASS],
}

impl Header {
    fn try_lock(&self, uid: u64) -> bool {
        Ok(0)
            == self
                .lock
                .compare_exchange(0, uid, Ordering::Relaxed, Ordering::Relaxed)
    }
    fn unlock(&self, uid: u64) -> bool {
        Ok(uid)
            == self
                .lock
                .compare_exchange(uid, 0, Ordering::Relaxed, Ordering::Relaxed)
    }
    fn uid(&self) -> u64 {
        self.lock.load(Ordering::Relaxed)
    }
}

//#[derive(Default, Debug, Serialize, Deserialize)]
pub struct CacheHashData {
    //pub data: SavedType,
    //pub storage_path: PathBuf,
    //pub expected_mod_date: u8,
    pub cell_size: u64,
    pub mmap: MmapMut,
    pub capacity: u64,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct CacheHashDataStats {
    pub storage_size: usize,
    pub cache_file_size: usize,
    pub cache_file_count: usize,
    pub entries: usize,
    pub loaded_from_cache: usize,
    pub entries_loaded_from_cache: usize,
    pub save_us: u64,
    pub write_to_mmap_us: u64,
    pub create_save_us: u64,
    pub load_total_us: u64,
    pub open_mmap_us: u64,
    pub decode_us: u64,
    pub calc_path_us: u64,
    pub merge_us: u64,
    pub new_cache_files: u64,
    pub files_purged: u64,
}

impl CacheHashDataStats {
    pub fn merge(&mut self, other: &CacheHashDataStats) {
        self.storage_size += other.storage_size;
        self.cache_file_size += other.cache_file_size;
        self.entries += other.entries;
        self.loaded_from_cache += other.loaded_from_cache;
        self.entries_loaded_from_cache += other.entries_loaded_from_cache;
        self.load_total_us += other.load_total_us;
        self.open_mmap_us += other.open_mmap_us;
        self.decode_us += other.decode_us;
        self.calc_path_us += other.calc_path_us;
        self.merge_us += other.merge_us;
        self.save_us += other.save_us;
        self.create_save_us += other.create_save_us;
        self.cache_file_count += other.cache_file_count;
        self.write_to_mmap_us += other.write_to_mmap_us;
        self.new_cache_files += other.new_cache_files;
        self.files_purged += other.files_purged;
    }
}

pub type PreExistingCacheFiles = HashSet<String>;

#[derive(Debug)]
pub struct CacheHashDataManager {
    cache_path: PathBuf,
    stats: RwLock<CacheHashDataStats>,
}

impl CacheHashDataManager {
    pub fn new<P: AsRef<Path>>(ledger_path: &P) -> io::Result<Self> {
        let cache_path = ledger_path.as_ref().join("calculate_cache_hash");
        std::fs::create_dir_all(cache_path.clone())?;
        info!("CacheHash folder: {:?}", cache_path);
        Ok(Self { cache_path,
            stats: RwLock::new(CacheHashDataStats::default()),
         })
    }
    pub fn update_stats(&self, stats: &CacheHashDataStats) {
        self.stats.write().unwrap().merge(stats);
    }
    pub fn report_stats(&self) {
        let mut stats = self.stats.write().unwrap();
        info!("hash calculation cache stats: {:?}", stats);
        *stats = CacheHashDataStats::default();
    }
    fn cache_path(&self) -> &PathBuf {
        &self.cache_path
    }
    fn directory<P: AsRef<Path>>(&self, storage_file: &P) -> String {
        let storage_file = storage_file.as_ref();
        let file_name = storage_file.file_name().unwrap();
        file_name.to_str().unwrap().to_string()
    }
    fn calc_path<P: AsRef<Path>>(
        &self,
        storage_file: &P,
        bin_range: &Range<usize>,
    ) -> Result<(PathBuf, String), std::io::Error> {
        let file_name = self.directory(storage_file);
        let amod = std::fs::metadata(storage_file)?.modified()?;
        let secs = amod.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let file_name = format!(
            "{}.{}.{}",
            file_name,
            secs.to_string(),
            format!("{}.{}", bin_range.start, bin_range.end),
        );
        let result = self.cache_path.join(file_name.clone());
        Ok((result.to_path_buf(), file_name))
    }
    pub fn delete_old_cache_files(&self, file_names: &PreExistingCacheFiles) {
        for file_name in file_names {
            let result = self.cache_path.join(file_name);
            let _ = fs::remove_file(result);
        }
    }
    pub fn get_cache_files(&self) -> PreExistingCacheFiles {
        let mut items = PreExistingCacheFiles::new();
        if self.cache_path.is_dir() {
            let dir = fs::read_dir(self.cache_path.clone());
            if let Ok(dir) = dir {
                for entry in dir {
                    if let Ok(entry) = entry {
                        if let Some(name) = entry.path().file_name() {
                            items.insert(name.to_str().unwrap().to_string());
                        }
                    }
                }
            }
        }
        items
    }
}

impl CacheHashData {
    fn new_map(file: &PathBuf, cell_size: usize, capacity: u64) -> MmapMut {
        let pos = format!("{}", thread_rng().gen_range(0, u128::MAX),);
        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(file.clone())
            .map_err(|e| {
                panic!(
                    "Unable to create data file {} in current dir({:?}): {:?}",
                    file.display(),
                    std::env::current_dir(),
                    e
                );
            })
            .unwrap();

        // Theoretical performance optimization: write a zero to the end of
        // the file so that we won't have to resize it later, which may be
        // expensive.
        //debug!("GROWING file {}", capacity * cell_size as u64);
        data.seek(SeekFrom::Start(capacity - 1)).unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        data.flush().unwrap();
        unsafe { MmapMut::map_mut(&data).unwrap() }
    }
    /*
    pub fn test() {
        let drives = Arc::new(vec![]);//    drives: Arc<Vec<PathBuf>>,
        let elements = 0;
        let index = Self::new_with_capacity(
            drives.clone(),
            1,
            std::mem::size_of::<CacheHashData>() as u64,
            elements,
        );
    }
    */
    /*
    pub fn new_with_capacity(
        drives: Arc<Vec<PathBuf>>,
        num_elems: u64,
        elem_size: u64,
        capacity: u8,
    ) {
        // todo
        let cell_size = elem_size * num_elems + std::mem::size_of::<Header>() as u64;
        let (mmap, path) = Self::new_map(&drives, cell_size as usize, capacity);
        /*
        Self {
            path,
            mmap,
            drives,
            cell_size,
            used: AtomicU64::new(0),
            capacity,
        }*/
    }
    */
    pub fn load<P: AsRef<Path>>(
        slot: Slot,
        storage_file: &P,
        bin_range: &Range<usize>,
        accumulator: &mut Vec<Vec<CalculateHashIntermediate>>,
        start_bin_index: usize,
        bin_calculator: &PubkeyBinCalculator16,
        preexisting: &RwLock<PreExistingCacheFiles>,
        manager: &CacheHashDataManager,
    ) -> Result<(SavedType, CacheHashDataStats), std::io::Error> {
        let mut m = Measure::start("overall");
        let create = false;
        let mut timings = CacheHashDataStats::default();
        let mut m0 = Measure::start("");
        let (path, file_name) = manager.calc_path(storage_file, bin_range)?;
        m0.stop();
        timings.calc_path_us += m0.as_us();
        let file_len = std::fs::metadata(path.clone())?.len();
        let mut m0 = Measure::start("");
        let mut m1 = Measure::start("");
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&path.clone())?;

        let elem_size = std::mem::size_of::<CalculateHashIntermediate>() as u64;
        let cell_size = elem_size;
        let mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        m1.stop();
        let mut chd = CacheHashData {
            //data: SavedType::default(),
            //storage_path
            mmap,
            cell_size,
            capacity: 0,
        };
        let mut header = chd.get_header_mut();
        let sum = header.count;

        let capacity = elem_size * (sum as u64) + std::mem::size_of::<Header>() as u64;
        chd.capacity = capacity;
        assert_eq!(
            capacity, file_len,
            "expected: {}, len on disk: {} {:?}, sum: {}, elem_size: {}",
            capacity, file_len, path, sum, cell_size
        );
        if false && slot == 86376721 {
            error!(
                "load: expected: {}, len on disk: {} {:?}, sum: {}, elem_size: {}",
                capacity, file_len, path, sum, cell_size
            );
        }

        //error!("writing {} bytes to: {:?}, lens: {:?}, storage_len: {}, storage: {:?}", encoded.len(), cache_path, file_data.data.iter().map(|x| x.len()).collect::<Vec<_>>(), file_len, storage_file);
        let mut stats = CacheHashDataStats {
            //storage_size: file_len as usize,
            entries: sum,
            ..CacheHashDataStats::default()
        };
        stats.open_mmap_us = m1.as_us();
        stats.cache_file_size += capacity as usize;

        let found = preexisting.write().unwrap().remove(&file_name);
        if !found {
            error!(
                "tried to mark {:?} as used, but it wasn't in the set: {:?}",
                file_name,
                preexisting.read().unwrap().iter().next()
            );
        }

        stats.entries_loaded_from_cache += sum;
        let mut m2 = Measure::start("");
        let mut i = 0;
        for i in 0..sum {
            let mut d = chd.get_mut::<CalculateHashIntermediate>(i as u64);
            let mut pubkey_to_bin_index = bin_calculator.bin_from_pubkey(&d.pubkey);
            pubkey_to_bin_index -= start_bin_index;
            accumulator[pubkey_to_bin_index].push(d.clone()); // may want to avoid clone here
        }

        m2.stop();
        stats.decode_us += m2.as_us();
        //stats.write_to_mmap_us += m2.as_us();
        //error!("wrote: {:?}, {}, sum: {}, elem_size: {}", cache_path, capacity, sum, elem_size);//, storage_file);
        m.stop();
        stats.load_total_us += m.as_us();
        //stats.save_us += m.as_us();
        Ok((vec![], stats))
    }
    pub fn get_mut<T: Sized>(&self, ix: u64) -> &mut T {
        let start = (ix * self.cell_size) as usize + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>();
        assert!(
            end <= self.capacity as usize,
            "end: {}, capacity: {}, ix: {}, cell size: {}",
            end,
            self.capacity,
            ix,
            self.cell_size
        );
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *mut T;
            &mut *item
        }
    }

    pub fn get_header_mut(&self) -> &mut Header {
        let start = 0 as usize;
        let end = start + std::mem::size_of::<Header>();
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *mut Header;
            &mut *item
        }
    }

    pub fn save2<P: AsRef<Path> + std::fmt::Debug>(
        slot: Slot,
        storage_file: &P,
        data: &mut SavedType,
        bin_range: &Range<usize>,
        manager: &CacheHashDataManager,
    ) -> Result<CacheHashDataStats, std::io::Error> {
        let mut m = Measure::start("save");
        let mut stats;
        //error!("raw path: {:?}", storage_file);
        let mut m0 = Measure::start("");
        let (cache_path, _) = manager.calc_path(storage_file, bin_range)?;
        m0.stop();
        stats = CacheHashDataStats {
            ..CacheHashDataStats::default()
        };

        stats.calc_path_us += m0.as_us();
        let create = true;
        if create {
            let _ignored = remove_file(&cache_path);
        }
        let elem_size = std::mem::size_of::<CalculateHashIntermediate>() as u64;
        stats.new_cache_files += 1;
        let entries = data
            .iter()
            .map(|x: &Vec<CalculateHashIntermediate>| x.len())
            .collect::<Vec<_>>();
        let sum = entries.iter().sum::<usize>();
        let cell_size = elem_size;
        let capacity = elem_size * (sum as u64) + std::mem::size_of::<Header>() as u64;
        let mut m1 = Measure::start("");
        //error!("writing: len on disk: {} {:?}, sum: {}", capacity, cache_path, sum);

        let mmap = Self::new_map(&cache_path, cell_size as usize, capacity);
        m1.stop();
        let mut chd = CacheHashData {
            //data: SavedType::default(),
            //storage_path
            mmap,
            cell_size,
            capacity,
        };
        stats.create_save_us = m1.as_us();
        stats.cache_file_count = 1;

        let mut header = chd.get_header_mut();
        header.count = sum;

        //error!("writing {} bytes to: {:?}, lens: {:?}, storage_len: {}, storage: {:?}", encoded.len(), cache_path, file_data.data.iter().map(|x| x.len()).collect::<Vec<_>>(), file_len, storage_file);
        stats = CacheHashDataStats {
            //storage_size: file_len as usize,
            cache_file_size: capacity as usize,
            entries: sum,
            ..CacheHashDataStats::default()
        };

        let mut m2 = Measure::start("");
        let mut i = 0;
        data.iter().for_each(|x| {
            x.iter().for_each(|item| {
                let mut d = chd.get_mut::<CalculateHashIntermediate>(i as u64);
                i += 1;
                *d = item.clone();
            })
        });
        assert_eq!(i, sum);
        m2.stop();
        stats.write_to_mmap_us += m2.as_us();
        m.stop();
        stats.save_us += m.as_us();
        //chd.mmap.flush()?;
        /*
        let expected_mod_date = 0; // TODO
        let file_size = 0; // TODO

        let mut data_bkup = SavedType::default();
        std::mem::swap(&mut data_bkup, data);
        let mut file_data = CacheHashData {
            expected_mod_date,
            storage_path: storage_file.as_ref().to_path_buf(),
            data: data_bkup,
        };

        let encoded: Vec<u8> = bincode::serialize(&file_data).unwrap();
        let file_len = std::fs::metadata(storage_file)?.len();
        let entries = file_data.data.iter().map(|x: &Vec<CalculateHashIntermediate>| x.len()).sum::<usize>();

        //error!("writing {} bytes to: {:?}, lens: {:?}, storage_len: {}, storage: {:?}", encoded.len(), cache_path, file_data.data.iter().map(|x| x.len()).collect::<Vec<_>>(), file_len, storage_file);
        let stats = CacheHashDataStats {
            storage_size: file_len as usize,
            cache_file_size: encoded.len(),
            entries,
            ..CacheHashDataStats::default()
        };
        std::mem::swap(&mut file_data.data, data);

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&cache_path)
            .map_err(|e| {
                panic!(
                    "Unable to {} data file {} in current dir({:?}): {:?}",
                    if create { "create" } else { "open" },
                    cache_path.display(),
                    std::env::current_dir(),
                    e
                );
            })
            .unwrap();
        file.write_all(&encoded)?;
        drop(file);
        */
        Ok(stats)
    }
}

#[cfg(test)]
pub mod tests {
    use super::test_utils::*;
    use super::*;
    use assert_matches::assert_matches;
    use rand::{thread_rng, Rng};
    use solana_sdk::{account::WritableAccount, timing::duration_as_ms};
    use std::time::Instant;
}
