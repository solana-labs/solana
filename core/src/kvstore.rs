use crate::kvstore::mapper::{Disk, Mapper, Memory};
use crate::kvstore::sstable::SSTable;
use crate::kvstore::storage::WriteState;
use crate::kvstore::writelog::WriteLog;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
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
mod writelog;
mod writetx;

pub use self::error::{Error, Result};
pub use self::readtx::ReadTx as Snapshot;
pub use self::sstable::Key;
pub use self::writelog::Config as LogConfig;
pub use self::writetx::WriteTx;

const TABLES_FILE: &str = "tables.meta";
const LOG_FILE: &str = "mem-log";
const DEFAULT_TABLE_SIZE: usize = 64 * 1024 * 1024;
const DEFAULT_MEM_SIZE: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_PAGES: usize = 10;

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
    write: RwLock<WriteState>,
    tables: RwLock<Vec<BTreeMap<Key, SSTable>>>,
    config: Config,
    root: PathBuf,
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
        self.ensure_mem()?;

        let mut write = self.write.write().unwrap();

        write.put(key, data)?;
        write.commit += 1;

        Ok(())
    }

    pub fn put_many<Iter, Tup, K, V>(&self, rows: Iter) -> Result<()>
    where
        Iter: Iterator<Item = Tup>,
        Tup: std::borrow::Borrow<(K, V)>,
        K: std::borrow::Borrow<Key>,
        V: std::borrow::Borrow<[u8]>,
    {
        {
            let mut write = self.write.write().unwrap();

            for pair in rows {
                let tup = pair.borrow();
                let (key, data) = (tup.0.borrow(), tup.1.borrow());
                write.put(key, data)?;
            }
            write.commit += 1;
        }

        self.ensure_mem()?;

        Ok(())
    }

    pub fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.query_compactor()?;

        let (write_state, tables) = (self.write.read().unwrap(), self.tables.read().unwrap());

        storage::get(&write_state.values, &*tables, key)
    }

    pub fn delete(&self, key: &Key) -> Result<()> {
        self.query_compactor()?;

        {
            let mut write = self.write.write().unwrap();

            write.delete(key)?;
            write.commit += 1;
        }

        self.ensure_mem()?;
        Ok(())
    }

    pub fn delete_many<Iter, K>(&self, rows: Iter) -> Result<()>
    where
        Iter: Iterator<Item = K>,
        K: std::borrow::Borrow<Key>,
    {
        self.query_compactor()?;

        {
            let mut write = self.write.write().unwrap();
            for k in rows {
                let key = k.borrow();
                write.delete(key)?;
            }
            write.commit += 1;
        }

        self.ensure_mem()?;
        Ok(())
    }

    pub fn transaction(&self) -> Result<WriteTx> {
        unimplemented!()
    }

    pub fn commit(&self, _txn: WriteTx) -> Result<()> {
        unimplemented!()
    }

    pub fn snapshot(&self) -> Snapshot {
        let (state, tables) = (self.write.read().unwrap(), self.tables.read().unwrap());

        Snapshot::new(state.values.clone(), tables.clone())
    }

    pub fn range(
        &self,
        range: RangeInclusive<Key>,
    ) -> Result<impl Iterator<Item = (Key, Vec<u8>)>> {
        self.query_compactor()?;

        let (write_state, tables) = (self.write.read().unwrap(), self.tables.read().unwrap());
        storage::range(&write_state.values, &*tables, range)
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

    fn ensure_mem(&self) -> Result<()> {
        let trigger_compact = {
            let mut write_rw = self.write.write().unwrap();

            if write_rw.mem_size < self.config.max_mem {
                return Ok(());
            }

            let mut tables = self.tables.write().unwrap();
            storage::flush_table(&write_rw.values, &*self.mapper, &mut *tables)?;

            write_rw.reset()?;
            write_rw.commit += 1;

            is_lvl0_full(&tables, &self.config)
        };

        dump_tables(&self.root, &*self.mapper).unwrap();
        if trigger_compact {
            let tables_path = self.root.join(TABLES_FILE);
            self.sender
                .lock()
                .unwrap()
                .send(compactor::Req::Start(tables_path))
                .expect("compactor thread dead");
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

    let write_log = WriteLog::open(&log_path, config.log_config)?;
    let mem = if restore_log && !config.in_memory {
        write_log.materialize()?
    } else {
        BTreeMap::new()
    };

    let write = RwLock::new(WriteState::new(write_log, mem));

    let tables = load_tables(&root, &*mapper)?;
    let tables = RwLock::new(tables);

    let cfg = compactor::Config {
        max_pages: config.max_tables,
        page_size: config.page_size,
    };
    let (sender, receiver, compactor_handle) = compactor::spawn_compactor(Arc::clone(&mapper), cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let (sender, receiver) = (Mutex::new(sender), Mutex::new(receiver));

    Ok(KvStore {
        write,
        tables,
        config,
        mapper,
        root,
        sender,
        receiver,
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

fn dump_tables(root: &Path, mapper: &Mapper) -> Result<()> {
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
