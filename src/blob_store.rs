//! The `blob_store` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use crate::entry::Entry;
use crate::leader_scheduler::{DEFAULT_BOOTSTRAP_HEIGHT, TICKS_PER_BLOCK};
use crate::packet::{self, Blob};

use serde::Serialize;

use std::borrow::Borrow;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, prelude::*, BufReader, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::result::Result as StdRes;

use store::Key;

mod appendvec;
mod slot;
pub mod store;
mod store_impl;

type Result<T> = StdRes<T, StoreError>;

pub const DEFAULT_BLOCKS_PER_SLOT: u64 = 1;

#[derive(Debug)]
pub struct BlobStore {
    store: store::Store,
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
    Serialization(String),
}

#[derive(Copy, Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct BlobIndex {
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

impl BlobStore {
    pub fn open<P>(path: &P) -> Result<BlobStore>
    where
        P: AsRef<Path>,
    {
        BlobStore::open_config(path, StoreConfig::default())
    }

    pub fn open_config<P>(path: &P, config: StoreConfig) -> Result<BlobStore>
    where
        P: AsRef<Path>,
    {
        Ok(BlobStore {
            store: store::Store::with_config(path, config)?,
        })
    }

    pub fn get_config(&self) -> &StoreConfig {
        &self.store.config
    }

    pub fn put_meta(&mut self, slot: u64, meta: SlotMeta) -> Result<()> {
        self.store.put_single(slot, &meta)
    }

    pub fn get_meta(&self, slot: u64) -> Result<SlotMeta> {
        self.store.get_single(slot)
    }

    pub fn put_blob(&mut self, blob: &Blob) -> Result<()> {
        let key = Key::from((blob.slot(), blob.index()));

        self.store.put(key, blob)
    }

    pub fn read_blob_data(&self, slot: u64, index: u64, buf: &mut [u8]) -> Result<()> {
        let (data_path, blob_idx) = store_impl::index_data(&self.store.root, slot, index)?;

        let mut data_file = BufReader::new(File::open(&data_path)?);
        data_file.seek(SeekFrom::Start(blob_idx.offset))?;
        data_file.read_exact(buf)?;

        Ok(())
    }

    pub fn get_blob_data(&self, slot: u64, index: u64) -> Result<Vec<u8>> {
        self.store.get(&(slot, index))
    }

    pub fn get_blob(&self, slot: u64, index: u64) -> Result<Blob> {
        let blob = Blob::new(&self.get_blob_data(slot, index)?);

        Ok(blob)
    }

    pub fn get_erasure(&self, slot: u64, index: u64) -> Result<Vec<u8>> {
        self.store.get(&(slot, index))
    }

    pub fn put_erasure(&mut self, slot: u64, index: u64, erasure: &[u8]) -> Result<()> {
        let key = Key::from((slot, index));

        self.store.put_no_copy(&key, erasure)
    }

    pub fn put_blobs<'a, I>(&'a mut self, iter: I) -> Result<()>
    where
        I: IntoIterator<Item = &'a Blob>,
        //I::Item: Borrow<Blob>,
    {
        let mut slots = HashMap::new();

        let iter = iter.into_iter().map(|b| {
            let blob = b.borrow();
            let (slot, index) = (blob.slot(), blob.index());
            let key = Key::from((slot, index));
            slots.entry(slot).or_insert_with(Vec::new).push((index, b));

            (key, blob)
        });

        let ret = self.store.put_many_no_copy(iter);
        for (slot, indices) in slots {
            let mut meta = match self.get_meta(slot) {
                Ok(m) => m,
                Err(StoreError::NoSuchSlot(_)) => SlotMeta::new(slot, DEFAULT_BLOCKS_PER_SLOT),
                Err(e) => {
                    return Err(e);
                }
            };

            for (blob_index, bl) in indices {
                //update meta. write to file once in outer loop
                if blob_index > meta.received {
                    meta.received = blob_index;
                }

                if blob_index == meta.consumed + 1 {
                    meta.consumed += 1;
                }

                let serialized_entry_data = &bl.data[crate::packet::BLOB_HEADER_SIZE..];
                let entry: Entry = bincode::deserialize(serialized_entry_data)
                    .expect("Blobs must be well formed by the time they reach the ledger");

                if entry.is_tick() {
                    meta.consumed_ticks = std::cmp::max(entry.tick_height, meta.consumed_ticks);
                    meta.is_trunk = meta.contains_all_ticks(&self.store.config);
                }
            }
            self.put_meta(slot, meta)?;
        }

        ret
    }

    pub fn slot_data_from(
        &self,
        slot: u64,
        start_index: u64,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>> + '_> {
        self.slot_data_range(slot, start_index..std::u64::MAX)
    }

    pub fn slot_data_range(
        &self,
        slot: u64,
        range: Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>> + '_> {
        self.store.slot_range(slot, range)
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
    ) -> Result<impl Iterator<Item = Result<Entry>> + '_> {
        self.slot_entries_range(slot, start_index..std::u64::MAX)
    }

    pub fn slot_entries_range(
        &self,
        slot: u64,
        range: Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Entry>> + '_> {
        let iter = self.store.slot_range(slot, range)?.map(|blob_data| {
            let entry_data = &blob_data?[packet::BLOB_HEADER_SIZE..];
            let entry =
                bincode::deserialize(entry_data).expect("Blobs must be well formed in the ledger");

            Ok(entry)
        });

        Ok(iter)
    }

    /// Return an iterator for all the entries in the given file.
    #[allow(unreachable_code)]
    pub fn entries(&self) -> Result<Entries> {
        unimplemented!();
    }

    pub fn entries_from(&self, _start: (u64, u64)) -> Result<Entries> {
        unimplemented!();
    }

    pub fn entries_range(&self, _start: (u64, u64), _end: (u64, u64)) -> Result<Entries> {
        unimplemented!();
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
            StoreError::Serialization(_) | StoreError::Bincode(_) => {
                write!(f, "Blob-Store Error: internal application error.")
            }
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
