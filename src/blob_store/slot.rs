use crate::blob_store::store::{Key, StorableNoCopy};
use crate::blob_store::store_impl::{
    ensure_slot, mk_paths, open_append, DATA_FILE_BUF_SIZE, INDEX_RECORD_SIZE,
};
use crate::blob_store::{BlobIndex, Result, StoreError};

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};

use std::fs::{File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter, Seek, SeekFrom};
use std::iter::{ExactSizeIterator, FusedIterator};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SlotIO {
    pub slot: u64,
    pub paths: SlotPaths,
    pub files: SlotFiles,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct SlotPaths {
    pub slot: PathBuf,
    pub index: PathBuf,
    pub data: PathBuf,
}

#[derive(Debug)]
pub struct SlotFiles {
    pub index: File,
    pub data: BufWriter<File>,
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
    pub fn new(slot: u64, paths: SlotPaths, files: SlotFiles) -> SlotIO {
        SlotIO { slot, paths, files }
    }

    pub fn insert<K>(&mut self, blobs: &[(K, Vec<u8>)]) -> Result<()>
    where
        Key: From<K>,
        K: Copy,
    {
        insert(&mut self.files, blobs)
    }

    pub fn insert_no_copy<K, T>(&mut self, blobs: &[(K, T)]) -> Result<()>
    where
        Key: From<K>,
        K: Copy,
        T: StorableNoCopy,
    {
        insert_no_copy(&mut self.files, blobs)
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
    pub fn bind<S>(self, _: &S) -> SlotData<S> {
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
        //let slot_path = self
        //.meta
        //.parent()
        //.expect("slot files must be in a directory");
        ensure_slot(&self.slot)?;

        let mut file_opts = OpenOptions::new();
        file_opts.read(true).write(true).create(true);
        let file_opts = file_opts;

        let index = file_opts.open(&self.index)?;
        let data = BufWriter::with_capacity(DATA_FILE_BUF_SIZE, open_append(&self.data)?);

        Ok(SlotFiles { index, data })
    }
}

fn insert<K>(files: &mut SlotFiles, blobs: &[(K, Vec<u8>)]) -> Result<()>
where
    Key: From<K>,
    K: Copy,
{
    let mut idx_buf: Vec<u8> = Vec::with_capacity(blobs.len() * INDEX_RECORD_SIZE as usize);
    let mut offset = files.data.seek(SeekFrom::Current(0))?;
    let mut blob_slices_to_write = Vec::with_capacity(blobs.len());

    for (key, blob) in blobs {
        let key = Key::from(*key);
        //let blob = blob
        //.to_data()
        //.map_err(|_| StoreError::Serialization("Bad Blob".to_string()))?;
        let blob_index = key.lower.expect("Single items should never get here");

        let serialized_blob_data = &blob[..];
        //let serialized_entry_data = &blob.data[BLOB_HEADER_SIZE..];
        //let entry: Entry = bincode::deserialize(serialized_entry_data)
        //.expect("Blobs must be well formed by the time they reach the ledger");

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
        //if blob_index > meta.received {
        //meta.received = blob_index;
        //}

        //if blob_index == meta.consumed + 1 {
        //meta.consumed += 1;
        //}

        //if entry.is_tick() {
        //meta.consumed_ticks = std::cmp::max(entry.tick_height, meta.consumed_ticks);
        //meta.is_trunk = meta.contains_all_ticks(cfg);
        //}
    }

    // write blob slices
    for slice in blob_slices_to_write {
        files.data.write_all(slice)?;
    }

    //files.meta.set_len(0)?;
    //bincode::serialize_into(&mut files.meta, &meta)?;
    files.index.write_all(&idx_buf)?;

    files.data.flush()?;
    files.index.flush()?;
    //files.meta.flush()?;

    Ok(())
}

fn insert_no_copy<K, T>(files: &mut SlotFiles, blobs: &[(K, T)]) -> Result<()>
where
    Key: From<K>,
    K: Copy,
    T: StorableNoCopy,
{
    let mut idx_buf: Vec<u8> = Vec::with_capacity(blobs.len() * INDEX_RECORD_SIZE as usize);
    let mut offset = files.data.seek(SeekFrom::Current(0))?;
    let mut blob_slices_to_write = Vec::with_capacity(blobs.len());

    for (key, blob) in blobs {
        let key = Key::from(*key);
        //let blob = blob
        //.to_data()
        //.map_err(|_| StoreError::Serialization("Bad Blob".to_string()))?;
        let blob_index = key.lower.expect("Single items should never get here");

        let serialized_blob_data = blob
            .as_data()
            .map_err(|_| StoreError::Serialization("Bad ToData Impl".to_string()))?;
        //let serialized_entry_data = &blob.data[BLOB_HEADER_SIZE..];
        //let entry: Entry = bincode::deserialize(serialized_entry_data)
        //.expect("Blobs must be well formed by the time they reach the ledger");

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

        files.data.write_all(serialized_blob_data)?;
        // update meta. write to file once in outer loop
        //if blob_index > meta.received {
        //meta.received = blob_index;
        //}

        //if blob_index == meta.consumed + 1 {
        //meta.consumed += 1;
        //}

        //if entry.is_tick() {
        //meta.consumed_ticks = std::cmp::max(entry.tick_height, meta.consumed_ticks);
        //meta.is_trunk = meta.contains_all_ticks(cfg);
        //}
    }

    // write blob slices
    //for slice in blob_slices_to_write {
    //files.data.write_all(slice)?;
    //}

    //files.meta.set_len(0)?;
    //bincode::serialize_into(&mut files.meta, &meta)?;
    files.index.write_all(&idx_buf)?;

    files.data.flush()?;
    files.index.flush()?;
    //files.meta.flush()?;

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
