use crate::blockstore_meta;
use bincode::{deserialize, serialize};
use byteorder::{BigEndian, ByteOrder};
use fs_extra;
use log::*;
pub use rocksdb::Direction as IteratorDirection;
use rocksdb::{
    self, ColumnFamily, ColumnFamilyDescriptor, DBIterator, DBRawIterator,
    IteratorMode as RocksIteratorMode, Options, WriteBatch as RWriteBatch, DB,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use solana_client::rpc_request::RpcTransactionStatus;
use solana_sdk::{clock::Slot, signature::Signature};
use std::{collections::HashMap, fs, marker::PhantomData, path::Path, sync::Arc};
use thiserror::Error;

const MAX_WRITE_BUFFER_SIZE: u64 = 256 * 1024 * 1024; // 256MB

// Column family for metadata about a leader slot
const META_CF: &str = "meta";
// Column family for slots that have been marked as dead
const DEAD_SLOTS_CF: &str = "dead_slots";
const ERASURE_META_CF: &str = "erasure_meta";
// Column family for orphans data
const ORPHANS_CF: &str = "orphans";
// Column family for root data
const ROOT_CF: &str = "root";
/// Column family for indexes
const INDEX_CF: &str = "index";
/// Column family for Data Shreds
const DATA_SHRED_CF: &str = "data_shred";
/// Column family for Code Shreds
const CODE_SHRED_CF: &str = "code_shred";
/// Column family for Transaction Status
const TRANSACTION_STATUS_CF: &str = "transaction_status";

#[derive(Error, Debug)]
pub enum BlockstoreError {
    ShredForIndexExists,
    InvalidShredData(Box<bincode::ErrorKind>),
    RocksDb(#[from] rocksdb::Error),
    SlotNotRooted,
    DeadSlot,
    IO(#[from] std::io::Error),
    Serialize(#[from] Box<bincode::ErrorKind>),
    FsExtraError(#[from] fs_extra::error::Error),
}
pub(crate) type Result<T> = std::result::Result<T, BlockstoreError>;

impl std::fmt::Display for BlockstoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "blockstore error")
    }
}

pub enum IteratorMode<Index> {
    Start,
    End,
    From(Index, IteratorDirection),
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

    #[derive(Debug)]
    /// The transaction status column
    pub struct TransactionStatus;
}

#[derive(Debug)]
struct Rocks(rocksdb::DB);

impl Rocks {
    fn open(path: &Path) -> Result<Rocks> {
        use columns::{
            DeadSlots, ErasureMeta, Index, Orphans, Root, ShredCode, ShredData, SlotMeta,
            TransactionStatus,
        };

        fs::create_dir_all(&path)?;

        // Use default database options
        let db_options = get_db_options();

        // Column family names
        let meta_cf_descriptor = ColumnFamilyDescriptor::new(SlotMeta::NAME, get_cf_options());
        let dead_slots_cf_descriptor =
            ColumnFamilyDescriptor::new(DeadSlots::NAME, get_cf_options());
        let erasure_meta_cf_descriptor =
            ColumnFamilyDescriptor::new(ErasureMeta::NAME, get_cf_options());
        let orphans_cf_descriptor = ColumnFamilyDescriptor::new(Orphans::NAME, get_cf_options());
        let root_cf_descriptor = ColumnFamilyDescriptor::new(Root::NAME, get_cf_options());
        let index_cf_descriptor = ColumnFamilyDescriptor::new(Index::NAME, get_cf_options());
        let shred_data_cf_descriptor =
            ColumnFamilyDescriptor::new(ShredData::NAME, get_cf_options());
        let shred_code_cf_descriptor =
            ColumnFamilyDescriptor::new(ShredCode::NAME, get_cf_options());
        let transaction_status_cf_descriptor =
            ColumnFamilyDescriptor::new(TransactionStatus::NAME, get_cf_options());

        let cfs = vec![
            meta_cf_descriptor,
            dead_slots_cf_descriptor,
            erasure_meta_cf_descriptor,
            orphans_cf_descriptor,
            root_cf_descriptor,
            index_cf_descriptor,
            shred_data_cf_descriptor,
            shred_code_cf_descriptor,
            transaction_status_cf_descriptor,
        ];

        // Open the database
        let db = Rocks(DB::open_cf_descriptors(&db_options, path, cfs)?);

        Ok(db)
    }

    fn columns(&self) -> Vec<&'static str> {
        use columns::{
            DeadSlots, ErasureMeta, Index, Orphans, Root, ShredCode, ShredData, SlotMeta,
            TransactionStatus,
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
            TransactionStatus::NAME,
        ]
    }

    fn destroy(path: &Path) -> Result<()> {
        DB::destroy(&Options::default(), path)?;

        Ok(())
    }

    fn cf_handle(&self, cf: &str) -> &ColumnFamily {
        self.0
            .cf_handle(cf)
            .expect("should never get an unknown column")
    }

    fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let opt = self.0.get_cf(cf, key)?.map(|db_vec| db_vec.to_vec());
        Ok(opt)
    }

    fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        self.0.put_cf(cf, key, value)?;
        Ok(())
    }

    fn iterator_cf<C>(
        &self,
        cf: &ColumnFamily,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<DBIterator>
    where
        C: Column,
    {
        let start_key;
        let iterator_mode = match iterator_mode {
            IteratorMode::From(start_from, direction) => {
                start_key = C::key(start_from);
                RocksIteratorMode::From(&start_key, direction)
            }
            IteratorMode::Start => RocksIteratorMode::Start,
            IteratorMode::End => RocksIteratorMode::End,
        };
        let iter = self.0.iterator_cf(cf, iterator_mode)?;
        Ok(iter)
    }

    fn raw_iterator_cf(&self, cf: &ColumnFamily) -> Result<DBRawIterator> {
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

    fn key_size() -> usize {
        std::mem::size_of::<Self::Index>()
    }

    fn key(index: Self::Index) -> Vec<u8>;
    fn index(key: &[u8]) -> Self::Index;
    fn slot(index: Self::Index) -> Slot;
    fn as_index(slot: Slot) -> Self::Index;
}

pub trait TypedColumn: Column {
    type Type: Serialize + DeserializeOwned;
}

impl TypedColumn for columns::TransactionStatus {
    type Type = RpcTransactionStatus;
}

impl Column for columns::TransactionStatus {
    const NAME: &'static str = TRANSACTION_STATUS_CF;
    type Index = (Slot, Signature);

    fn key((slot, index): (Slot, Signature)) -> Vec<u8> {
        let mut key = vec![0; 8 + 64];
        BigEndian::write_u64(&mut key[..8], slot);
        key[8..72].clone_from_slice(&index.as_ref()[0..64]);
        key
    }

    fn index(key: &[u8]) -> (Slot, Signature) {
        let slot = BigEndian::read_u64(&key[..8]);
        let index = Signature::new(&key[8..72]);
        (slot, index)
    }

    fn slot(index: Self::Index) -> Slot {
        index.0
    }

    fn as_index(slot: Slot) -> Self::Index {
        (slot, Signature::default())
    }
}

impl Column for columns::ShredCode {
    const NAME: &'static str = CODE_SHRED_CF;
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
    const NAME: &'static str = DATA_SHRED_CF;
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
    const NAME: &'static str = INDEX_CF;
    type Index = u64;

    fn key(slot: Slot) -> Vec<u8> {
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
    type Type = blockstore_meta::Index;
}

impl Column for columns::DeadSlots {
    const NAME: &'static str = DEAD_SLOTS_CF;
    type Index = u64;

    fn key(slot: Slot) -> Vec<u8> {
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
    const NAME: &'static str = ORPHANS_CF;
    type Index = u64;

    fn key(slot: Slot) -> Vec<u8> {
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
    const NAME: &'static str = ROOT_CF;
    type Index = u64;

    fn key(slot: Slot) -> Vec<u8> {
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
    const NAME: &'static str = META_CF;
    type Index = u64;

    fn key(slot: Slot) -> Vec<u8> {
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
    type Type = blockstore_meta::SlotMeta;
}

impl Column for columns::ErasureMeta {
    const NAME: &'static str = ERASURE_META_CF;
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
    type Type = blockstore_meta::ErasureMeta;
}

#[derive(Debug, Clone)]
pub struct Database {
    backend: Arc<Rocks>,
    path: Arc<Path>,
}

#[derive(Debug, Clone)]
pub struct LedgerColumn<C>
where
    C: Column,
{
    backend: Arc<Rocks>,
    column: PhantomData<C>,
}

pub struct WriteBatch<'a> {
    write_batch: RWriteBatch,
    map: HashMap<&'static str, &'a ColumnFamily>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let backend = Arc::new(Rocks::open(path)?);

        Ok(Database {
            backend,
            path: Arc::from(path),
        })
    }

    pub fn destroy(path: &Path) -> Result<()> {
        Rocks::destroy(path)?;

        Ok(())
    }

    pub fn get<C>(&self, key: C::Index) -> Result<Option<C::Type>>
    where
        C: TypedColumn,
    {
        if let Some(serialized_value) = self.backend.get_cf(self.cf_handle::<C>(), &C::key(key))? {
            let value = deserialize(&serialized_value)?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn iter<'a, C>(
        &'a self,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)> + 'a>
    where
        C: Column,
    {
        let cf = self.cf_handle::<C>();
        let iter = self.backend.iterator_cf::<C>(cf, iterator_mode)?;
        Ok(iter.map(|(key, value)| (C::index(&key), value)))
    }

    #[inline]
    pub fn cf_handle<C>(&self) -> &ColumnFamily
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
    pub fn raw_iterator_cf(&self, cf: &ColumnFamily) -> Result<DBRawIterator> {
        self.backend.raw_iterator_cf(cf)
    }

    pub fn batch(&self) -> Result<WriteBatch> {
        let write_batch = self.backend.batch()?;
        let map = self
            .backend
            .columns()
            .into_iter()
            .map(|desc| (desc, self.backend.cf_handle(desc)))
            .collect();

        Ok(WriteBatch { write_batch, map })
    }

    pub fn write(&self, batch: WriteBatch) -> Result<()> {
        self.backend.write(batch.write_batch)
    }

    pub fn storage_size(&self) -> Result<u64> {
        Ok(fs_extra::dir::get_size(&self.path)?)
    }

    // Adds a range to delete to the given write batch and returns whether or not the column has reached
    // its end
    pub fn delete_range_cf<C>(&self, batch: &mut WriteBatch, from: Slot, to: Slot) -> Result<bool>
    where
        C: Column,
    {
        let cf = self.cf_handle::<C>();
        let from_index = C::as_index(from);
        let to_index = C::as_index(to);
        let result = batch.delete_range_cf::<C>(cf, from_index, to_index);
        let max_slot = self
            .iter::<C>(IteratorMode::End)?
            .next()
            .map(|(i, _)| C::slot(i))
            .unwrap_or(0);
        let end = max_slot <= to;
        result.map(|_| end)
    }
}

impl<C> LedgerColumn<C>
where
    C: Column,
{
    pub fn get_bytes(&self, key: C::Index) -> Result<Option<Vec<u8>>> {
        self.backend.get_cf(self.handle(), &C::key(key))
    }

    pub fn iter<'a>(
        &'a self,
        iterator_mode: IteratorMode<C::Index>,
    ) -> Result<impl Iterator<Item = (C::Index, Box<[u8]>)> + 'a> {
        let cf = self.handle();
        let iter = self.backend.iterator_cf::<C>(cf, iterator_mode)?;
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

    pub fn compact_range(&self, from: Slot, to: Slot) -> Result<bool>
    where
        C::Index: PartialOrd + Copy,
    {
        let cf = self.handle();
        let from = Some(C::key(C::as_index(from)));
        let to = Some(C::key(C::as_index(to)));
        self.backend.0.compact_range_cf(cf, from, to);
        Ok(true)
    }

    #[inline]
    pub fn handle(&self) -> &ColumnFamily {
        self.backend.cf_handle(C::NAME)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> Result<bool> {
        let mut iter = self.backend.raw_iterator_cf(self.handle())?;
        iter.seek_to_first();
        Ok(!iter.valid())
    }

    pub fn put_bytes(&self, key: C::Index, value: &[u8]) -> Result<()> {
        self.backend.put_cf(self.handle(), &C::key(key), value)
    }
}

impl<C> LedgerColumn<C>
where
    C: TypedColumn,
{
    pub fn get(&self, key: C::Index) -> Result<Option<C::Type>> {
        if let Some(serialized_value) = self.backend.get_cf(self.handle(), &C::key(key))? {
            let value = deserialize(&serialized_value)?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn put(&self, key: C::Index, value: &C::Type) -> Result<()> {
        let serialized_value = serialize(value)?;

        self.backend
            .put_cf(self.handle(), &C::key(key), &serialized_value)
    }
}

impl<'a> WriteBatch<'a> {
    pub fn put_bytes<C: Column>(&mut self, key: C::Index, bytes: &[u8]) -> Result<()> {
        self.write_batch
            .put_cf(self.get_cf::<C>(), &C::key(key), bytes)?;
        Ok(())
    }

    pub fn delete<C: Column>(&mut self, key: C::Index) -> Result<()> {
        self.write_batch
            .delete_cf(self.get_cf::<C>(), &C::key(key))?;
        Ok(())
    }

    pub fn put<C: TypedColumn>(&mut self, key: C::Index, value: &C::Type) -> Result<()> {
        let serialized_value = serialize(&value)?;
        self.write_batch
            .put_cf(self.get_cf::<C>(), &C::key(key), &serialized_value)?;
        Ok(())
    }

    #[inline]
    fn get_cf<C: Column>(&self) -> &'a ColumnFamily {
        self.map[C::NAME]
    }

    pub fn delete_range_cf<C: Column>(
        &mut self,
        cf: &ColumnFamily,
        from: C::Index,
        to: C::Index,
    ) -> Result<()> {
        self.write_batch
            .delete_range_cf(cf, C::key(from), C::key(to))?;
        Ok(())
    }
}

fn get_cf_options() -> Options {
    let mut options = Options::default();
    // 256 * 8 = 2GB. 6 of these columns should take at most 12GB of RAM
    options.set_max_write_buffer_number(8);
    options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE as usize);
    let file_num_compaction_trigger = 4;
    // Recommend that this be around the size of level 0. Level 0 estimated size in stable state is
    // write_buffer_size * min_write_buffer_number_to_merge * level0_file_num_compaction_trigger
    // Source: https://docs.rs/rocksdb/0.6.0/rocksdb/struct.Options.html#method.set_level_zero_file_num_compaction_trigger
    let total_size_base = MAX_WRITE_BUFFER_SIZE * file_num_compaction_trigger;
    let file_size_base = total_size_base / 10;
    options.set_level_zero_file_num_compaction_trigger(file_num_compaction_trigger as i32);
    options.set_max_bytes_for_level_base(total_size_base);
    options.set_target_file_size_base(file_size_base);
    options
}

fn get_db_options() -> Options {
    let mut options = Options::default();
    options.create_if_missing(true);
    options.create_missing_column_families(true);
    // A good value for this is the number of cores on the machine
    options.increase_parallelism(sys_info::cpu_num().unwrap() as i32);
    options
}
