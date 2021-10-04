use crate::bucket_stats::BucketStats;
use crate::MaxSearch;
use memmap2::MmapMut;
use rand::{thread_rng, Rng};
use solana_measure::measure::Measure;
use std::fs::{remove_file, OpenOptions};
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/*
1	2
2	4
3	8
4	16
5	32
6	64
7	128
8	256
9	512
10	1,024
11	2,048
12	4,096
13	8,192
14	16,384
23  8,388,608
24  16,777,216
*/
const DEFAULT_CAPACITY_POW2: u8 = 5;

/// A Header UID of 0 indicates that the header is unlocked
pub(crate) const UID_UNLOCKED: Uid = 0;

pub(crate) type Uid = u64;

#[repr(C)]
struct Header {
    lock: AtomicU64,
}

impl Header {
    fn try_lock(&self, uid: Uid) -> bool {
        Ok(UID_UNLOCKED)
            == self
                .lock
                .compare_exchange(UID_UNLOCKED, uid, Ordering::Acquire, Ordering::Relaxed)
    }
    fn unlock(&self) -> Uid {
        self.lock.swap(UID_UNLOCKED, Ordering::Release)
    }
    fn uid(&self) -> Uid {
        self.lock.load(Ordering::Relaxed)
    }
}

pub struct BucketStorage {
    drives: Arc<Vec<PathBuf>>,
    path: PathBuf,
    mmap: MmapMut,
    pub cell_size: u64,
    pub capacity_pow2: u8,
    pub used: AtomicU64,
    pub stats: Arc<BucketStats>,
    pub max_search: MaxSearch,
}

#[derive(Debug)]
pub enum BucketStorageError {
    AlreadyAllocated,
}

impl Drop for BucketStorage {
    fn drop(&mut self) {
        let _ = remove_file(&self.path);
    }
}

impl BucketStorage {
    pub fn new_with_capacity(
        drives: Arc<Vec<PathBuf>>,
        num_elems: u64,
        elem_size: u64,
        capacity_pow2: u8,
        max_search: MaxSearch,
        stats: Arc<BucketStats>,
    ) -> Self {
        let cell_size = elem_size * num_elems + std::mem::size_of::<Header>() as u64;
        let (mmap, path) = Self::new_map(&drives, cell_size as usize, capacity_pow2, &stats);
        Self {
            path,
            mmap,
            drives,
            cell_size,
            used: AtomicU64::new(0),
            capacity_pow2,
            stats,
            max_search,
        }
    }

    pub fn max_search(&self) -> u64 {
        self.max_search as u64
    }

    pub fn new(
        drives: Arc<Vec<PathBuf>>,
        num_elems: u64,
        elem_size: u64,
        max_search: MaxSearch,
        stats: Arc<BucketStats>,
    ) -> Self {
        Self::new_with_capacity(
            drives,
            num_elems,
            elem_size,
            DEFAULT_CAPACITY_POW2,
            max_search,
            stats,
        )
    }

    pub fn uid(&self, ix: u64) -> Uid {
        if ix >= self.capacity() {
            panic!("bad index size");
        }
        let ix = (ix * self.cell_size) as usize;
        let hdr_slice: &[u8] = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            return hdr.as_ref().unwrap().uid();
        }
    }

    pub fn allocate(&self, ix: u64, uid: Uid) -> Result<(), BucketStorageError> {
        if ix >= self.capacity() {
            panic!("allocate: bad index size");
        }
        if UID_UNLOCKED == uid {
            panic!("allocate: bad uid");
        }
        let mut e = Err(BucketStorageError::AlreadyAllocated);
        let ix = (ix * self.cell_size) as usize;
        //debug!("ALLOC {} {}", ix, uid);
        let hdr_slice: &[u8] = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            if hdr.as_ref().unwrap().try_lock(uid) {
                e = Ok(());
                self.used.fetch_add(1, Ordering::Relaxed);
            }
        };
        e
    }

    pub fn free(&self, ix: u64, uid: Uid) {
        if ix >= self.capacity() {
            panic!("free: bad index size");
        }
        if UID_UNLOCKED == uid {
            panic!("free: bad uid");
        }
        let ix = (ix * self.cell_size) as usize;
        //debug!("FREE {} {}", ix, uid);
        let hdr_slice: &[u8] = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            //debug!("FREE uid: {}", hdr.as_ref().unwrap().uid());
            let previous_uid = hdr.as_ref().unwrap().unlock();
            assert_eq!(
                previous_uid, uid,
                "free: unlocked a header with a differet uid: {}",
                previous_uid
            );
            self.used.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn get<T: Sized>(&self, ix: u64) -> &T {
        if ix >= self.capacity() {
            panic!("bad index size");
        }
        let start = (ix * self.cell_size) as usize + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>();
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *const T;
            &*item
        }
    }

    pub fn get_empty_cell_slice<T: Sized>(&self) -> &[T] {
        let len = 0;
        let item_slice: &[u8] = &self.mmap[0..0];
        unsafe {
            let item = item_slice.as_ptr() as *const T;
            std::slice::from_raw_parts(item, len as usize)
        }
    }

    pub fn get_cell_slice<T: Sized>(&self, ix: u64, len: u64) -> &[T] {
        if ix >= self.capacity() {
            panic!("bad index size");
        }
        let ix = self.cell_size * ix;
        let start = ix as usize + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>() * len as usize;
        //debug!("GET slice {} {}", start, end);
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *const T;
            std::slice::from_raw_parts(item, len as usize)
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn get_mut<T: Sized>(&self, ix: u64) -> &mut T {
        if ix >= self.capacity() {
            panic!("bad index size");
        }
        let start = (ix * self.cell_size) as usize + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>();
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *mut T;
            &mut *item
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn get_mut_cell_slice<T: Sized>(&self, ix: u64, len: u64) -> &mut [T] {
        if ix >= self.capacity() {
            panic!("bad index size");
        }
        let ix = self.cell_size * ix;
        let start = ix as usize + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>() * len as usize;
        //debug!("GET mut slice {} {}", start, end);
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *mut T;
            std::slice::from_raw_parts_mut(item, len as usize)
        }
    }

    fn new_map(
        drives: &[PathBuf],
        cell_size: usize,
        capacity_pow2: u8,
        stats: &BucketStats,
    ) -> (MmapMut, PathBuf) {
        let mut measure_new_file = Measure::start("measure_new_file");
        let capacity = 1u64 << capacity_pow2;
        let r = thread_rng().gen_range(0, drives.len());
        let drive = &drives[r];
        let pos = format!("{}", thread_rng().gen_range(0, u128::MAX),);
        let file = drive.join(pos);
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
        data.seek(SeekFrom::Start(capacity * cell_size as u64 - 1))
            .unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        measure_new_file.stop();
        let mut measure_flush = Measure::start("measure_flush");
        data.flush().unwrap(); // can we skip this?
        measure_flush.stop();
        let mut measure_mmap = Measure::start("measure_mmap");
        let res = (unsafe { MmapMut::map_mut(&data).unwrap() }, file);
        measure_mmap.stop();
        stats
            .new_file_us
            .fetch_add(measure_new_file.as_us(), Ordering::Relaxed);
        stats
            .flush_file_us
            .fetch_add(measure_flush.as_us(), Ordering::Relaxed);
        stats
            .mmap_us
            .fetch_add(measure_mmap.as_us(), Ordering::Relaxed);
        res
    }

    pub fn grow(&mut self) {
        let mut m = Measure::start("grow");
        let old_cap = self.capacity();
        let old_map = &self.mmap;
        let old_file = self.path.clone();

        let increment = 1;
        let index_grow = 1 << increment;
        let (new_map, new_file) = Self::new_map(
            &self.drives,
            self.cell_size as usize,
            self.capacity_pow2 + increment,
            &self.stats,
        );
        (0..old_cap as usize).into_iter().for_each(|i| {
            let old_ix = i * self.cell_size as usize;
            let new_ix = old_ix * index_grow;
            let dst_slice: &[u8] = &new_map[new_ix..new_ix + self.cell_size as usize];
            let src_slice: &[u8] = &old_map[old_ix..old_ix + self.cell_size as usize];

            unsafe {
                let dst = dst_slice.as_ptr() as *mut u8;
                let src = src_slice.as_ptr() as *const u8;
                std::ptr::copy_nonoverlapping(src, dst, self.cell_size as usize);
            };
        });
        self.mmap = new_map;
        self.path = new_file;
        self.capacity_pow2 += increment;
        remove_file(old_file).unwrap();
        m.stop();
        let sz = 1 << self.capacity_pow2;
        {
            let mut max = self.stats.max_size.lock().unwrap();
            *max = std::cmp::max(*max, sz);
        }
        self.stats.resizes.fetch_add(1, Ordering::Relaxed);
        self.stats.resize_us.fetch_add(m.as_us(), Ordering::Relaxed);
    }

    /// Return the number of cells currently allocated
    pub fn capacity(&self) -> u64 {
        1 << self.capacity_pow2
    }
}
