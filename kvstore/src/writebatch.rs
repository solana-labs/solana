use crate::error::{Error, Result};
use crate::sstable::Key;
use crate::storage::MemTable;
use crate::writelog::WriteLog;
use crate::DEFAULT_MEM_SIZE;
use std::sync::{Arc, RwLock};

/// Configuration for `WriteBatch`
#[derive(Debug)]
pub struct Config {
    /// Determines whether writes using this batch will be written to the write-ahead-log
    /// immediately, or only all-at-once when the batch is being committed.
    pub log_writes: bool,
    /// Size cap for the write-batch. Inserts after it is full will return an `Err`;
    pub max_size: usize,
}

#[derive(Debug)]
pub struct WriteBatch {
    pub(crate) log: Arc<RwLock<WriteLog>>,
    pub(crate) memtable: MemTable,
    pub(crate) commit: i64,
    pub(crate) config: Config,
}

impl WriteBatch {
    pub fn put(&mut self, key: &Key, data: &[u8]) -> Result<()> {
        self.check_capacity()?;

        if self.config.log_writes {
            let mut log = self.log.write().unwrap();
            log.log_put(key, self.commit, data).unwrap();
        }

        self.memtable.put(key, self.commit, data);

        Ok(())
    }

    pub fn put_many<Iter, Tup, K, V>(&mut self, rows: Iter) -> Result<()>
    where
        Iter: Iterator<Item = Tup>,
        Tup: std::borrow::Borrow<(K, V)>,
        K: std::borrow::Borrow<Key>,
        V: std::borrow::Borrow<[u8]>,
    {
        self.check_capacity()?;

        if self.config.log_writes {
            let mut log = self.log.write().unwrap();

            for pair in rows {
                let (ref key, ref data) = pair.borrow();
                let (key, data) = (key.borrow(), data.borrow());
                log.log_put(key, self.commit, data).unwrap();

                self.memtable.put(key, self.commit, data);
            }
        } else {
            for pair in rows {
                let (ref key, ref data) = pair.borrow();
                self.memtable.put(key.borrow(), self.commit, data.borrow());
            }
        }

        Ok(())
    }

    pub fn delete(&mut self, key: &Key) {
        if self.config.log_writes {
            let mut log = self.log.write().unwrap();
            log.log_delete(key, self.commit).unwrap();
        }

        self.memtable.delete(key, self.commit);
    }

    pub fn delete_many<Iter, K>(&mut self, rows: Iter)
    where
        Iter: Iterator<Item = K>,
        K: std::borrow::Borrow<Key>,
    {
        if self.config.log_writes {
            let mut log = self.log.write().unwrap();

            for key in rows {
                let key = key.borrow();
                log.log_delete(key, self.commit).unwrap();

                self.memtable.delete(key, self.commit);
            }
        } else {
            for key in rows {
                self.memtable.delete(key.borrow(), self.commit);
            }
        }
    }

    #[inline]
    fn check_capacity(&self) -> Result<()> {
        if self.memtable.mem_size >= self.config.max_size {
            return Err(Error::WriteBatchFull(self.config.max_size));
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            log_writes: true,
            max_size: DEFAULT_MEM_SIZE,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::gen;
    use crate::writelog::Config as WalConfig;

    const CAPACITY: usize = 10 * 1024;

    #[test]
    fn test_put_associative() {
        let mut writebatch = setup();
        let input: Vec<_> = gen::pairs(32).take(100).collect();

        writebatch.put_many(input.iter()).unwrap();

        let mut writebatch2 = setup();
        for (key, data) in &input {
            writebatch2.put(key, data).unwrap();
        }

        let (materialized_1, materialized_2) = (
            writebatch.log.write().unwrap().materialize().unwrap(),
            writebatch2.log.write().unwrap().materialize().unwrap(),
        );

        assert_eq!(materialized_1, materialized_2);
    }

    #[test]
    fn test_delete_associative() {
        let (mut writebatch, mut writebatch2) = (setup(), setup());
        let input: Vec<_> = gen::pairs(32).take(100).collect();

        writebatch.put_many(input.iter()).unwrap();
        writebatch2.put_many(input.iter()).unwrap();

        writebatch.delete_many(input.iter().map(|(k, _)| k));

        for (key, _) in &input {
            writebatch2.delete(key);
        }

        let (materialized_1, materialized_2) = (
            writebatch.log.write().unwrap().materialize().unwrap(),
            writebatch2.log.write().unwrap().materialize().unwrap(),
        );

        assert_eq!(materialized_1, materialized_2);
    }

    #[test]
    fn test_no_put_when_full() {
        const AMT_RECORDS: usize = 64;

        let mut writebatch = setup();

        let space_per_record = CAPACITY / AMT_RECORDS - MemTable::OVERHEAD_PER_RECORD;
        let input: Vec<_> = gen::pairs(space_per_record).take(AMT_RECORDS).collect();

        writebatch.put_many(input.iter()).unwrap();

        match writebatch.check_capacity() {
            Err(Error::WriteBatchFull(CAPACITY)) => {}
            _ => panic!("Writebatch should be exactly at capacity"),
        }

        let (key, data) = gen::pairs(space_per_record).next().unwrap();
        let result = writebatch.put(&key, &data);
        assert!(result.is_err());

        // Free up space
        writebatch.delete(&input[0].0);
        let result = writebatch.put(&key, &data);
        assert!(result.is_ok());
    }

    fn setup() -> WriteBatch {
        let config = Config {
            log_writes: true,
            max_size: CAPACITY,
        };

        let log = WriteLog::memory(WalConfig::default());

        WriteBatch {
            config,
            commit: -1,
            memtable: MemTable::default(),
            log: Arc::new(RwLock::new(log)),
        }
    }
}
