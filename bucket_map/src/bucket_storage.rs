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

/// keep track of an individual element's occupied vs. free state
/// every element must either be occupied or free and should never be double occupied or double freed
/// For parameters below, `element` is used to view/modify header fields or fields within the element data.
pub trait BucketOccupied {
    /// set entry at `ix` as occupied (as opposed to free)
    fn occupy(&mut self, element: &mut [u8], ix: usize);
    /// set entry at `ix` as free
    fn free(&mut self, element: &mut [u8], ix: usize);
    /// return true if entry at `ix` is free
    fn is_free(&self, element: &[u8], ix: usize) -> bool;
    /// # of bytes prior to first data held in the element.
    /// This is the header size, if a header exists per element in the data.
    /// This must be a multiple of sizeof(u64)
    fn offset_to_first_data() -> usize;
    /// initialize this struct
    /// `num_elements` is the number of elements allocated in the bucket
    fn new(num_elements: usize) -> Self;
}

pub struct BucketStorage<O: BucketOccupied> {
    path: PathBuf,
    mmap: MmapMut,
    pub cell_size: u64,
    pub capacity_pow2: u8,
    pub count: Arc<AtomicU64>,
    pub stats: Arc<BucketStats>,
    pub max_search: MaxSearch,
    pub contents: O,
}

#[derive(Debug)]
pub enum BucketStorageError {
    AlreadyOccupied,
}

impl<O: BucketOccupied> Drop for BucketStorage<O> {
    fn drop(&mut self) {
        _ = remove_file(&self.path);
    }
}

impl<O: BucketOccupied> BucketStorage<O> {
    pub fn new_with_capacity(
        drives: Arc<Vec<PathBuf>>,
        num_elems: u64,
        elem_size: u64,
        capacity_pow2: u8,
        max_search: MaxSearch,
        stats: Arc<BucketStats>,
        count: Arc<AtomicU64>,
    ) -> Self {
        let offset = O::offset_to_first_data();
        let size_of_u64 = std::mem::size_of::<u64>();
        assert_eq!(
            offset / size_of_u64 * size_of_u64,
            offset,
            "header size must be a multiple of u64"
        );
        let cell_size = elem_size * num_elems + offset as u64;
        let bytes = (1u64 << capacity_pow2) * cell_size;
        let (mmap, path) = Self::new_map(&drives, bytes, &stats);
        Self {
            path,
            mmap,
            cell_size,
            count,
            capacity_pow2,
            stats,
            max_search,
            contents: O::new(1 << capacity_pow2),
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

    /// true if the entry at index 'ix' is free (as opposed to being occupied)
    pub fn is_free(&self, ix: u64) -> bool {
        let start = self.get_start_offset_with_header(ix);
        let entry = &self.mmap[start..];
        self.contents.is_free(entry, ix as usize)
    }

    fn try_lock(&mut self, ix: u64) -> bool {
        let start = self.get_start_offset_with_header(ix);
        let entry = &mut self.mmap[start..];
        if self.contents.is_free(entry, ix as usize) {
            self.contents.occupy(entry, ix as usize);
            true
        } else {
            false
        }
    }

    /// 'is_resizing' true if caller is resizing the index (so don't increment count)
    /// 'is_resizing' false if caller is adding an item to the index (so increment count)
    pub fn occupy(&mut self, ix: u64, is_resizing: bool) -> Result<(), BucketStorageError> {
        assert!(ix < self.capacity(), "occupy: bad index size");
        let mut e = Err(BucketStorageError::AlreadyOccupied);
        //debug!("ALLOC {} {}", ix, uid);
        if self.try_lock(ix) {
            e = Ok(());
            if !is_resizing {
                self.count.fetch_add(1, Ordering::Relaxed);
            }
        }
        e
    }

    pub fn free(&mut self, ix: u64) {
        assert!(ix < self.capacity(), "bad index size");
        let start = self.get_start_offset_with_header(ix);
        self.contents.free(&mut self.mmap[start..], ix as usize);
        self.count.fetch_sub(1, Ordering::Relaxed);
    }

    fn get_start_offset_with_header(&self, ix: u64) -> usize {
        assert!(ix < self.capacity(), "bad index size");
        (self.cell_size * ix) as usize
    }

    fn get_start_offset_no_header(&self, ix: u64) -> usize {
        self.get_start_offset_with_header(ix) + O::offset_to_first_data()
    }

    pub fn get<T>(&self, ix: u64) -> &T {
        let slice = self.get_cell_slice::<T>(ix, 1);
        // SAFETY: `get_cell_slice` ensures there's at least one element in the slice
        unsafe { slice.get_unchecked(0) }
    }

    pub fn get_mut<T>(&mut self, ix: u64) -> &mut T {
        let slice = self.get_mut_cell_slice::<T>(ix, 1);
        // SAFETY: `get_mut_cell_slice` ensures there's at least one element in the slice
        unsafe { slice.get_unchecked_mut(0) }
    }

    pub(crate) fn get_mut_from_parts<T>(item_slice: &mut [u8]) -> &mut T {
        debug_assert!(std::mem::size_of::<T>() <= item_slice.len());
        let item = item_slice.as_mut_ptr() as *mut T;
        unsafe { &mut *item }
    }

    pub(crate) fn get_from_parts<T>(item_slice: &[u8]) -> &T {
        debug_assert!(std::mem::size_of::<T>() <= item_slice.len());
        let item = item_slice.as_ptr() as *const T;
        unsafe { &*item }
    }

    pub fn get_cell_slice<T>(&self, ix: u64, len: u64) -> &[T] {
        let start = self.get_start_offset_no_header(ix);
        let slice = {
            let size = std::mem::size_of::<T>() * len as usize;
            let slice = &self.mmap[start..];
            debug_assert!(slice.len() >= size);
            &slice[..size]
        };
        let ptr = {
            let ptr = slice.as_ptr() as *const T;
            debug_assert!(ptr as usize % std::mem::align_of::<T>() == 0);
            ptr
        };
        unsafe { std::slice::from_raw_parts(ptr, len as usize) }
    }

    pub fn get_mut_cell_slice<T>(&mut self, ix: u64, len: u64) -> &mut [T] {
        let start = self.get_start_offset_no_header(ix);
        let slice = {
            let size = std::mem::size_of::<T>() * len as usize;
            let slice = &mut self.mmap[start..];
            debug_assert!(slice.len() >= size);
            &mut slice[..size]
        };
        let ptr = {
            let ptr = slice.as_mut_ptr() as *mut T;
            debug_assert!(ptr as usize % std::mem::align_of::<T>() == 0);
            ptr
        };
        unsafe { std::slice::from_raw_parts_mut(ptr, len as usize) }
    }

    /// allocate a new memory mapped file of size `bytes` on one of `drives`
    fn new_map(drives: &[PathBuf], bytes: u64, stats: &BucketStats) -> (MmapMut, PathBuf) {
        let mut measure_new_file = Measure::start("measure_new_file");
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
        data.seek(SeekFrom::Start(bytes - 1)).unwrap();
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
    /// This is used by data buckets
    fn copy_contents(&mut self, old_bucket: &Self) {
        let mut m = Measure::start("grow");
        let old_cap = old_bucket.capacity();
        let old_map = &old_bucket.mmap;

        let increment = self.capacity_pow2 - old_bucket.capacity_pow2;
        let index_grow = 1 << increment;
        (0..old_cap as usize).for_each(|i| {
            if !old_bucket.is_free(i as u64) {
                {
                    // copying from old to new. If 'occupied' bit is stored outside the data, then
                    // occupied has to be set on the new entry in the new bucket.
                    let start = self.get_start_offset_with_header((i * index_grow) as u64);
                    self.contents
                        .occupy(&mut self.mmap[start..], i * index_grow);
                }
                let old_ix = i * old_bucket.cell_size as usize;
                let new_ix = old_ix * index_grow;
                let dst_slice: &[u8] = &self.mmap[new_ix..new_ix + old_bucket.cell_size as usize];
                let src_slice: &[u8] = &old_map[old_ix..old_ix + old_bucket.cell_size as usize];

                unsafe {
                    let dst = dst_slice.as_ptr() as *mut u8;
                    let src = src_slice.as_ptr() as *const u8;
                    std::ptr::copy_nonoverlapping(src, dst, old_bucket.cell_size as usize);
                };
            }
        });
        m.stop();
        // resized so update total file size
        self.stats.resizes.fetch_add(1, Ordering::Relaxed);
        self.stats.resize_us.fetch_add(m.as_us(), Ordering::Relaxed);
    }

    pub fn update_max_size(&self) {
        self.stats.update_max_size(self.capacity());
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
        new_bucket.update_max_size();
        new_bucket
    }

    /// Return the number of bytes currently allocated
    pub(crate) fn capacity_bytes(&self) -> u64 {
        self.capacity() * self.cell_size
    }

    /// Return the number of cells currently allocated
    pub fn capacity(&self) -> u64 {
        1 << self.capacity_pow2
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{bucket_storage::BucketOccupied, index_entry::IndexBucket},
        tempfile::tempdir,
    };

    #[test]
    fn test_bucket_storage() {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());

        let mut storage = BucketStorage::<IndexBucket<u64>>::new(
            Arc::new(paths),
            1,
            std::mem::size_of::<crate::index_entry::IndexEntry<u64>>() as u64,
            1,
            Arc::default(),
            Arc::default(),
        );
        let ix = 0;
        assert!(storage.is_free(ix));
        assert!(storage.occupy(ix, false).is_ok());
        assert!(storage.occupy(ix, false).is_err());
        assert!(!storage.is_free(ix));
        storage.free(ix);
        assert!(storage.is_free(ix));
        assert!(storage.is_free(ix));
        assert!(storage.occupy(ix, false).is_ok());
        assert!(storage.occupy(ix, false).is_err());
        assert!(!storage.is_free(ix));
        storage.free(ix);
        assert!(storage.is_free(ix));
    }

    struct BucketBadHeader {}

    impl BucketOccupied for BucketBadHeader {
        fn occupy(&mut self, _element: &mut [u8], _ix: usize) {
            unimplemented!();
        }
        fn free(&mut self, _element: &mut [u8], _ix: usize) {
            unimplemented!();
        }
        fn is_free(&self, _element: &[u8], _ix: usize) -> bool {
            unimplemented!();
        }
        fn offset_to_first_data() -> usize {
            // not multiple of u64
            std::mem::size_of::<u64>() - 1
        }
        /// initialize this struct
        fn new(_num_elements: usize) -> Self {
            Self {}
        }
    }

    #[test]
    #[should_panic(expected = "assertion failed: `(left == right)`")]
    fn test_header_size() {
        _ = BucketStorage::<BucketBadHeader>::new_with_capacity(
            Arc::default(),
            0,
            0,
            0,
            0,
            Arc::default(),
            Arc::default(),
        );
    }
}
