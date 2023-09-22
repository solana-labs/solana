//! Persistent info of disk index files to allow files to be reused on restart.
#![allow(dead_code)]
use {
    crate::bucket_map::{BucketMapConfig, MAX_SEARCH_DEFAULT},
    memmap2::MmapMut,
    std::{
        fmt::{Debug, Formatter},
        fs::{remove_file, OpenOptions},
        io::{Seek, SeekFrom, Write},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    },
};

/// written into file. Change this if expected file contents change.
const HEADER_VERSION: u64 = 1;

/// written into file at top.
#[derive(Debug)]
#[repr(C)]
pub(crate) struct Header {
    /// version of this file. Differences here indicate the file is not usable.
    version: u64,
    /// number of buckets these files represent.
    buckets: usize,
    /// u8 representing how many entries to search for during collisions.
    /// If this is different, then the contents of the index file's contents are likely not as helpful.
    max_search: u8,
    /// padding to get header to u128 aligned
    _dummy: [u8; 15],
}

#[derive(Debug)]
#[repr(C)]
pub(crate) struct OneIndexBucket {
    /// disk bucket file names are random u128s
    file_name: u128,
    /// each bucket uses a random value to hash with pubkeys. Without this, hashing would be inconsistent between restarts.
    random: u64,
    /// padding to make u128 aligned
    _dummy: u64,
}

pub(crate) struct Restart {
    mmap: MmapMut,
}

#[derive(Clone, Default)]
/// keep track of mapping from a single bucket to the shared mmap file
pub(crate) struct RestartableBucket {
    /// shared struct keeping track of each bucket's file
    pub(crate) restart: Option<Arc<Mutex<Restart>>>,
    /// which index self represents inside `restart`
    pub(crate) index: usize,
    /// path disk index file is at for startup
    pub(crate) path: Option<PathBuf>,
}

impl RestartableBucket {
    /// this bucket is now using `file_name` and `random`.
    /// This gets written into the restart file so that on restart we can re-open the file and re-hash with the same random.
    pub(crate) fn set_file(&self, file_name: u128, random: u64) {
        if let Some(mut restart) = self.restart.as_ref().map(|restart| restart.lock().unwrap()) {
            let bucket = restart.get_bucket_mut(self.index);
            bucket.file_name = file_name;
            bucket.random = random;
        }
    }
    /// retreive the file_name and random that were used prior to the current restart.
    /// This was written into the restart file on the prior run by `set_file`.
    pub(crate) fn get(&self) -> Option<(u128, u64)> {
        self.restart.as_ref().map(|restart| {
            let restart = restart.lock().unwrap();
            let bucket = restart.get_bucket(self.index);
            (bucket.file_name, bucket.random)
        })
    }
}

impl Debug for RestartableBucket {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{:?}",
            &self.restart.as_ref().map(|restart| restart.lock().unwrap())
        )?;
        Ok(())
    }
}

impl Debug for Restart {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let header = self.get_header();
        writeln!(f, "{:?}", header)?;
        write!(
            f,
            "{:?}",
            (0..header.buckets)
                .map(|index| self.get_bucket(index))
                .take(10)
                .collect::<Vec<_>>()
        )?;
        Ok(())
    }
}

impl Restart {
    /// create a new restart file for use next time we restart on this machine
    pub(crate) fn new(config: &BucketMapConfig) -> Option<Restart> {
        let expected_len = Self::expected_len(config.max_buckets);

        let path = config.restart_config_file.as_ref();
        let path = path?;
        _ = remove_file(path);

        let mmap = Self::new_map(path, expected_len as u64).ok()?;

        let mut restart = Restart { mmap };
        let header = restart.get_header_mut();
        header.version = HEADER_VERSION;
        header.buckets = config.max_buckets;
        header.max_search = config.max_search.unwrap_or(MAX_SEARCH_DEFAULT);

        (0..config.max_buckets).for_each(|index| {
            let bucket = restart.get_bucket_mut(index);
            bucket.file_name = 0;
            bucket.random = 0;
        });

        Some(restart)
    }

    /// expected len of file given this many buckets
    fn expected_len(max_buckets: usize) -> usize {
        std::mem::size_of::<Header>() + max_buckets * std::mem::size_of::<OneIndexBucket>()
    }

    /// create mmap from `file`
    fn new_map(file: impl AsRef<Path>, capacity: u64) -> Result<MmapMut, std::io::Error> {
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
        data.rewind().unwrap();
        data.flush().unwrap();
        Ok(unsafe { MmapMut::map_mut(&data).unwrap() })
    }

    fn get_header(&self) -> &Header {
        let start = 0_usize;
        let end = start + std::mem::size_of::<Header>();
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *const Header;
            &*item
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

    fn get_bucket(&self, index: usize) -> &OneIndexBucket {
        let record_len = std::mem::size_of::<OneIndexBucket>();
        let start = std::mem::size_of::<Header>() + record_len * index;
        let end = start + record_len;
        let item_slice: &[u8] = &self.mmap[start..end];
        unsafe {
            let item = item_slice.as_ptr() as *const OneIndexBucket;
            &*item
        }
    }

    fn get_bucket_mut(&mut self, index: usize) -> &mut OneIndexBucket {
        let record_len = std::mem::size_of::<OneIndexBucket>();
        let start = std::mem::size_of::<Header>() + record_len * index;
        let end = start + record_len;
        let item_slice: &mut [u8] = &mut self.mmap[start..end];
        unsafe {
            let item = item_slice.as_mut_ptr() as *mut OneIndexBucket;
            &mut *item
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, tempfile::tempdir};

    #[test]
    fn test_header_alignment() {
        assert_eq!(
            0,
            std::mem::size_of::<Header>() % std::mem::size_of::<u128>()
        );
        assert_eq!(
            0,
            std::mem::size_of::<OneIndexBucket>() % std::mem::size_of::<u128>()
        );
    }

    #[test]
    fn test_restartable_bucket() {
        solana_logger::setup();
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());
        let config_file = tmpdir.path().join("config");

        let config = BucketMapConfig {
            drives: Some(paths),
            restart_config_file: Some(config_file),
            ..BucketMapConfig::new(1 << 1)
        };
        let buckets = config.max_buckets;
        let restart = Arc::new(Mutex::new(Restart::new(&config).unwrap()));
        (0..buckets).for_each(|bucket| {
            let restartable_bucket = RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            };
            // default values
            assert_eq!(restartable_bucket.get(), Some((0, 0)));
        });
        (0..buckets).for_each(|bucket| {
            let restartable_bucket = RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            };
            assert!(restartable_bucket.get().is_some());
            let file_name = bucket as u128;
            let random = (bucket as u64 + 5) * 2;
            restartable_bucket.set_file(bucket as u128, random);
            assert_eq!(restartable_bucket.get(), Some((file_name, random)));
        });
        (0..buckets).for_each(|bucket| {
            let restartable_bucket = RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            };
            let file_name = bucket as u128;
            let random = (bucket as u64 + 5) * 2;
            assert_eq!(restartable_bucket.get(), Some((file_name, random)));
        });
    }
}
