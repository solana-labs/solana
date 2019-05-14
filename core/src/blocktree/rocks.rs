use crate::blocktree::db::columns as cf;
use crate::blocktree::db::{Backend, Column, DbCursor, IWriteBatch, TypedColumn};
use crate::blocktree::BlocktreeError;
use crate::result::{Error, Result};

use byteorder::{BigEndian, ByteOrder};

use rocksdb::{
    self, ColumnFamily, ColumnFamilyDescriptor, DBIterator, DBRawIterator, Direction, IteratorMode,
    Options, WriteBatch as RWriteBatch, DB,
};

use std::fs;
use std::path::Path;

// A good value for this is the number of cores on the machine
const TOTAL_THREADS: i32 = 8;
const MAX_WRITE_BUFFER_SIZE: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct Rocks(rocksdb::DB);

impl Backend for Rocks {
    type Key = [u8];
    type OwnedKey = Vec<u8>;
    type ColumnFamily = ColumnFamily;
    type Cursor = DBRawIterator;
    type Iter = DBIterator;
    type WriteBatch = RWriteBatch;
    type Error = rocksdb::Error;

    fn open(path: &Path) -> Result<Rocks> {
        use crate::blocktree::db::columns::{Coding, Data, ErasureMeta, Orphans, Root, SlotMeta};

        fs::create_dir_all(&path)?;

        // Use default database options
        let db_options = get_db_options();

        // Column family names
        let meta_cf_descriptor = ColumnFamilyDescriptor::new(SlotMeta::NAME, get_cf_options());
        let data_cf_descriptor = ColumnFamilyDescriptor::new(Data::NAME, get_cf_options());
        let erasure_cf_descriptor = ColumnFamilyDescriptor::new(Coding::NAME, get_cf_options());
        let erasure_meta_cf_descriptor =
            ColumnFamilyDescriptor::new(ErasureMeta::NAME, get_cf_options());
        let orphans_cf_descriptor = ColumnFamilyDescriptor::new(Orphans::NAME, get_cf_options());
        let root_cf_descriptor = ColumnFamilyDescriptor::new(Root::NAME, get_cf_options());

        let cfs = vec![
            meta_cf_descriptor,
            data_cf_descriptor,
            erasure_cf_descriptor,
            erasure_meta_cf_descriptor,
            orphans_cf_descriptor,
            root_cf_descriptor,
        ];

        // Open the database
        let db = Rocks(DB::open_cf_descriptors(&db_options, path, cfs)?);

        Ok(db)
    }

    fn columns(&self) -> Vec<&'static str> {
        use crate::blocktree::db::columns::{Coding, Data, ErasureMeta, Orphans, Root, SlotMeta};

        vec![
            Coding::NAME,
            ErasureMeta::NAME,
            Data::NAME,
            Orphans::NAME,
            Root::NAME,
            SlotMeta::NAME,
        ]
    }

    fn destroy(path: &Path) -> Result<()> {
        DB::destroy(&Options::default(), path)?;

        Ok(())
    }

    fn cf_handle(&self, cf: &str) -> ColumnFamily {
        self.0
            .cf_handle(cf)
            .expect("should never get an unknown column")
    }

    fn get_cf(&self, cf: ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let opt = self.0.get_cf(cf, key)?.map(|db_vec| db_vec.to_vec());
        Ok(opt)
    }

    fn put_cf(&self, cf: ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        self.0.put_cf(cf, key, value)?;
        Ok(())
    }

    fn delete_cf(&self, cf: ColumnFamily, key: &[u8]) -> Result<()> {
        self.0.delete_cf(cf, key)?;
        Ok(())
    }

    fn iterator_cf(&self, cf: ColumnFamily, start_from: Option<&[u8]>) -> Result<DBIterator> {
        let iter = {
            if let Some(start_from) = start_from {
                self.0
                    .iterator_cf(cf, IteratorMode::From(start_from, Direction::Forward))?
            } else {
                self.0.iterator_cf(cf, IteratorMode::Start)?
            }
        };

        Ok(iter)
    }

    fn raw_iterator_cf(&self, cf: ColumnFamily) -> Result<DBRawIterator> {
        let raw_iter = self.0.raw_iterator_cf(cf)?;

        Ok(raw_iter)
    }

    fn batch(&self) -> Result<RWriteBatch> {
        Ok(RWriteBatch::default())
    }

    fn write(&self, batch: RWriteBatch) -> Result<()> {
        self.0.write(batch)?;
        Ok(())
    }
}

impl Column<Rocks> for cf::Coding {
    const NAME: &'static str = super::ERASURE_CF;
    type Index = (u64, u64);

    fn key(index: (u64, u64)) -> Vec<u8> {
        cf::Data::key(index)
    }

    fn index(key: &[u8]) -> (u64, u64) {
        cf::Data::index(key)
    }
}

impl Column<Rocks> for cf::Data {
    const NAME: &'static str = super::DATA_CF;
    type Index = (u64, u64);

    fn key((slot, index): (u64, u64)) -> Vec<u8> {
        let mut key = vec![0; 16];
        BigEndian::write_u64(&mut key[..8], slot);
        BigEndian::write_u64(&mut key[8..16], index);
        key
    }

    fn index(key: &[u8]) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key[..8]);
        let index = BigEndian::read_u64(&key[8..16]);
        (slot, index)
    }
}

impl Column<Rocks> for cf::Orphans {
    const NAME: &'static str = super::ORPHANS_CF;
    type Index = u64;

    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }
}

impl TypedColumn<Rocks> for cf::Orphans {
    type Type = bool;
}

impl Column<Rocks> for cf::Root {
    const NAME: &'static str = super::ROOT_CF;
    type Index = ();

    fn key(_: ()) -> Vec<u8> {
        vec![0; 8]
    }

    fn index(_: &[u8]) {}
}

impl TypedColumn<Rocks> for cf::Root {
    type Type = u64;
}

impl Column<Rocks> for cf::SlotMeta {
    const NAME: &'static str = super::META_CF;
    type Index = u64;

    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }
}

impl TypedColumn<Rocks> for cf::SlotMeta {
    type Type = super::SlotMeta;
}

impl Column<Rocks> for cf::ErasureMeta {
    const NAME: &'static str = super::ERASURE_META_CF;
    type Index = (u64, u64);

    fn index(key: &[u8]) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key[..8]);
        let set_index = BigEndian::read_u64(&key[8..]);

        (slot, set_index)
    }

    fn key((slot, set_index): (u64, u64)) -> Vec<u8> {
        let mut key = vec![0; 16];
        BigEndian::write_u64(&mut key[..8], slot);
        BigEndian::write_u64(&mut key[8..], set_index);
        key
    }
}

impl TypedColumn<Rocks> for cf::ErasureMeta {
    type Type = super::ErasureMeta;
}

impl DbCursor<Rocks> for DBRawIterator {
    fn valid(&self) -> bool {
        DBRawIterator::valid(self)
    }

    fn seek(&mut self, key: &[u8]) {
        DBRawIterator::seek(self, key);
    }

    fn seek_to_first(&mut self) {
        DBRawIterator::seek_to_first(self);
    }

    fn next(&mut self) {
        DBRawIterator::next(self);
    }

    fn key(&self) -> Option<Vec<u8>> {
        DBRawIterator::key(self)
    }

    fn value(&self) -> Option<Vec<u8>> {
        DBRawIterator::value(self)
    }
}

impl IWriteBatch<Rocks> for RWriteBatch {
    fn put_cf(&mut self, cf: ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        RWriteBatch::put_cf(self, cf, key, value)?;
        Ok(())
    }

    fn delete_cf(&mut self, cf: ColumnFamily, key: &[u8]) -> Result<()> {
        RWriteBatch::delete_cf(self, cf, key)?;
        Ok(())
    }
}

impl std::convert::From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Error {
        Error::BlocktreeError(BlocktreeError::RocksDb(e))
    }
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
