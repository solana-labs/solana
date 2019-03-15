use crate::entry::Entry;
use crate::packet::Blob;
use crate::result::{Error, Result};
use solana_kvstore::{self as kvstore, Key, KvStore};

use std::sync::Arc;

use super::db::{
    Cursor, Database, IDataCf, IErasureCf, IMetaCf, IWriteBatch, LedgerColumnFamily,
    LedgerColumnFamilyRaw,
};
use super::{Blocktree, BlocktreeError};

#[derive(Debug)]
pub struct Kvs(KvStore);

/// The metadata column family
#[derive(Debug)]
pub struct MetaCf {
    db: Arc<Kvs>,
}

/// The data column family
#[derive(Debug)]
pub struct DataCf {
    db: Arc<Kvs>,
}

/// The erasure column family
#[derive(Debug)]
pub struct ErasureCf {
    db: Arc<Kvs>,
}

/// Dummy struct to get things compiling
/// TODO: all this goes away with Blocktree
pub struct EntryIterator(i32);
/// Dummy struct to get things compiling
pub struct KvsCursor;
/// Dummy struct to get things compiling
pub struct ColumnFamily;
/// Dummy struct to get things compiling
pub struct KvsWriteBatch;

impl Blocktree {
    /// Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(_ledger_path: &str) -> Result<Blocktree> {
        unimplemented!()
    }

    #[allow(unreachable_code)]
    pub fn read_ledger_blobs(&self) -> impl Iterator<Item = Blob> {
        unimplemented!();
        self.read_ledger().unwrap().map(|_| Blob::new(&[]))
    }

    /// Return an iterator for all the entries in the given file.
    #[allow(unreachable_code)]
    pub fn read_ledger(&self) -> Result<impl Iterator<Item = Entry>> {
        Ok(EntryIterator(unimplemented!()))
    }

    pub fn destroy(_ledger_path: &str) -> Result<()> {
        unimplemented!()
    }
}

impl Database for Kvs {
    type Error = kvstore::Error;
    type Key = Key;
    type KeyRef = Key;
    type ColumnFamily = ColumnFamily;
    type Cursor = KvsCursor;
    type EntryIter = EntryIterator;
    type WriteBatch = KvsWriteBatch;

    fn cf_handle(&self, _cf: &str) -> Option<ColumnFamily> {
        unimplemented!()
    }

    fn get_cf(&self, _cf: ColumnFamily, _key: &Key) -> Result<Option<Vec<u8>>> {
        unimplemented!()
    }

    fn put_cf(&self, _cf: ColumnFamily, _key: &Key, _data: &[u8]) -> Result<()> {
        unimplemented!()
    }

    fn delete_cf(&self, _cf: Self::ColumnFamily, _key: &Key) -> Result<()> {
        unimplemented!()
    }

    fn raw_iterator_cf(&self, _cf: Self::ColumnFamily) -> Result<Self::Cursor> {
        unimplemented!()
    }

    fn write(&self, _batch: Self::WriteBatch) -> Result<()> {
        unimplemented!()
    }

    fn batch(&self) -> Result<Self::WriteBatch> {
        unimplemented!()
    }
}

impl Cursor<Kvs> for KvsCursor {
    fn valid(&self) -> bool {
        unimplemented!()
    }

    fn seek(&mut self, _key: &Key) {
        unimplemented!()
    }

    fn seek_to_first(&mut self) {
        unimplemented!()
    }

    fn next(&mut self) {
        unimplemented!()
    }

    fn key(&self) -> Option<Key> {
        unimplemented!()
    }

    fn value(&self) -> Option<Vec<u8>> {
        unimplemented!()
    }
}

impl IWriteBatch<Kvs> for KvsWriteBatch {
    fn put_cf(&mut self, _cf: ColumnFamily, _key: &Key, _data: &[u8]) -> Result<()> {
        unimplemented!()
    }
}

impl IDataCf<Kvs> for DataCf {
    fn new(db: Arc<Kvs>) -> Self {
        DataCf { db }
    }

    fn get_by_slot_index(&self, _slot: u64, _index: u64) -> Result<Option<Vec<u8>>> {
        unimplemented!()
    }

    fn delete_by_slot_index(&self, _slot: u64, _index: u64) -> Result<()> {
        unimplemented!()
    }

    fn put_by_slot_index(&self, _slot: u64, _index: u64, _serialized_value: &[u8]) -> Result<()> {
        unimplemented!()
    }

    fn key(_slot: u64, _index: u64) -> Key {
        unimplemented!()
    }

    fn slot_from_key(_key: &Key) -> Result<u64> {
        unimplemented!()
    }

    fn index_from_key(_key: &Key) -> Result<u64> {
        unimplemented!()
    }
}

impl IErasureCf<Kvs> for ErasureCf {
    fn new(db: Arc<Kvs>) -> Self {
        ErasureCf { db }
    }

    fn delete_by_slot_index(&self, _slot: u64, _index: u64) -> Result<()> {
        unimplemented!()
    }

    fn get_by_slot_index(&self, _slot: u64, _index: u64) -> Result<Option<Vec<u8>>> {
        unimplemented!()
    }

    fn put_by_slot_index(&self, _slot: u64, _index: u64, _serialized_value: &[u8]) -> Result<()> {
        unimplemented!()
    }

    fn key(slot: u64, index: u64) -> Key {
        DataCf::key(slot, index)
    }

    fn slot_from_key(key: &Key) -> Result<u64> {
        DataCf::slot_from_key(key)
    }

    fn index_from_key(key: &Key) -> Result<u64> {
        DataCf::index_from_key(key)
    }
}

impl IMetaCf<Kvs> for MetaCf {
    fn new(db: Arc<Kvs>) -> Self {
        MetaCf { db }
    }

    fn key(_slot: u64) -> Key {
        unimplemented!()
    }

    fn get_slot_meta(&self, _slot: u64) -> Result<Option<super::SlotMeta>> {
        unimplemented!()
    }

    fn put_slot_meta(&self, _slot: u64, _slot_meta: &super::SlotMeta) -> Result<()> {
        unimplemented!()
    }

    fn index_from_key(_key: &Key) -> Result<u64> {
        unimplemented!()
    }
}

impl LedgerColumnFamilyRaw<Kvs> for DataCf {
    fn db(&self) -> &Arc<Kvs> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(super::DATA_CF).unwrap()
    }
}

impl LedgerColumnFamilyRaw<Kvs> for ErasureCf {
    fn db(&self) -> &Arc<Kvs> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(super::ERASURE_CF).unwrap()
    }
}

impl LedgerColumnFamily<Kvs> for MetaCf {
    type ValueType = super::SlotMeta;

    fn db(&self) -> &Arc<Kvs> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(super::META_CF).unwrap()
    }
}

impl std::convert::From<kvstore::Error> for Error {
    fn from(e: kvstore::Error) -> Error {
        Error::BlocktreeError(BlocktreeError::KvsDb(e))
    }
}

/// TODO: all this goes away with Blocktree
impl Iterator for EntryIterator {
    type Item = Entry;

    fn next(&mut self) -> Option<Entry> {
        unimplemented!()
    }
}
