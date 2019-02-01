use crate::blob_store::appendvec::AppendVec;
use crate::blob_store::slot::SlotData;
use crate::blob_store::store_impl::{self as simpl, SlotCache};
use crate::blob_store::{Result, SlotMeta, StoreConfig, StoreError};
use crate::packet::{Blob, BLOB_HEADER_SIZE};

use byteorder::{BigEndian, ByteOrder};

use std::borrow::Borrow;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{prelude::*, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::result::Result as StdRes;

pub const DATA_FILE_NAME: &str = "data";
pub const META_FILE_NAME: &str = "meta";
pub const INDEX_FILE_NAME: &str = "index";
pub const ERASURE_FILE_NAME: &str = "erasure";
pub const ERASURE_INDEX_FILE_NAME: &str = "erasure_index";

pub const DEFAULT_SLOT_CACHE_SIZE: usize = 10;
pub const DATA_FILE_BUF_SIZE: usize = 64 * 1024;
pub const INDEX_RECORD_SIZE: u64 = 3 * 8;

#[derive(Debug)]
pub struct Store {
    pub root: PathBuf,
    pub config: StoreConfig,
    pub cache: SlotCache,
    pub slots_mmap: AppendVec<u64>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub upper: u64,
    pub lower: Option<u64>,
}

pub trait Storable {
    type ToErr: fmt::Display;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr>;
}

pub trait StorableNoCopy: Storable {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr>;
}

pub trait Retrievable: Sized {
    type FromErr: fmt::Display;

    fn from_data(data: &[u8]) -> StdRes<Self, Self::FromErr>;
}

pub trait StoreColumn: Retrievable + Storable {
    const COLUMN: &'static str;
    //type ToErr: fmt::Display;
    //type FromErr: fmt::Display;

    //fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr>;
    //fn from_data(data: &[u8]) -> StdRes<Self, Self::FromErr>;
}

pub trait StoreColumnSingle: Sized {
    const COLUMN_NAME: &'static str;

    type ToErr: fmt::Display;
    type FromErr: fmt::Display;

    fn to_data(&self) -> StdRes<Vec<u8>, Self::ToErr>;
    fn from_data(data: &[u8]) -> StdRes<Self, Self::FromErr>;
}

pub trait StoreColumnNoCopy: StoreColumn + StorableNoCopy {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr>;
}

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

impl Store {
    pub fn open<P: AsRef<Path>>(path: &P) -> Result<Store> {
        Store::with_config(path, StoreConfig::default())
    }

    pub fn with_config<P: AsRef<Path>>(path: &P, config: StoreConfig) -> Result<Store> {
        Ok(Store {
            root: PathBuf::from(path.as_ref()),
            config,
            cache: SlotCache::new(),
            slots_mmap: AppendVec::new()?,
        })
    }

    pub fn put_no_copy<K, T>(&mut self, key: K, obj: T) -> Result<()>
    where
        Key: From<K>,
        K: Copy,
        T: StorableNoCopy,
    {
        let iter = std::iter::once((key, obj));
        simpl::insert_blobs_no_copy(&self.root, &mut self.cache, iter)
    }

    pub fn put<K, T>(&mut self, key: K, obj: T) -> Result<()>
    where
        Key: From<K>,
        K: Copy,
        T: Storable,
    {
        let iter = std::iter::once((key, obj));
        simpl::insert_blobs(&self.root, &mut self.cache, iter)
    }

    pub fn put_many<K, T, I>(&mut self, iter: I) -> Result<()>
    where
        Key: From<K>,
        K: Copy,
        T: Storable,
        I: IntoIterator<Item = (K, T)>,
    {
        simpl::insert_blobs(&self.root, &mut self.cache, iter)
    }

    pub fn put_many_no_copy<K, T, I>(&mut self, iter: I) -> Result<()>
    where
        Key: From<K>,
        T: StorableNoCopy,
        K: Copy,
        I: IntoIterator<Item = (K, T)>,
    {
        simpl::insert_blobs_no_copy(&self.root, &mut self.cache, iter)
    }

    pub fn get<K, T>(&self, key: K) -> Result<T>
    where
        Key: From<K>,
        T: Retrievable,
    {
        let key = Key::from(key);
        let (data_path, blob_idx) = simpl::index_data(&self.root, key.upper, key.lower.unwrap())?;

        let mut data_file = File::open(&data_path)?;
        data_file.seek(SeekFrom::Start(blob_idx.offset))?;
        let mut blob_data = vec![0u8; blob_idx.size as usize];
        data_file.read_exact(&mut blob_data)?;

        let x = T::from_data(&blob_data)
            .map_err(|_| StoreError::Serialization("Bad ToData".to_string()))?;

        Ok(x)
    }

    pub fn get_single<T>(&self, slot: u64) -> Result<T>
    where
        T: StoreColumnSingle,
    {
        let slot_path = simpl::mk_slot_path(&self.root, slot);
        let path = slot_path.join(T::COLUMN_NAME);
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
        T: StoreColumnSingle,
    {
        let slot_path = simpl::mk_slot_path(&self.root, slot);
        simpl::ensure_slot(&slot_path)?;
        let path = slot_path.join(T::COLUMN_NAME);

        let mut f = OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(&path)?;

        f.write_all(&obj.to_data().map_err(_str)?)?;
        f.sync_data()?;
        Ok(())
    }

    pub fn slot_range(&self, slot: u64, range: Range<u64>) -> Result<SlotData<Store>> {
        simpl::slot_data(self, &self.root, slot, range)
    }
}

fn _str<S: ToString>(s: S) -> StoreError {
    StoreError::Serialization(s.to_string())
}

impl StoreColumnSingle for SlotMeta {
    type ToErr = bincode::Error;
    type FromErr = bincode::Error;

    const COLUMN_NAME: &'static str = "slot-meta";

    fn to_data(&self) -> StdRes<Vec<u8>, bincode::Error> {
        Ok(bincode::serialize(self)?)
    }

    fn from_data(data: &[u8]) -> StdRes<SlotMeta, bincode::Error> {
        bincode::deserialize(data)
    }
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

    fn from_data(data: &[u8]) -> StdRes<Blob, Self::FromErr> {
        Ok(Blob::new(data))
    }
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

impl<'a> StorableNoCopy for &'a [u8] {
    fn as_data(&self) -> StdRes<&[u8], Self::ToErr> {
        Ok(self)
    }
}

impl Retrievable for Vec<u8> {
    type FromErr = &'static str;

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
        Key {
            upper: hi,
            lower: Some(lo),
        }
    }
}

impl From<([u8; 8], [u8; 8])> for Key {
    fn from((upper_bytes, lower_bytes): ([u8; 8], [u8; 8])) -> Key {
        let upper = BigEndian::read_u64(&upper_bytes);
        let lower = BigEndian::read_u64(&lower_bytes);

        Key {
            upper,
            lower: Some(lower),
        }
    }
}

impl From<(u64, Option<u64>)> for Key {
    fn from((upper, lower): (u64, Option<u64>)) -> Key {
        Key { upper, lower }
    }
}

impl From<([u8; 8], Option<[u8; 8]>)> for Key {
    fn from((upper_bytes, lower_bytes): ([u8; 8], Option<[u8; 8]>)) -> Key {
        let upper = BigEndian::read_u64(&upper_bytes);
        let lower = lower_bytes.map(|lower_bytes| BigEndian::read_u64(&lower_bytes));

        Key { upper, lower }
    }
}
