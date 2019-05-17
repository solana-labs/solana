use crate::blocktree::db::columns as cf;
use crate::blocktree::db::{Backend, Column, DbCursor, IWriteBatch, TypedColumn};
use crate::blocktree::BlocktreeError;
use crate::result::{Error, Result};
use byteorder::{BigEndian, ByteOrder};
use solana_kvstore::{self as kvstore, Key, KvStore};
use std::path::Path;

type ColumnFamily = u64;

#[derive(Debug)]
pub struct Kvs(KvStore);

/// Dummy struct for now
#[derive(Debug, Clone, Copy)]
pub struct Dummy;

impl Backend for Kvs {
    type Key = Key;
    type OwnedKey = Key;
    type ColumnFamily = ColumnFamily;
    type Cursor = Dummy;
    type Iter = Dummy;
    type WriteBatch = Dummy;
    type Error = kvstore::Error;

    fn open(_path: &Path) -> Result<Kvs> {
        unimplemented!()
    }

    fn columns(&self) -> Vec<&'static str> {
        unimplemented!()
    }

    fn destroy(_path: &Path) -> Result<()> {
        unimplemented!()
    }

    fn cf_handle(&self, _cf: &str) -> ColumnFamily {
        unimplemented!()
    }

    fn get_cf(&self, _cf: ColumnFamily, _key: &Key) -> Result<Option<Vec<u8>>> {
        unimplemented!()
    }

    fn put_cf(&self, _cf: ColumnFamily, _key: &Key, _value: &[u8]) -> Result<()> {
        unimplemented!()
    }

    fn delete_cf(&self, _cf: ColumnFamily, _key: &Key) -> Result<()> {
        unimplemented!()
    }

    fn iterator_cf(&self, _cf: ColumnFamily) -> Result<Dummy> {
        unimplemented!()
    }

    fn raw_iterator_cf(&self, _cf: ColumnFamily) -> Result<Dummy> {
        unimplemented!()
    }

    fn batch(&self) -> Result<Dummy> {
        unimplemented!()
    }

    fn write(&self, _batch: Dummy) -> Result<()> {
        unimplemented!()
    }
}

impl Column<Kvs> for cf::Coding {
    const NAME: &'static str = super::ERASURE_CF;
    type Index = (u64, u64);

    fn key(index: (u64, u64)) -> Key {
        cf::Data::key(index)
    }

    fn index(key: &Key) -> (u64, u64) {
        cf::Data::index(key)
    }
}

impl Column<Kvs> for cf::Data {
    const NAME: &'static str = super::DATA_CF;
    type Index = (u64, u64);

    fn key((slot, index): (u64, u64)) -> Key {
        let mut key = Key::default();
        BigEndian::write_u64(&mut key.0[8..16], slot);
        BigEndian::write_u64(&mut key.0[16..], index);
        key
    }

    fn index(key: &Key) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key.0[8..16]);
        let index = BigEndian::read_u64(&key.0[16..]);
        (slot, index)
    }
}

impl Column<Kvs> for cf::Orphans {
    const NAME: &'static str = super::ORPHANS_CF;
    type Index = u64;

    fn key(slot: u64) -> Key {
        let mut key = Key::default();
        BigEndian::write_u64(&mut key.0[8..16], slot);
        key
    }

    fn index(key: &Key) -> u64 {
        BigEndian::read_u64(&key.0[8..16])
    }
}

impl TypedColumn<Kvs> for cf::Orphans {
    type Type = bool;
}

impl Column<Kvs> for cf::Root {
    const NAME: &'static str = super::ROOT_CF;
    type Index = u64;

    fn key(slot: u64) -> Key {
        let mut key = Key::default();
        BigEndian::write_u64(&mut key.0[8..16], slot);
        key
    }

    fn index(key: &Key) -> u64 {
        BigEndian::read_u64(&key.0[8..16])
    }
}

impl TypedColumn<Kvs> for cf::Root {
    type Type = bool;
}

impl Column<Kvs> for cf::SlotMeta {
    const NAME: &'static str = super::META_CF;
    type Index = u64;

    fn key(slot: u64) -> Key {
        let mut key = Key::default();
        BigEndian::write_u64(&mut key.0[8..16], slot);
        key
    }

    fn index(key: &Key) -> u64 {
        BigEndian::read_u64(&key.0[8..16])
    }
}

impl Column<Kvs> for cf::SlotMeta {
    const NAME: &'static str = super::META_CF;
    type Index = u64;

    fn key(slot: u64) -> Key {
        let mut key = Key::default();
        BigEndian::write_u64(&mut key.0[8..16], slot);
        key
    }

    fn index(key: &Key) -> u64 {
        BigEndian::read_u64(&key.0[8..16])
    }
}

impl TypedColumn<Kvs> for cf::SlotMeta {
    type Type = super::SlotMeta;
}

impl Column<Kvs> for cf::ErasureMeta {
    const NAME: &'static str = super::ERASURE_META_CF;
    type Index = (u64, u64);

    fn key((slot, set_index): (u64, u64)) -> Key {
        let mut key = Key::default();
        BigEndian::write_u64(&mut key.0[8..16], slot);
        BigEndian::write_u64(&mut key.0[16..], set_index);
        key
    }

    fn index(key: &Key) -> (u64, u64) {
        let slot = BigEndian::read_u64(&key.0[8..16]);
        let set_index = BigEndian::read_u64(&key.0[16..]);
        (slot, set_index)
    }
}

impl TypedColumn<Kvs> for cf::ErasureMeta {
    type Type = super::ErasureMeta;
}

impl DbCursor<Kvs> for Dummy {
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

impl IWriteBatch<Kvs> for Dummy {
    fn put_cf(&mut self, _cf: ColumnFamily, _key: &Key, _value: &[u8]) -> Result<()> {
        unimplemented!()
    }

    fn delete_cf(&mut self, _cf: ColumnFamily, _key: &Key) -> Result<()> {
        unimplemented!()
    }
}

impl Iterator for Dummy {
    type Item = (Box<Key>, Box<[u8]>);

    fn next(&mut self) -> Option<Self::Item> {
        unimplemented!()
    }
}

impl std::convert::From<kvstore::Error> for Error {
    fn from(e: kvstore::Error) -> Error {
        Error::BlocktreeError(BlocktreeError::KvsDb(e))
    }
}
