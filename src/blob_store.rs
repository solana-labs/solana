//! The `blob_store` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.
// TODO: Remove when implementation complete
// TODO: use parallelism at slots? should be able to max disk eventually...
// probably set up some kind of write queue that can accept 'jobs' via a channel or something
// and then farm
#![allow(unreachable_code, unused_imports, unused_variables, dead_code)]

use crate::entry::Entry;
use crate::leader_scheduler::{DEFAULT_BOOTSTRAP_HEIGHT, TICKS_PER_BLOCK};
use crate::packet::{self, Blob};

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

pub const DEFAULT_BLOCKS_PER_SLOT: u64 = 32;

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    config: StoreConfig,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct StoreConfig {
    pub ticks_per_block: u64,
    pub num_bootstrap_ticks: u64,
    pub num_blocks_per_slot: u64,
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

/// Per-slot meta-data
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SlotMeta {
    /// The total number of consecutive blob starting from index 0
    /// we have received for this slot.
    pub consumed: u64,
    /// The entry height of the highest blob received for this slot.
    pub received: u64,
    /// The index of thist slot
    pub slot_index: u64,
    /// Tick height of the highest received blob. Used to detect when a slot is full
    pub consumed_ticks: u64,
    /// Number of blocks in this slot
    pub num_blocks: u64, // TODO: once blobs have a `num_blocks` property, the first blob for this slot will determine this
    /// List of slots that chain to this slot
    pub next_slots: Vec<u64>, //TODO: slot_x.slot_index - `num_blocks` should have this `slot_index` in it's next_slots if it's incomplete
    /// True iff all blocks from `0..self.slot_index` are full
    pub is_trunk: bool, // TODO: true iff this slot is complete plus all slots in its chain are complete
}

pub struct Entries;

pub struct DataIter;

impl Store {
    pub fn open<P>(path: &P) -> Store
    where
        P: AsRef<Path>,
    {
        Store::open_config(path, StoreConfig::default())
    }

    pub fn open_config<P>(path: &P, config: StoreConfig) -> Store
    where
        P: AsRef<Path>,
    {
        Store {
            root: PathBuf::from(path.as_ref()),
            config,
        }
    }

    pub fn get_config(&self) -> &StoreConfig {
        &self.config
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

        meta_file.write_all(&bincode::serialize(&meta)?)?;
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

        index_wtr.write_all(&idx_buf)?;

        erasure_wtr.flush()?;
        index_wtr.flush()?;

        let erasure_file = erasure_wtr.into_inner()?;
        let index_file = index_wtr.into_inner()?;

        erasure_file.sync_data()?;
        index_file.sync_data()?;

        Ok(())
    }

    pub fn put_blobs<I>(&self, iter: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        self.insert_blobs(iter)
    }

    pub fn slot_data_from(
        &self,
        slot: u64,
        start_index: u64,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>>> {
        self.slot_data(slot, start_index..std::u64::MAX)
    }

    pub fn slot_data_range(
        &self,
        slot: u64,
        range: std::ops::Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>>> {
        self.slot_data(slot, range)
    }

    /// Returns the entry vector for the slot starting with `entry_start_index`, capping the result
    /// at `max` if `max_entries == Some(max)`, otherwise, no upper limit on the length of the return
    /// vector is imposed.
    pub fn get_slot_entries(
        &self,
        slot: u64,
        entry_start_index: usize,
        max_entries: Option<u64>,
    ) -> Result<Vec<Entry>> {
        let range = (entry_start_index as u64)..(max_entries.unwrap_or(std::u64::MAX));

        let entries = self
            .slot_entries_range(slot, range)?
            .collect::<Result<Vec<Entry>>>()?;

        Ok(entries)
    }

    pub fn slot_entries_from(
        &self,
        slot: u64,
        start_index: u64,
    ) -> Result<impl Iterator<Item = Result<Entry>>> {
        self.slot_entries_range(slot, start_index..std::u64::MAX)
    }

    pub fn slot_entries_range(
        &self,
        slot: u64,
        range: std::ops::Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Entry>>> {
        let iter = self.slot_data(slot, range)?.map(|blob_data| {
            let entry_data = &blob_data?[packet::BLOB_HEADER_SIZE..];
            let entry =
                bincode::deserialize(entry_data).expect("Blobs must be well formed in the ledger");

            Ok(entry)
        });

        Ok(iter)
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

impl Default for StoreConfig {
    fn default() -> Self {
        StoreConfig {
            ticks_per_block: TICKS_PER_BLOCK,
            num_bootstrap_ticks: DEFAULT_BOOTSTRAP_HEIGHT,
            num_blocks_per_slot: DEFAULT_BLOCKS_PER_SLOT,
        }
    }
}

impl Default for SlotMeta {
    fn default() -> SlotMeta {
        SlotMeta {
            consumed: 0,
            received: 0,
            slot_index: 0,
            consumed_ticks: 0,
            num_blocks: 1,
            next_slots: Vec::new(),
            is_trunk: false,
        }
    }
}

impl SlotMeta {
    pub fn new(slot: u64, num_blocks: u64) -> SlotMeta {
        SlotMeta {
            slot_index: slot,
            num_blocks,
            ..SlotMeta::default()
        }
    }

    pub fn num_expected_ticks(&self, config: &StoreConfig) -> u64 {
        if self.slot_index == 0 {
            config.num_bootstrap_ticks
        } else {
            config.ticks_per_block * self.num_blocks
        }
    }

    pub fn contains_all_ticks(&self, config: &StoreConfig) -> bool {
        if self.num_blocks == 0 {
            // A placeholder slot does not contain all the ticks
            false
        } else {
            self.num_expected_ticks(config) <= self.consumed_ticks + 1
        }
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

pub fn get_tmp_store_path(name: &str) -> Result<PathBuf> {
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::env;

    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let keypair = Keypair::new();

    let path: PathBuf = [
        out_dir,
        "tmp".into(),
        format!("store-{}-{}", name, keypair.pubkey()),
    ]
    .iter()
    .collect();

    // whack any possible collision
    let _ignore = fs::remove_dir_all(&path);
    fs::create_dir_all(&path)?;

    Ok(path)
}
