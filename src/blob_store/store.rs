use crate::blob_store::recordfile::RecordFile;
use crate::blob_store::slot::{SlotData, SlotIO};
use crate::blob_store::store_impl as simpl;
use crate::blob_store::{Result, SlotMeta, StoreConfig, StoreError};
use crate::entry::Entry;
use crate::packet::{Blob, BLOB_HEADER_SIZE};

use byteorder::{BigEndian, ByteOrder};

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::result::Result as StdRes;

pub const DEFAULT_SLOT_CACHE_SIZE: usize = 10;
pub const DATA_FILE_BUF_SIZE: usize = 64 * 1024;
pub const INDEX_RECORD_SIZE: u64 = 3 * 8;

const SLOT_REC_NAME: &str = "slots";

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    config: StoreConfig,
    cache: SlotCache,
    slots_rec: RecordFile<u64>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key(pub u64, pub u64);

#[derive(Debug)]
pub struct SlotCache {
    max_size: usize,
    map: BTreeMap<u64, SlotIO>,
}

pub trait Storable {
    type ToErr: fmt::Display;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr>;
}

pub trait StorableNoCopy: Storable {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr>;
}

pub trait Retrievable {
    type FromErr: fmt::Display;
    type Output;

    fn from_data(data: &[u8]) -> StdRes<Self::Output, Self::FromErr>;
}

pub trait Named {
    const COLUMN: &'static str;
}

pub trait Column: Named + Storable + Retrievable {}

pub trait ColumnSingle: Named + Storable + Retrievable {}

pub trait ColumnNoCopy: Column + StorableNoCopy {}

impl Store {
    pub fn open<P: AsRef<Path>>(path: &P) -> Result<Store> {
        Store::with_config(path, StoreConfig::default())
    }

    pub fn with_config<P: AsRef<Path>>(path: &P, config: StoreConfig) -> Result<Store> {
        Ok(Store {
            root: PathBuf::from(path.as_ref()),
            config,
            cache: SlotCache::default(),
            slots_rec: RecordFile::open(path.as_ref().join(SLOT_REC_NAME))?,
        })
    }

    pub fn config(&self) -> &StoreConfig {
        &self.config
    }

    #[inline]
    pub fn put<T>(&mut self, key: Key, obj: T) -> Result<()>
    where
        T: Column,
    {
        self.put_dyn(T::COLUMN, key, obj)
    }

    #[inline]
    pub fn put_dyn<T>(&mut self, column: &str, key: Key, obj: T) -> Result<()>
    where
        T: Storable,
    {
        let iter = std::iter::once((key, obj));
        let touched = simpl::insert_blobs(&self.root, column, &mut self.cache, iter)?;

        self.record_slots(&touched)
    }

    #[inline]
    pub fn put_dyn_no_copy<T>(&mut self, column: &str, key: Key, obj: T) -> Result<()>
    where
        T: StorableNoCopy,
    {
        let iter = std::iter::once((key, obj));
        let touched = simpl::insert_blobs_no_copy(&self.root, column, &mut self.cache, iter)?;

        self.record_slots(&touched)
    }

    #[inline]
    pub fn put_no_copy<T>(&mut self, key: Key, obj: T) -> Result<()>
    where
        T: ColumnNoCopy,
    {
        self.put_dyn_no_copy(T::COLUMN, key, obj)
    }

    #[inline]
    pub fn put_many<T, I>(&mut self, iter: I) -> Result<()>
    where
        T: Column,
        I: IntoIterator<Item = (Key, T)>,
    {
        let touched = simpl::insert_blobs(&self.root, T::COLUMN, &mut self.cache, iter)?;

        self.record_slots(&touched)
    }

    #[inline]
    pub fn put_many_no_copy<T, I>(&mut self, iter: I) -> Result<()>
    where
        T: ColumnNoCopy,
        I: IntoIterator<Item = (Key, T)>,
    {
        let touched = simpl::insert_blobs_no_copy(&self.root, T::COLUMN, &mut self.cache, iter)?;

        self.record_slots(&touched)
    }

    #[inline]
    pub fn get<T>(&self, key: Key) -> Result<T::Output>
    where
        T: Column,
    {
        self.get_dyn::<T>(T::COLUMN, key)
    }

    #[inline]
    pub fn get_dyn<T>(&self, column: &str, key: Key) -> Result<T::Output>
    where
        T: Retrievable,
    {
        simpl::get::<T>(&self.root, column, &self.cache, key)
    }

    pub fn get_single<T>(&self, slot: u64) -> Result<T::Output>
    where
        T: ColumnSingle,
    {
        let slot_path = simpl::mk_slot_path(&self.root, slot);
        let path = slot_path.join(T::COLUMN);
        if !slot_path.exists() || !path.exists() {
            return Err(StoreError::NoSuchSlot(slot));
        }

        let mut file = File::open(&path)?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let res = T::from_data(&buf).map_err(_str)?;

        Ok(res)
    }

    pub fn put_single<T>(&mut self, slot: u64, obj: &T) -> Result<()>
    where
        T: ColumnSingle,
    {
        let slot_path = simpl::mk_slot_path(&self.root, slot);
        simpl::ensure_slot(&slot_path)?;
        let path = slot_path.join(T::COLUMN);

        let mut f = OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(&path)?;

        f.write_all(&obj.to_data().map_err(_str)?)?;
        f.sync_data()?;
        Ok(())
    }

    #[inline]
    pub fn slot_range(
        &self,
        column: &str,
        slot: u64,
        range: Range<u64>,
    ) -> Result<SlotData<Store>> {
        simpl::slot_data(self, &self.root, &self.cache, column, slot, range)
    }

    pub fn values_dyn<'a, T>(
        &'a self,
        column: &'a str,
        start: Key,
        end: Key,
    ) -> impl Iterator<Item = Result<T::Output>> + 'a
    where
        T: Retrievable,
    {
        use std::u64;

        let slot_bounds = start.0..end.0;
        self.slots_rec
            .records(slot_bounds)
            .flat_map(move |slot| {
                let start = if slot == start.0 { start.1 } else { 0 };
                let end = if slot == end.0 - 1 { end.1 } else { u64::MAX };

                simpl::slot_data(self, &self.root, &self.cache, column, slot, start..end)
                    .into_iter()
            })
            .flatten()
            .flatten()
            .map(|data| T::from_data(&data).map_err(_str))
    }

    #[inline]
    pub fn values<'a, T>(
        &'a self,
        start: Key,
        end: Key,
    ) -> impl Iterator<Item = Result<T::Output>> + 'a
    where
        T: Column + 'a,
    {
        self.values_dyn::<T>(T::COLUMN, start, end)
    }

    fn record_slots(&mut self, slots: &[u64]) -> Result<()> {
        for slot in slots {
            self.slots_rec.set(*slot, *slot)?;
        }

        Ok(())
    }
}

impl Default for SlotCache {
    fn default() -> SlotCache {
        SlotCache::with_capacity(SlotCache::DEFAULT_CAPACITY)
    }
}

impl SlotCache {
    pub const DEFAULT_CAPACITY: usize = 1024;

    pub fn with_capacity(max_size: usize) -> Self {
        let map = BTreeMap::new();
        SlotCache { map, max_size }
    }

    pub fn get_mut(&mut self, slot: u64) -> Option<&mut SlotIO> {
        self.map.get_mut(&slot)
    }

    pub fn get(&self, slot: u64) -> Option<&SlotIO> {
        self.map.get(&slot)
    }

    /// Returns the `SlotIO` that was popped to make room, if any
    pub fn push(&mut self, sio: SlotIO) -> Option<SlotIO> {
        let popped = if self.map.len() >= self.max_size {
            self.pop()
        } else {
            None
        };

        self.map.insert(sio.slot, sio);
        assert!(self.map.len() <= self.max_size);

        popped
    }

    pub fn pop(&mut self) -> Option<SlotIO> {
        if self.map.is_empty() {
            return None;
        }

        let first_key = *self.map.keys().next().unwrap();
        self.map.remove(&first_key)
    }
}

impl<T> Column for T where T: Named + Storable + Retrievable {}

impl<T> ColumnNoCopy for T where T: Column + StorableNoCopy {}

impl<T> ColumnSingle for T where T: Named + Storable + Retrievable {}

impl<'a, T> Storable for &'a T
where
    T: Storable,
{
    type ToErr = T::ToErr;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr> {
        (*self).to_data()
    }
}

impl<'a, T> StorableNoCopy for &'a T
where
    T: StorableNoCopy,
{
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr> {
        (*self).as_data()
    }
}

impl<'a, T> Retrievable for &'a T
where
    T: Retrievable,
{
    type FromErr = T::FromErr;
    type Output = T::Output;

    fn from_data(data: &[u8]) -> StdRes<Self::Output, Self::FromErr> {
        T::from_data(data)
    }
}

impl<'a, T> Named for &'a T
where
    T: Named,
{
    const COLUMN: &'static str = T::COLUMN;
}

impl Named for SlotMeta {
    const COLUMN: &'static str = "slot-meta";
}

impl Storable for SlotMeta {
    type ToErr = bincode::Error;

    fn to_data(&self) -> StdRes<Vec<u8>, bincode::Error> {
        Ok(bincode::serialize(self)?)
    }
}

impl Retrievable for SlotMeta {
    type FromErr = bincode::Error;
    type Output = SlotMeta;

    fn from_data(data: &[u8]) -> StdRes<SlotMeta, bincode::Error> {
        bincode::deserialize(data)
    }
}

impl Storable for Entry {
    type ToErr = &'static str;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr> {
        let blob = self.to_blob();
        Ok(Vec::from(&blob.data[..BLOB_HEADER_SIZE + blob.size()]))
    }
}

impl Retrievable for Entry {
    type FromErr = bincode::Error;
    type Output = Entry;

    fn from_data(data: &[u8]) -> StdRes<Entry, Self::FromErr> {
        bincode::deserialize(&data[BLOB_HEADER_SIZE..])
    }
}

impl Named for Entry {
    const COLUMN: &'static str = "blob";
}

impl Storable for Blob {
    type ToErr = &'static str;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr> {
        let blob = self.borrow();
        Ok(Vec::from(&blob.data[..BLOB_HEADER_SIZE + self.size()]))
    }
}

impl Retrievable for Blob {
    type FromErr = &'static str;
    type Output = Blob;

    fn from_data(data: &[u8]) -> StdRes<Blob, Self::FromErr> {
        Ok(Blob::new(data))
    }
}

impl Named for Blob {
    const COLUMN: &'static str = "blob";
}

impl StorableNoCopy for Blob {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr> {
        Ok(&self.data[..BLOB_HEADER_SIZE + self.size()])
    }
}

impl Storable for Vec<u8> {
    type ToErr = &'static str;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr> {
        Ok(self.clone())
    }
}

impl<'a> Storable for &'a [u8] {
    type ToErr = &'static str;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr> {
        Ok(self.to_vec())
    }
}

impl<'a> Retrievable for &'a [u8] {
    type FromErr = &'static str;
    type Output = Vec<u8>;

    fn from_data(data: &[u8]) -> StdRes<Vec<u8>, Self::FromErr> {
        Ok(Vec::from(data))
    }
}

impl<'a> StorableNoCopy for &'a [u8] {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr> {
        Ok(self)
    }
}

impl Retrievable for Vec<u8> {
    type FromErr = &'static str;
    type Output = Self;

    fn from_data(data: &[u8]) -> StdRes<Vec<u8>, Self::FromErr> {
        Ok(Vec::from(data))
    }
}

impl StorableNoCopy for Vec<u8> {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr> {
        Ok(&self[..])
    }
}

impl<'a, T> From<&'a T> for Key
where
    Key: From<T>,
    T: Copy,
{
    fn from(k: &'a T) -> Key {
        Key::from(*k)
    }
}
impl From<(u64, u64)> for Key {
    fn from((hi, lo): (u64, u64)) -> Key {
        Key(hi, lo)
    }
}

impl From<([u8; 8], [u8; 8])> for Key {
    fn from((upper_bytes, lower_bytes): ([u8; 8], [u8; 8])) -> Key {
        let upper = BigEndian::read_u64(&upper_bytes);
        let lower = BigEndian::read_u64(&lower_bytes);

        Key(upper, lower)
    }
}

pub fn _str<S: ToString>(s: S) -> StoreError {
    StoreError::Serialization(s.to_string())
}
