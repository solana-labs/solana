use crate::entry::Entry;
use crate::result::{Error, Result};

use bincode::{deserialize, serialize};

use serde::de::DeserializeOwned;
use serde::Serialize;

use std::borrow::Borrow;
use std::sync::Arc;

pub trait Database: Sized + Send + Sync {
    type Error: Into<Error>;
    type Key: Borrow<Self::KeyRef>;
    type KeyRef: ?Sized;
    type ColumnFamily;
    type Cursor: Cursor<Self>;
    type EntryIter: Iterator<Item = Entry>;
    type WriteBatch: IWriteBatch<Self>;

    fn cf_handle(&self, cf: &str) -> Option<Self::ColumnFamily>;

    fn get_cf(&self, cf: Self::ColumnFamily, key: &Self::KeyRef) -> Result<Option<Vec<u8>>>;

    fn put_cf(&self, cf: Self::ColumnFamily, key: &Self::KeyRef, data: &[u8]) -> Result<()>;

    fn delete_cf(&self, cf: Self::ColumnFamily, key: &Self::KeyRef) -> Result<()>;

    fn raw_iterator_cf(&self, cf: Self::ColumnFamily) -> Result<Self::Cursor>;

    fn write(&self, batch: Self::WriteBatch) -> Result<()>;

    fn batch(&self) -> Result<Self::WriteBatch>;
}

pub trait Cursor<D: Database> {
    fn valid(&self) -> bool;

    fn seek(&mut self, key: &D::KeyRef);

    fn seek_to_first(&mut self);

    fn next(&mut self);

    fn key(&self) -> Option<D::Key>;

    fn value(&self) -> Option<Vec<u8>>;
}

pub trait IWriteBatch<D: Database> {
    fn put_cf(&mut self, cf: D::ColumnFamily, key: &D::KeyRef, data: &[u8]) -> Result<()>;
}

pub trait IDataCf<D: Database>: LedgerColumnFamilyRaw<D> {
    fn new(db: Arc<D>) -> Self;

    fn get_by_slot_index(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        let key = Self::key(slot, index);
        self.get(key.borrow())
    }

    fn delete_by_slot_index(&self, slot: u64, index: u64) -> Result<()> {
        let key = Self::key(slot, index);
        self.delete(&key.borrow())
    }

    fn put_by_slot_index(&self, slot: u64, index: u64, serialized_value: &[u8]) -> Result<()> {
        let key = Self::key(slot, index);
        self.put(key.borrow(), serialized_value)
    }

    fn key(slot: u64, index: u64) -> D::Key;

    fn slot_from_key(key: &D::KeyRef) -> Result<u64>;

    fn index_from_key(key: &D::KeyRef) -> Result<u64>;
}

pub trait IErasureCf<D: Database>: LedgerColumnFamilyRaw<D> {
    fn new(db: Arc<D>) -> Self;

    fn delete_by_slot_index(&self, slot: u64, index: u64) -> Result<()> {
        let key = Self::key(slot, index);
        self.delete(key.borrow())
    }

    fn get_by_slot_index(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        let key = Self::key(slot, index);
        self.get(key.borrow())
    }

    fn put_by_slot_index(&self, slot: u64, index: u64, serialized_value: &[u8]) -> Result<()> {
        let key = Self::key(slot, index);
        self.put(key.borrow(), serialized_value)
    }

    fn key(slot: u64, index: u64) -> D::Key;

    fn slot_from_key(key: &D::KeyRef) -> Result<u64>;

    fn index_from_key(key: &D::KeyRef) -> Result<u64>;
}

pub trait IMetaCf<D: Database>: LedgerColumnFamily<D, ValueType = super::SlotMeta> {
    fn new(db: Arc<D>) -> Self;

    fn key(slot: u64) -> D::Key;

    fn get_slot_meta(&self, slot: u64) -> Result<Option<super::SlotMeta>> {
        let key = Self::key(slot);
        self.get(key.borrow())
    }

    fn put_slot_meta(&self, slot: u64, slot_meta: &super::SlotMeta) -> Result<()> {
        let key = Self::key(slot);
        self.put(key.borrow(), slot_meta)
    }

    fn index_from_key(key: &D::KeyRef) -> Result<u64>;
}

pub trait LedgerColumnFamily<D: Database> {
    type ValueType: DeserializeOwned + Serialize;

    fn get(&self, key: &D::KeyRef) -> Result<Option<Self::ValueType>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;

        if let Some(raw) = data_bytes {
            let result: Self::ValueType = deserialize(&raw)?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn get_bytes(&self, key: &D::KeyRef) -> Result<Option<Vec<u8>>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put_bytes(&self, key: &D::KeyRef, serialized_value: &[u8]) -> Result<()> {
        let db = self.db();
        db.put_cf(self.handle(), key, &serialized_value)?;
        Ok(())
    }

    fn put(&self, key: &D::KeyRef, value: &Self::ValueType) -> Result<()> {
        let db = self.db();
        let serialized = serialize(value)?;
        db.put_cf(self.handle(), key, &serialized)?;
        Ok(())
    }

    fn delete(&self, key: &D::KeyRef) -> Result<()> {
        let db = self.db();
        db.delete_cf(self.handle(), key)?;
        Ok(())
    }

    fn db(&self) -> &Arc<D>;

    fn handle(&self) -> D::ColumnFamily;
}

pub trait LedgerColumnFamilyRaw<D: Database> {
    fn get(&self, key: &D::KeyRef) -> Result<Option<Vec<u8>>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put(&self, key: &D::KeyRef, serialized_value: &[u8]) -> Result<()> {
        let db = self.db();
        db.put_cf(self.handle(), &key, &serialized_value)?;
        Ok(())
    }

    fn delete(&self, key: &D::KeyRef) -> Result<()> {
        let db = self.db();
        db.delete_cf(self.handle(), &key)?;
        Ok(())
    }

    fn raw_iterator(&self) -> D::Cursor {
        let db = self.db();
        db.raw_iterator_cf(self.handle())
            .expect("Expected to be able to open database iterator")
    }

    fn handle(&self) -> D::ColumnFamily;

    fn db(&self) -> &Arc<D>;
}
