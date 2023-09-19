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
pub trait BucketOccupied: BucketCapacity {
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
    /// `capacity` is the number of elements allocated in the bucket
    fn new(capacity: Capacity) -> Self;
    /// copying entry. Any in-memory (per-bucket) data structures may need to be copied for this `ix_old`.
    /// no-op by default
    fn copying_entry(
        &mut self,
        _element_new: &mut [u8],
        _ix_new: usize,
        _other: &Self,
        _element_old: &[u8],
        _ix_old: usize,
    ) {
    }
}

pub trait BucketCapacity {
    fn capacity(&self) -> u64;
    fn capacity_pow2(&self) -> u8 {
        unimplemented!();
    }
}

pub struct BucketStorage<O: BucketOccupied> {
    path: PathBuf,
    mmap: MmapMut,
    pub cell_size: u64,
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
        self.delete();
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub(crate) enum IncludeHeader {
    /// caller wants header included
    Header,
    /// caller wants header skipped
    NoHeader,
}

/// 2 common ways of specifying capacity
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Capacity {
    /// 1 << Pow2 produces # elements
    Pow2(u8),
    /// Actual # elements
    Actual(u64),
}

impl BucketCapacity for Capacity {
    fn capacity(&self) -> u64 {
        match self {
            Capacity::Pow2(pow2) => 1 << *pow2,
            Capacity::Actual(elements) => *elements,
        }
    }
    fn capacity_pow2(&self) -> u8 {
        match self {
            Capacity::Pow2(pow2) => *pow2,
            Capacity::Actual(_elements) => {
                panic!("illegal to ask for pow2 from random capacity");
            }
        }
    }
}

impl<O: BucketOccupied> BucketStorage<O> {
    /// allocate a bucket of at least `capacity` elements.
    /// if capacity can be random, more may be allocated to fill the last page.
    pub fn new_with_capacity(
        drives: Arc<Vec<PathBuf>>,
        num_elems: u64,
        elem_size: u64,
        mut capacity: Capacity,
        max_search: MaxSearch,
        stats: Arc<BucketStats>,
        count: Arc<AtomicU64>,
    ) -> (Self, u128) {
        let offset = O::offset_to_first_data();
        let size_of_u64 = std::mem::size_of::<u64>();
        assert_eq!(
            offset / size_of_u64 * size_of_u64,
            offset,
            "header size must be a multiple of u64"
        );
        let cell_size = elem_size * num_elems + offset as u64;
        let bytes = Self::allocate_to_fill_page(&mut capacity, cell_size);
        let (mmap, path, file_name) = Self::new_map(&drives, bytes, &stats);
        (
            Self {
                path,
                mmap,
                cell_size,
                count,
                stats,
                max_search,
                contents: O::new(capacity),
            },
            file_name,
        )
    }

    fn allocate_to_fill_page(capacity: &mut Capacity, cell_size: u64) -> u64 {
        let mut bytes = capacity.capacity() * cell_size;
        if let Capacity::Actual(_) = capacity {
            // maybe bump up allocation to fit a page size
            const PAGE_SIZE: u64 = 4 * 1024;
            let full_page_bytes = bytes / PAGE_SIZE * PAGE_SIZE / cell_size * cell_size;
            if full_page_bytes < bytes {
                let bytes_new = ((bytes / PAGE_SIZE) + 1) * PAGE_SIZE / cell_size * cell_size;
                assert!(bytes_new >= bytes, "allocating less than requested, capacity: {}, bytes: {}, bytes_new: {}, full_page_bytes: {}", capacity.capacity(), bytes, bytes_new, full_page_bytes);
                assert_eq!(bytes_new % cell_size, 0);
                bytes = bytes_new;
                *capacity = Capacity::Actual(bytes / cell_size);
            }
        }
        bytes
    }

    /// delete the backing file on disk
    fn delete(&self) {
         _ = remove_file(&self.path);
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
    ) -> (Self, u128) {
        Self::new_with_capacity(
            drives,
            num_elems,
            elem_size,
            Capacity::Pow2(DEFAULT_CAPACITY_POW2),
            max_search,
            stats,
            count,
        )
    }

    pub(crate) fn copying_entry(&mut self, ix_new: u64, other: &Self, ix_old: u64) {
        let start = self.get_start_offset_with_header(ix_new);
        let start_old = other.get_start_offset_with_header(ix_old);
        self.contents.copying_entry(
            &mut self.mmap[start..],
            ix_new as usize,
            &other.contents,
            &other.mmap[start_old..],
            ix_old as usize,
        );
    }

    /// true if the entry at index 'ix' is free (as opposed to being occupied)
    pub fn is_free(&self, ix: u64) -> bool {
        let start = self.get_start_offset_with_header(ix);
        let entry = &self.mmap[start..];
        self.contents.is_free(entry, ix as usize)
    }

    /// try to occupy `ix`. return true if successful
    pub(crate) fn try_lock(&mut self, ix: u64) -> bool {
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
        debug_assert!(ix < self.capacity(), "occupy: bad index size");
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
        debug_assert!(ix < self.capacity(), "bad index size");
        let start = self.get_start_offset_with_header(ix);
        self.contents.free(&mut self.mmap[start..], ix as usize);
        self.count.fetch_sub(1, Ordering::Relaxed);
    }

    fn get_start_offset_with_header(&self, ix: u64) -> usize {
        debug_assert!(ix < self.capacity(), "bad index size");
        (self.cell_size * ix) as usize
    }

    fn get_start_offset(&self, ix: u64, header: IncludeHeader) -> usize {
        self.get_start_offset_with_header(ix)
            + match header {
                IncludeHeader::Header => 0,
                IncludeHeader::NoHeader => O::offset_to_first_data(),
            }
    }

    pub(crate) fn get_header<T>(&self, ix: u64) -> &T {
        let slice = self.get_slice::<T>(ix, 1, IncludeHeader::Header);
        // SAFETY: `get_cell_slice` ensures there's at least one element in the slice
        unsafe { slice.get_unchecked(0) }
    }

    pub(crate) fn get_header_mut<T>(&mut self, ix: u64) -> &mut T {
        let slice = self.get_slice_mut::<T>(ix, 1, IncludeHeader::Header);
        // SAFETY: `get_mut_cell_slice` ensures there's at least one element in the slice
        unsafe { slice.get_unchecked_mut(0) }
    }

    pub(crate) fn get<T>(&self, ix: u64) -> &T {
        let slice = self.get_slice::<T>(ix, 1, IncludeHeader::NoHeader);
        // SAFETY: `get_cell_slice` ensures there's at least one element in the slice
        unsafe { slice.get_unchecked(0) }
    }

    pub(crate) fn get_mut<T>(&mut self, ix: u64) -> &mut T {
        let slice = self.get_slice_mut::<T>(ix, 1, IncludeHeader::NoHeader);
        // SAFETY: `get_mut_cell_slice` ensures there's at least one element in the slice
        unsafe { slice.get_unchecked_mut(0) }
    }

    pub(crate) fn get_slice<T>(&self, ix: u64, len: u64, header: IncludeHeader) -> &[T] {
        // If the caller is including the header, then `len` *must* be 1
        debug_assert!(
            (header == IncludeHeader::NoHeader) || (header == IncludeHeader::Header && len == 1)
        );
        let start = self.get_start_offset(ix, header);
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

    pub(crate) fn get_slice_mut<T>(
        &mut self,
        ix: u64,
        len: u64,
        header: IncludeHeader,
    ) -> &mut [T] {
        // If the caller is including the header, then `len` *must* be 1
        debug_assert!(
            (header == IncludeHeader::NoHeader) || (header == IncludeHeader::Header && len == 1)
        );
        let start = self.get_start_offset(ix, header);
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
    fn new_map(drives: &[PathBuf], bytes: u64, stats: &BucketStats) -> (MmapMut, PathBuf, u128) {
        let mut measure_new_file = Measure::start("measure_new_file");
        let r = thread_rng().gen_range(0..drives.len());
        let drive = &drives[r];
        let file_random = thread_rng().gen_range(0..u128::MAX);
        let pos = format!("{}", file_random,);
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
        let res = (
            unsafe { MmapMut::map_mut(&data).unwrap() },
            file,
            file_random,
        );
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

        let increment = self.contents.capacity_pow2() - old_bucket.contents.capacity_pow2();
        let index_grow = 1 << increment;
        (0..old_cap as usize).for_each(|i| {
            if !old_bucket.is_free(i as u64) {
                self.copying_entry((i * index_grow) as u64, old_bucket, i as u64);

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
                    let src = src_slice.as_ptr();
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
        capacity: Capacity,
        num_elems: u64,
        elem_size: u64,
        stats: &Arc<BucketStats>,
    ) -> (Self, u128) {
        let (mut new_bucket, file_name) = Self::new_with_capacity(
            Arc::clone(drives),
            num_elems,
            elem_size,
            capacity,
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
        (new_bucket, file_name)
    }

    /// Return the number of bytes currently allocated
    pub(crate) fn capacity_bytes(&self) -> u64 {
        self.capacity() * self.cell_size
    }

    /// Return the number of cells currently allocated
    pub fn capacity(&self) -> u64 {
        self.contents.capacity()
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
        )
        .0;
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

    #[test]
    #[should_panic]
    fn test_header_bad_size() {
        struct BucketBadHeader;
        impl BucketCapacity for BucketBadHeader {
            fn capacity(&self) -> u64 {
                unimplemented!();
            }
        }
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
            fn new(_num_elements: Capacity) -> Self {
                Self
            }
        }

        // ensure we panic if the header size (i.e. offset to first data) is not aligned to eight bytes
        BucketStorage::<BucketBadHeader>::new_with_capacity(
            Arc::default(),
            0,
            0,
            Capacity::Pow2(0),
            0,
            Arc::default(),
            Arc::default(),
        );
    }
}
