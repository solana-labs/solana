use log::*;
use memmap2::MmapMut;
use rand::{thread_rng, Rng};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::fs::{remove_file, OpenOptions};
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const DEFAULT_CAPACITY: u64 = 16;

#[repr(C)]
struct Header {
    lock: AtomicU64,
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

pub struct DataBucket {
    drives: Arc<Vec<PathBuf>>,
    path: PathBuf,
    mmap: MmapMut,
    pub cell_size: usize,
    pub capacity: u64,
    pub used: AtomicU64,
}

#[derive(Debug)]
pub enum DataBucketError {
    AlreadyAllocated,
}

impl Drop for DataBucket {
    fn drop(&mut self) {
        if let Err(e) = remove_file(&self.path) {
            error!("failed to remove {:?}: {:?}", &self.path, e);
        }
    }
}

impl DataBucket {
    pub fn new(drives: Arc<Vec<PathBuf>>, elem_size: usize) -> Self {
        let cell_size = elem_size + std::mem::size_of::<Header>();
        let (mmap, path) = Self::new_map(&drives, cell_size, DEFAULT_CAPACITY);
        Self {
            path,
            mmap,
            drives,
            cell_size,
            used: AtomicU64::new(0),
            capacity: DEFAULT_CAPACITY,
        }
    }

    pub fn uid(&self, ix: u64) -> u64 {
        if ix >= self.capacity {
            panic!("bad index size");
        }
        let ix = ix as usize * self.cell_size;
        let hdr_slice = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            return hdr.as_ref().unwrap().uid();
        }
    }

    pub fn allocate(&self, ix: u64, uid: u64) -> Result<(), DataBucketError> {
        if ix >= self.capacity {
            panic!("bad index size");
        }
        let mut e = Err(DataBucketError::AlreadyAllocated);
        let ix = ix as usize * self.cell_size;
        let hdr_slice = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            if hdr.as_ref().unwrap().try_lock(uid) {
                e = Ok(());
                self.used.fetch_add(1, Ordering::Relaxed);
            }
        };
        e
    }

    pub fn free(&self, ix: u64, uid: u64) {
        if ix >= self.capacity {
            panic!("bad index size");
        }
        let ix = ix as usize * self.cell_size;
        let hdr_slice = &self.mmap[ix..ix + std::mem::size_of::<Header>()];
        unsafe {
            let hdr = hdr_slice.as_ptr() as *const Header;
            if hdr.as_ref().unwrap().unlock(uid) {
                self.used.fetch_sub(1, Ordering::Relaxed);
            }
        };
    }

    pub fn get<T: Sized + AsRef<T>>(&self, ix: u64) -> &T {
        if ix >= self.capacity {
            panic!("bad index size");
        }
        let start = ix as usize * self.cell_size + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>();
        let item_slice = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *const T;
            return item.as_ref().unwrap();
        };
    }

    pub fn get_mut<T: Sized + AsMut<T>>(&self, ix: u64) -> &mut T {
        if ix >= self.capacity {
            panic!("bad index size");
        }
        let start = ix as usize * self.cell_size + std::mem::size_of::<Header>();
        let end = start + std::mem::size_of::<T>();
        let item_slice = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *mut T;
            return item.as_mut().unwrap();
        };
    }

    fn new_map(drives: &[PathBuf], cell_size: usize, capacity: u64) -> (MmapMut, PathBuf) {
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
        data.seek(SeekFrom::Start(capacity * cell_size as u64 - 1))
            .unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        data.flush().unwrap();
        (unsafe { MmapMut::map_mut(&data).unwrap() }, file)
    }

    pub fn grow(&mut self) {
        let old_cap = self.capacity;
        let old_map = &self.mmap;
        let old_file = self.path.clone();
        let (new_map, new_file) = Self::new_map(&self.drives, self.cell_size, self.capacity * 2);
        (0..old_cap as usize).into_par_iter().for_each(|i| {
            let old_ix = i * self.cell_size;
            let new_ix = old_ix * 2;
            let dst_slice = &new_map[new_ix..new_ix + self.cell_size];
            let src_slice = &old_map[old_ix..old_ix + self.cell_size];

            unsafe {
                let dst = dst_slice.as_ptr() as *mut u8;
                let src = src_slice.as_ptr() as *const u8;
                std::ptr::copy(src, dst, self.cell_size);
            };
        });
        self.mmap = new_map;
        self.path = new_file;
        self.capacity = self.capacity * 2;
        remove_file(old_file).unwrap();
    }
}
