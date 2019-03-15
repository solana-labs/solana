use crate::error::Result;
use crate::io_utils::{MemMap, Writer};

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};

use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};
use std::io::{prelude::*, Cursor, Seek, SeekFrom};
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::u64;

// ___________________________________________
// | start_key | end_key | level | data_size |
// -------------------------------------------
const IDX_META_SIZE: usize = KEY_LEN + KEY_LEN + 1 + 8;

const KEY_LEN: usize = 3 * 8;
// _________________
// | offset | size |
// -----------------
const PTR_SIZE: usize = 2 * 8;
// __________________________________________
// | key | timestamp | pointer OR tombstone |
// ------------------------------------------
const INDEX_ENTRY_SIZE: usize = KEY_LEN + 8 + PTR_SIZE;
// Represented by zero offset and size
const TOMBSTONE: [u8; PTR_SIZE] = [0u8; PTR_SIZE];

#[derive(Clone, Debug)]
pub struct SSTable {
    data: Arc<MemMap>,
    index: Arc<MemMap>,
    meta: IndexMeta,
}

#[derive(Debug, PartialEq, Clone)]
pub struct IndexMeta {
    pub level: u8,
    pub data_size: u64,
    pub start: Key,
    pub end: Key,
}

#[derive(Debug, Default, PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Hash)]
pub struct Key(pub [u8; 24]);

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Copy, Clone)]
pub struct Index {
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Value {
    pub ts: i64,
    pub val: Option<Vec<u8>>,
}

/// An iterator that produces logical view over a set of SSTables
pub struct Merged<I> {
    sources: Vec<I>,
    heads: BTreeMap<(Key, usize), Value>,
    seen: HashMap<Key, i64>,
}

impl SSTable {
    pub fn meta(&self) -> &IndexMeta {
        &self.meta
    }

    #[allow(dead_code)]
    pub fn num_keys(&self) -> u64 {
        ((self.index.len() - IDX_META_SIZE) / INDEX_ENTRY_SIZE) as u64
    }

    pub fn get(&self, key: &Key) -> Result<Option<Value>> {
        let range = *key..=*key;
        let found_opt = self.range(&range)?.find(|(k, _)| k == key).map(|(_, v)| v);
        Ok(found_opt)
    }

    pub fn range(&self, range: &RangeInclusive<Key>) -> Result<impl Iterator<Item = (Key, Value)>> {
        Ok(Scan::new(
            range.clone(),
            Arc::clone(&self.data),
            Arc::clone(&self.index),
        ))
    }

    pub fn create_capped<I, K, V>(
        rows: &mut I,
        level: u8,
        max_table_size: u64,
        data_wtr: &mut Writer,
        index_wtr: &mut Writer,
    ) where
        I: Iterator<Item = (K, V)>,
        K: Borrow<Key>,
        V: Borrow<Value>,
    {
        const DATA_ERR: &str = "Error writing table data";
        const INDEX_ERR: &str = "Error writing index data";

        let (data_size, index) =
            flush_mem_table_capped(rows, data_wtr, max_table_size).expect(DATA_ERR);

        data_wtr.flush().expect(DATA_ERR);

        let (&start, &end) = (
            index.keys().next().unwrap(),
            index.keys().next_back().unwrap(),
        );

        let meta = IndexMeta {
            start,
            end,
            level,
            data_size,
        };

        flush_index(&index, &meta, index_wtr).expect(INDEX_ERR);
        index_wtr.flush().expect(INDEX_ERR);
    }

    pub fn create<I, K, V>(rows: &mut I, level: u8, data_wtr: &mut Writer, index_wtr: &mut Writer)
    where
        I: Iterator<Item = (K, V)>,
        K: Borrow<Key>,
        V: Borrow<Value>,
    {
        SSTable::create_capped(rows, level, u64::MAX, data_wtr, index_wtr);
    }

    pub fn from_parts(data: Arc<MemMap>, index: Arc<MemMap>) -> Result<Self> {
        sst_from_parts(data, index)
    }

    pub fn could_contain(&self, key: &Key) -> bool {
        self.meta.start <= *key && *key <= self.meta.end
    }

    pub fn is_overlap(&self, range: &RangeInclusive<Key>) -> bool {
        let r = self.meta.start..=self.meta.end;
        overlapping(&r, range)
    }

    pub fn sorted_tables(tables: &[SSTable]) -> Vec<BTreeMap<Key, SSTable>> {
        let mut sorted = Vec::new();

        for sst in tables {
            let (key, level) = {
                let meta = sst.meta();
                (meta.start, meta.level)
            };

            while level as usize >= tables.len() {
                sorted.push(BTreeMap::new());
            }
            sorted[level as usize].insert(key, sst.clone());
        }

        sorted
    }
}

impl Key {
    pub const MIN: Key = Key([0u8; KEY_LEN as usize]);
    pub const MAX: Key = Key([255u8; KEY_LEN as usize]);
    pub const ALL_INCLUSIVE: RangeInclusive<Key> = RangeInclusive::new(Key::MIN, Key::MAX);

    pub fn write<W: Write>(&self, wtr: &mut W) -> Result<()> {
        wtr.write_all(&self.0)?;
        Ok(())
    }

    pub fn read(bytes: &[u8]) -> Key {
        let mut key = Key::default();
        key.0.copy_from_slice(bytes);
        key
    }
}

struct Scan {
    bounds: RangeInclusive<Key>,
    data: Arc<MemMap>,
    index: Arc<MemMap>,
    index_pos: usize,
}

impl Scan {
    fn new(bounds: RangeInclusive<Key>, data: Arc<MemMap>, index: Arc<MemMap>) -> Self {
        Scan {
            bounds,
            data,
            index,
            index_pos: IDX_META_SIZE as usize,
        }
    }

    fn step(&mut self) -> Result<Option<(Key, Value)>> {
        while self.index_pos < self.index.len() {
            let pos = self.index_pos as usize;
            let end = pos + INDEX_ENTRY_SIZE;
            let (key, ts, idx) = read_index_rec(&self.index[pos..end]);

            if key < *self.bounds.start() {
                self.index_pos = end;
                continue;
            }

            if *self.bounds.end() < key {
                self.index_pos = std::usize::MAX;
                return Ok(None);
            }

            let bytes_opt = idx.map(|ptr| get_val(&self.data, ptr).to_vec());

            let val = Value { ts, val: bytes_opt };

            self.index_pos = end;

            return Ok(Some((key, val)));
        }

        Ok(None)
    }
}

impl From<(u64, u64, u64)> for Key {
    fn from((k0, k1, k2): (u64, u64, u64)) -> Self {
        let mut buf = [0u8; KEY_LEN as usize];

        BigEndian::write_u64(&mut buf[..8], k0);
        BigEndian::write_u64(&mut buf[8..16], k1);
        BigEndian::write_u64(&mut buf[16..], k2);

        Key(buf)
    }
}

impl Index {
    fn write<W: Write>(&self, wtr: &mut W) -> Result<()> {
        wtr.write_u64::<BigEndian>(self.offset)?;
        wtr.write_u64::<BigEndian>(self.size)?;
        Ok(())
    }

    #[inline]
    fn read(bytes: &[u8]) -> Index {
        let offset = BigEndian::read_u64(&bytes[..8]);
        let size = BigEndian::read_u64(&bytes[8..16]);

        Index { offset, size }
    }
}

impl IndexMeta {
    fn write<W: Write>(&self, wtr: &mut W) -> Result<()> {
        self.start.write(wtr)?;
        self.end.write(wtr)?;
        wtr.write_u8(self.level)?;
        wtr.write_u64::<BigEndian>(self.data_size)?;
        Ok(())
    }

    fn read(data: &[u8]) -> Self {
        let start = Key::read(&data[..24]);
        let end = Key::read(&data[24..48]);
        let level = data[48];
        let data_size = BigEndian::read_u64(&data[49..57]);

        IndexMeta {
            start,
            end,
            level,
            data_size,
        }
    }
}

impl<I> Merged<I>
where
    I: Iterator<Item = (Key, Value)>,
{
    pub fn new(mut sources: Vec<I>) -> Self {
        let mut heads = BTreeMap::new();

        for (source_idx, source) in sources.iter_mut().enumerate() {
            if let Some((k, v)) = source.next() {
                heads.insert((k, source_idx), v);
            }
        }

        Merged {
            sources,
            heads,
            seen: HashMap::new(),
        }
    }
}

impl<I> Iterator for Merged<I>
where
    I: Iterator<Item = (Key, Value)>,
{
    type Item = (Key, Value);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.heads.is_empty() {
            let (key, source_idx) = *self.heads.keys().next().unwrap();
            let val = self.heads.remove(&(key, source_idx)).unwrap();

            // replace
            if let Some((k, v)) = self.sources[source_idx].next() {
                self.heads.insert((k, source_idx), v);
            }

            // merge logic
            // if deleted, remember
            let (deleted, stale) = match self.seen.get(&key) {
                Some(&seen_ts) if seen_ts < val.ts => {
                    // fresh val
                    self.seen.insert(key, val.ts);
                    (val.val.is_none(), false)
                }
                Some(_) => (val.val.is_none(), true),
                None => {
                    self.seen.insert(key, val.ts);
                    (val.val.is_none(), false)
                }
            };

            if !(stale || deleted) {
                return Some((key, val));
            }
        }

        None
    }
}

impl Iterator for Scan {
    type Item = (Key, Value);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index_pos as usize >= self.index.len() {
            return None;
        }

        match self.step() {
            Ok(opt) => opt,
            Err(_) => {
                self.index_pos = std::usize::MAX;
                None
            }
        }
    }
}

fn sst_from_parts(data: Arc<MemMap>, index: Arc<MemMap>) -> Result<SSTable> {
    let len = index.len() as usize;

    assert!(len > IDX_META_SIZE);
    assert_eq!((len - IDX_META_SIZE) % INDEX_ENTRY_SIZE, 0);

    let mut rdr = Cursor::new(&**index);
    let mut idx_buf = [0; IDX_META_SIZE];
    rdr.read_exact(&mut idx_buf)?;

    let meta = IndexMeta::read(&idx_buf);

    Ok(SSTable { data, index, meta })
}

fn flush_index(
    index: &BTreeMap<Key, (i64, Option<Index>)>,
    meta: &IndexMeta,
    wtr: &mut Writer,
) -> Result<()> {
    meta.write(wtr)?;

    for (&key, &(ts, idx)) in index.iter() {
        write_index_rec(wtr, (key, ts, idx))?;
    }

    Ok(())
}
#[allow(clippy::type_complexity)]
fn flush_mem_table_capped<I, K, V>(
    rows: &mut I,
    wtr: &mut Writer,
    max_table_size: u64,
) -> Result<(u64, BTreeMap<Key, (i64, Option<Index>)>)>
where
    I: Iterator<Item = (K, V)>,
    K: Borrow<Key>,
    V: Borrow<Value>,
{
    let mut ssi = BTreeMap::new();
    let mut size = 0;

    for (key, val) in rows {
        let (key, val) = (key.borrow(), val.borrow());
        let ts = val.ts;

        let (index, item_size) = match val.val {
            Some(ref bytes) => (Some(write_val(wtr, bytes)?), bytes.len()),
            None => (None, 0),
        };

        size += item_size as u64;
        ssi.insert(*key, (ts, index));

        if size >= max_table_size {
            break;
        }
    }

    Ok((size, ssi))
}

#[inline]
fn overlapping<T: Ord + Eq>(r1: &RangeInclusive<T>, r2: &RangeInclusive<T>) -> bool {
    r1.start() <= r2.end() && r2.start() <= r1.end()
}

#[inline]
fn write_val<W: Write + Seek>(wtr: &mut W, val: &[u8]) -> Result<Index> {
    let offset = wtr.seek(SeekFrom::Current(0))?;
    let size = val.len() as u64;

    wtr.write_all(val)?;
    Ok(Index { offset, size })
}

#[inline]
fn get_val(mmap: &MemMap, idx: Index) -> &[u8] {
    let row = &mmap[idx.offset as usize..(idx.offset + idx.size) as usize];
    assert_eq!(row.len(), idx.size as usize);
    row
}

#[inline]
fn write_index_rec<W: Write>(wtr: &mut W, (key, ts, ptr): (Key, i64, Option<Index>)) -> Result<()> {
    key.write(wtr)?;

    wtr.write_i64::<BigEndian>(ts)?;

    match ptr {
        Some(idx) => idx.write(wtr)?,
        None => wtr.write_all(&TOMBSTONE)?,
    };

    Ok(())
}

#[inline]
fn read_index_rec(bytes: &[u8]) -> (Key, i64, Option<Index>) {
    assert_eq!(bytes.len(), INDEX_ENTRY_SIZE);
    const TS_END: usize = KEY_LEN + 8;

    let mut key_buf = [0; KEY_LEN as usize];
    key_buf.copy_from_slice(&bytes[..KEY_LEN as usize]);
    let key = Key(key_buf);
    let ts = BigEndian::read_i64(&bytes[KEY_LEN..TS_END]);

    let idx_slice = &bytes[TS_END..INDEX_ENTRY_SIZE];
    let idx = if idx_slice == TOMBSTONE {
        None
    } else {
        Some(Index::read(idx_slice))
    };

    (key, ts, idx)
}
