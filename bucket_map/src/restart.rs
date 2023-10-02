//! Persistent info of disk index files to allow files to be reused on restart.
use {
    crate::bucket_map::{BucketMapConfig, MAX_SEARCH_DEFAULT},
    bytemuck::{Pod, Zeroable},
    memmap2::MmapMut,
    std::{
        collections::HashMap,
        fmt::{Debug, Formatter},
        fs::{self, remove_file, OpenOptions},
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
    /// padding to make size of Header be an even multiple of u128
    _dummy: [u8; 15],
}

// In order to safely guarantee Header is Pod, it cannot have any padding.
const _: () = assert!(
    std::mem::size_of::<Header>() == std::mem::size_of::<u128>() * 2,
    "incorrect size of header struct"
);

#[derive(Debug, Pod, Zeroable, Copy, Clone)]
#[repr(C)]
pub(crate) struct OneIndexBucket {
    /// disk bucket file names are random u128s
    file_name: u128,
    /// each bucket uses a random value to hash with pubkeys. Without this, hashing would be inconsistent between restarts.
    random: u64,
    /// padding to make size of OneIndexBucket be an even multiple of u128
    _dummy: u64,
}

// In order to safely guarantee Header is Pod, it cannot have any padding.
const _: () = assert!(
    std::mem::size_of::<OneIndexBucket>() == std::mem::size_of::<u128>() * 2,
    "incorrect size of header struct"
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

    /// return all files that matched bucket files in `drives`
    /// matching files will be parsable as u128
    fn get_all_possible_index_files_in_drives(drives: &[PathBuf]) -> HashMap<u128, PathBuf> {
        let mut result = HashMap::default();
        drives.iter().for_each(|drive| {
            if drive.is_dir() {
                let dir = fs::read_dir(drive);
                if let Ok(dir) = dir {
                    for entry in dir.flatten() {
                        if let Some(name) = entry.path().file_name() {
                            if let Some(id) = name.to_str().and_then(|str| str.parse::<u128>().ok())
                            {
                                result.insert(id, entry.path());
                            }
                        }
                    }
                }
            }
        });
        result
    }

    /// get one `RestartableBucket` for each bucket.
    /// If a potentially reusable file exists, then put that file's path in `RestartableBucket` for that bucket.
    /// Delete all files that cannot possibly be re-used.
    pub(crate) fn get_restartable_buckets(
        restart: Option<&Arc<Mutex<Restart>>>,
        drives: &Arc<Vec<PathBuf>>,
        num_buckets: usize,
    ) -> Vec<RestartableBucket> {
        let mut paths = Self::get_all_possible_index_files_in_drives(drives);
        let results = (0..num_buckets)
            .map(|index| {
                let path = restart.and_then(|restart| {
                    let restart = restart.lock().unwrap();
                    let id = restart.get_bucket(index).file_name;
                    paths.remove(&id)
                });
                RestartableBucket {
                    restart: restart.map(Arc::clone),
                    index,
                    path,
                }
            })
            .collect();

        paths.into_iter().for_each(|path| {
            // delete any left over files that we won't be using
            _ = fs::remove_file(path.1);
        });

        results
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
    fn test_get_restartable_buckets() {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());
        let config_file = tmpdir.path().join("config");

        let config = BucketMapConfig {
            drives: Some(paths.clone()),
            restart_config_file: Some(config_file.clone()),
            ..BucketMapConfig::new(1 << 2)
        };

        // create restart file
        let restart = Arc::new(Mutex::new(Restart::new(&config).unwrap()));
        let files = Restart::get_all_possible_index_files_in_drives(&paths);
        assert!(files.is_empty());

        let restartable_buckets = (0..config.max_buckets)
            .map(|bucket| RestartableBucket {
                restart: Some(restart.clone()),
                index: bucket,
                path: None,
            })
            .collect::<Vec<_>>();

        let skip = 2; // skip this file
                      // note starting at 1 to avoid default values of 0 for file_name
                      // create 4 bucket files.
                      // 1,3,4 will match buckets 0,2,3
                      // 5 is an extra file that will get deleted
        (0..config.max_buckets + 1).for_each(|i| {
            if i == skip {
                return;
            }
            let file_name = (i + 1) as u128;
            let random = (i * 2) as u64;
            let file = tmpdir.path().join(file_name.to_string());
            create_dummy_file(&file);

            // bucket is connected to this file_name
            if i < config.max_buckets {
                restartable_buckets[i].set_file(file_name, random);
                assert_eq!(Some((file_name, random)), restartable_buckets[i].get());
            }
        });

        let deleted_file = tmpdir.path().join((1 + config.max_buckets).to_string());
        assert!(std::fs::metadata(deleted_file.clone()).is_ok());
        let calc_restartable_buckets = Restart::get_restartable_buckets(
            Some(&restart),
            &Arc::new(paths.clone()),
            config.max_buckets,
        );

        // make sure all bucket files were associated correctly
        // and all files still exist
        (0..config.max_buckets).for_each(|i| {
            if i == skip {
                assert_eq!(Some((0, 0)), restartable_buckets[i].get());
                assert_eq!(None, calc_restartable_buckets[i].path);
            } else {
                let file_name = (i + 1) as u128;
                let random = (i * 2) as u64;
                let expected_path = tmpdir.path().join(file_name.to_string());
                assert!(std::fs::metadata(expected_path.clone()).is_ok());

                assert_eq!(Some((file_name, random)), restartable_buckets[i].get());
                assert_eq!(Some(expected_path), calc_restartable_buckets[i].path);
            }
        });

        // this file wasn't associated with a bucket
        assert!(
            std::fs::metadata(deleted_file).is_err(),
            "should have been deleted"
        );
    }

    fn create_dummy_file(path: &Path) {
        // easy enough to create a test file in the right spot with creating a 'restart' file of a given name.
        let config = BucketMapConfig {
            drives: None,
            restart_config_file: Some(path.to_path_buf()),
            ..BucketMapConfig::new(1 << 1)
        };

        // create file
        assert!(Restart::new(&config).is_some());
    }

    #[test]
    fn test_get_all_possible_index_files_in_drives() {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());
        create_dummy_file(&tmpdir.path().join("config"));

        // create a file with a valid u128 name
        for file_name in [u128::MAX, 0, 123] {
            let file = tmpdir.path().join(file_name.to_string());
            create_dummy_file(&file);
            let mut files = Restart::get_all_possible_index_files_in_drives(&paths);
            assert_eq!(files.remove(&file_name), Some(&file).cloned());
            assert!(files.is_empty());
            _ = fs::remove_file(&file);
        }

        // create a file with a u128 name that fails to convert
        for file_name in [
            u128::MAX.to_string() + ".",
            u128::MAX.to_string() + "0",
            "-123".to_string(),
        ] {
            let file = tmpdir.path().join(file_name);
            create_dummy_file(&file);
            let files = Restart::get_all_possible_index_files_in_drives(&paths);
            assert!(files.is_empty(), "{files:?}");
            _ = fs::remove_file(&file);
        }

        // 2 drives, 2 files in each
        // create 2nd tmpdir (ie. drive)
        let tmpdir2 = tempdir().unwrap();
        let paths2: Vec<PathBuf> =
            vec![paths.first().unwrap().clone(), tmpdir2.path().to_path_buf()];
        (0..4).for_each(|i| {
            let parent = if i < 2 { &tmpdir } else { &tmpdir2 };
            let file = parent.path().join(i.to_string());
            create_dummy_file(&file);
        });

        let mut files = Restart::get_all_possible_index_files_in_drives(&paths);
        assert_eq!(files.len(), 2);
        (0..2).for_each(|file_name| {
            let path = files.remove(&file_name).unwrap();
            assert_eq!(tmpdir.path().join(file_name.to_string()), path);
        });
        let mut files = Restart::get_all_possible_index_files_in_drives(&paths2);
        assert_eq!(files.len(), 4);
        (0..2).for_each(|file_name| {
            let path = files.remove(&file_name).unwrap();
            assert_eq!(tmpdir.path().join(file_name.to_string()), path);
        });
        (2..4).for_each(|file_name| {
            let path = files.remove(&file_name).unwrap();
            assert_eq!(tmpdir2.path().join(file_name.to_string()), path);
        });
        assert!(files.is_empty());
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
