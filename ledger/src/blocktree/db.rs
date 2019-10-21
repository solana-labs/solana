use crate::blocktree::{BlocktreeError, Result};

use bincode::{deserialize, serialize};
use byteorder::{BigEndian, ByteOrder};
use log::*;

use serde::de::DeserializeOwned;
use serde::Serialize;

use solana_sdk::clock::Slot;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use rocksdb::{
    self, ColumnFamily, ColumnFamilyDescriptor, DBIterator, DBRawIterator, Direction,
    IteratorMode as RocksIteratorMode, Options, WriteBatch as RWriteBatch, DB,
};

// A good value for this is the number of cores on the machine
const TOTAL_THREADS: i32 = 8;
const MAX_WRITE_BUFFER_SIZE: u64 = 256 * 1024 * 1024; // 256MB
const MIN_WRITE_BUFFER_SIZE: u64 = 64 * 1024; // 64KB

pub enum IteratorMode<Index> {
    Start,
    End,
    From(Index, IteratorDirection),
}

pub enum IteratorDirection {
    Forward,
    Reverse,
}

pub mod columns {
    #[derive(Debug)]
    /// SlotMeta Column
    pub struct SlotMeta;

    #[derive(Debug)]
    /// Orphans Column
    pub struct Orphans;

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

#[derive(Debug)]
struct Rocks(rocksdb::DB);

impl Rocks {
    fn open(path: &Path) -> Result<Rocks> {
        use crate::blocktree::db::columns::{
            DeadSlots, ErasureMeta, Index, Orphans, Root, ShredCode, ShredData, SlotMeta,
        };

        fs::create_dir_all(&path)?;

        // Use default database options
        let db_options = get_db_options();

        // Column family names
        let meta_cf_descriptor =
            ColumnFamilyDescriptor::new(SlotMeta::NAME, get_cf_options(SlotMeta::NAME));
        let dead_slots_cf_descriptor =
            ColumnFamilyDescriptor::new(DeadSlots::NAME, get_cf_options(DeadSlots::NAME));
        let erasure_meta_cf_descriptor =
            ColumnFamilyDescriptor::new(ErasureMeta::NAME, get_cf_options(ErasureMeta::NAME));
        let orphans_cf_descriptor =
            ColumnFamilyDescriptor::new(Orphans::NAME, get_cf_options(Orphans::NAME));
        let root_cf_descriptor =
            ColumnFamilyDescriptor::new(Root::NAME, get_cf_options(Root::NAME));
        let index_cf_descriptor =
            ColumnFamilyDescriptor::new(Index::NAME, get_cf_options(Index::NAME));
        let shred_data_cf_descriptor =
            ColumnFamilyDescriptor::new(ShredData::NAME, get_cf_options(ShredData::NAME));
        let shred_code_cf_descriptor =
            ColumnFamilyDescriptor::new(ShredCode::NAME, get_cf_options(ShredCode::NAME));

        let cfs = vec![
            meta_cf_descriptor,
            dead_slots_cf_descriptor,
            erasure_meta_cf_descriptor,
            orphans_cf_descriptor,
            root_cf_descriptor,
            index_cf_descriptor,
            shred_data_cf_descriptor,
            shred_code_cf_descriptor,
        ];

        // Open the database
        let db = Rocks(DB::open_cf_descriptors(&db_options, path, cfs)?);

        Ok(db)
    }

    fn columns(&self) -> Vec<&'static str> {
        use crate::blocktree::db::columns::{
            DeadSlots, ErasureMeta, Index, Orphans, Root, ShredCode, ShredData, SlotMeta,
        };

        vec![
            ErasureMeta::NAME,
            DeadSlots::NAME,
            Index::NAME,
            Orphans::NAME,
            Root::NAME,
            SlotMeta::NAME,
            ShredData::NAME,
            ShredCode::NAME,
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

    fn iterator_cf(
        &self,
        cf: ColumnFamily,
        iterator_mode: IteratorMode<&[u8]>,
    ) -> Result<DBIterator> {
        let iter = {
            match iterator_mode {
                IteratorMode::Start => self.0.iterator_cf(cf, RocksIteratorMode::Start)?,
                IteratorMode::End => self.0.iterator_cf(cf, RocksIteratorMode::End)?,
                IteratorMode::From(start_from, direction) => {
                    let rocks_direction = match direction {
                        IteratorDirection::Forward => Direction::Forward,
                        IteratorDirection::Reverse => Direction::Reverse,
                    };
                    self.0
                        .iterator_cf(cf, RocksIteratorMode::From(start_from, rocks_direction))?
                }
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

pub trait Column {
    const NAME: &'static str;
    type Index;

    fn key(index: Self::Index) -> Vec<u8>;
    fn index(key: &[u8]) -> Self::Index;
    fn slot(index: Self::Index) -> Slot;
    fn as_index(slot: Slot) -> Self::Index;
}

pub trait TypedColumn: Column {
    type Type: Serialize + DeserializeOwned;
}

impl Column for columns::ShredCode {
    const NAME: &'static str = super::CODE_SHRED_CF;
    type Index = (u64, u64);

    fn key(index: (u64, u64)) -> Vec<u8> {
        columns::ShredData::key(index)
    }

    fn index(key: &[u8]) -> (u64, u64) {
        columns::ShredData::index(key)
    }

    fn slot(index: Self::Index) -> Slot {
        index.0
    }

    fn as_index(slot: Slot) -> Self::Index {
        (slot, 0)
    }
}

impl Column for columns::ShredData {
    const NAME: &'static str = super::DATA_SHRED_CF;
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

    fn slot(index: Self::Index) -> Slot {
        index.0
    }

    fn as_index(slot: Slot) -> Self::Index {
        (slot, 0)
    }
}

impl Column for columns::Index {
    const NAME: &'static str = super::INDEX_CF;
    type Index = u64;

    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    fn slot(index: Self::Index) -> Slot {
        index
    }

    fn as_index(slot: Slot) -> Self::Index {
        slot
    }
}

impl TypedColumn for columns::Index {
    type Type = crate::blocktree::meta::Index;
}

impl Column for columns::DeadSlots {
    const NAME: &'static str = super::DEAD_SLOTS_CF;
    type Index = u64;

    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    fn slot(index: Self::Index) -> Slot {
        index
    }

    fn as_index(slot: Slot) -> Self::Index {
        slot
    }
}

impl TypedColumn for columns::DeadSlots {
    type Type = bool;
}

impl Column for columns::Orphans {
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

    fn slot(index: Self::Index) -> Slot {
        index
    }

    fn as_index(slot: Slot) -> Self::Index {
        slot
    }
}

impl TypedColumn for columns::Orphans {
    type Type = bool;
}

impl Column for columns::Root {
    const NAME: &'static str = super::ROOT_CF;
    type Index = u64;

    fn key(slot: u64) -> Vec<u8> {
        let mut key = vec![0; 8];
        BigEndian::write_u64(&mut key[..], slot);
        key
    }

    fn index(key: &[u8]) -> u64 {
        BigEndian::read_u64(&key[..8])
    }

    fn slot(index: Self::Index) -> Slot {
        index
    }

    fn as_index(slot: Slot) -> Self::Index {
        slot
    }
}

impl TypedColumn for columns::Root {
    type Type = bool;
}

impl Column for columns::SlotMeta {
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

    fn slot(index: Self::Index) -> Slot {
        index
    }

    fn as_index(slot: Slot) -> Self::Index {
        slot
    }
}

impl TypedColumn for columns::SlotMeta {
    type Type = super::SlotMeta;
}

impl Column for columns::ErasureMeta {
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

    fn slot(index: Self::Index) -> Slot {
        index.0
    }

    fn as_index(slot: Slot) -> Self::Index {
        (slot, 0)
    }
}

impl TypedColumn for columns::ErasureMeta {
    type Type = super::ErasureMeta;
}

#[derive(Debug, Clone)]
pub struct Database {
    backend: Arc<Rocks>,
}

#[derive(Debug, Clone)]
pub struct BatchProcessor {
    backend: Arc<Rocks>,
}

#[derive(Debug, Clone)]
pub struct LedgerColumn<C>
where
    C: Column,
{
    backend: Arc<Rocks>,
    column: PhantomData<C>,
}

pub struct WriteBatch {
    write_batch: RWriteBatch,
    backend: PhantomData<Rocks>,
    map: HashMap<&'static str, ColumnFamily>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let backend = Arc::new(Rocks::open(path)?);

        Ok(Database { backend })
    }

    pub fn destroy(path: &Path) -> Result<()> {
        Rocks::destroy(path)?;

        Ok(())
    }

    pub fn get_bytes<C>(&self, key: C::Index) -> Result<Option<Vec<u8>>>
    where
        C: Column,
    {
        self.backend
            .get_cf(self.cf_handle::<C>(), C::key(key).borrow())
    }

    pub fn put_bytes<C>(&self, key: C::Index, data: &[u8]) -> Result<()>
    where
        C: Column,
    {
        self.backend
            .put_cf(self.cf_handle::<C>(), C::key(key).borrow(), data)
    }

    pub fn delete<C>(&self, key: C::Index) -> Result<()>
    where
        C: Column,
    {
        self.backend
            .delete_cf(self.cf_handle::<C>(), C::key(key).borrow())
    }

    pub fn get<C>(&self, key: C::Index) -> Result<Option<C::Type>>
    where
        C: TypedColumn,
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
        C: TypedColumn,
    {
        let serialized_value = serialize(value)?;

        self.backend.put_cf(
            self.cf_handle::<C>(),
            C::key(key).borrow(),
            &serialized_value,
        )
    }

    pub fn iter<C>(
        &self,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)>>
    where
        C: Column,
    {
        let iter = {
            match iterator_mode {
                IteratorMode::From(start_from, direction) => {
                    let key = C::key(start_from);
                    self.backend.iterator_cf(
                        self.cf_handle::<C>(),
                        IteratorMode::From(key.borrow(), direction),
                    )?
                }
                IteratorMode::Start => self
                    .backend
                    .iterator_cf(self.cf_handle::<C>(), IteratorMode::Start)?,
                IteratorMode::End => self
                    .backend
                    .iterator_cf(self.cf_handle::<C>(), IteratorMode::End)?,
            }
        };

        Ok(iter.map(|(key, value)| (C::index(&key), value)))
    }

    #[inline]
    pub fn cf_handle<C>(&self) -> ColumnFamily
    where
        C: Column,
    {
        self.backend.cf_handle(C::NAME)
    }

    pub fn column<C>(&self) -> LedgerColumn<C>
    where
        C: Column,
    {
        LedgerColumn {
            backend: Arc::clone(&self.backend),
            column: PhantomData,
        }
    }

    #[inline]
    pub fn raw_iterator_cf(&self, cf: ColumnFamily) -> Result<DBRawIterator> {
        self.backend.raw_iterator_cf(cf)
    }

    // Note this returns an object that can be used to directly write to multiple column families.
    // This circumvents the synchronization around APIs that in Blocktree that use
    // blocktree.batch_processor, so this API should only be used if the caller is sure they
    // are writing to data in columns that will not be corrupted by any simultaneous blocktree
    // operations.
    pub unsafe fn batch_processor(&self) -> BatchProcessor {
        BatchProcessor {
            backend: Arc::clone(&self.backend),
        }
    }
}

impl BatchProcessor {
    pub fn batch(&mut self) -> Result<WriteBatch> {
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

    pub fn write(&mut self, batch: WriteBatch) -> Result<()> {
        self.backend.write(batch.write_batch)
    }
}

impl<C> LedgerColumn<C>
where
    C: Column,
{
    pub fn get_bytes(&self, key: C::Index) -> Result<Option<Vec<u8>>> {
        self.backend.get_cf(self.handle(), C::key(key).borrow())
    }

    pub fn iter(
        &self,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)>> {
        let iter = {
            match iterator_mode {
                IteratorMode::From(start_from, direction) => {
                    let key = C::key(start_from);
                    self.backend
                        .iterator_cf(self.handle(), IteratorMode::From(key.borrow(), direction))?
                }
                IteratorMode::Start => self
                    .backend
                    .iterator_cf(self.handle(), IteratorMode::Start)?,
                IteratorMode::End => self.backend.iterator_cf(self.handle(), IteratorMode::End)?,
            }
        };

        Ok(iter.map(|(key, value)| (C::index(&key), value)))
    }

    pub fn delete_slot(
        &self,
        batch: &mut WriteBatch,
        from: Option<Slot>,
        to: Option<Slot>,
    ) -> Result<bool>
    where
        C::Index: PartialOrd + Copy,
    {
        let mut end = true;
        let iter_config = match from {
            Some(s) => IteratorMode::From(C::as_index(s), IteratorDirection::Forward),
            None => IteratorMode::Start,
        };
        let iter = self.iter(iter_config)?;
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
    pub fn handle(&self) -> ColumnFamily {
        self.backend.cf_handle(C::NAME)
    }

    pub fn is_empty(&self) -> Result<bool> {
        let mut iter = self.backend.raw_iterator_cf(self.handle())?;
        iter.seek_to_first();
        Ok(!iter.valid())
    }

    pub fn put_bytes(&self, key: C::Index, value: &[u8]) -> Result<()> {
        self.backend
            .put_cf(self.handle(), C::key(key).borrow(), value)
    }

    pub fn delete(&self, key: C::Index) -> Result<()> {
        self.backend.delete_cf(self.handle(), C::key(key).borrow())
    }
}

impl<C> LedgerColumn<C>
where
    C: TypedColumn,
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

impl WriteBatch {
    pub fn put_bytes<C: Column>(&mut self, key: C::Index, bytes: &[u8]) -> Result<()> {
        self.write_batch
            .put_cf(self.get_cf::<C>(), C::key(key).borrow(), bytes)
            .map_err(|e| e.into())
    }

    pub fn delete<C: Column>(&mut self, key: C::Index) -> Result<()> {
        self.write_batch
            .delete_cf(self.get_cf::<C>(), C::key(key).borrow())
            .map_err(|e| e.into())
    }

    pub fn put<C: TypedColumn>(&mut self, key: C::Index, value: &C::Type) -> Result<()> {
        let serialized_value = serialize(&value)?;
        self.write_batch
            .put_cf(self.get_cf::<C>(), C::key(key).borrow(), &serialized_value)
            .map_err(|e| e.into())
    }

    #[inline]
    fn get_cf<C: Column>(&self) -> ColumnFamily {
        self.map[C::NAME]
    }
}

impl std::convert::From<rocksdb::Error> for BlocktreeError {
    fn from(e: rocksdb::Error) -> BlocktreeError {
        BlocktreeError::RocksDb(e)
    }
}

fn get_cf_options(name: &'static str) -> Options {
    use crate::blocktree::db::columns::{ErasureMeta, Index, ShredCode, ShredData};

    let mut options = Options::default();
    match name {
        ShredCode::NAME | ShredData::NAME | Index::NAME | ErasureMeta::NAME => {
            // 512MB * 8 = 4GB. 2 of these columns should take no more than 8GB of RAM
            options.set_max_write_buffer_number(8);
            options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE as usize);
            options.set_target_file_size_base(MAX_WRITE_BUFFER_SIZE / 10);
            options.set_max_bytes_for_level_base(MAX_WRITE_BUFFER_SIZE);
        }
        _ => {
            // We want smaller CFs to flush faster. This results in more WAL files but lowers
            // overall WAL space utilization and increases flush frequency
            options.set_write_buffer_size(MIN_WRITE_BUFFER_SIZE as usize);
            options.set_target_file_size_base(MIN_WRITE_BUFFER_SIZE);
            options.set_max_bytes_for_level_base(MIN_WRITE_BUFFER_SIZE);
            options.set_level_zero_file_num_compaction_trigger(1);
        }
    }
    options
}

fn get_db_options() -> Options {
    let mut options = Options::default();
    options.create_if_missing(true);
    options.create_missing_column_families(true);
    options.increase_parallelism(TOTAL_THREADS);
    options.set_max_background_flushes(4);
    options.set_max_background_compactions(4);
    options
}
