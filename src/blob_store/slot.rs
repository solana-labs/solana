use crate::blob_store::store_impl::{
    bad_blob, ensure_slot, mk_paths, open_append, DATA_FILE_BUF_SIZE, INDEX_RECORD_SIZE,
};
use crate::blob_store::{BlobIndex, Result, SlotMeta, StoreConfig};
use crate::entry::Entry;
use crate::packet::{Blob, BlobError, BLOB_HEADER_SIZE};
use crate::result::Error as SErr;

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};

use tokio::{fs as tfs, io as tio, prelude::*};

use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter, Seek, SeekFrom};
use std::iter::{ExactSizeIterator, FusedIterator};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug)]
pub struct SlotIO {
    pub slot: u64,
    pub meta: SlotMeta,
    pub paths: SlotPaths,
    pub files: SlotFiles,
    pub erasure_index_cache: HashMap<u64, BlobIndex>,
    pub index_cache: HashMap<u64, BlobIndex>,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct SlotPaths {
    pub meta: PathBuf,
    pub index: PathBuf,
    pub data: PathBuf,
    pub erasure_index: PathBuf,
    pub erasure: PathBuf,
}

#[derive(Debug)]
pub struct SlotFiles {
    pub meta: File,
    pub index: File,
    pub erasure_index: File,
    pub data: BufWriter<File>,
    pub erasure: BufWriter<File>,
}

#[derive(Debug)]
pub struct SlotData<'a, T> {
    inner: FreeSlotData,
    marker: PhantomData<&'a T>,
}

#[derive(Debug)]
pub struct FreeSlotData {
    f: BufReader<File>,
    idxs: Vec<BlobIndex>,
    pos: u64,
}

impl SlotIO {
    pub fn new(slot: u64, meta: SlotMeta, paths: SlotPaths, files: SlotFiles) -> SlotIO {
        SlotIO {
            slot,
            meta,
            paths,
            files,
            erasure_index_cache: HashMap::new(),
            index_cache: HashMap::new(),
        }
    }

    pub fn insert<B: Borrow<Blob>>(&mut self, blobs: &[B], cfg: &StoreConfig) -> Result<()> {
        insert(&mut self.meta, &mut self.files, &self.paths, blobs, cfg)
    }

    pub fn data(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>> + '_> {
        let it = mk_slot_data_iter(&self.paths.index, &self.paths.data, range)?.bind(self);

        Ok(it)
    }
}

impl FreeSlotData {
    pub fn bind<'a, S>(self, src: &'a S) -> SlotData<'a, S> {
        SlotData {
            inner: self,
            marker: PhantomData,
        }
    }
}

pub fn mk_slot_data_iter<P>(
    index_path: P,
    data_path: P,
    range: std::ops::Range<u64>,
) -> Result<FreeSlotData>
where
    P: AsRef<Path>,
{
    let index_rdr = File::open(&index_path)?;

    let index_size = index_rdr.metadata()?.len();

    let mut index_rdr = BufReader::new(index_rdr);
    let mut buf = [0u8; INDEX_RECORD_SIZE as usize];
    let mut blob_indices: Vec<BlobIndex> =
        Vec::with_capacity((index_size / INDEX_RECORD_SIZE) as usize);

    while let Ok(_) = index_rdr.read_exact(&mut buf) {
        let index = BigEndian::read_u64(&buf[0..8]);
        if index < range.start || range.end <= index {
            continue;
        }

        let offset = BigEndian::read_u64(&buf[8..16]);
        let size = BigEndian::read_u64(&buf[16..24]);
        let blob_idx = BlobIndex {
            index,
            offset,
            size,
        };

        println!("mk_slot_data_iter: blob_idx = {:?}", blob_idx);
        blob_indices.push(blob_idx);
    }

    blob_indices.sort_unstable_by_key(|bix| bix.index);

    let data_rdr = BufReader::new(File::open(&data_path)?);

    Ok(FreeSlotData {
        f: data_rdr,
        idxs: blob_indices,
        pos: 0,
    })
}

impl SlotPaths {
    pub fn new(slot_dir: &Path) -> SlotPaths {
        mk_paths(slot_dir)
    }

    pub fn open(&self) -> Result<SlotFiles> {
        // Ensure slot
        let slot_path = self
            .meta
            .parent()
            .expect("slot files must be in a directory");

        let mut file_opts = OpenOptions::new();
        file_opts.read(true).write(true).create(true);
        let file_opts = file_opts;

        let meta = file_opts.open(&self.meta)?;
        let index = file_opts.open(&self.index)?;
        let erasure_index = file_opts.open(&self.erasure_index)?;
        let data = BufWriter::with_capacity(DATA_FILE_BUF_SIZE, open_append(&self.data)?);
        let erasure = BufWriter::new(open_append(&self.erasure)?);

        Ok(SlotFiles {
            meta,
            index,
            erasure_index,
            data,
            erasure,
        })
    }
}

fn insert<B: Borrow<Blob>>(
    meta: &mut SlotMeta,
    files: &mut SlotFiles,
    paths: &SlotPaths,
    blobs: &[B],
    cfg: &StoreConfig,
) -> Result<()> {
    let mut idx_buf: Vec<u8> = Vec::with_capacity(blobs.len() * INDEX_RECORD_SIZE as usize);
    let mut offset = files.data.seek(SeekFrom::Current(0))?;
    let mut blob_slices_to_write = Vec::with_capacity(blobs.len());

    for blob in blobs {
        let blob = blob.borrow();
        let blob_index = blob.index();
        let blob_size = blob.size();

        let serialized_blob_data = &blob.data[..BLOB_HEADER_SIZE + blob_size];
        let serialized_entry_data = &blob.data[BLOB_HEADER_SIZE..];
        let entry: Entry = bincode::deserialize(serialized_entry_data)
            .expect("Blobs must be well formed by the time they reach the ledger");

        blob_slices_to_write.push(serialized_blob_data);
        let data_len = serialized_blob_data.len() as u64;

        let blob_idx = BlobIndex {
            index: blob_index,
            size: data_len,
            offset,
        };

        offset += data_len;

        // Write indices to buffer, which will be written to index file
        // in the outer (per-slot) loop
        idx_buf.write_u64::<BigEndian>(blob_idx.index)?;
        idx_buf.write_u64::<BigEndian>(blob_idx.offset)?;
        idx_buf.write_u64::<BigEndian>(blob_idx.size)?;

        // update meta. write to file once in outer loop
        if blob_index > meta.received {
            meta.received = blob_index;
        }

        if blob_index == meta.consumed + 1 {
            meta.consumed += 1;
        }

        if entry.is_tick() {
            meta.consumed_ticks = std::cmp::max(entry.tick_height, meta.consumed_ticks);
            meta.is_trunk = meta.contains_all_ticks(cfg);
        }
    }

    // write blob slices
    for slice in blob_slices_to_write {
        files.data.write_all(slice)?;
    }

    files.meta.set_len(0)?;
    bincode::serialize_into(&mut files.meta, &meta)?;
    files.index.write_all(&idx_buf)?;

    files.data.flush()?;
    files.index.flush()?;
    files.meta.flush()?;

    Ok(())
}

impl<'a, T> Iterator for SlotData<'a, T> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.inner.pos >= self.inner.idxs.len() as u64 {
            return None;
        }

        let mut hack = || {
            let bix = self.inner.idxs[self.inner.pos as usize];

            let mut buf = vec![0u8; bix.size as usize];
            self.inner.f.seek(SeekFrom::Start(bix.offset))?;
            self.inner.f.read_exact(&mut buf)?;

            self.inner.pos += 1;
            Ok(buf)
        };
        Some(hack())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let diff = self.inner.idxs.len() - self.inner.pos as usize;
        (diff, Some(diff))
    }
}

impl<'a, T> ExactSizeIterator for SlotData<'a, T> {}

impl<'a, T> FusedIterator for SlotData<'a, T> {}
