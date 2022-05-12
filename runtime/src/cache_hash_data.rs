//! Cached data for hashing accounts
use {
    crate::{
        accounts_hash::CalculateHashIntermediate, cache_hash_data_stats::CacheHashDataStats,
        pubkey_bins::PubkeyBinCalculator24,
    },
    log::*,
    memmap2::MmapMut,
    solana_measure::measure::Measure,
    std::{
        collections::HashSet,
        fs::{self, remove_file, OpenOptions},
        io::{Seek, SeekFrom, Write},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    },
};

pub type EntryType = CalculateHashIntermediate;
pub type SavedType = Vec<Vec<EntryType>>;
pub type SavedTypeSlice = [Vec<EntryType>];

#[repr(C)]
pub struct Header {
    count: usize,
}

struct CacheHashDataFile {
    cell_size: u64,
    mmap: MmapMut,
    capacity: u64,
}

impl CacheHashDataFile {
    fn get_mut<T: Sized>(&mut self, ix: u64) -> &mut T {
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

    fn get_header_mut(&mut self) -> &mut Header {
        let start = 0_usize;
        let end = start + std::mem::size_of::<Header>();
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *mut Header;
            &mut *item
        }
    }

    fn new_map(file: &Path, capacity: u64) -> Result<MmapMut, std::io::Error> {
        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(file)?;

        // Theoretical performance optimization: write a zero to the end of
        // the file so that we won't have to resize it later, which may be
        // expensive.
        data.seek(SeekFrom::Start(capacity - 1)).unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        data.flush().unwrap();
        Ok(unsafe { MmapMut::map_mut(&data).unwrap() })
    }

    fn load_map(file: &Path) -> Result<MmapMut, std::io::Error> {
        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(file)?;

        Ok(unsafe { MmapMut::map_mut(&data).unwrap() })
    }
}

pub type PreExistingCacheFiles = HashSet<String>;
pub struct CacheHashData {
    cache_folder: PathBuf,
    pre_existing_cache_files: Arc<Mutex<PreExistingCacheFiles>>,
    pub stats: Arc<Mutex<CacheHashDataStats>>,
}

impl Drop for CacheHashData {
    fn drop(&mut self) {
        self.delete_old_cache_files();
        self.stats.lock().unwrap().report();
    }
}

impl CacheHashData {
    pub fn new<P: AsRef<Path> + std::fmt::Debug>(parent_folder: &P) -> CacheHashData {
        let cache_folder = Self::get_cache_root_path(parent_folder);

        std::fs::create_dir_all(cache_folder.clone())
            .unwrap_or_else(|_| panic!("error creating cache dir: {:?}", cache_folder));

        let result = CacheHashData {
            cache_folder,
            pre_existing_cache_files: Arc::new(Mutex::new(PreExistingCacheFiles::default())),
            stats: Arc::new(Mutex::new(CacheHashDataStats::default())),
        };

        result.get_cache_files();
        result
    }
    fn delete_old_cache_files(&self) {
        let pre_existing_cache_files = self.pre_existing_cache_files.lock().unwrap();
        if !pre_existing_cache_files.is_empty() {
            self.stats.lock().unwrap().unused_cache_files += pre_existing_cache_files.len();
            for file_name in pre_existing_cache_files.iter() {
                let result = self.cache_folder.join(file_name);
                let _ = fs::remove_file(result);
            }
        }
    }
    fn get_cache_files(&self) {
        if self.cache_folder.is_dir() {
            let dir = fs::read_dir(self.cache_folder.clone());
            if let Ok(dir) = dir {
                let mut pre_existing = self.pre_existing_cache_files.lock().unwrap();
                for entry in dir.flatten() {
                    if let Some(name) = entry.path().file_name() {
                        pre_existing.insert(name.to_str().unwrap().to_string());
                    }
                }
                self.stats.lock().unwrap().cache_file_count += pre_existing.len();
            }
        }
    }

    fn get_cache_root_path<P: AsRef<Path>>(parent_folder: &P) -> PathBuf {
        parent_folder.as_ref().join("calculate_accounts_hash_cache")
    }

    /// load from 'file_name' into 'accumulator'
    pub fn load<P: AsRef<Path> + std::fmt::Debug>(
        &self,
        file_name: &P,
        accumulator: &mut SavedType,
        start_bin_index: usize,
        bin_calculator: &PubkeyBinCalculator24,
    ) -> Result<(), std::io::Error> {
        let mut stats = CacheHashDataStats::default();
        let result = self.load_internal(
            file_name,
            accumulator,
            start_bin_index,
            bin_calculator,
            &mut stats,
        );
        self.stats.lock().unwrap().merge(&stats);
        result
    }

    fn load_internal<P: AsRef<Path> + std::fmt::Debug>(
        &self,
        file_name: &P,
        accumulator: &mut SavedType,
        start_bin_index: usize,
        bin_calculator: &PubkeyBinCalculator24,
        stats: &mut CacheHashDataStats,
    ) -> Result<(), std::io::Error> {
        let mut m = Measure::start("overall");
        let path = self.cache_folder.join(file_name);
        let file_len = std::fs::metadata(path.clone())?.len();
        let mut m1 = Measure::start("read_file");
        let mmap = CacheHashDataFile::load_map(&path)?;
        m1.stop();
        stats.read_us = m1.as_us();
        let header_size = std::mem::size_of::<Header>() as u64;
        if file_len < header_size {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }

        let cell_size = std::mem::size_of::<EntryType>() as u64;
        let mut cache_file = CacheHashDataFile {
            mmap,
            cell_size,
            capacity: 0,
        };
        let header = cache_file.get_header_mut();
        let entries = header.count;

        let capacity = cell_size * (entries as u64) + header_size;
        if file_len < capacity {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }
        cache_file.capacity = capacity;
        assert_eq!(
            capacity, file_len,
            "expected: {}, len on disk: {} {:?}, entries: {}, cell_size: {}",
            capacity, file_len, path, entries, cell_size
        );

        stats.total_entries = entries;
        stats.cache_file_size += capacity as usize;

        let file_name_lookup = file_name.as_ref().to_str().unwrap().to_string();
        let found = self
            .pre_existing_cache_files
            .lock()
            .unwrap()
            .remove(&file_name_lookup);
        if !found {
            info!(
                "tried to mark {:?} as used, but it wasn't in the set, one example: {:?}",
                file_name_lookup,
                self.pre_existing_cache_files.lock().unwrap().iter().next()
            );
        }

        stats.loaded_from_cache += 1;
        stats.entries_loaded_from_cache += entries;
        let mut m2 = Measure::start("decode");
        for i in 0..entries {
            let d = cache_file.get_mut::<EntryType>(i as u64);
            let mut pubkey_to_bin_index = bin_calculator.bin_from_pubkey(&d.pubkey);
            assert!(
                pubkey_to_bin_index >= start_bin_index,
                "{}, {}",
                pubkey_to_bin_index,
                start_bin_index
            ); // this would indicate we put a pubkey in too high of a bin
            pubkey_to_bin_index -= start_bin_index;
            accumulator[pubkey_to_bin_index].push(d.clone()); // may want to avoid clone here
        }

        m2.stop();
        stats.decode_us += m2.as_us();
        m.stop();
        stats.load_us += m.as_us();
        Ok(())
    }

    /// save 'data' to 'file_name'
    pub fn save(&self, file_name: &Path, data: &SavedTypeSlice) -> Result<(), std::io::Error> {
        let mut stats = CacheHashDataStats::default();
        let result = self.save_internal(file_name, data, &mut stats);
        self.stats.lock().unwrap().merge(&stats);
        result
    }

    fn save_internal(
        &self,
        file_name: &Path,
        data: &SavedTypeSlice,
        stats: &mut CacheHashDataStats,
    ) -> Result<(), std::io::Error> {
        let mut m = Measure::start("save");
        let cache_path = self.cache_folder.join(file_name);
        let create = true;
        if create {
            let _ignored = remove_file(&cache_path);
        }
        let cell_size = std::mem::size_of::<EntryType>() as u64;
        let mut m1 = Measure::start("create save");
        let entries = data
            .iter()
            .map(|x: &Vec<EntryType>| x.len())
            .collect::<Vec<_>>();
        let entries = entries.iter().sum::<usize>();
        let capacity = cell_size * (entries as u64) + std::mem::size_of::<Header>() as u64;

        let mmap = CacheHashDataFile::new_map(&cache_path, capacity)?;
        m1.stop();
        stats.create_save_us += m1.as_us();
        let mut cache_file = CacheHashDataFile {
            mmap,
            cell_size,
            capacity,
        };

        let mut header = cache_file.get_header_mut();
        header.count = entries;

        stats.cache_file_size = capacity as usize;
        stats.total_entries = entries;

        let mut m2 = Measure::start("write_to_mmap");
        let mut i = 0;
        data.iter().for_each(|x| {
            x.iter().for_each(|item| {
                let d = cache_file.get_mut::<EntryType>(i as u64);
                i += 1;
                *d = item.clone();
            })
        });
        assert_eq!(i, entries);
        m2.stop();
        stats.write_to_mmap_us += m2.as_us();
        m.stop();
        stats.save_us += m.as_us();
        stats.saved_to_cache += 1;
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, rand::Rng};

    #[test]
    fn test_read_write() {
        // generate sample data
        // write to file
        // read
        // compare
        use tempfile::TempDir;
        let tmpdir = TempDir::new().unwrap();
        std::fs::create_dir_all(&tmpdir).unwrap();

        for bins in [1, 2, 4] {
            let bin_calculator = PubkeyBinCalculator24::new(bins);
            let num_points = 5;
            let (data, _total_points) = generate_test_data(num_points, bins, &bin_calculator);
            for passes in [1, 2] {
                let bins_per_pass = bins / passes;
                if bins_per_pass == 0 {
                    continue; // illegal test case
                }
                for pass in 0..passes {
                    for flatten_data in [true, false] {
                        let mut data_this_pass = if flatten_data {
                            vec![vec![], vec![]]
                        } else {
                            vec![]
                        };
                        let start_bin_this_pass = pass * bins_per_pass;
                        for bin in 0..bins_per_pass {
                            let mut this_bin_data = data[bin + start_bin_this_pass].clone();
                            if flatten_data {
                                data_this_pass[0].append(&mut this_bin_data);
                            } else {
                                data_this_pass.push(this_bin_data);
                            }
                        }
                        let cache = CacheHashData::new(&tmpdir);
                        let file_name = "test";
                        let file = Path::new(file_name).to_path_buf();
                        cache.save(&file, &data_this_pass).unwrap();
                        cache.get_cache_files();
                        assert_eq!(
                            cache
                                .pre_existing_cache_files
                                .lock()
                                .unwrap()
                                .iter()
                                .collect::<Vec<_>>(),
                            vec![file_name]
                        );
                        let mut accum = (0..bins_per_pass).into_iter().map(|_| vec![]).collect();
                        cache
                            .load(&file, &mut accum, start_bin_this_pass, &bin_calculator)
                            .unwrap();
                        if flatten_data {
                            bin_data(
                                &mut data_this_pass,
                                &bin_calculator,
                                bins_per_pass,
                                start_bin_this_pass,
                            );
                        }
                        assert_eq!(
                            accum, data_this_pass,
                            "bins: {}, start_bin_this_pass: {}, pass: {}, flatten: {}, passes: {}",
                            bins, start_bin_this_pass, pass, flatten_data, passes
                        );
                    }
                }
            }
        }
    }

    fn bin_data(
        data: &mut SavedType,
        bin_calculator: &PubkeyBinCalculator24,
        bins: usize,
        start_bin: usize,
    ) {
        let mut accum: SavedType = (0..bins).into_iter().map(|_| vec![]).collect();
        data.drain(..).into_iter().for_each(|mut x| {
            x.drain(..).into_iter().for_each(|item| {
                let bin = bin_calculator.bin_from_pubkey(&item.pubkey);
                accum[bin - start_bin].push(item);
            })
        });
        *data = accum;
    }

    fn generate_test_data(
        count: usize,
        bins: usize,
        binner: &PubkeyBinCalculator24,
    ) -> (SavedType, usize) {
        let mut rng = rand::thread_rng();
        let mut ct = 0;
        (
            (0..bins)
                .into_iter()
                .map(|bin| {
                    let rnd = rng.gen::<u64>() % (bins as u64);
                    if rnd < count as u64 {
                        (0..std::cmp::max(1, count / bins))
                            .into_iter()
                            .map(|_| {
                                ct += 1;
                                let mut pk;
                                loop {
                                    // expensive, but small numbers and for tests, so ok
                                    pk = solana_sdk::pubkey::new_rand();
                                    if binner.bin_from_pubkey(&pk) == bin {
                                        break;
                                    }
                                }

                                CalculateHashIntermediate::new(
                                    solana_sdk::hash::new_rand(&mut rng),
                                    ct as u64,
                                    pk,
                                )
                            })
                            .collect::<Vec<_>>()
                    } else {
                        vec![]
                    }
                })
                .collect::<Vec<_>>(),
            ct,
        )
    }
}
