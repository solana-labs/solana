use {
    crate::{bucket_stats::BucketStats, MaxSearch},
    memmap2::MmapMut,
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    std::{
        fs::{remove_file, OpenOptions},
        io::{Seek, SeekFrom, Write},
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    },
};

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
pub const DEFAULT_CAPACITY_POW2: u8 = 5;

/// A Header UID of 0 indicates that the header is unlocked
const UID_UNLOCKED: Uid = 0;

pub(crate) type Uid = u64;

#[repr(C)]
struct Header {
    lock: u64,
}

impl Header {
    /// try to lock this entry with 'uid'
    /// return true if it could be locked
    fn try_lock(&mut self, uid: Uid) -> bool {
        if self.lock == UID_UNLOCKED {
            self.lock = uid;
            true
        } else {
            false
        }
    }
    /// mark this entry as unlocked
    fn unlock(&mut self, expected: Uid) {
        assert_eq!(expected, self.lock);
        self.lock = UID_UNLOCKED;
    }
    /// uid that has locked this entry or None if unlocked
    fn uid(&self) -> Option<Uid> {
        if self.lock == UID_UNLOCKED {
            None
        } else {
            Some(self.lock)
        }
    }
    /// true if this entry is unlocked
    fn is_unlocked(&self) -> bool {
        self.lock == UID_UNLOCKED
    }
}

pub struct BucketStorage {
    path: PathBuf,
    mmap: MmapMut,
    pub cell_size: u64,
    pub capacity_pow2: u8,
    pub count: Arc<AtomicU64>,
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
        count: Arc<AtomicU64>,
    ) -> Self {
        let cell_size = elem_size * num_elems + std::mem::size_of::<Header>() as u64;
        let (mmap, path) = Self::new_map(&drives, cell_size as usize, capacity_pow2, &stats);
        Self {
            path,
            mmap,
            cell_size,
            count,
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
        count: Arc<AtomicU64>,
    ) -> Self {
        Self::new_with_capacity(
            drives,
            num_elems,
            elem_size,
            DEFAULT_CAPACITY_POW2,
            max_search,
            stats,
            count,
        )
    }

    /// return ref to header of item 'ix' in mmapped file
    fn header_ptr(&self, ix: u64) -> &Header {
        let ix = (ix * self.cell_size) as usize;
        let hdr_slice: &[u8] = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            hdr.as_ref().unwrap()
        }
    }

    /// return ref to header of item 'ix' in mmapped file
    #[allow(clippy::mut_from_ref)]
    fn header_mut_ptr(&self, ix: u64) -> &mut Header {
        let ix = (ix * self.cell_size) as usize;
        let hdr_slice: &[u8] = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *mut Header;
            hdr.as_mut().unwrap()
        }
    }

    /// return uid allocated at index 'ix' or None if vacant
    pub fn uid(&self, ix: u64) -> Option<Uid> {
        assert!(ix < self.capacity(), "bad index size");
        self.header_ptr(ix).uid()
    }

    /// true if the entry at index 'ix' is free (as opposed to being allocated)
    pub fn is_free(&self, ix: u64) -> bool {
        // note that the terminology in the implementation is locked or unlocked.
        // but our api is allocate/free
        self.header_ptr(ix).is_unlocked()
    }

    /// caller knows id is not empty
    pub fn uid_unchecked(&self, ix: u64) -> Uid {
        self.uid(ix).unwrap()
    }

    /// 'is_resizing' true if caller is resizing the index (so don't increment count)
    /// 'is_resizing' false if caller is adding an item to the index (so increment count)
    pub fn allocate(&self, ix: u64, uid: Uid, is_resizing: bool) -> Result<(), BucketStorageError> {
        assert!(ix < self.capacity(), "allocate: bad index size");
        assert!(UID_UNLOCKED != uid, "allocate: bad uid");
        let mut e = Err(BucketStorageError::AlreadyAllocated);
        //debug!("ALLOC {} {}", ix, uid);
        if self.header_mut_ptr(ix).try_lock(uid) {
            e = Ok(());
            if !is_resizing {
                self.count.fetch_add(1, Ordering::Relaxed);
            }
        }
        e
    }

    pub fn free(&mut self, ix: u64, uid: Uid) {
        assert!(ix < self.capacity(), "bad index size");
        assert!(UID_UNLOCKED != uid, "free: bad uid");
        self.header_mut_ptr(ix).unlock(uid);
        self.count.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn get<T: Sized>(&self, ix: u64) -> &T {
        assert!(ix < self.capacity(), "bad index size");
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
        assert!(ix < self.capacity(), "bad index size");
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
        assert!(ix < self.capacity(), "bad index size");
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
        assert!(ix < self.capacity(), "bad index size");
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
        data.rewind().unwrap();
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

    /// copy contents from 'old_bucket' to 'self'
    fn copy_contents(&mut self, old_bucket: &Self) {
        let mut m = Measure::start("grow");
        let old_cap = old_bucket.capacity();
        let old_map = &old_bucket.mmap;

        let increment = self.capacity_pow2 - old_bucket.capacity_pow2;
        let index_grow = 1 << increment;
        (0..old_cap as usize).for_each(|i| {
            let old_ix = i * old_bucket.cell_size as usize;
            let new_ix = old_ix * index_grow;
            let dst_slice: &[u8] = &self.mmap[new_ix..new_ix + old_bucket.cell_size as usize];
            let src_slice: &[u8] = &old_map[old_ix..old_ix + old_bucket.cell_size as usize];

            unsafe {
                let dst = dst_slice.as_ptr() as *mut u8;
                let src = src_slice.as_ptr() as *const u8;
                std::ptr::copy_nonoverlapping(src, dst, old_bucket.cell_size as usize);
            };
        });
        m.stop();
        self.stats.resizes.fetch_add(1, Ordering::Relaxed);
        self.stats.resize_us.fetch_add(m.as_us(), Ordering::Relaxed);
    }

    /// allocate a new bucket, copying data from 'bucket'
    pub fn new_resized(
        drives: &Arc<Vec<PathBuf>>,
        max_search: MaxSearch,
        bucket: Option<&Self>,
        capacity_pow_2: u8,
        num_elems: u64,
        elem_size: u64,
        stats: &Arc<BucketStats>,
    ) -> Self {
        let mut new_bucket = Self::new_with_capacity(
            Arc::clone(drives),
            num_elems,
            elem_size,
            capacity_pow_2,
            max_search,
            Arc::clone(stats),
            bucket
                .map(|bucket| Arc::clone(&bucket.count))
                .unwrap_or_default(),
        );
        if let Some(bucket) = bucket {
            new_bucket.copy_contents(bucket);
        }
        let sz = new_bucket.capacity();
        {
            let mut max = new_bucket.stats.max_size.lock().unwrap();
            *max = std::cmp::max(*max, sz);
        }
        new_bucket
    }

    /// Return the number of cells currently allocated
    pub fn capacity(&self) -> u64 {
        1 << self.capacity_pow2
    }
}

#[cfg(test)]
mod test {
    use {super::*, tempfile::tempdir};

    #[test]
    fn test_bucket_storage() {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());

        let mut storage =
            BucketStorage::new(Arc::new(paths), 1, 1, 1, Arc::default(), Arc::default());
        let ix = 0;
        let uid = Uid::MAX;
        assert!(storage.is_free(ix));
        assert!(storage.allocate(ix, uid, false).is_ok());
        assert!(storage.allocate(ix, uid, false).is_err());
        assert!(!storage.is_free(ix));
        assert_eq!(storage.uid(ix), Some(uid));
        assert_eq!(storage.uid_unchecked(ix), uid);
        storage.free(ix, uid);
        assert!(storage.is_free(ix));
        assert_eq!(storage.uid(ix), None);
        let uid = 1;
        assert!(storage.is_free(ix));
        assert!(storage.allocate(ix, uid, false).is_ok());
        assert!(storage.allocate(ix, uid, false).is_err());
        assert!(!storage.is_free(ix));
        assert_eq!(storage.uid(ix), Some(uid));
        assert_eq!(storage.uid_unchecked(ix), uid);
        storage.free(ix, uid);
        assert!(storage.is_free(ix));
        assert_eq!(storage.uid(ix), None);
    }
}
