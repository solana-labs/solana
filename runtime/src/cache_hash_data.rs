//! Cached data for hashing accounts
//!
use {
    crate::{
        accounts_hash::CalculateHashIntermediate, cache_hash_data_stats::CacheHashDataStats,
        pubkey_bins::PubkeyBinCalculator24,
    },
    memmap2::MmapMut,
    solana_measure::measure::Measure,
    std::{
        collections::HashSet,
        fs::{self, remove_file, OpenOptions},
        io::{Seek, SeekFrom, Write},
        marker::PhantomData,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    },
};

pub type EntryType = CalculateHashIntermediate;
pub type SavedType = Vec<Vec<EntryType>>;
pub type SavedTypeSlice = [Vec<EntryType>];

const CELL_SIZE: usize = std::mem::size_of::<EntryType>();
const CELL_ALIGNMENT: usize = std::mem::align_of::<EntryType>();

// NOTE: if change the layout of Headers, i.e. add or remove fields, you MUST ensure that size_of::<Header>() is equal or a multiple of CELL_ALIGNMENT
#[repr(C)]
pub struct Header {
    count: usize,
    _align: [u8; CELL_ALIGNMENT - std::mem::size_of::<usize>()],
}
const HEADER_SIZE: usize = std::mem::size_of::<Header>();

trait Constants<H, T> {
    const HEADER_SIZE: usize = std::mem::size_of::<H>();
    const CELL_SIZE: usize = std::mem::size_of::<T>();
    const CELL_ALIGNMENT: usize = std::mem::align_of::<T>();
}

pub struct CacheDataFile<H, T> {
    mmap: MmapMut,
    capacity: u64,
    phantom_h: PhantomData<H>,
    phantom_t: PhantomData<T>,
}
impl<H, T> Constants<H, T> for CacheDataFile<H, T> {}

impl<H, T> CacheDataFile<H, T> {
    /// return a slice of a reference to all the cache hash data from the mmapped file
    pub(crate) fn get_cache_hash_data(&self) -> &[T] {
        self.get_slice(0)
    }

    /// get '&[T]' from cache file [ix..]
    fn get_slice(&self, ix: u64) -> &[T] {
        let start = self.get_element_offset_byte(ix);
        let item_slice: &[u8] = &self.mmap[start..];
        let remaining_elements = item_slice.len() / CELL_SIZE;
        unsafe {
            let item = item_slice.as_ptr() as *const T;
            std::slice::from_raw_parts(item, remaining_elements)
        }
    }

    /// get '&mut [T]' from cache file [ix..]
    fn get_slice_mut(&mut self, ix: u64) -> &mut [T] {
        let start = self.get_element_offset_byte(ix);
        let item_slice: &[u8] = &self.mmap[start..];
        let remaining_elements = item_slice.len() / CELL_SIZE;
        unsafe {
            let item = item_slice.as_ptr() as *mut T;
            std::slice::from_raw_parts_mut(item, remaining_elements)
        }
    }

    /// return byte offset of entry 'ix' into a slice which contains a header and at least ix elements
    fn get_element_offset_byte(&self, ix: u64) -> usize {
        let start = (ix as usize) * CELL_SIZE + HEADER_SIZE;
        debug_assert_eq!(start % CELL_ALIGNMENT, 0);
        start
    }

    fn get_header_mut(&mut self) -> &mut Header {
        let start = 0_usize;
        let end = start + HEADER_SIZE;
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

pub(crate) type CacheHashDataFile = CacheDataFile<Header, EntryType>;

/// Specialization for CacheHashDataFile
impl CacheDataFile<Header, EntryType> {
    /// Populate 'accumulator' from entire contents of the cache file.
    pub(crate) fn load_all(
        &self,
        accumulator: &mut SavedType,
        start_bin_index: usize,
        bin_calculator: &PubkeyBinCalculator24,
        stats: &mut CacheHashDataStats,
    ) {
        let mut m2 = Measure::start("decode");
        let slices = self.get_cache_hash_data();
        for d in slices {
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
    }
}

pub type PreExistingCacheFiles = HashSet<String>;
pub struct CacheData<H, T> {
    cache_folder: PathBuf,
    pre_existing_cache_files: Arc<Mutex<PreExistingCacheFiles>>,
    pub stats: Arc<Mutex<CacheHashDataStats>>,
    phantom_h: PhantomData<H>,
    phantom_t: PhantomData<T>,
}

impl<H, T> Constants<H, T> for CacheData<H, T> {}

impl<H, T> Drop for CacheData<H, T> {
    fn drop(&mut self) {
        self.delete_old_cache_files();
        self.stats.lock().unwrap().report();
    }
}

impl<H, T> CacheData<H, T> {
    type SavedType = Vec<Vec<T>>;
    type SavedTypeSlice = [Vec<T>];

    pub fn new<P: AsRef<Path> + std::fmt::Debug>(parent_folder: &P) -> CacheData<H, T> {
        let cache_folder = Self::get_cache_root_path(parent_folder);

        std::fs::create_dir_all(cache_folder.clone())
            .unwrap_or_else(|_| panic!("error creating cache dir: {:?}", cache_folder));

        let result = CacheData {
            cache_folder,
            pre_existing_cache_files: Arc::new(Mutex::new(PreExistingCacheFiles::default())),
            stats: Arc::new(Mutex::new(CacheHashDataStats::default())),
            phantom_h: PhantomData,
            phantom_t: PhantomData,
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

    /// map 'file_name' into memory
    pub(crate) fn load_map<P: AsRef<Path> + std::fmt::Debug>(
        &self,
        file_name: &P,
    ) -> Result<CacheDataFile<H, T>, std::io::Error> {
        let mut stats = CacheHashDataStats::default();
        let result = self.map(file_name, &mut stats);
        self.stats.lock().unwrap().merge(&stats);
        result
    }

    /// create and return a MappedCacheFile for a cache file path
    fn map<P: AsRef<Path> + std::fmt::Debug>(
        &self,
        file_name: &P,
        stats: &mut CacheHashDataStats,
    ) -> Result<CacheDataFile<H, T>, std::io::Error> {
        let path = self.cache_folder.join(file_name);
        let file_len = std::fs::metadata(path.clone())?.len();
        let mut m1 = Measure::start("read_file");
        let mmap = CacheDataFile::<H, T>::load_map(&path)?;
        m1.stop();
        stats.read_us = m1.as_us();

        if file_len < HEADER_SIZE as u64 {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }

        unsafe {
            assert_eq!(
                mmap.align_to::<EntryType>().0.len(),
                0,
                "mmap is not aligned"
            );
        }
        const_assert_eq!((CELL_SIZE) % std::mem::size_of::<u64>(), 0);
        let mut cache_file = CacheDataFile {
            mmap,
            capacity: 0,
            phantom_h: PhantomData,
            phantom_t: PhantomData,
        };
        let header = cache_file.get_header_mut();
        let entries = header.count;

        let capacity = (CELL_SIZE as u64) * (entries as u64) + (HEADER_SIZE as u64);
        if file_len < capacity {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }
        cache_file.capacity = capacity;
        assert_eq!(
            capacity, file_len,
            "expected: {}, len on disk: {} {:?}, entries: {}, cell_size: {}",
            capacity, file_len, path, entries, CELL_SIZE,
        );

        stats.total_entries = entries;
        stats.cache_file_size += capacity as usize;

        let file_name_lookup = file_name.as_ref().to_str().unwrap().to_string();
        self.pre_existing_cache_files
            .lock()
            .unwrap()
            .remove(&file_name_lookup);

        stats.loaded_from_cache += 1;
        stats.entries_loaded_from_cache += entries;

        Ok(cache_file)
    }
}

/// Specialization for CacheHashData
impl CacheData<Header, EntryType> {
    /// load from 'file_name' into 'accumulator'
    pub(crate) fn load<P: AsRef<Path> + std::fmt::Debug>(
        &self,
        file_name: &P,
        accumulator: &mut SavedType,
        start_bin_index: usize,
        bin_calculator: &PubkeyBinCalculator24,
    ) -> Result<(), std::io::Error> {
        let mut m = Measure::start("overall");
        let cache_file = self.load_map(file_name)?;
        let mut stats = CacheHashDataStats::default();
        cache_file.load_all(accumulator, start_bin_index, bin_calculator, &mut stats);
        m.stop();
        self.stats.lock().unwrap().load_us += m.as_us();
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
        let mut m1 = Measure::start("create save");
        let entries = data
            .iter()
            .map(|x: &Vec<EntryType>| x.len())
            .collect::<Vec<_>>();
        let entries = entries.iter().sum::<usize>();
        let capacity = (CELL_SIZE as u64) * (entries as u64) + HEADER_SIZE as u64;

        let mmap = CacheHashDataFile::new_map(&cache_path, capacity)?;
        m1.stop();
        stats.create_save_us += m1.as_us();
        let mut cache_file = CacheHashDataFile {
            mmap,
            capacity,
            phantom_h: PhantomData,
            phantom_t: PhantomData,
        };

        let mut header = cache_file.get_header_mut();
        header.count = entries;

        stats.cache_file_size = capacity as usize;
        stats.total_entries = entries;

        let mut m2 = Measure::start("write_to_mmap");
        let mut i = 0;

        let cache = cache_file.get_slice_mut(0_u64);
        for item in data.iter().flatten() {
            cache[i] = item.clone();
            i += 1;
        }
        assert_eq!(i, entries);
        m2.stop();
        stats.write_to_mmap_us += m2.as_us();
        m.stop();
        stats.save_us += m.as_us();
        stats.saved_to_cache += 1;
        Ok(())
    }
}

pub(crate) type CacheHashData = CacheData<Header, EntryType>;

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
