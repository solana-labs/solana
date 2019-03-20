use crate::error::Result;
use crate::io_utils::{Fill, MemMap};

use byteorder::{BigEndian, ByteOrder};

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::io::prelude::*;
use std::mem;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::u64;

const INDEX_META_SIZE: usize = mem::size_of::<IndexMeta>();
const KEY_LEN: usize = mem::size_of::<Key>();
const INDEX_ENTRY_SIZE: usize = mem::size_of::<IndexEntry>();
const INDEX_RECORD_SIZE: usize = KEY_LEN + INDEX_ENTRY_SIZE;

#[derive(Clone, Debug)]
pub struct SSTable {
    data: Arc<MemMap>,
    index: Arc<MemMap>,
    meta: IndexMeta,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub level: u8,
    pub data_size: u64,
    pub start: Key,
    pub end: Key,
}

#[derive(
    Debug, Default, PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Hash, Serialize, Deserialize,
)]
pub struct Key(pub [u8; 24]);

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub timestamp: i64,
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Value {
    pub ts: i64,
    pub val: Option<Vec<u8>>,
}

/// An iterator that produces logical view over a set of SSTables.
/// It implements [direct k-way merge](https://en.wikipedia.org/wiki/K-way_merge_algorithm#Heap)
/// and reconciles out-of-date/deleted values in a lazy fashion. Inputs *MUST* be sorted
pub struct Merged<I> {
    sources: Vec<I>,
    heads: BTreeMap<(Key, usize), Value>,
}

impl SSTable {
    pub fn meta(&self) -> &IndexMeta {
        &self.meta
    }

    #[allow(dead_code)]
    pub fn num_keys(&self) -> u64 {
        ((self.index.len() - INDEX_META_SIZE) / INDEX_ENTRY_SIZE) as u64
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
        data_wtr: &mut dyn Write,
        index_wtr: &mut dyn Write,
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

    pub fn create<I, K, V>(
        rows: &mut I,
        level: u8,
        data_wtr: &mut dyn Write,
        index_wtr: &mut dyn Write,
    ) where
        I: Iterator<Item = (K, V)>,
        K: Borrow<Key>,
        V: Borrow<Value>,
    {
        SSTable::create_capped(rows, level, u64::MAX, data_wtr, index_wtr);
    }

    pub fn from_parts(data: Arc<MemMap>, index: Arc<MemMap>) -> Result<Self> {
        let len = index.len() as usize;

        assert!(len > INDEX_META_SIZE);
        assert_eq!((len - INDEX_META_SIZE) % INDEX_RECORD_SIZE, 0);

        let meta = bincode::deserialize_from(&index[..INDEX_META_SIZE])?;

        Ok(SSTable { data, index, meta })
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

            while level as usize >= sorted.len() {
                sorted.push(BTreeMap::new());
            }
            sorted[level as usize].insert(key, sst.clone());
        }

        sorted
    }
}

impl Key {
    pub const MIN: Key = Key([0u8; KEY_LEN]);
    pub const MAX: Key = Key([255u8; KEY_LEN]);
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

impl Value {
    pub fn new(commit: i64, data: Option<Vec<u8>>) -> Value {
        Value {
            ts: commit,
            val: data,
        }
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
            index_pos: INDEX_META_SIZE as usize,
        }
    }

    fn step(&mut self) -> Result<Option<(Key, Value)>> {
        while self.index_pos < self.index.len() {
            let pos = self.index_pos as usize;
            let end = pos + INDEX_RECORD_SIZE;

            let (key, entry): (Key, IndexEntry) = bincode::deserialize_from(&self.index[pos..end])?;
            self.index_pos = end;

            if key < *self.bounds.start() {
                continue;
            }

            if *self.bounds.end() < key {
                self.index_pos = std::usize::MAX;
                return Ok(None);
            }

            let record_range = entry.offset as usize..(entry.offset + entry.size) as usize;
            let (data_key, value) = bincode::deserialize_from(&self.data[record_range])?;
            assert_eq!(data_key, key);

            return Ok(Some((data_key, value)));
        }

        Ok(None)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let k0 = BigEndian::read_u64(&self.0[..8]);
        let k1 = BigEndian::read_u64(&self.0[8..16]);
        let k2 = BigEndian::read_u64(&self.0[16..]);
        write!(f, "Key({}, {}, {})", k0, k1, k2)
    }
}

impl From<(u64, u64, u64)> for Key {
    fn from((k0, k1, k2): (u64, u64, u64)) -> Self {
        let mut buf = [0u8; KEY_LEN];

        BigEndian::write_u64(&mut buf[..8], k0);
        BigEndian::write_u64(&mut buf[8..16], k1);
        BigEndian::write_u64(&mut buf[16..], k2);

        Key(buf)
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

        Merged { sources, heads }
    }
}

impl<I> Iterator for Merged<I>
where
    I: Iterator<Item = (Key, Value)>,
{
    type Item = (Key, Value);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.heads.is_empty() {
            // get new key
            let (key, source_idx) = *self.heads.keys().next().unwrap();
            let mut val = self.heads.remove(&(key, source_idx)).unwrap();

            // replace
            if let Some((k, v)) = self.sources[source_idx].next() {
                self.heads.insert((k, source_idx), v);
            }

            // check for other versions of this record
            while !self.heads.is_empty() {
                let (next_key, source_idx) = *self.heads.keys().next().unwrap();

                // Found a different version of the record
                if key == next_key {
                    // pop this version, check if it's newer
                    let other_version = self.heads.remove(&(next_key, source_idx)).unwrap();
                    if other_version.ts > val.ts {
                        val = other_version;
                    }

                    // replace
                    if let Some((k, v)) = self.sources[source_idx].next() {
                        self.heads.insert((k, source_idx), v);
                    }
                } else {
                    break;
                }
            }

            // Don't produce deleted records
            if val.val.is_some() {
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

fn flush_index(
    index: &BTreeMap<Key, IndexEntry>,
    meta: &IndexMeta,
    writer: &mut dyn Write,
) -> Result<()> {
    let mut entry_buffer = [0u8; INDEX_RECORD_SIZE];
    let mut meta_buffer = [0u8; INDEX_META_SIZE];

    bincode::serialize_into(&mut meta_buffer[..], meta)?;
    writer.write_all(&meta_buffer)?;

    for (key, entry) in index.iter() {
        let rec = (key, entry);
        entry_buffer.fill(0);

        bincode::serialize_into(&mut entry_buffer[..], &rec)?;
        writer.write_all(&entry_buffer)?;
    }

    Ok(())
}

fn flush_mem_table_capped<I, K, V>(
    rows: &mut I,
    mut wtr: &mut dyn Write,
    max_table_size: u64,
) -> Result<(u64, BTreeMap<Key, IndexEntry>)>
where
    I: Iterator<Item = (K, V)>,
    K: Borrow<Key>,
    V: Borrow<Value>,
{
    let mut index = BTreeMap::new();
    let mut size = 0;
    let bincode_config = bincode::config();

    for (key, val) in rows {
        let record = (key.borrow(), val.borrow());

        let serialized_size = bincode_config.serialized_size(&record)?;
        bincode::serialize_into(&mut wtr, &record)?;

        let entry = IndexEntry {
            timestamp: record.1.ts,
            offset: size,
            size: serialized_size,
        };

        size += serialized_size;

        index.insert(*record.0, entry);

        if size >= max_table_size {
            break;
        }
    }

    Ok((size, index))
}

#[inline]
fn overlapping<T: Ord + Eq>(r1: &RangeInclusive<T>, r2: &RangeInclusive<T>) -> bool {
    r1.start() <= r2.end() && r2.start() <= r1.end()
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test::gen;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_dump_data() {
        let mut data_buffer = vec![];
        let records: BTreeMap<_, _> = gen_records().take(512).collect();

        let (_, index) =
            flush_mem_table_capped(&mut records.iter(), &mut data_buffer, u64::MAX).unwrap();

        assert_eq!(index.len(), records.len());
        assert!(index.keys().eq(records.keys()));

        let mut retrieved = BTreeMap::new();

        for (key, entry) in index.iter() {
            let range = entry.offset as usize..(entry.offset + entry.size) as usize;
            let (data_key, value) = bincode::deserialize_from(&data_buffer[range]).unwrap();
            assert_eq!(&data_key, key);
            retrieved.insert(data_key, value);
        }

        assert_eq!(records, retrieved);
    }

    #[test]
    fn test_dump_indexes() {
        let mut data_buffer = vec![];
        let mut index_buffer = vec![];
        let records: BTreeMap<_, _> = gen_records().take(512).collect();

        let (data_size, index) =
            flush_mem_table_capped(&mut records.iter(), &mut data_buffer, u64::MAX).unwrap();

        let (&start, &end) = (
            index.keys().next().unwrap(),
            index.keys().next_back().unwrap(),
        );

        let meta = IndexMeta {
            start,
            end,
            data_size,
            level: 0,
        };

        flush_index(&index, &meta, &mut index_buffer).unwrap();

        let retrieved_meta = bincode::deserialize_from(&index_buffer[..INDEX_META_SIZE]).unwrap();
        assert_eq!(meta, retrieved_meta);

        // By iterating over the BTreeMap we also check the order of index entries as written
        for (i, (key, entry)) in index.iter().enumerate() {
            let start = i * INDEX_RECORD_SIZE + INDEX_META_SIZE;
            let end = start + INDEX_RECORD_SIZE;

            let (retrieved_key, retrieved_entry) =
                bincode::deserialize_from(&index_buffer[start..end]).unwrap();

            assert_eq!(key, &retrieved_key);
            assert_eq!(entry, &retrieved_entry);
        }
    }

    #[test]
    fn test_sstable_scan() {
        let mut data_buffer = vec![];
        let mut index_buffer = vec![];
        let records: BTreeMap<_, _> = gen_records().take(512).collect();

        SSTable::create(&mut records.iter(), 0, &mut data_buffer, &mut index_buffer);

        let data = MemMap::Mem(Arc::new(RwLock::new(data_buffer)));
        let index = MemMap::Mem(Arc::new(RwLock::new(index_buffer)));

        let sst = SSTable::from_parts(Arc::new(data), Arc::new(index)).unwrap();

        let output_iter = Scan::new(
            Key::ALL_INCLUSIVE,
            Arc::clone(&sst.data),
            Arc::clone(&sst.index),
        );

        assert!(output_iter.eq(records.into_iter()));
    }

    #[test]
    fn test_merge_2way() {
        let records: BTreeMap<_, _> = gen_records().take(512).collect();
        let updates: BTreeMap<_, _> = records
            .iter()
            .map(|(k, v)| (*k, Value::new(v.ts + 1, Some(vec![]))))
            .collect();
        let deletes: BTreeMap<_, _> = records
            .iter()
            .map(|(k, v)| (*k, Value::new(v.ts + 1, None)))
            .collect();

        let owned = |(k, v): (&Key, &Value)| (*k, v.clone());

        let sources = vec![records.iter().map(owned), updates.iter().map(owned)];
        let merged: Vec<_> = Merged::new(sources).collect();
        assert!(merged.into_iter().eq(updates.into_iter()));

        let sources = vec![records.into_iter(), deletes.into_iter()];
        let merged: Vec<_> = Merged::new(sources).collect();
        assert_eq!(merged.len(), 0);
    }

    #[test]
    fn test_merge_4way() {
        // delete last half, then update first half, then delete last half of first half
        let start: BTreeMap<_, _> = gen_records().take(512).collect();
        let deletes: BTreeMap<_, _> = start
            .iter()
            .skip(256)
            .map(|(k, v)| (*k, Value::new(v.ts + 1, None)))
            .collect();
        let updates: BTreeMap<_, _> = start
            .iter()
            .take(256)
            .map(|(k, v)| (*k, Value::new(v.ts + 2, Some(vec![]))))
            .collect();
        let more_deletes: BTreeMap<_, _> = updates
            .iter()
            .skip(128)
            .map(|(k, v)| (*k, Value::new(v.ts + 3, None)))
            .collect();

        let sources = vec![
            more_deletes.into_iter(),
            updates.clone().into_iter(),
            start.into_iter(),
            deletes.into_iter(),
        ];

        let merged: Vec<_> = Merged::new(sources).collect();
        let expected: Vec<_> = updates.into_iter().take(128).collect();

        assert_eq!(merged.len(), expected.len());
        assert_eq!(merged, expected);
    }

    fn gen_records() -> impl Iterator<Item = (Key, Value)> {
        gen::pairs_vary(0..255)
            .map(|(key, bytes)| (key, Value::new(bytes.len() as i64, Some(bytes))))
    }

}
