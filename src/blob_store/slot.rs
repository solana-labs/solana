use crate::blob_store::store::{Key, Retrievable, StorableNoCopy};
use crate::blob_store::store_impl::{
    ensure_slot, open_append, DATA_FILE_BUF_SIZE, INDEX_RECORD_SIZE,
};
use crate::blob_store::{BlobIndex, Result, StoreError};

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter, Seek, SeekFrom};
use std::iter::{ExactSizeIterator, FusedIterator};
use std::marker::PhantomData;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SlotIO {
    pub slot: u64,
    pub slot_path: PathBuf,
    pub columns: HashMap<String, ColumnIO>,
}

#[derive(Debug)]
pub struct ColumnIO {
    pub paths: SlotPaths,
    pub files: SlotFiles,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct SlotPaths {
    pub index: PathBuf,
    pub data: PathBuf,
}

#[derive(Debug)]
pub struct SlotFiles {
    pub index: File,
    pub data: File,
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
    pub fn open(slot: u64, slot_path: PathBuf) -> Result<SlotIO> {
        ensure_slot(&slot_path)?;

        Ok(SlotIO {
            slot,
            slot_path,
            columns: HashMap::new(),
        })
    }

    pub fn get<T>(&self, column: &str, key: Key) -> Result<T::Output>
    where
        T: Retrievable,
    {
        match self.columns.get(column) {
            Some(cio) => cio.get::<T>(key),
            None => {
                let cio = ColumnIO::open(&self.slot_path, column)?;
                cio.get::<T>(key)
            }
        }
    }

    pub fn insert(&mut self, column: &str, blobs: &[(Key, Vec<u8>)]) -> Result<()> {
        match self.columns.get_mut(column) {
            Some(cio) => cio.insert(blobs),
            None => {
                let mut cio = ColumnIO::open(&self.slot_path, column)?;
                let ret = cio.insert(blobs);
                self.columns.insert(column.to_string(), cio);
                ret
            }
        }
    }

    pub fn insert_no_copy<T>(&mut self, column: &str, blobs: &[(Key, T)]) -> Result<()>
    where
        T: StorableNoCopy,
    {
        match self.columns.get_mut(column) {
            Some(cio) => cio.insert_no_copy(blobs),
            None => {
                let mut cio = ColumnIO::open(&self.slot_path, column)?;
                let ret = cio.insert_no_copy(blobs);
                self.columns.insert(column.to_string(), cio);
                ret
            }
        }
    }

    pub fn data(&self, column: &str, range: Range<u64>) -> Result<FreeSlotData> {
        match self.columns.get(column) {
            Some(cio) => cio.data(range),
            None => {
                let cio = ColumnIO::open(&self.slot_path, column)?;
                cio.data(range)
            }
        }
    }
}

impl ColumnIO {
    pub fn new(paths: SlotPaths, files: SlotFiles) -> ColumnIO {
        ColumnIO { paths, files }
    }

    pub fn open(slot_path: &Path, column: &str) -> Result<ColumnIO> {
        let paths = SlotPaths::new(slot_path, column);
        let files = paths.open()?;

        Ok(ColumnIO { paths, files })
    }

    pub fn insert(&mut self, blobs: &[(Key, Vec<u8>)]) -> Result<()>
where {
        insert(&mut self.files, blobs)
    }

    pub fn insert_no_copy<T>(&mut self, blobs: &[(Key, T)]) -> Result<()>
    where
        T: StorableNoCopy,
    {
        insert_no_copy(&mut self.files, blobs)
    }

    pub fn get<T>(&self, key: Key) -> Result<T::Output>
    where
        T: Retrievable,
    {
        let blob_idx = self.index(key.0, key.1)?;

        let mut data_rdr = File::open(&self.paths.data)?;
        data_rdr.seek(SeekFrom::Start(blob_idx.offset))?;
        let mut blob_data = vec![0u8; blob_idx.size as usize];
        data_rdr.read_exact(&mut blob_data)?;

        let x = T::from_data(&blob_data)
            .map_err(|_| StoreError::Serialization("Bad ToData".to_string()))?;

        Ok(x)
    }

    fn index(&self, slot: u64, idx: u64) -> Result<BlobIndex> {
        let mut index_file = BufReader::new(File::open(&self.paths.index)?);

        let mut buf = [0u8; INDEX_RECORD_SIZE as usize];
        while let Ok(_) = index_file.read_exact(&mut buf) {
            let index = BigEndian::read_u64(&buf[0..8]);
            if index == idx {
                let offset = BigEndian::read_u64(&buf[8..16]);
                let size = BigEndian::read_u64(&buf[16..24]);
                return Ok(BlobIndex {
                    index,
                    offset,
                    size,
                });
            }
        }

        Err(StoreError::NoSuchBlob(slot, idx))
    }

    pub fn data(&self, range: Range<u64>) -> Result<FreeSlotData> {
        mk_slot_data_iter(&self.paths.index, &self.paths.data, range)
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

impl SlotPaths {
    const INDEX_EXT: &'static str = ".index";

    pub fn new(slot_dir: &Path, column: &str) -> SlotPaths {
        let index = slot_dir.join([column, Self::INDEX_EXT].concat());
        let data = slot_dir.join(column);

        SlotPaths { index, data }
    }

    pub fn open(&self) -> Result<SlotFiles> {
        let mut file_opts = OpenOptions::new();
        file_opts.read(true).write(true).create(true);
        let file_opts = file_opts;

        let index = file_opts.open(&self.index)?;
        let data = open_append(&self.data)?;

        Ok(SlotFiles { index, data })
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

fn insert<K>(files: &mut SlotFiles, blobs: &[(K, Vec<u8>)]) -> Result<()>
where
    Key: From<K>,
    K: Copy,
{
    let mut idx_buf: Vec<u8> = Vec::with_capacity(blobs.len() * INDEX_RECORD_SIZE as usize);
    let mut offset = files.data.seek(SeekFrom::Current(0))?;
    let mut blob_slices_to_write = Vec::with_capacity(blobs.len());
    let mut data_wtr = BufWriter::with_capacity(DATA_FILE_BUF_SIZE, &files.data);

    for (key, blob) in blobs {
        let key = Key::from(*key);
        let blob_index = key.1;

        let serialized_blob_data = &blob[..];

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
    }

    // write blob slices
    for slice in blob_slices_to_write {
        data_wtr.write_all(slice)?;
    }

    files.index.write_all(&idx_buf)?;

    data_wtr.flush()?;
    files.index.flush()?;

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
    let mut data_wtr = BufWriter::with_capacity(DATA_FILE_BUF_SIZE, &files.data);

    for (key, blob) in blobs {
        let key = Key::from(*key);
        let blob_index = key.1;

        let serialized_blob_data = blob
            .as_data()
            .map_err(|_| StoreError::Serialization("Bad ToData Impl".to_string()))?;

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
    }

    // write blob slices
    for slice in blob_slices_to_write {
        data_wtr.write_all(slice)?;
    }

    files.index.write_all(&idx_buf)?;

    data_wtr.flush()?;
    files.index.flush()?;

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
