// TODO: document module, especially log format
use crate::kvstore::error::Result;
use crate::kvstore::io_utils::{CRCReader, CRCWriter};
use crate::kvstore::sstable::Value;
use crate::kvstore::Key;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use memmap::Mmap;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

const BLOCK_SIZE: usize = 32 * 1024;

#[derive(Debug)]
pub struct WriteLog {
    log_path: PathBuf,
    logger: RwLock<Logger>,
    config: Config,
    in_memory: bool,
}

#[derive(Debug)]
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
            logger: RwLock::new(Logger::disk(file)),
            in_memory: false,
        })
    }

    #[allow(dead_code)]
    pub fn memory(config: Config) -> WriteLog {
        WriteLog {
            config,
            logger: RwLock::new(Logger::memory()),
            log_path: Path::new("").to_path_buf(),
            in_memory: true,
        }
    }

    pub fn reset(&self) -> Result<()> {
        let new_logger = if self.in_memory {
            Logger::memory()
        } else {
            let file = file_opts().truncate(true).open(&self.log_path)?;
            Logger::disk(file)
        };

        *self.logger.write().unwrap() = new_logger;

        Ok(())
    }

    pub fn log_put(&self, key: &Key, ts: i64, val: &[u8]) -> Result<()> {
        let mut logger = self.logger.write().unwrap();

        log(&mut logger, key, ts, Some(val))?;

        if self.config.sync_every_write {
            sync(&mut logger, self.config.use_fsync)?;
        }

        Ok(())
    }

    pub fn log_delete(&self, key: &Key, ts: i64) -> Result<()> {
        let mut logger = self.logger.write().unwrap();

        log(&mut logger, key, ts, None)?;

        if self.config.sync_every_write {
            sync(&mut logger, self.config.use_fsync)?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn sync(&self) -> Result<()> {
        let mut logger = self.logger.write().unwrap();

        sync(&mut logger, self.config.use_fsync)
    }

    pub fn materialize(&self) -> Result<BTreeMap<Key, Value>> {
        let mmap = self.logger.write().unwrap().writer.mmap()?;
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
    writer: Box<LogWriter>,
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
        Ok(self.flush()?)
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
    let wtr = &mut logger.writer;
    write_value(wtr, key, commit, data)?;

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

fn read_log(wal: &[u8]) -> Result<BTreeMap<Key, Value>> {
    let mut map = BTreeMap::new();
    if wal.len() <= 8 + 24 + 8 + 1 {
        return Ok(map);
    }

    let mut rdr = CRCReader::new(wal, BLOCK_SIZE);

    while let Ok((key, val)) = read_value(&mut rdr) {
        map.insert(key, val);
    }

    Ok(map)
}

#[inline]
fn write_value<W: Write>(wtr: &mut W, key: &Key, commit: i64, data: Option<&[u8]>) -> Result<()> {
    let len = 24 + 8 + 1 + data.map(<[u8]>::len).unwrap_or(0);

    wtr.write_u64::<BigEndian>(len as u64)?;
    wtr.write_all(&key.0)?;
    wtr.write_i64::<BigEndian>(commit)?;

    match data {
        Some(data) => {
            wtr.write_u8(1)?;
            wtr.write_all(data)?;
        }
        None => {
            wtr.write_u8(0)?;
        }
    }

    Ok(())
}

#[inline]
fn read_value<R: Read>(rdr: &mut R) -> Result<(Key, Value)> {
    let len = rdr.read_u64::<BigEndian>()?;
    let data_len = len as usize - (24 + 8 + 1);

    let mut rdr = rdr.by_ref().take(len);

    let mut key_buf = [0; 24];
    rdr.read_exact(&mut key_buf)?;
    let key = Key(key_buf);

    let commit = rdr.read_i64::<BigEndian>()?;
    let exists = rdr.read_u8()? != 0;

    let data = if exists {
        let mut buf = Vec::with_capacity(data_len);
        rdr.read_to_end(&mut buf)?;
        Some(buf)
    } else {
        None
    };

    let val = Value {
        ts: commit,
        val: data,
    };
    Ok((key, val))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_log_serialization() {
        let (key, commit, data) = (&Key::from((1, 2, 3)), 4, vec![0; 1024]);

        let mut buf = vec![];

        write_value(&mut buf, key, commit, Some(&data)).unwrap();

        let (stored_key, stored_val) = read_value(&mut &buf[..]).unwrap();
        assert_eq!(&stored_key, key);
        assert_eq!(stored_val.val.as_ref().unwrap(), &data);
        assert_eq!(stored_val.ts, commit);
    }

    #[test]
    fn test_log_round_trip() {
        let wal = WriteLog::memory(Config::default());

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
        use crate::kvstore::error::Error;

        let wal = WriteLog::memory(Config::default());

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
