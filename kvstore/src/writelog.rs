use crate::error::Result;
use crate::io_utils::{CRCReader, CRCWriter};
use crate::sstable::Value;
use crate::Key;
use memmap::Mmap;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

// RocksDb's log uses this size.
// May be worth making configurable and experimenting
const BLOCK_SIZE: usize = 32 * 1024;

#[derive(Debug)]
pub struct WriteLog {
    log_path: PathBuf,
    logger: Logger,
    config: Config,
    in_memory: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub use_fsync: bool,
    pub sync_every_write: bool,
}

impl WriteLog {
    pub fn open(path: &Path, config: Config) -> Result<Self> {
        let file = file_opts().open(path)?;

        Ok(WriteLog {
            config,
            log_path: path.to_path_buf(),
            logger: Logger::disk(file),
            in_memory: false,
        })
    }

    #[allow(dead_code)]
    pub fn memory(config: Config) -> WriteLog {
        WriteLog {
            config,
            logger: Logger::memory(),
            log_path: Path::new("").to_path_buf(),
            in_memory: true,
        }
    }

    pub fn reset(&mut self) -> Result<()> {
        let new_logger = if self.in_memory {
            Logger::memory()
        } else {
            let file = file_opts().truncate(true).open(&self.log_path)?;
            Logger::disk(file)
        };

        self.logger = new_logger;

        Ok(())
    }

    pub fn log_put(&mut self, key: &Key, ts: i64, val: &[u8]) -> Result<()> {
        log(&mut self.logger, key, ts, Some(val))?;

        if self.config.sync_every_write {
            sync(&mut self.logger, self.config.use_fsync)?;
        }

        Ok(())
    }

    pub fn log_delete(&mut self, key: &Key, ts: i64) -> Result<()> {
        log(&mut self.logger, key, ts, None)?;

        if self.config.sync_every_write {
            sync(&mut self.logger, self.config.use_fsync)?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn sync(&mut self) -> Result<()> {
        sync(&mut self.logger, self.config.use_fsync)
    }

    pub fn materialize(&mut self) -> Result<BTreeMap<Key, Value>> {
        let mmap = self.logger.writer.mmap()?;
        read_log(&mmap)
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            use_fsync: false,
            sync_every_write: true,
        }
    }
}

trait LogWriter: std::fmt::Debug + Write + Send + Sync {
    fn sync(&mut self, fsync: bool) -> Result<()>;
    fn mmap(&self) -> Result<Mmap>;
}

/// Holds actual logging related state
#[derive(Debug)]
struct Logger {
    writer: Box<dyn LogWriter>,
}

impl Logger {
    fn memory() -> Self {
        Logger {
            writer: Box::new(CRCWriter::new(vec![], BLOCK_SIZE)),
        }
    }

    fn disk(file: File) -> Self {
        Logger {
            writer: Box::new(CRCWriter::new(file, BLOCK_SIZE)),
        }
    }
}

impl LogWriter for CRCWriter<Vec<u8>> {
    fn sync(&mut self, _: bool) -> Result<()> {
        self.flush()?;
        Ok(())
    }

    fn mmap(&self) -> Result<Mmap> {
        let mut map = memmap::MmapMut::map_anon(self.get_ref().len())?;
        (&mut map[..]).copy_from_slice(self.get_ref());
        Ok(map.make_read_only()?)
    }
}

impl LogWriter for CRCWriter<File> {
    fn sync(&mut self, fsync: bool) -> Result<()> {
        self.flush()?;

        let file = self.get_mut();
        if fsync {
            file.sync_all()?;
        } else {
            file.sync_data()?;
        }

        Ok(())
    }

    fn mmap(&self) -> Result<Mmap> {
        let map = unsafe { Mmap::map(self.get_ref())? };
        Ok(map)
    }
}

fn log(logger: &mut Logger, key: &Key, commit: i64, data: Option<&[u8]>) -> Result<()> {
    let writer = &mut logger.writer;

    bincode::serialize_into(writer, &(key, commit, data))?;

    Ok(())
}

fn sync(logger: &mut Logger, sync_all: bool) -> Result<()> {
    let writer = &mut logger.writer;

    writer.sync(sync_all)?;

    Ok(())
}

#[inline]
fn file_opts() -> fs::OpenOptions {
    let mut opts = fs::OpenOptions::new();
    opts.read(true).write(true).create(true);
    opts
}

fn read_log(log_buf: &[u8]) -> Result<BTreeMap<Key, Value>> {
    let mut map = BTreeMap::new();

    let mut reader = CRCReader::new(log_buf, BLOCK_SIZE);

    while let Ok((key, commit, opt_bytes)) = bincode::deserialize_from(&mut reader) {
        map.insert(key, Value::new(commit, opt_bytes));
    }

    Ok(map)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_log_serialization() {
        let (key, commit, data) = (Key::from((1, 2, 3)), 4, Some(vec![0; 1024]));

        let mut buf = vec![];

        bincode::serialize_into(&mut buf, &(&key, commit, &data)).unwrap();
        buf.extend(std::iter::repeat(0).take(buf.len()));

        let log_record: (Key, i64, Option<Vec<u8>>) = bincode::deserialize_from(&buf[..]).unwrap();
        assert_eq!(log_record.0, key);
        assert_eq!(log_record.1, commit);
        assert_eq!(log_record.2, data);
    }

    #[test]
    fn test_log_round_trip() {
        let mut wal = WriteLog::memory(Config::default());

        let values: BTreeMap<Key, Value> = (0u64..100)
            .map(|n| {
                let val = if n % 2 == 0 {
                    Some(vec![0; 1024])
                } else {
                    None
                };
                (Key::from((n, n, n)), Value { ts: n as i64, val })
            })
            .collect();

        for (k, v) in values.iter() {
            if v.val.is_some() {
                wal.log_put(k, v.ts, v.val.as_ref().unwrap())
                    .expect("Wal::put");
            } else {
                wal.log_delete(k, v.ts).expect("Wal::delete");
            }
        }

        let reloaded = wal.materialize().expect("Wal::materialize");

        assert_eq!(values.len(), reloaded.len());
        assert_eq!(values, reloaded);
    }

    #[test]
    fn test_reset() {
        use crate::error::Error;

        let mut wal = WriteLog::memory(Config::default());

        let values: BTreeMap<Key, Value> = (0u64..100)
            .map(|n| {
                let val = Some(vec![0; 64]);
                (Key::from((n, n, n)), Value { ts: n as i64, val })
            })
            .collect();

        for (k, v) in values.iter() {
            wal.log_put(k, v.ts, v.val.as_ref().unwrap())
                .expect("Wal::put");
        }

        wal.reset().expect("Wal::reset");

        // Should result in an error due to attempting to make a memory map of length 0
        let result = wal.materialize();

        assert!(result.is_err());
        if let Err(Error::Io(e)) = result {
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        } else {
            panic!("should fail to create 0-length memory-map with an empty log");
        }
    }
}
