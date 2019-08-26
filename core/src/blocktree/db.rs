use crate::result::{Error, Result};

use bincode::{deserialize, serialize};

use serde::de::DeserializeOwned;
use serde::Serialize;

use solana_sdk::timing::Slot;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

pub mod columns {
    #[derive(Debug)]
    /// SlotMeta Column
    pub struct SlotMeta;

    #[derive(Debug)]
    /// Orphans Column
    pub struct Orphans;

    #[derive(Debug)]
    /// Erasure Column
    pub struct Coding;

    #[derive(Debug)]
    /// Data Column
    pub struct Data;

    #[derive(Debug)]
    /// Data Column
    pub struct DeadSlots;

    #[derive(Debug)]
    /// The erasure meta column
    pub struct ErasureMeta;

    #[derive(Debug)]
    /// The root column
    pub struct Root;

    #[derive(Debug)]
    /// The index column
    pub struct Index;

    #[derive(Debug)]
    /// The shred data column
    pub struct ShredData;

    #[derive(Debug)]
    /// The shred erasure code column
    pub struct ShredCode;
}

pub trait Backend: Sized + Send + Sync {
    type Key: ?Sized + ToOwned<Owned = Self::OwnedKey>;
    type OwnedKey: Borrow<Self::Key>;
    type ColumnFamily: Clone;
    type Cursor: DbCursor<Self>;
    type Iter: Iterator<Item = (Box<Self::Key>, Box<[u8]>)>;
    type WriteBatch: IWriteBatch<Self>;
    type Error: Into<Error>;

    fn open(path: &Path) -> Result<Self>;

    fn columns(&self) -> Vec<&'static str>;

    fn destroy(path: &Path) -> Result<()>;

    fn cf_handle(&self, cf: &str) -> Self::ColumnFamily;

    fn get_cf(&self, cf: Self::ColumnFamily, key: &Self::Key) -> Result<Option<Vec<u8>>>;

    fn put_cf(&self, cf: Self::ColumnFamily, key: &Self::Key, value: &[u8]) -> Result<()>;

    fn delete_cf(&self, cf: Self::ColumnFamily, key: &Self::Key) -> Result<()>;

    fn iterator_cf(&self, cf: Self::ColumnFamily, from: Option<&Self::Key>) -> Result<Self::Iter>;

    fn raw_iterator_cf(&self, cf: Self::ColumnFamily) -> Result<Self::Cursor>;

    fn write(&self, batch: Self::WriteBatch) -> Result<()>;

    fn batch(&self) -> Result<Self::WriteBatch>;
}

pub trait Column<B>
where
    B: Backend,
{
    const NAME: &'static str;
    type Index;

    fn key(index: Self::Index) -> B::OwnedKey;
    fn index(key: &B::Key) -> Self::Index;
    fn slot(index: Self::Index) -> Slot;
    fn as_index(slot: Slot) -> Self::Index;
}

pub trait DbCursor<B>
where
    B: Backend,
{
    fn valid(&self) -> bool;

    fn seek(&mut self, key: &B::Key);

    fn seek_to_first(&mut self);

    fn next(&mut self);

    fn key(&self) -> Option<B::OwnedKey>;

    fn value(&self) -> Option<Vec<u8>>;
}

pub trait IWriteBatch<B>
where
    B: Backend,
{
    fn put_cf(&mut self, cf: B::ColumnFamily, key: &B::Key, value: &[u8]) -> Result<()>;
    fn delete_cf(&mut self, cf: B::ColumnFamily, key: &B::Key) -> Result<()>;
}

pub trait TypedColumn<B>: Column<B>
where
    B: Backend,
{
    type Type: Serialize + DeserializeOwned;
}

#[derive(Debug, Clone)]
pub struct Database<B>
where
    B: Backend,
{
    backend: Arc<B>,
}

#[derive(Debug, Clone)]
pub struct BatchProcessor<B>
where
    B: Backend,
{
    backend: Arc<B>,
}

#[derive(Debug, Clone)]
pub struct Cursor<B, C>
where
    B: Backend,
    C: Column<B>,
{
    db_cursor: B::Cursor,
    column: PhantomData<C>,
    backend: PhantomData<B>,
}

#[derive(Debug, Clone)]
pub struct LedgerColumn<B, C>
where
    B: Backend,
    C: Column<B>,
{
    backend: Arc<B>,
    column: PhantomData<C>,
}

#[derive(Debug)]
pub struct WriteBatch<B>
where
    B: Backend,
{
    write_batch: B::WriteBatch,
    backend: PhantomData<B>,
    map: HashMap<&'static str, B::ColumnFamily>,
}

impl<B> Database<B>
where
    B: Backend,
{
    pub fn open(path: &Path) -> Result<Self> {
        let backend = Arc::new(B::open(path)?);

        Ok(Database { backend })
    }

    pub fn destroy(path: &Path) -> Result<()> {
        B::destroy(path)?;

        Ok(())
    }

    pub fn get_bytes<C>(&self, key: C::Index) -> Result<Option<Vec<u8>>>
    where
        C: Column<B>,
    {
        self.backend
            .get_cf(self.cf_handle::<C>(), C::key(key).borrow())
    }

    pub fn put_bytes<C>(&self, key: C::Index, data: &[u8]) -> Result<()>
    where
        C: Column<B>,
    {
        self.backend
            .put_cf(self.cf_handle::<C>(), C::key(key).borrow(), data)
    }

    pub fn delete<C>(&self, key: C::Index) -> Result<()>
    where
        C: Column<B>,
    {
        self.backend
            .delete_cf(self.cf_handle::<C>(), C::key(key).borrow())
    }

    pub fn get<C>(&self, key: C::Index) -> Result<Option<C::Type>>
    where
        C: TypedColumn<B>,
    {
        if let Some(serialized_value) = self
            .backend
            .get_cf(self.cf_handle::<C>(), C::key(key).borrow())?
        {
            let value = deserialize(&serialized_value)?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn put<C>(&self, key: C::Index, value: &C::Type) -> Result<()>
    where
        C: TypedColumn<B>,
    {
        let serialized_value = serialize(value)?;

        self.backend.put_cf(
            self.cf_handle::<C>(),
            C::key(key).borrow(),
            &serialized_value,
        )
    }

    pub fn cursor<C>(&self) -> Result<Cursor<B, C>>
    where
        C: Column<B>,
    {
        let db_cursor = self.backend.raw_iterator_cf(self.cf_handle::<C>())?;

        Ok(Cursor {
            db_cursor,
            column: PhantomData,
            backend: PhantomData,
        })
    }

    pub fn iter<C>(
        &self,
        start_from: Option<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)>>
    where
        C: Column<B>,
    {
        let iter = {
            if let Some(index) = start_from {
                let key = C::key(index);
                self.backend
                    .iterator_cf(self.cf_handle::<C>(), Some(key.borrow()))?
            } else {
                self.backend.iterator_cf(self.cf_handle::<C>(), None)?
            }
        };

        Ok(iter.map(|(key, value)| (C::index(&key), value)))
    }

    #[inline]
    pub fn cf_handle<C>(&self) -> B::ColumnFamily
    where
        C: Column<B>,
    {
        self.backend.cf_handle(C::NAME).clone()
    }

    pub fn column<C>(&self) -> LedgerColumn<B, C>
    where
        C: Column<B>,
    {
        LedgerColumn {
            backend: Arc::clone(&self.backend),
            column: PhantomData,
        }
    }

    // Note this returns an object that can be used to directly write to multiple column families.
    // This circumvents the synchronization around APIs that in Blocktree that use
    // blocktree.batch_processor, so this API should only be used if the caller is sure they
    // are writing to data in columns that will not be corrupted by any simultaneous blocktree
    // operations.
    pub unsafe fn batch_processor(&self) -> BatchProcessor<B> {
        BatchProcessor {
            backend: Arc::clone(&self.backend),
        }
    }
}

impl<B> BatchProcessor<B>
where
    B: Backend,
{
    pub fn batch(&mut self) -> Result<WriteBatch<B>> {
        let db_write_batch = self.backend.batch()?;
        let map = self
            .backend
            .columns()
            .into_iter()
            .map(|desc| (desc, self.backend.cf_handle(desc)))
            .collect();

        Ok(WriteBatch {
            write_batch: db_write_batch,
            backend: PhantomData,
            map,
        })
    }

    pub fn write(&mut self, batch: WriteBatch<B>) -> Result<()> {
        self.backend.write(batch.write_batch)
    }
}

impl<B, C> Cursor<B, C>
where
    B: Backend,
    C: Column<B>,
{
    pub fn valid(&self) -> bool {
        self.db_cursor.valid()
    }

    pub fn seek(&mut self, key: C::Index) {
        self.db_cursor.seek(C::key(key).borrow());
    }

    pub fn seek_to_first(&mut self) {
        self.db_cursor.seek_to_first();
    }

    pub fn next(&mut self) {
        self.db_cursor.next();
    }

    pub fn key(&self) -> Option<C::Index> {
        if let Some(key) = self.db_cursor.key() {
            Some(C::index(key.borrow()))
        } else {
            None
        }
    }

    pub fn value_bytes(&self) -> Option<Vec<u8>> {
        self.db_cursor.value()
    }
}

impl<B, C> Cursor<B, C>
where
    B: Backend,
    C: TypedColumn<B>,
{
    pub fn value(&self) -> Option<C::Type> {
        if let Some(bytes) = self.db_cursor.value() {
            let value = deserialize(&bytes).ok()?;
            Some(value)
        } else {
            None
        }
    }
}

impl<B, C> LedgerColumn<B, C>
where
    B: Backend,
    C: Column<B>,
{
    pub fn get_bytes(&self, key: C::Index) -> Result<Option<Vec<u8>>> {
        self.backend.get_cf(self.handle(), C::key(key).borrow())
    }

    pub fn cursor(&self) -> Result<Cursor<B, C>> {
        let db_cursor = self.backend.raw_iterator_cf(self.handle())?;

        Ok(Cursor {
            db_cursor,
            column: PhantomData,
            backend: PhantomData,
        })
    }

    pub fn iter(
        &self,
        start_from: Option<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)>> {
        let iter = {
            if let Some(index) = start_from {
                let key = C::key(index);
                self.backend
                    .iterator_cf(self.handle(), Some(key.borrow()))?
            } else {
                self.backend.iterator_cf(self.handle(), None)?
            }
        };

        Ok(iter.map(|(key, value)| (C::index(&key), value)))
    }

    pub fn delete_slot(
        &self,
        batch: &mut WriteBatch<B>,
        from: Option<Slot>,
        to: Option<Slot>,
    ) -> Result<bool>
    where
        C::Index: PartialOrd + Copy,
    {
        let mut end = true;
        let iter = self.iter(from.map(C::as_index))?;
        for (index, _) in iter {
            if let Some(to) = to {
                if C::slot(index) > to {
                    end = false;
                    break;
                }
            };
            if let Err(e) = batch.delete::<C>(index) {
                error!(
                    "Error: {:?} while adding delete from_slot {:?} to batch {:?}",
                    e,
                    from,
                    C::NAME
                )
            }
        }
        Ok(end)
    }

    #[inline]
    pub fn handle(&self) -> B::ColumnFamily {
        self.backend.cf_handle(C::NAME).clone()
    }

    pub fn is_empty(&self) -> Result<bool> {
        let mut cursor = self.cursor()?;
        cursor.seek_to_first();
        Ok(!cursor.valid())
    }

    pub fn put_bytes(&self, key: C::Index, value: &[u8]) -> Result<()> {
        self.backend
            .put_cf(self.handle(), C::key(key).borrow(), value)
    }

    pub fn delete(&self, key: C::Index) -> Result<()> {
        self.backend.delete_cf(self.handle(), C::key(key).borrow())
    }
}

impl<B, C> LedgerColumn<B, C>
where
    B: Backend,
    C: TypedColumn<B>,
{
    pub fn get(&self, key: C::Index) -> Result<Option<C::Type>> {
        if let Some(serialized_value) = self.backend.get_cf(self.handle(), C::key(key).borrow())? {
            let value = deserialize(&serialized_value)?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn put(&self, key: C::Index, value: &C::Type) -> Result<()> {
        let serialized_value = serialize(value)?;

        self.backend
            .put_cf(self.handle(), C::key(key).borrow(), &serialized_value)
    }
}

impl<B> WriteBatch<B>
where
    B: Backend,
{
    pub fn put_bytes<C: Column<B>>(&mut self, key: C::Index, bytes: &[u8]) -> Result<()> {
        self.write_batch
            .put_cf(self.get_cf::<C>(), C::key(key).borrow(), bytes)
    }

    pub fn delete<C: Column<B>>(&mut self, key: C::Index) -> Result<()> {
        self.write_batch
            .delete_cf(self.get_cf::<C>(), C::key(key).borrow())
    }

    pub fn put<C: TypedColumn<B>>(&mut self, key: C::Index, value: &C::Type) -> Result<()> {
        let serialized_value = serialize(&value)?;
        self.write_batch
            .put_cf(self.get_cf::<C>(), C::key(key).borrow(), &serialized_value)
    }

    #[inline]
    fn get_cf<C: Column<B>>(&self) -> B::ColumnFamily {
        self.map[C::NAME].clone()
    }
}
