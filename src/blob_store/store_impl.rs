use crate::blob_store::slot::{SlotData, SlotFiles, SlotIO, SlotPaths};
use crate::packet::{Blob, BlobError, BLOB_HEADER_SIZE};
use crate::result::Error as SErr;

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};

use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Instant;

use super::*;

const DATA_FILE_NAME: &str = "data";
pub const META_FILE_NAME: &str = "meta";
const INDEX_FILE_NAME: &str = "index";
pub const ERASURE_FILE_NAME: &str = "erasure";
pub const ERASURE_INDEX_FILE_NAME: &str = "erasure_index";

//const DATA_FILE_BUF_SIZE: usize = 2 * 1024 * 1024;
pub const DATA_FILE_BUF_SIZE: usize = 64 * 1024;

pub const INDEX_RECORD_SIZE: u64 = 3 * 8;

impl Store {
    pub(super) fn mk_slot_path(&self, slot_height: u64) -> PathBuf {
        let splat = slot_height.to_be_bytes();
        let mut path = self.root.join(format!("{:#04x}", splat[0]));

        for byte in &splat[1..] {
            path = path.join(format!("{:02x}", byte));
        }

        path
    }

    pub(super) fn mk_data_path(&self, slot_height: u64) -> PathBuf {
        self.mk_slot_path(slot_height).join(DATA_FILE_NAME)
    }

    pub(super) fn mk_index_path(&self, slot_height: u64) -> PathBuf {
        self.mk_slot_path(slot_height).join(INDEX_FILE_NAME)
    }

    pub(super) fn mk_erasure_path(&self, slot_height: u64) -> PathBuf {
        self.mk_slot_path(slot_height).join(ERASURE_FILE_NAME)
    }

    pub(super) fn mk_erasure_index_path(&self, slot: u64) -> PathBuf {
        self.mk_slot_path(slot).join(ERASURE_INDEX_FILE_NAME)
    }

    // TODO: possibly optimize by checking metadata and immediately quiting based on too big indices
    pub(super) fn index_data(
        &self,
        slot_height: u64,
        blob_index: u64,
    ) -> Result<(PathBuf, BlobIndex)> {
        let slot_path = self.mk_slot_path(slot_height);
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
    pub(super) fn index_erasure(
        &self,
        slot: u64,
        erasure_index: u64,
    ) -> Result<(PathBuf, BlobIndex)> {
        let slot_path = self.mk_slot_path(slot);
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
    pub(super) fn insert_blobs<I>(&mut self, iter: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        let mut blobs: Vec<_> = iter.into_iter().collect();
        assert!(!blobs.is_empty());

        // sort on lexi order (slot_idx, blob_idx)
        // TODO: this sort may cause panics while malformed blobs result in `Result`s elsewhere
        blobs.sort_unstable_by_key(|elem| {
            let blob = elem.borrow();
            (
                blob.slot().expect("bad blob"),
                blob.index().expect("bad blob"),
            )
        });

        // contains the indices into blobs of the first blob for that slot

        let mut slot_ranges = HashMap::new();

        for (index, blob) in blobs.iter().enumerate() {
            let slot = blob.borrow().slot().map_err(bad_blob)?;
            slot_ranges
                .entry(slot)
                .and_modify(|r: &mut std::ops::Range<usize>| {
                    r.start = std::cmp::min(r.start, index);
                    r.end = std::cmp::max(r.end, index + 1);
                })
                .or_insert(index..(index + 1));
        }

        let mut slots_to_cache = Vec::with_capacity(slot_ranges.len());

        for (slot, range) in slot_ranges {
            let slot_blobs = &blobs[range];

            //for blob in slot_blobs {
            //}
            let cache = self.cache.read().expect("concurrency error with cache");
            match cache.get(&slot) {
                Some(sio_lock) => {
                    let mut sio = sio_lock.write().expect("concurrency error with SlotIo");
                    sio.insert(slot_blobs, &self.config)?;
                }
                None => {
                    let mut sio = self.open_slot(slot)?;
                    sio.insert(slot_blobs, &self.config)?;
                    slots_to_cache.push(sio);
                }
            }
        }

        // cache slots
        {
            let mut cache = self.cache.write().expect("concurrency error with cache");
            for sio in slots_to_cache {
                cache.insert(sio.slot, RwLock::new(sio));
            }
        }

        Ok(())
    }

    pub(super) fn slot_data(
        &self,
        slot: u64,
        range: std::ops::Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>> + '_> {
        // iterate over index file, gather blob indexes
        // sort by blob_index,

        //let sio = self.open_slot(slot)?;
        //sio.data(range)
        let (data_path, index_path) = {
            let slot_path = self.mk_slot_path(slot);
            (
                slot_path.join(DATA_FILE_NAME),
                slot_path.join(INDEX_FILE_NAME),
            )
        };

        let it = slot::mk_slot_data_iter(&index_path, &data_path, range, self)?;
        Ok(it)
    }

    pub(super) fn mk_paths(&self, slot: u64) -> SlotPaths {
        let slot_dir = self.mk_slot_path(slot);

        SlotPaths {
            meta: slot_dir.join(META_FILE_NAME),
            index: slot_dir.join(INDEX_FILE_NAME),
            data: slot_dir.join(DATA_FILE_NAME),
            erasure_index: slot_dir.join(ERASURE_INDEX_FILE_NAME),
            erasure: slot_dir.join(ERASURE_FILE_NAME),
        }
    }

    pub(super) fn open_slot(&self, slot: u64) -> Result<SlotIO> {
        let paths = self.mk_paths(slot);
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
