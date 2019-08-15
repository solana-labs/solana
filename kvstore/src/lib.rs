use crate::mapper::{Disk, Mapper, Memory};
use crate::sstable::SSTable;
use crate::storage::MemTable;
use crate::writelog::WriteLog;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

mod compactor;
mod error;
mod io_utils;
mod mapper;
mod readtx;
mod sstable;
mod storage;
mod writebatch;
mod writelog;

#[macro_use]
extern crate serde_derive;

pub use self::error::{Error, Result};
pub use self::readtx::ReadTx as Snapshot;
pub use self::sstable::Key;
pub use self::writebatch::{Config as WriteBatchConfig, WriteBatch};
pub use self::writelog::Config as LogConfig;

const TABLES_FILE: &str = "tables.meta";
const LOG_FILE: &str = "mem-log";
const DEFAULT_TABLE_SIZE: usize = 64 * 1024 * 1024;
const DEFAULT_MEM_SIZE: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_PAGES: usize = 10;
const COMMIT_ORDERING: Ordering = Ordering::Relaxed;

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct Config {
    pub max_mem: usize,
    pub max_tables: usize,
    pub page_size: usize,
    pub in_memory: bool,
    pub log_config: LogConfig,
}

#[derive(Debug)]
pub struct KvStore {
    config: Config,
    root: PathBuf,
    commit: AtomicUsize,
    mem: RwLock<MemTable>,
    log: Arc<RwLock<WriteLog>>,
    tables: RwLock<Vec<BTreeMap<Key, SSTable>>>,
    mapper: Arc<dyn Mapper>,
    sender: Mutex<Sender<compactor::Req>>,
    receiver: Mutex<Receiver<compactor::Resp>>,
    compactor_handle: JoinHandle<()>,
}

impl KvStore {
    pub fn open_default<P>(root: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let mapper = Disk::single(root.as_ref());
        open(root.as_ref(), Arc::new(mapper), Config::default())
    }

    pub fn open<P>(root: P, config: Config) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let mapper: Arc<dyn Mapper> = if config.in_memory {
            Arc::new(Memory::new())
        } else {
            Arc::new(Disk::single(root.as_ref()))
        };
        open(root.as_ref(), mapper, config)
    }

    pub fn partitioned<P, P2>(root: P, storage_dirs: &[P2], config: Config) -> Result<Self>
    where
        P: AsRef<Path>,
        P2: AsRef<Path>,
    {
        let mapper = Disk::new(storage_dirs);
        open(root.as_ref(), Arc::new(mapper), config)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn put(&self, key: &Key, data: &[u8]) -> Result<()> {
        let mut memtable = self.mem.write().unwrap();
        let mut log = self.log.write().unwrap();
        let commit = self.commit.fetch_add(1, COMMIT_ORDERING) as i64;

        log.log_put(key, commit, data).unwrap();
        memtable.put(key, commit, data);

        self.ensure_memtable(&mut *memtable, &mut *log)?;

        Ok(())
    }

    pub fn put_many<Iter, Tup, K, V>(&self, rows: Iter) -> Result<()>
    where
        Iter: Iterator<Item = Tup>,
        Tup: std::borrow::Borrow<(K, V)>,
        K: std::borrow::Borrow<Key>,
        V: std::borrow::Borrow<[u8]>,
    {
        let mut memtable = self.mem.write().unwrap();
        let mut log = self.log.write().unwrap();
        let commit = self.commit.fetch_add(1, COMMIT_ORDERING) as i64;

        for pair in rows {
            let (ref k, ref d) = pair.borrow();
            let (key, data) = (k.borrow(), d.borrow());

            log.log_put(key, commit, data).unwrap();
            memtable.put(key, commit, data);
        }

        self.ensure_memtable(&mut *memtable, &mut *log)?;

        Ok(())
    }

    pub fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.query_compactor()?;

        let (memtable, tables) = (self.mem.read().unwrap(), self.tables.read().unwrap());

        storage::get(&memtable.values, &*tables, key)
    }

    pub fn delete(&self, key: &Key) -> Result<()> {
        let mut memtable = self.mem.write().unwrap();
        let mut log = self.log.write().unwrap();
        let commit = self.commit.fetch_add(1, COMMIT_ORDERING) as i64;

        log.log_delete(key, commit).unwrap();
        memtable.delete(key, commit);

        self.ensure_memtable(&mut *memtable, &mut *log)?;

        Ok(())
    }

    pub fn delete_many<Iter, K>(&self, rows: Iter) -> Result<()>
    where
        Iter: Iterator<Item = K>,
        K: std::borrow::Borrow<Key>,
    {
        let mut memtable = self.mem.write().unwrap();
        let mut log = self.log.write().unwrap();
        let commit = self.commit.fetch_add(1, COMMIT_ORDERING) as i64;

        for k in rows {
            let key = k.borrow();
            log.log_delete(key, commit).unwrap();
            memtable.delete(key, commit);
        }

        self.ensure_memtable(&mut *memtable, &mut *log)?;

        Ok(())
    }

    pub fn batch(&self, config: WriteBatchConfig) -> WriteBatch {
        let commit = self.commit.fetch_add(1, COMMIT_ORDERING) as i64;

        WriteBatch {
            config,
            commit,
            memtable: MemTable::new(BTreeMap::new()),
            log: Arc::clone(&self.log),
        }
    }

    pub fn commit(&self, mut batch: WriteBatch) -> Result<()> {
        let mut memtable = self.mem.write().unwrap();
        let mut log = self.log.write().unwrap();

        memtable.values.append(&mut batch.memtable.values);
        self.ensure_memtable(&mut *memtable, &mut *log)?;

        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot {
        let (memtable, tables) = (
            self.mem.read().unwrap().values.clone(),
            self.tables.read().unwrap().clone(),
        );

        Snapshot::new(memtable, tables)
    }

    pub fn range(
        &self,
        range: RangeInclusive<Key>,
    ) -> Result<impl Iterator<Item = (Key, Vec<u8>)>> {
        self.query_compactor()?;

        let (memtable, tables) = (self.mem.read().unwrap(), self.tables.read().unwrap());

        storage::range(&memtable.values, &*tables, range)
    }

    pub fn destroy<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(());
        }

        fs::remove_dir_all(path)?;
        Ok(())
    }

    fn query_compactor(&self) -> Result<()> {
        if let (Ok(mut sender), Ok(mut receiver), Ok(mut tables)) = (
            self.sender.try_lock(),
            self.receiver.try_lock(),
            self.tables.try_write(),
        ) {
            query_compactor(
                &self.root,
                &*self.mapper,
                &mut *tables,
                &mut *receiver,
                &mut *sender,
            )?;
        }

        Ok(())
    }

    fn ensure_memtable(&self, mem: &mut MemTable, log: &mut WriteLog) -> Result<()> {
        if mem.mem_size < self.config.max_mem {
            return Ok(());
        }

        let mut tables = self.tables.write().unwrap();

        storage::flush_table(&mem.values, &*self.mapper, &mut *tables)?;
        mem.values.clear();
        mem.mem_size = 0;
        log.reset().expect("Write-log rotation failed");

        if is_lvl0_full(&tables, &self.config) {
            let sender = self.sender.lock().unwrap();

            sender.send(compactor::Req::Start(PathBuf::new()))?;
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_mem: DEFAULT_MEM_SIZE,
            max_tables: DEFAULT_MAX_PAGES,
            page_size: DEFAULT_TABLE_SIZE,
            in_memory: false,
            log_config: LogConfig::default(),
        }
    }
}

fn open(root: &Path, mapper: Arc<dyn Mapper>, config: Config) -> Result<KvStore> {
    let root = root.to_path_buf();
    let log_path = root.join(LOG_FILE);
    let restore_log = log_path.exists();

    if !root.exists() {
        fs::create_dir(&root)?;
    }

    let commit = chrono::Utc::now().timestamp();
    let mut log = WriteLog::open(&log_path, config.log_config)?;
    let values = if restore_log && !config.in_memory {
        log.materialize()?
    } else {
        BTreeMap::new()
    };
    let mem = MemTable::new(values);

    let tables = load_tables(&root, &*mapper)?;

    let cfg = compactor::Config {
        max_pages: config.max_tables,
        page_size: config.page_size,
    };
    let (sender, receiver, compactor_handle) = compactor::spawn_compactor(Arc::clone(&mapper), cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(KvStore {
        config,
        root,
        commit: AtomicUsize::new(commit as usize),
        mem: RwLock::new(mem),
        log: Arc::new(RwLock::new(log)),
        tables: RwLock::new(tables),
        mapper,
        sender: Mutex::new(sender),
        receiver: Mutex::new(receiver),
        compactor_handle,
    })
}

fn load_tables(root: &Path, mapper: &dyn Mapper) -> Result<Vec<BTreeMap<Key, SSTable>>> {
    let mut tables = Vec::new();
    let meta_path = root.join(TABLES_FILE);

    if meta_path.exists() {
        mapper.load_state_from(&meta_path)?;
        tables = SSTable::sorted_tables(&mapper.active_set()?);
    }

    Ok(tables)
}

fn dump_tables(root: &Path, mapper: &dyn Mapper) -> Result<()> {
    mapper.serialize_state_to(&root.join(TABLES_FILE))?;
    Ok(())
}

fn query_compactor(
    root: &Path,
    mapper: &dyn Mapper,
    tables: &mut Vec<BTreeMap<Key, SSTable>>,
    receiver: &mut Receiver<compactor::Resp>,
    sender: &mut Sender<compactor::Req>,
) -> Result<()> {
    match receiver.try_recv() {
        Ok(compactor::Resp::Done(new_tables)) => {
            std::mem::replace(tables, new_tables);
            dump_tables(root, mapper)?;
            sender.send(compactor::Req::Gc).unwrap();
        }
        Ok(compactor::Resp::Failed(e)) => {
            return Err(e);
        }
        // Nothing available, do nothing
        _ => {}
    }

    Ok(())
}

#[inline]
fn is_lvl0_full(tables: &[BTreeMap<Key, SSTable>], config: &Config) -> bool {
    if tables.is_empty() {
        false
    } else {
        tables[0].len() > config.max_tables
    }
}

pub mod test {
    pub mod gen {
        use crate::Key;
        use rand::distributions::Uniform;
        use rand::{rngs::SmallRng, FromEntropy, Rng};
        use std::iter;
        use std::ops::Range;

        pub fn keys() -> impl Iterator<Item = Key> {
            let mut rng = SmallRng::from_entropy();
            iter::repeat_with(move || Key(rng.gen()))
        }

        pub fn data(size: usize) -> impl Iterator<Item = Vec<u8>> {
            iter::repeat(vec![0; size])
        }

        pub fn data_vary(range: Range<u64>) -> impl Iterator<Item = Vec<u8>> {
            let dist = Uniform::from(range);
            let mut rng = SmallRng::from_entropy();

            iter::repeat_with(move || {
                let size: u64 = rng.sample(dist);
                vec![0; size as usize]
            })
        }

        pub fn pairs(size: usize) -> impl Iterator<Item = (Key, Vec<u8>)> {
            keys().zip(data(size))
        }

        pub fn pairs_vary(range: Range<u64>) -> impl Iterator<Item = (Key, Vec<u8>)> {
            keys().zip(data_vary(range))
        }
    }
}
