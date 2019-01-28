use crate::db_ledger::SlotMeta;
use crate::packet::{Blob, BlobError, BLOB_HEADER_SIZE};
use crate::result::Error as SErr;

use byteorder::{BigEndian, ByteOrder};

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use super::*;

const DATA_FILE_NAME: &str = "data";
pub(super) const META_FILE_NAME: &str = "meta";
const INDEX_FILE_NAME: &str = "index";
pub(super) const ERASURE_FILE_NAME: &str = "erasure";
pub(super) const ERASURE_INDEX_FILE_NAME: &str = "erasure_index";

const DATA_FILE_BUF_SIZE: usize = 64 * 1024 * 32;

pub(super) const INDEX_RECORD_SIZE: u64 = 3 * 8;

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

    pub(super) fn insert_blobs<I>(&self, iter: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        let mut blobs: Vec<_> = iter.into_iter().collect();
        assert!(!blobs.is_empty());
        println!("insert_blobs: inserting {} blobs", blobs.len());

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

        println!("insert_blobs: arranged slots and stuff");

        for (slot, range) in slot_ranges {
            println!("insert_blobs: slot = {}", slot);
            println!(
                "insert_blobs: range.start = {}, range.end = {}",
                range.start, range.end
            );
            let slot_path = self.mk_slot_path(slot);
            ensure_slot(&slot_path)?;

            let (data_path, meta_path, index_path) = (
                slot_path.join(store_impl::DATA_FILE_NAME),
                slot_path.join(store_impl::META_FILE_NAME),
                slot_path.join(store_impl::INDEX_FILE_NAME),
            );

            println!(
                "insert_blobs: paths: data = {}",
                data_path.to_string_lossy()
            );
            println!(
                "insert_blobs: paths: meta = {}",
                meta_path.to_string_lossy()
            );
            println!(
                "insert_blobs: paths: index = {}",
                index_path.to_string_lossy()
            );
            // load meta_data
            let (mut meta_file, mut meta) = if meta_path.exists() {
                let mut f = OpenOptions::new().read(true).write(true).open(&meta_path)?;
                let m = bincode::deserialize_from(&mut f)?;
                f.seek(SeekFrom::Start(0))?;
                f.set_len(0)?;
                (f, m)
            } else {
                (
                    File::create(&meta_path)?,
                    SlotMeta {
                        consumed: 0,
                        consumed_slot: 0,
                        received: 0,
                        received_slot: 0,
                    },
                )
            };
            println!("insert_blobs: loaded meta data: {:?}", meta);

            let mut data_wtr =
                BufWriter::with_capacity(DATA_FILE_BUF_SIZE, open_append(&data_path)?);
            let mut index_wtr = BufWriter::new(open_append(&index_path)?);
            let slot_blobs = &blobs[range];
            let mut blob_indices = Vec::with_capacity(slot_blobs.len());

            let mut idx_buf = [0u8; INDEX_RECORD_SIZE as usize];

            println!(
                "insert_blobs: about to attempt adding {} blosbs",
                slot_blobs.len()
            );
            for blob in slot_blobs {
                let blob = blob.borrow();
                let blob_index = blob.index().map_err(bad_blob)?;

                let offset = data_wtr.seek(SeekFrom::Current(0))?;
                let serialized_blob_datas =
                    &blob.data[..BLOB_HEADER_SIZE + blob.size().map_err(bad_blob)?];

                data_wtr.write_all(&serialized_blob_datas)?;
                let data_len = serialized_blob_datas.len() as u64;

                let blob_idx = BlobIndex {
                    index: blob_index,
                    size: data_len,
                    offset,
                };

                BigEndian::write_u64(&mut idx_buf[0..8], blob_idx.index);
                BigEndian::write_u64(&mut idx_buf[8..16], blob_idx.offset);
                BigEndian::write_u64(&mut idx_buf[16..24], blob_idx.size);

                // update index file
                blob_indices.push(blob_idx);
                println!("insert_blobs: blob_idx = {:?}", blob_idx);
                println!("insert_blobs: idx_buf.len() = {}", idx_buf.len());
                index_wtr.write_all(&idx_buf)?;

                // update meta. write to file once in outer loop
                if blob_index > meta.received {
                    meta.received = blob_index;
                }

                if blob_index == meta.consumed + 1 {
                    meta.consumed += 1;
                }
            }

            bincode::serialize_into(&mut meta_file, &meta)?;

            data_wtr.flush()?;
            let data_f = data_wtr.into_inner()?;
            let index_f = index_wtr.into_inner()?;

            data_f.sync_data()?;
            index_f.sync_data()?;
            meta_file.sync_data()?;
        }

        Ok(())
    }

    pub(super) fn slot_data(
        &self,
        slot: u64,
        range: std::ops::Range<u64>,
    ) -> Result<impl Iterator<Item = Result<Vec<u8>>>> {
        // iterate over index file, gather blob indexes
        // sort by blob_index,

        let (data_path, index_path) = {
            let slot_path = self.mk_slot_path(slot);
            (
                slot_path.join(DATA_FILE_NAME),
                slot_path.join(INDEX_FILE_NAME),
            )
        };

        let index_rdr = File::open(&index_path)?;

        let index_size = index_rdr.metadata()?.len();
        println!("slot_data: index_size = {}", index_size);
        let mut index_rdr = BufReader::new(index_rdr);
        let mut buf = [0u8; INDEX_RECORD_SIZE as usize];
        let mut blob_indices: Vec<BlobIndex> =
            Vec::with_capacity((index_size / INDEX_RECORD_SIZE) as usize);

        while let Ok(_) = index_rdr.read_exact(&mut buf) {
            let index = BigEndian::read_u64(&buf[0..8]);
            if index < range.start || range.end < index {
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
        println!("slot_data: blob_indices: {:#?}", blob_indices);

        let data_rdr = BufReader::new(File::open(&data_path)?);

        Ok(SlotData {
            f: data_rdr,
            idxs: blob_indices,
            pos: 0,
        })
    }
}

#[derive(Debug)]
struct SlotData {
    f: BufReader<File>,
    idxs: Vec<BlobIndex>,
    pos: u64,
}

impl Iterator for SlotData {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.idxs.len() as u64 {
            return None;
        }

        let mut hack = || {
            let bix = self.idxs[self.pos as usize];

            let mut buf = vec![0u8; bix.size as usize];
            self.f.seek(SeekFrom::Start(bix.offset))?;
            self.f.read_exact(&mut buf)?;

            self.pos += 1;
            Ok(buf)
        };
        Some(hack())
    }
}

pub(super) fn bad_blob(err: SErr) -> StoreError {
    match err {
        SErr::BlobError(BlobError::BadState) => StoreError::BadBlob,
        _ => panic!("Swallowing error in blob store impl: {}", err),
    }
}

pub(super) fn open_append<P>(path: P) -> Result<File>
where
    P: AsRef<Path>,
{
    let f = OpenOptions::new().append(true).create(true).open(path)?;

    Ok(f)
}

pub(super) fn ensure_slot<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();

    if !path.exists() {
        fs::create_dir_all(&path)?;
    }

    Ok(())
}
