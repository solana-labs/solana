use crate::entry::Entry;
use crate::result::{Error, Result};

use bincode::{deserialize, serialize};

use serde::de::DeserializeOwned;
use serde::Serialize;

use std::borrow::Borrow;
use std::sync::Arc;

pub trait Database: Sized + Send + Sync {
    type Error: Into<Error>;
    type Key: ?Sized;
    type OwnedKey: Borrow<Self::Key>;
    type ColumnFamily;
    type Cursor: Cursor<Self>;
    type EntryIter: Iterator<Item = Entry>;
    type WriteBatch: IWriteBatch<Self>;

    fn cf_handle(&self, cf: &str) -> Option<Self::ColumnFamily>;

    fn get_cf(&self, cf: Self::ColumnFamily, key: &Self::Key) -> Result<Option<Vec<u8>>>;

    fn put_cf(&self, cf: Self::ColumnFamily, key: &Self::Key, data: &[u8]) -> Result<()>;

    fn delete_cf(&self, cf: Self::ColumnFamily, key: &Self::Key) -> Result<()>;

    fn raw_iterator_cf(&self, cf: Self::ColumnFamily) -> Result<Self::Cursor>;

    fn write(&self, batch: Self::WriteBatch) -> Result<()>;

    fn batch(&self) -> Result<Self::WriteBatch>;
}

pub trait Cursor<D: Database> {
    fn valid(&self) -> bool;

    fn seek(&mut self, key: &D::Key);

    fn seek_to_first(&mut self);

    fn next(&mut self);

    fn key(&self) -> Option<D::OwnedKey>;

    fn value(&self) -> Option<Vec<u8>>;
}

pub trait IWriteBatch<D: Database> {
    fn put_cf(&mut self, cf: D::ColumnFamily, key: &D::Key, data: &[u8]) -> Result<()>;
}

pub trait LedgerColumnFamily<D: Database>: LedgerColumnFamilyRaw<D> {
    type ValueType: DeserializeOwned + Serialize;

    fn get(&self, key: &D::Key) -> Result<Option<Self::ValueType>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;

        if let Some(raw) = data_bytes {
            let result: Self::ValueType = deserialize(&raw)?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn put(&self, key: &D::Key, value: &Self::ValueType) -> Result<()> {
        let db = self.db();
        let serialized = serialize(value)?;
        db.put_cf(self.handle(), key, &serialized)?;
        Ok(())
    }
}

pub trait LedgerColumnFamilyRaw<D: Database> {
    fn get_bytes(&self, key: &D::Key) -> Result<Option<Vec<u8>>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key.borrow())?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put_bytes(&self, key: &D::Key, serialized_value: &[u8]) -> Result<()> {
        let db = self.db();
        db.put_cf(self.handle(), key.borrow(), &serialized_value)?;
        Ok(())
    }

    fn delete(&self, key: &D::Key) -> Result<()> {
        let db = self.db();
        db.delete_cf(self.handle(), key.borrow())?;
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

pub trait IndexColumn<D: Database>: LedgerColumnFamilyRaw<D> {
    type Index;

    fn get_by_index(&self, index: &Self::Index) -> Result<Option<Vec<u8>>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), Self::key(index).borrow())?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put_by_index(&self, index: &Self::Index, serialized_value: &[u8]) -> Result<()> {
        let db = self.db();
        db.put_cf(self.handle(), Self::key(index).borrow(), &serialized_value)?;
        Ok(())
    }

    fn delete_by_index(&self, index: &Self::Index) -> Result<()> {
        self.delete(Self::key(index).borrow())
    }

    fn index(key: &D::Key) -> Self::Index;

    fn key(index: &Self::Index) -> D::OwnedKey;
}
