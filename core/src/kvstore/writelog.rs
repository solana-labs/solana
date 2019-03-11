use crate::kvstore::error::Result;
use crate::kvstore::sstable::Value;
use crate::kvstore::Key;

use byteorder::{BigEndian, ByteOrder, ReadBytesExt};

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct WriteLog {
    log_path: PathBuf,
    log_writer: BufWriter<File>,
    max_batch_size: usize,
}

impl WriteLog {
    pub fn open(path: &Path, max_batch_size: usize) -> Result<Self> {
        let log_writer = BufWriter::new(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        );
        let log_path = path.to_path_buf();

        Ok(WriteLog {
            log_writer,
            log_path,
            max_batch_size,
        })
    }

    pub fn reset(&mut self) -> Result<()> {
        self.log_writer.flush()?;
        let file = self.log_writer.get_mut();
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;

        Ok(())
    }

    pub fn log_put(&mut self, key: &Key, ts: i64, val: &[u8]) -> Result<()> {
        let rec_len = 24 + 8 + 1 + val.len() as u64;
        let mut buf = vec![0u8; rec_len as usize + 8];

        log_to_buffer(&mut buf, rec_len, key, ts, val);

        self.log_writer.write_all(&buf)?;
        Ok(())
    }

    pub fn log_delete(&mut self, key: &Key, ts: i64) -> Result<()> {
        self.log_put(key, ts, &[])
    }

    // TODO: decide how to configure/schedule calling this
    #[allow(dead_code)]
    pub fn sync(&mut self) -> Result<()> {
        self.log_writer.flush()?;
        self.log_writer.get_mut().sync_all()?;
        Ok(())
    }

    pub fn materialize(&self) -> Result<BTreeMap<Key, Value>> {
        let mut table = BTreeMap::new();
        if !self.log_path.exists() {
            return Ok(table);
        }

        let mut rdr = BufReader::new(File::open(&self.log_path)?);
        let mut buf = vec![];

        while let Ok(rec_len) = rdr.read_u64::<BigEndian>() {
            buf.resize(rec_len as usize, 0);
            rdr.read_exact(&mut buf)?;

            let key = Key::read(&buf[0..24]);
            let ts = BigEndian::read_i64(&buf[24..32]);
            let exists = buf[32] != 0;

            let val = if exists {
                Some(buf[33..].to_vec())
            } else {
                None
            };
            let value = Value { ts, val };

            table.insert(key, value);
        }

        Ok(table)
    }
}

#[inline]
fn log_to_buffer(buf: &mut [u8], rec_len: u64, key: &Key, ts: i64, val: &[u8]) {
    BigEndian::write_u64(&mut buf[..8], rec_len);
    (&mut buf[8..32]).copy_from_slice(&key.0);
    BigEndian::write_i64(&mut buf[32..40], ts);
    buf[40] = (!val.is_empty()) as u8;
    (&mut buf[41..]).copy_from_slice(val);
}
