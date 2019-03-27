use crate::entry::{Entry, EntrySlice};
use crate::packet::{Blob, BLOB_HEADER_SIZE};
use crate::result::{Error, Result};

use bincode::deserialize;

use byteorder::{BigEndian, ByteOrder};

use rocksdb::{
    self, ColumnFamily, ColumnFamilyDescriptor, DBRawIterator, IteratorMode, Options,
    WriteBatch as RWriteBatch, DB,
};

use solana_sdk::hash::Hash;

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::db::{
    Cursor, Database, IWriteBatch, IndexColumn, LedgerColumnFamily, LedgerColumnFamilyRaw,
};
use super::{Blocktree, BlocktreeError};

// A good value for this is the number of cores on the machine
const TOTAL_THREADS: i32 = 8;
const MAX_WRITE_BUFFER_SIZE: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct Rocks(rocksdb::DB);

/// The metadata column family
#[derive(Debug)]
pub struct MetaCf {
    db: Arc<Rocks>,
}

/// The data column family
#[derive(Debug)]
pub struct DataCf {
    db: Arc<Rocks>,
}

/// The erasure column family
#[derive(Debug)]
pub struct ErasureCf {
    db: Arc<Rocks>,
}

/// TODO: all this goes away with Blocktree
pub struct EntryIterator {
    db_iterator: DBRawIterator,

    // TODO: remove me when replay_stage is iterating by block (Blocktree)
    //    this verification is duplicating that of replay_stage, which
    //    can do this in parallel
    blockhash: Option<Hash>,
    // https://github.com/rust-rocksdb/rust-rocksdb/issues/234
    //   rocksdb issue: the _blocktree member must be lower in the struct to prevent a crash
    //   when the db_iterator member above is dropped.
    //   _blocktree is unused, but dropping _blocktree results in a broken db_iterator
    //   you have to hold the database open in order to iterate over it, and in order
    //   for db_iterator to be able to run Drop
    //    _blocktree: Blocktree,
    entries: VecDeque<Entry>,
}

impl Blocktree {
    /// Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(ledger_path: &str) -> Result<Blocktree> {
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(super::BLOCKTREE_DIRECTORY);

        // Use default database options
        let db_options = Blocktree::get_db_options();

        // Column family names
        let meta_cf_descriptor =
            ColumnFamilyDescriptor::new(super::META_CF, Blocktree::get_cf_options());
        let data_cf_descriptor =
            ColumnFamilyDescriptor::new(super::DATA_CF, Blocktree::get_cf_options());
        let erasure_cf_descriptor =
            ColumnFamilyDescriptor::new(super::ERASURE_CF, Blocktree::get_cf_options());
        let cfs = vec![
            meta_cf_descriptor,
            data_cf_descriptor,
            erasure_cf_descriptor,
        ];

        // Open the database
        let db = Arc::new(Rocks(DB::open_cf_descriptors(
            &db_options,
            ledger_path,
            cfs,
        )?));

        // Create the metadata column family
        let meta_cf = MetaCf { db: db.clone() };

        // Create the data column family
        let data_cf = DataCf { db: db.clone() };

        // Create the erasure column family
        let erasure_cf = ErasureCf { db: db.clone() };

        Ok(Blocktree {
            db,
            meta_cf,
            data_cf,
            erasure_cf,
            new_blobs_signals: vec![],
        })
    }

    pub fn read_ledger_blobs(&self) -> impl Iterator<Item = Blob> {
        self.db
            .0
            .iterator_cf(self.data_cf.handle(), IteratorMode::Start)
            .unwrap()
            .map(|(_, blob_data)| Blob::new(&blob_data))
    }

    /// Return an iterator for all the entries in the given file.
    pub fn read_ledger(&self) -> Result<impl Iterator<Item = Entry>> {
        let mut db_iterator = self.db.raw_iterator_cf(self.data_cf.handle())?;

        db_iterator.seek_to_first();
        Ok(EntryIterator {
            entries: VecDeque::new(),
            db_iterator,
            blockhash: None,
        })
    }

    pub fn destroy(ledger_path: &str) -> Result<()> {
        // DB::destroy() fails if `ledger_path` doesn't exist
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(super::BLOCKTREE_DIRECTORY);
        DB::destroy(&Options::default(), &ledger_path)?;
        Ok(())
    }

    fn get_cf_options() -> Options {
        let mut options = Options::default();
        options.set_max_write_buffer_number(32);
        options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE);
        options.set_max_bytes_for_level_base(MAX_WRITE_BUFFER_SIZE as u64);
        options
    }

    fn get_db_options() -> Options {
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);
        options.increase_parallelism(TOTAL_THREADS);
        options.set_max_background_flushes(4);
        options.set_max_background_compactions(4);
        options.set_max_write_buffer_number(32);
        options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE);
        options.set_max_bytes_for_level_base(MAX_WRITE_BUFFER_SIZE as u64);
        options
    }
}

impl Database for Rocks {
    type Error = rocksdb::Error;
    type Key = [u8];
    type OwnedKey = Vec<u8>;
    type ColumnFamily = ColumnFamily;
    type Cursor = DBRawIterator;
    type EntryIter = EntryIterator;
    type WriteBatch = RWriteBatch;

    fn cf_handle(&self, cf: &str) -> Option<ColumnFamily> {
        self.0.cf_handle(cf)
    }

    fn get_cf(&self, cf: ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let opt = self.0.get_cf(cf, key)?;
        Ok(opt.map(|dbvec| dbvec.to_vec()))
    }

    fn put_cf(&self, cf: ColumnFamily, key: &[u8], data: &[u8]) -> Result<()> {
        self.0.put_cf(cf, key, data)?;
        Ok(())
    }

    fn delete_cf(&self, cf: Self::ColumnFamily, key: &[u8]) -> Result<()> {
        self.0.delete_cf(cf, key).map_err(From::from)
    }

    fn raw_iterator_cf(&self, cf: Self::ColumnFamily) -> Result<Self::Cursor> {
        Ok(self.0.raw_iterator_cf(cf)?)
    }

    fn write(&self, batch: Self::WriteBatch) -> Result<()> {
        self.0.write(batch).map_err(From::from)
    }

    fn batch(&self) -> Result<Self::WriteBatch> {
        Ok(RWriteBatch::default())
    }
}

impl Cursor<Rocks> for DBRawIterator {
    fn valid(&self) -> bool {
        DBRawIterator::valid(self)
    }

    fn seek(&mut self, key: &[u8]) {
        DBRawIterator::seek(self, key)
    }

    fn seek_to_first(&mut self) {
        DBRawIterator::seek_to_first(self)
    }

    fn next(&mut self) {
        DBRawIterator::next(self)
    }

    fn key(&self) -> Option<Vec<u8>> {
        DBRawIterator::key(self)
    }

    fn value(&self) -> Option<Vec<u8>> {
        DBRawIterator::value(self)
    }
}

impl IWriteBatch<Rocks> for RWriteBatch {
    fn put_cf(&mut self, cf: ColumnFamily, key: &[u8], data: &[u8]) -> Result<()> {
        RWriteBatch::put_cf(self, cf, key, data)?;
        Ok(())
    }
}

impl LedgerColumnFamilyRaw<Rocks> for DataCf {
    fn db(&self) -> &Arc<Rocks> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(super::DATA_CF).unwrap()
    }
}

impl IndexColumn<Rocks> for DataCf {
    type Index = (u64, u64);

    fn index(key: &[u8]) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key[..8]);
        let index = BigEndian::read_u64(&key[8..16]);
        (slot, index)
    }

    fn key(idx: &(u64, u64)) -> Vec<u8> {
        let mut key = vec![0u8; 16];
        BigEndian::write_u64(&mut key[0..8], idx.0);
        BigEndian::write_u64(&mut key[8..16], idx.1);
        key
    }
}

impl LedgerColumnFamilyRaw<Rocks> for ErasureCf {
    fn db(&self) -> &Arc<Rocks> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(super::ERASURE_CF).unwrap()
    }
}

impl IndexColumn<Rocks> for ErasureCf {
    type Index = (u64, u64);

    fn index(key: &[u8]) -> (u64, u64) {
        DataCf::index(key)
    }

    fn key(idx: &(u64, u64)) -> Vec<u8> {
        DataCf::key(idx)
    }
}

impl LedgerColumnFamilyRaw<Rocks> for MetaCf {
    fn db(&self) -> &Arc<Rocks> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(super::META_CF).unwrap()
    }
}

impl LedgerColumnFamily<Rocks> for MetaCf {
    type ValueType = super::SlotMeta;
}

impl IndexColumn<Rocks> for MetaCf {
    type Index = u64;

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    fn key(slot: &u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], *slot);
        key
    }
}

impl std::convert::From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Error {
        Error::BlocktreeError(BlocktreeError::RocksDb(e))
    }
}

/// TODO: all this goes away with Blocktree
impl Iterator for EntryIterator {
    type Item = Entry;

    fn next(&mut self) -> Option<Entry> {
        if !self.entries.is_empty() {
            return Some(self.entries.pop_front().unwrap());
        }

        if self.db_iterator.valid() {
            if let Some(value) = self.db_iterator.value() {
                if let Ok(next_entries) = deserialize::<Vec<Entry>>(&value[BLOB_HEADER_SIZE..]) {
                    if let Some(blockhash) = self.blockhash {
                        if !next_entries.verify(&blockhash) {
                            return None;
                        }
                    }
                    self.db_iterator.next();
                    if next_entries.is_empty() {
                        return None;
                    }
                    self.entries = VecDeque::from(next_entries);
                    let entry = self.entries.pop_front().unwrap();
                    self.blockhash = Some(entry.hash);
                    return Some(entry);
                }
            }
        }
        None
    }
}
