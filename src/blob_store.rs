//! The `blob_store` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.
// TODO: Remove when implementation complete
// TODO: use parallelism at slots? should be able to max disk eventually...
// probably set up some kind of write queue that can accept 'jobs' via a channel or something
// and then farm
#![allow(unreachable_code, unused_imports, unused_variables, dead_code)]

use crate::db_ledger::SlotMeta;
use crate::entry::Entry;
use crate::packet::Blob;

use byteorder::{BigEndian, ByteOrder};

use serde::Serialize;

use std::borrow::Borrow;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, prelude::*, BufReader, BufWriter, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::result::Result as StdRes;

type Result<T> = StdRes<T, StoreError>;

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    BadBlob,
    NoSuchBlob(u64, u64),
    NoSuchSlot(u64),
    Bincode(bincode::Error),
}

#[derive(Copy, Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
struct BlobIndex {
    pub index: u64,
    pub offset: u64,
    pub size: u64,
}

pub struct Entries;

pub struct DataIter;

impl Store {
    pub fn new<P>(path: &P) -> Store
    where
        P: AsRef<Path>,
    {
        Store {
            root: PathBuf::from(path.as_ref()),
        }
    }

    pub fn put_meta(&self, slot: u64, meta: SlotMeta) -> Result<()> {
        let slot_path = self.mk_slot_path(slot);
        store_impl::ensure_slot(&slot_path)?;

        let meta_path = slot_path.join(store_impl::META_FILE_NAME);

        let mut meta_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&meta_path)?;

        meta_file.write_all(&mut bincode::serialize(&meta)?)?;
        Ok(())
    }

    pub fn get_meta(&self, slot: u64) -> Result<SlotMeta> {
        let slot_path = self.mk_slot_path(slot);
        if !slot_path.exists() {
            return Err(StoreError::NoSuchSlot(slot));
        }

        let meta_path = slot_path.join(store_impl::META_FILE_NAME);

        let mut meta_file = File::open(&meta_path)?;
        let meta = bincode::deserialize_from(&mut meta_file)?;
        Ok(meta)
    }

    pub fn put_blob(&self, blob: &Blob) -> Result<()> {
        self.insert_blobs(std::iter::once(blob))
    }

    pub fn read_blob_data(&self, slot: u64, index: u64, buf: &mut [u8]) -> Result<()> {
        let (data_path, blob_idx) = self.index_data(slot, index)?;

        let mut data_file = BufReader::new(File::open(&data_path)?);
        data_file.seek(SeekFrom::Start(blob_idx.offset))?;
        data_file.read_exact(buf)?;

        Ok(())
    }

    pub fn get_blob_data(&self, slot: u64, index: u64) -> Result<Vec<u8>> {
        let (data_path, blob_idx) = self.index_data(slot, index)?;

        let mut data_file = File::open(&data_path)?;
        data_file.seek(SeekFrom::Start(blob_idx.offset))?;
        let mut blob_data = vec![0u8; blob_idx.size as usize];
        data_file.read_exact(&mut blob_data)?;

        Ok(blob_data)
    }

    pub fn get_blob(&self, slot: u64, index: u64) -> Result<Blob> {
        let blob = Blob::new(&self.get_blob_data(slot, index)?);

        Ok(blob)
    }

    pub fn get_erasure(&self, slot: u64, index: u64) -> Result<Vec<u8>> {
        let (erasure_path, index) = self.index_erasure(slot, index)?;

        let mut erasure_file = File::open(&erasure_path)?;
        erasure_file.seek(SeekFrom::Start(index.offset))?;
        let mut data = vec![0u8; index.size as usize];
        erasure_file.read_exact(&mut data)?;

        Ok(data)
    }

    pub fn put_erasure(&self, slot: u64, index: u64, erasure: &[u8]) -> Result<()> {
        let slot_path = self.mk_slot_path(slot);
        store_impl::ensure_slot(&slot_path)?;

        let (index_path, data_path) = (
            slot_path.join(store_impl::ERASURE_INDEX_FILE_NAME),
            slot_path.join(store_impl::ERASURE_FILE_NAME),
        );

        println!(
            "put_erasure: index_path = {}, slot_path = {}",
            index_path.to_string_lossy(),
            slot_path.to_string_lossy()
        );

        let mut erasure_wtr = BufWriter::new(store_impl::open_append(&data_path)?);
        let mut index_wtr = BufWriter::new(store_impl::open_append(&index_path)?);

        let mut idx_buf = [0u8; store_impl::INDEX_RECORD_SIZE as usize];

        let offset = erasure_wtr.seek(SeekFrom::Current(0))?;
        let size = erasure.len() as u64;

        erasure_wtr.write_all(&erasure)?;

        let idx = BlobIndex {
            index,
            size,
            offset,
        };

        BigEndian::write_u64(&mut idx_buf[0..8], idx.index);
        BigEndian::write_u64(&mut idx_buf[8..16], idx.offset);
        BigEndian::write_u64(&mut idx_buf[16..24], idx.size);

        println!("put_erasure: idx = {:?}", idx);
        index_wtr.write_all(&idx_buf)?;

        erasure_wtr.flush()?;
        index_wtr.flush()?;

        let erasure_file = erasure_wtr.into_inner()?;
        let index_file = index_wtr.into_inner()?;

        erasure_file.sync_data()?;
        index_file.sync_data()?;

        Ok(())
    }

    pub fn put_blobs<'a, I>(&self, iter: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        self.insert_blobs(iter)
    }

    pub fn slot_data_from(
        &self,
        slot: u64,
        range: std::ops::RangeFrom<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>>> {
        self.slot_data(slot, range.start..std::u64::MAX)
    }

    pub fn slot_data_range(
        &self,
        slot: u64,
        range: std::ops::Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>>> {
        self.slot_data(slot, range)
    }

    /// Return an iterator for all the entries in the given file.
    #[allow(unreachable_code)]
    pub fn entries(&self) -> Result<impl Iterator<Item = Result<Entry>>> {
        unimplemented!();
        Ok(Entries)
    }

    pub fn entries_from(&self, start: (u64, u64)) -> Result<impl Iterator<Item = Result<Entry>>> {
        unimplemented!();
        Ok(Entries)
    }

    pub fn entries_range(
        &self,
        (start_slot, start_index): (u64, u64),
        (end_slot, end_index): (u64, u64),
    ) -> Result<impl Iterator<Item = Result<Entry>>> {
        unimplemented!();
        Ok(Entries)
    }

    pub fn destroy<P>(root: &P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        fs::remove_dir_all(root)?;
        Ok(())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StoreError::Io(_) => write!(
                f,
                "Blob-Store Error: I/O. The storage folder may be corrupted"
            ),
            StoreError::BadBlob => write!(
                f,
                "Blob-Store Error: Malformed Blob: The Store may be corrupted"
            ),
            StoreError::NoSuchBlob(slot, index) => write!(
                f,
                "Blob-Store Error: Invalid Blob Index [ {}, {} ]. No such blob exists.",
                slot, index,
            ),

            StoreError::NoSuchSlot(slot) => write!(
                f,
                "Blob-Store Error: Invalid Slot Index [ {} ]. No such slot exists.",
                slot,
            ),
            StoreError::Bincode(_) => write!(f, "Blob-Store Error: internal application error."),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            StoreError::Io(e) => Some(e),
            StoreError::Bincode(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> StoreError {
        StoreError::Io(e)
    }
}

impl From<bincode::Error> for StoreError {
    fn from(e: bincode::Error) -> StoreError {
        StoreError::Bincode(e)
    }
}

impl<W> From<io::IntoInnerError<W>> for StoreError {
    fn from(e: io::IntoInnerError<W>) -> StoreError {
        StoreError::Io(io::Error::from(e))
    }
}

impl Iterator for Entries {
    type Item = Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        unimplemented!()
    }
}

impl Iterator for DataIter {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        unimplemented!()
    }
}

mod store_impl;

#[cfg(test)]
mod tests;
