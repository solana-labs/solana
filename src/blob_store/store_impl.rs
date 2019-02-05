use crate::blob_store::slot::{SlotData, SlotIO};
use crate::blob_store::store::{Key, Retrievable, SlotCache, Storable, StorableNoCopy};
use crate::blob_store::{Result, StoreError};

use byteorder::{BigEndian, ByteOrder};

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::result::Result as StdRes;

pub const DATA_FILE_BUF_SIZE: usize = 64 * 1024;
pub const INDEX_RECORD_SIZE: u64 = 3 * 8;

pub fn mk_slot_path(root: &Path, slot: u64) -> PathBuf {
    let mut splat = [0u8; 8];
    BigEndian::write_u64(&mut splat, slot);
    let mut path = root.join(format!("{:#04x}", splat[0]));

    for byte in &splat[1..] {
        path = path.join(format!("{:02x}", byte));
    }

    path
}

#[allow(clippy::range_plus_one)]
pub fn insert_blobs<I, T>(
    root: &Path,
    column: &str,
    cache: &mut SlotCache,
    iter: I,
) -> Result<Vec<u64>>
where
    I: IntoIterator<Item = (Key, T)>,
    T: Storable,
{
    let blobs: StdRes<Vec<(Key, Vec<u8>)>, _> = iter
        .into_iter()
        .map(|(k, v)| v.to_data().map(|data| (k, data)))
        .collect();

    let mut blobs: Vec<_> =
        blobs.map_err(|_| StoreError::Serialization("Bad ToData Impl".into()))?;
    assert!(!blobs.is_empty());

    // sort on lexi order (slot_idx, blob_idx)
    blobs.sort_unstable_by_key(|(k, _)| *k);

    // contains the indices into blobs of the first blob for that slot
    let mut slot_ranges = HashMap::new();

    for (index, (k, _)) in blobs.iter().enumerate() {
        let slot = k.0;
        slot_ranges
            .entry(slot)
            .and_modify(|r: &mut Range<usize>| {
                r.start = std::cmp::min(r.start, index);
                r.end = std::cmp::max(r.end, index + 1);
            })
            .or_insert(index..(index + 1));
    }

    let mut slots_to_cache = Vec::new();
    let slots = slot_ranges.keys().cloned().collect();

    for (slot, range) in slot_ranges {
        let slot_blobs = &blobs[range];

        match cache.get_mut(slot) {
            Some(sio) => {
                sio.insert(column, slot_blobs)?;
            }
            None => {
                let slot_path = mk_slot_path(root, slot);
                let mut sio = SlotIO::open(slot, slot_path)?;
                sio.insert(column, slot_blobs)?;
                slots_to_cache.push(sio);
            }
        }
    }

    // cache slots
    for sio in slots_to_cache {
        cache.push(sio);
    }

    Ok(slots)
}

#[allow(clippy::range_plus_one)]
pub fn insert_blobs_no_copy<I, T>(
    root: &Path,
    column: &str,
    cache: &mut SlotCache,
    iter: I,
) -> Result<Vec<u64>>
where
    I: IntoIterator<Item = (Key, T)>,
    T: StorableNoCopy,
{
    let mut blobs: Vec<_> = iter.into_iter().collect();
    assert!(!blobs.is_empty());

    // sort on lexi order (slot_idx, blob_idx)
    blobs.sort_unstable_by_key(|(k, _)| *k);

    // contains the indices into blobs of the first blob for that slot
    let mut slot_ranges = HashMap::new();
    for (index, (k, _)) in blobs.iter().enumerate() {
        let slot = k.0;
        slot_ranges
            .entry(slot)
            .and_modify(|r: &mut Range<usize>| {
                r.start = std::cmp::min(r.start, index);
                r.end = std::cmp::max(r.end, index + 1);
            })
            .or_insert(index..(index + 1));
    }

    let mut slots_to_cache = Vec::new();
    let slots = slot_ranges.keys().cloned().collect();

    for (slot, range) in slot_ranges {
        let slot_blobs = &blobs[range];

        match cache.get_mut(slot) {
            Some(sio) => {
                sio.insert_no_copy(column, slot_blobs)?;
            }
            None => {
                let slot_path = mk_slot_path(root, slot);
                let mut sio = SlotIO::open(slot, slot_path)?;
                sio.insert_no_copy(column, slot_blobs)?;
                slots_to_cache.push(sio);
            }
        }
    }

    // cache slots
    for sio in slots_to_cache {
        cache.push(sio);
    }

    Ok(slots)
}

pub fn get<T>(root: &Path, column: &str, cache: &SlotCache, key: Key) -> Result<T::Output>
where
    T: Retrievable,
{
    match cache.get(key.0) {
        Some(sio) => sio.get::<T>(column, key),
        None => {
            let slot_path = mk_slot_path(root, key.0);
            let sio = SlotIO::open(key.0, slot_path)?;
            sio.get::<T>(column, key)
        }
    }
}

pub fn slot_data<'a, S>(
    src: &'a S,
    root: &Path,
    cache: &SlotCache,
    column: &str,
    slot: u64,
    range: std::ops::Range<u64>,
) -> Result<SlotData<'a, S>> {
    let it = match cache.get(slot) {
        Some(sio) => sio.data(column, range)?,
        None => {
            let slot_path = mk_slot_path(root, slot);
            let sio = SlotIO::open(slot, slot_path)?;
            sio.data(column, range)?
        }
    };

    Ok(it.bind(src))
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
