use crate::blob_store::slot::{self, FreeSlotData, SlotData, SlotFiles, SlotIO, SlotPaths};
use crate::blob_store::DEFAULT_BLOCKS_PER_SLOT;
use crate::blob_store::{BlobIndex, Result, SlotMeta, Store, StoreConfig, StoreError};
use crate::packet::{Blob, BlobError, BLOB_HEADER_SIZE};
use crate::result::Error as SErr;

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};

use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter, Seek, SeekFrom};
use std::marker::PhantomData;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Instant;

pub const DATA_FILE_NAME: &str = "data";
pub const META_FILE_NAME: &str = "meta";
pub const INDEX_FILE_NAME: &str = "index";
pub const ERASURE_FILE_NAME: &str = "erasure";
pub const ERASURE_INDEX_FILE_NAME: &str = "erasure_index";

pub const DEFAULT_SLOT_CACHE_SIZE: usize = 10;
pub const DATA_FILE_BUF_SIZE: usize = 64 * 1024;
pub const INDEX_RECORD_SIZE: u64 = 3 * 8;

#[derive(Debug)]
pub struct Data<'a, S> {
    slot_iter: Option<FreeSlotData>,
    marker: PhantomData<&'a S>,
}

#[derive(Debug)]
pub struct SlotCache {
    max_size: usize,
    map: BTreeMap<u64, SlotIO>,
}

impl SlotCache {
    pub const DEFAULT_CAPACITY: usize = 1024;

    pub fn new() -> SlotCache {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(max_size: usize) -> Self {
        let map = BTreeMap::new();
        SlotCache { map, max_size }
    }

    pub fn get(&self, slot: u64) -> Option<&SlotIO> {
        self.map.get(&slot)
    }

    pub fn get_mut(&mut self, slot: u64) -> Option<&mut SlotIO> {
        self.map.get_mut(&slot)
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

pub fn mk_slot_path(root: &Path, slot: u64) -> PathBuf {
    let splat = slot.to_be_bytes();
    let mut path = root.join(format!("{:#04x}", splat[0]));

    for byte in &splat[1..] {
        path = path.join(format!("{:02x}", byte));
    }

    path
}

pub fn index_data(root: &Path, slot_height: u64, blob_index: u64) -> Result<(PathBuf, BlobIndex)> {
    let slot_path = mk_slot_path(root, slot_height);
    if !slot_path.exists() {
        return Err(StoreError::NoSuchSlot(slot_height));
    }

    let (data_path, index_path) = (
        slot_path.join(DATA_FILE_NAME),
        slot_path.join(INDEX_FILE_NAME),
    );

    let mut index_file = BufReader::new(File::open(&index_path)?);

    let mut buf = [0u8; INDEX_RECORD_SIZE as usize];
    while let Ok(_) = index_file.read_exact(&mut buf) {
        let index = BigEndian::read_u64(&buf[0..8]);
        if index == blob_index {
            let offset = BigEndian::read_u64(&buf[8..16]);
            let size = BigEndian::read_u64(&buf[16..24]);
            return Ok((
                data_path,
                BlobIndex {
                    index,
                    offset,
                    size,
                },
            ));
        }
    }

    Err(StoreError::NoSuchBlob(slot_height, blob_index))
}
// TODO: possibly optimize by checking metadata and immediately quiting based on too big indices
pub fn index_erasure(root: &Path, slot: u64, erasure_index: u64) -> Result<(PathBuf, BlobIndex)> {
    let slot_path = mk_slot_path(&root, slot);
    if !slot_path.exists() {
        return Err(StoreError::NoSuchSlot(slot));
    }

    let (erasure_path, index_path) = (
        slot_path.join(ERASURE_FILE_NAME),
        slot_path.join(ERASURE_INDEX_FILE_NAME),
    );

    let mut index_file = BufReader::new(File::open(&index_path)?);

    let mut buf = [0u8; INDEX_RECORD_SIZE as usize];
    while let Ok(_) = index_file.read_exact(&mut buf) {
        let index = BigEndian::read_u64(&buf[0..8]);
        if index == erasure_index {
            let offset = BigEndian::read_u64(&buf[8..16]);
            let size = BigEndian::read_u64(&buf[16..24]);
            return Ok((
                erasure_path,
                BlobIndex {
                    index,
                    offset,
                    size,
                },
            ));
        }
    }

    Err(StoreError::NoSuchBlob(slot, erasure_index))
}

#[allow(clippy::range_plus_one)]
pub fn insert_blobs<I>(
    root: &Path,
    cache: &mut SlotCache,
    config: &StoreConfig,
    iter: I,
) -> Result<()>
where
    I: IntoIterator,
    I::Item: Borrow<Blob>,
{
    let mut blobs: Vec<_> = iter.into_iter().collect();
    assert!(!blobs.is_empty());

    // sort on lexi order (slot_idx, blob_idx)
    blobs.sort_unstable_by_key(|elem| {
        let blob = elem.borrow();
        (blob.slot(), blob.index())
    });

    // contains the indices into blobs of the first blob for that slot
    let mut slot_ranges = HashMap::new();

    for (index, blob) in blobs.iter().enumerate() {
        let slot = blob.borrow().slot();
        slot_ranges
            .entry(slot)
            .and_modify(|r: &mut Range<usize>| {
                r.start = std::cmp::min(r.start, index);
                r.end = std::cmp::max(r.end, index + 1);
            })
            .or_insert(index..(index + 1));
    }

    let mut slots_to_cache = Vec::new();

    for (slot, range) in slot_ranges {
        let slot_blobs = &blobs[range];

        match cache.get_mut(slot) {
            Some(sio) => {
                sio.insert(slot_blobs, config)?;
            }
            None => {
                let slot_path = mk_slot_path(root, slot);
                ensure_slot(&slot_path)?;
                let mut sio = open_slot(&slot_path, slot)?;
                sio.insert(slot_blobs, config)?;
                slots_to_cache.push(sio);
            }
        }
    }

    // cache slots
    for sio in slots_to_cache {
        cache.push(sio);
    }

    Ok(())
}

pub fn slot_data<'a, S>(
    src: &'a S,
    root: &Path,
    slot: u64,
    range: std::ops::Range<u64>,
) -> Result<SlotData<'a, S>> {
    let (data_path, index_path) = {
        let slot_path = mk_slot_path(root, slot);
        (
            slot_path.join(DATA_FILE_NAME),
            slot_path.join(INDEX_FILE_NAME),
        )
    };

    let it = slot::mk_slot_data_iter(&index_path, &data_path, range)?;
    Ok(it.bind(src))
}

pub fn mk_paths(slot_dir: &Path) -> SlotPaths {
    SlotPaths {
        meta: slot_dir.join(META_FILE_NAME),
        index: slot_dir.join(INDEX_FILE_NAME),
        data: slot_dir.join(DATA_FILE_NAME),
        erasure_index: slot_dir.join(ERASURE_INDEX_FILE_NAME),
        erasure: slot_dir.join(ERASURE_FILE_NAME),
    }
}

pub fn open_slot(slot_path: &Path, slot: u64) -> Result<SlotIO> {
    let paths = SlotPaths::new(&slot_path);
    let new_meta = !paths.meta.exists();
    let mut files = paths.open()?;

    let meta = if new_meta {
        SlotMeta::new(slot, DEFAULT_BLOCKS_PER_SLOT)
    } else {
        let meta = bincode::deserialize_from(&mut files.meta)?;
        files.meta.seek(SeekFrom::Start(0))?;
        meta
    };

    Ok(SlotIO::new(slot, meta, paths, files))
}

pub fn bad_blob(err: SErr) -> StoreError {
    match err {
        SErr::BlobError(BlobError::BadState) => StoreError::BadBlob,
        _ => panic!("Swallowing error in blob store impl: {}", err),
    }
}

pub fn open_append<P>(path: P) -> Result<File>
where
    P: AsRef<Path>,
{
    let f = OpenOptions::new().append(true).create(true).open(path)?;

    Ok(f)
}

pub fn ensure_slot<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();

    if !path.exists() {
        fs::create_dir_all(&path)?;
    }

    Ok(())
}
