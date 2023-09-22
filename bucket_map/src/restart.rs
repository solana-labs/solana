//! Persistent info of disk index files to allow files to be reused on restart.
#![allow(dead_code)]
use {
    crate::bucket_map::{BucketMapConfig, MAX_SEARCH_DEFAULT},
    bytemuck::{Pod, Zeroable},
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
#[derive(Debug, Pod, Zeroable, Copy, Clone)]
#[repr(C)]
pub(crate) struct Header {
    /// version of this file. Differences here indicate the file is not usable.
    version: u64,
    /// number of buckets these files represent.
    buckets: u64,
    /// u8 representing how many entries to search for during collisions.
    /// If this is different, then the contents of the index file's contents are likely not as helpful.
    max_search: u8,
    /// padding to get header to u128 aligned
    _dummy: [u8; 15],
}

// In order to safely guarantee Header is Pod, it cannot have any padding.
const _: () = assert!(
    std::mem::size_of::<Header>() == std::mem::size_of::<u128>() * 2,
    "Header cannot have any padding"
);

#[derive(Debug, Pod, Zeroable, Copy, Clone)]
#[repr(C)]
pub(crate) struct OneIndexBucket {
    /// disk bucket file names are random u128s
    file_name: u128,
    /// each bucket uses a random value to hash with pubkeys. Without this, hashing would be inconsistent between restarts.
    random: u64,
    /// padding to make u128 aligned
    _dummy: u64,
}

// In order to safely guarantee Header is Pod, it cannot have any padding.
const _: () = assert!(
    std::mem::size_of::<OneIndexBucket>() == std::mem::size_of::<u128>() * 2,
    "Header cannot have any padding"
);

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
                .map(|index| self.get_bucket(index as usize))
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
        header.buckets = config.max_buckets as u64;
        header.max_search = config.max_search.unwrap_or(MAX_SEARCH_DEFAULT);

        (0..config.max_buckets).for_each(|index| {
            let bucket = restart.get_bucket_mut(index);
            bucket.file_name = 0;
            bucket.random = 0;
        });

        Some(restart)
    }

    /// loads and mmaps restart file if it exists
    /// returns None if the file doesn't exist or is incompatible or corrupt (in obvious ways)
    pub(crate) fn get_restart_file(config: &BucketMapConfig) -> Option<Restart> {
        let path = config.restart_config_file.as_ref()?;
        let metadata = std::fs::metadata(path).ok()?;
        let file_len = metadata.len();

        let expected_len = Self::expected_len(config.max_buckets);
        if expected_len as u64 != file_len {
            // mismatched len, so ignore this file
            return None;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(path)
            .ok()?;
        let mmap = unsafe { MmapMut::map_mut(&file).unwrap() };

        let restart = Restart { mmap };
        let header = restart.get_header();
        if header.version != HEADER_VERSION
            || header.buckets != config.max_buckets as u64
            || header.max_search != config.max_search.unwrap_or(MAX_SEARCH_DEFAULT)
        {
            // file doesn't match our current configuration, so we have to restart with fresh buckets
            return None;
        }

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

        if capacity > 0 {
            // Theoretical performance optimization: write a zero to the end of
            // the file so that we won't have to resize it later, which may be
            // expensive.
            data.seek(SeekFrom::Start(capacity - 1)).unwrap();
            data.write_all(&[0]).unwrap();
            data.rewind().unwrap();
        }
        data.flush().unwrap();
        Ok(unsafe { MmapMut::map_mut(&data).unwrap() })
    }

    fn get_header(&self) -> &Header {
        let item_slice = &self.mmap[..std::mem::size_of::<Header>()];
        bytemuck::from_bytes(item_slice)
    }

    fn get_header_mut(&mut self) -> &mut Header {
        let bytes = &mut self.mmap[..std::mem::size_of::<Header>()];
        bytemuck::from_bytes_mut(bytes)
    }

    fn get_bucket(&self, index: usize) -> &OneIndexBucket {
        let record_len = std::mem::size_of::<OneIndexBucket>();
        let start = std::mem::size_of::<Header>() + record_len * index;
        let end = start + record_len;
        let item_slice: &[u8] = &self.mmap[start..end];
        bytemuck::from_bytes(item_slice)
    }

    fn get_bucket_mut(&mut self, index: usize) -> &mut OneIndexBucket {
        let record_len = std::mem::size_of::<OneIndexBucket>();
        let start = std::mem::size_of::<Header>() + record_len * index;
        let end = start + record_len;
        let item_slice: &mut [u8] = &mut self.mmap[start..end];
        bytemuck::from_bytes_mut(item_slice)
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
    fn test_restartable_bucket_load() {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());
        let config_file = tmpdir.path().join("config");

        for bucket_pow in [1, 2] {
            let config = BucketMapConfig {
                drives: Some(paths.clone()),
                restart_config_file: Some(config_file.clone()),
                ..BucketMapConfig::new(1 << bucket_pow)
            };

            // create file
            let restart = Arc::new(Mutex::new(Restart::new(&config).unwrap()));
            test_default_restart(&restart, &config);
            drop(restart);

            // successful open
            let restart = Restart::get_restart_file(&config);
            assert!(restart.is_some());
            drop(restart);

            // unsuccessful: buckets wrong
            let config_wrong_buckets = BucketMapConfig {
                drives: Some(paths.clone()),
                restart_config_file: Some(config_file.clone()),
                ..BucketMapConfig::new(1 << (bucket_pow + 1))
            };
            let restart = Restart::get_restart_file(&config_wrong_buckets);
            assert!(restart.is_none());

            // unsuccessful: max search wrong
            let config_wrong_buckets = BucketMapConfig {
                max_search: Some(MAX_SEARCH_DEFAULT + 1),
                drives: Some(paths.clone()),
                restart_config_file: Some(config_file.clone()),
                ..BucketMapConfig::new(1 << bucket_pow)
            };
            let restart = Restart::get_restart_file(&config_wrong_buckets);
            assert!(restart.is_none());

            // create file with different header
            let restart = Arc::new(Mutex::new(Restart::new(&config).unwrap()));
            test_default_restart(&restart, &config);
            restart.lock().unwrap().get_header_mut().version = HEADER_VERSION + 1;
            drop(restart);
            // unsuccessful: header wrong
            let restart = Restart::get_restart_file(&config);
            assert!(restart.is_none());

            // file 0 len
            let wrong_file_len = 0;
            let path = config.restart_config_file.as_ref();
            let path = path.unwrap();
            _ = remove_file(path);
            let mmap = Restart::new_map(path, wrong_file_len as u64).unwrap();
            drop(mmap);
            // unsuccessful: header wrong
            let restart = Restart::get_restart_file(&config);
            assert!(restart.is_none());

            // file too big or small
            for smaller_bigger in [0, 1, 2] {
                let wrong_file_len = Restart::expected_len(config.max_buckets) - 1 + smaller_bigger;
                let path = config.restart_config_file.as_ref();
                let path = path.unwrap();
                _ = remove_file(path);
                let mmap = Restart::new_map(path, wrong_file_len as u64).unwrap();
                let mut restart = Restart { mmap };
                let header = restart.get_header_mut();
                header.version = HEADER_VERSION;
                header.buckets = config.max_buckets as u64;
                header.max_search = config.max_search.unwrap_or(MAX_SEARCH_DEFAULT);
                drop(restart);
                // unsuccessful: header wrong
                let restart = Restart::get_restart_file(&config);
                // 0, 2 are wrong, 1 is right
                assert_eq!(restart.is_none(), smaller_bigger != 1);
            }
        }
    }

    #[test]
    fn test_restartable_bucket() {
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
        test_default_restart(&restart, &config);
        let last_offset = 1;
        (0..=last_offset).for_each(|offset| test_set_get(&restart, buckets, offset));
        // drop file (as if process exit)
        drop(restart);
        // re-load file (as if at next launch)
        let restart = Arc::new(Mutex::new(Restart::get_restart_file(&config).unwrap()));
        // make sure same as last set prior to reload
        test_get(&restart, buckets, last_offset);
        (4..6).for_each(|offset| test_set_get(&restart, buckets, offset));
        drop(restart);
        // create a new file without deleting old one. Make sure it is default and not re-used.
        let restart = Arc::new(Mutex::new(Restart::new(&config).unwrap()));
        test_default_restart(&restart, &config);
    }

    fn test_set_get(restart: &Arc<Mutex<Restart>>, buckets: usize, test_offset: usize) {
        test_set(restart, buckets, test_offset);
        test_get(restart, buckets, test_offset);
    }

    fn test_set(restart: &Arc<Mutex<Restart>>, buckets: usize, test_offset: usize) {
        (0..buckets).for_each(|bucket| {
            let restartable_bucket = RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            };
            assert!(restartable_bucket.get().is_some());
            let file_name = bucket as u128 + test_offset as u128;
            let random = (file_name as u64 + 5) * 2;
            restartable_bucket.set_file(file_name, random);
            assert_eq!(restartable_bucket.get(), Some((file_name, random)));
        });
    }

    fn test_get(restart: &Arc<Mutex<Restart>>, buckets: usize, test_offset: usize) {
        (0..buckets).for_each(|bucket| {
            let restartable_bucket = RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            };
            let file_name = bucket as u128 + test_offset as u128;
            let random = (file_name as u64 + 5) * 2;
            assert_eq!(restartable_bucket.get(), Some((file_name, random)));
        });
    }

    /// make sure restart is default values we expect
    fn test_default_restart(restart: &Arc<Mutex<Restart>>, config: &BucketMapConfig) {
        {
            let restart = restart.lock().unwrap();
            let header = restart.get_header();
            assert_eq!(header.version, HEADER_VERSION);
            assert_eq!(header.buckets, config.max_buckets as u64);
            assert_eq!(
                header.max_search,
                config.max_search.unwrap_or(MAX_SEARCH_DEFAULT)
            );
        }

        let buckets = config.max_buckets;
        (0..buckets).for_each(|bucket| {
            let restartable_bucket = RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            };
            // default values
            assert_eq!(restartable_bucket.get(), Some((0, 0)));
        });
    }
}
