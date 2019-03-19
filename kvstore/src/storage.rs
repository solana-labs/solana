use crate::error::Result;
use crate::mapper::{Kind, Mapper};
use crate::sstable::{Key, Merged, SSTable, Value};
use crate::writelog::WriteLog;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

// Size of timestamp + size of key
const OVERHEAD: usize = 8 + 3 * 8;
const LOG_ERR: &str = "Write to log failed! Halting.";

#[derive(Debug)]
pub struct MemTable {
    pub mem_size: usize,
    pub values: BTreeMap<Key, Value>,
}

impl MemTable {
    pub fn new(values: BTreeMap<Key, Value>) -> MemTable {
        let mem_size = values.values().fold(0, |acc, elem| acc + val_mem_use(elem));
        MemTable { mem_size, values }
    }
}

pub fn put(
    mem: &mut MemTable,
    log: &mut WriteLog,
    key: &Key,
    commit: i64,
    data: &[u8],
) -> Result<()> {
    log.log_put(key, commit, data).expect(LOG_ERR);

    let value = Value {
        ts: commit,
        val: Some(data.to_vec()),
    };

    mem.mem_size += val_mem_use(&value);

    match mem.values.entry(*key) {
        Entry::Vacant(entry) => {
            entry.insert(value);
        }
        Entry::Occupied(mut entry) => {
            let old = entry.insert(value);
            mem.mem_size -= val_mem_use(&old);
        }
    }

    Ok(())
}

pub fn delete(mem: &mut MemTable, log: &mut WriteLog, key: &Key, commit: i64) -> Result<()> {
    log.log_delete(key, commit).expect(LOG_ERR);
    let value = Value {
        ts: commit,
        val: None,
    };

    mem.mem_size += val_mem_use(&value);

    match mem.values.entry(*key) {
        Entry::Vacant(entry) => {
            entry.insert(value);
        }
        Entry::Occupied(mut entry) => {
            let old = entry.insert(value);
            mem.mem_size -= val_mem_use(&old);
        }
    }

    Ok(())
}

pub fn flush_table(
    mem: &BTreeMap<Key, Value>,
    mapper: &dyn Mapper,
    pages: &mut Vec<BTreeMap<Key, SSTable>>,
) -> Result<()> {
    if mem.is_empty() {
        return Ok(());
    };

    if pages.is_empty() {
        pages.push(BTreeMap::new());
    }

    let mut iter = mem.iter();
    let sst = mapper.make_table(Kind::Active, &mut |mut data_wtr, mut index_wtr| {
        SSTable::create(&mut iter, 0, &mut data_wtr, &mut index_wtr);
    })?;

    let first = sst.meta().start;

    pages[0].insert(first, sst);
    Ok(())
}

pub fn get(
    mem: &BTreeMap<Key, Value>,
    pages: &[BTreeMap<Key, SSTable>],
    key: &Key,
) -> Result<Option<Vec<u8>>> {
    if let Some(idx) = mem.get(key) {
        return Ok(idx.val.clone());
    }

    let mut candidates = Vec::new();

    for level in pages.iter() {
        for (_, sst) in level.iter().rev() {
            if sst.could_contain(key) {
                if let Some(val) = sst.get(&key)? {
                    candidates.push((*key, val));
                }
            }
        }
    }

    let merged = Merged::new(vec![candidates.into_iter()])
        .next()
        .map(|(_, v)| v.val.unwrap());
    Ok(merged)
}

pub fn range(
    mem: &BTreeMap<Key, Value>,
    tables: &[BTreeMap<Key, SSTable>],
    range: std::ops::RangeInclusive<Key>,
) -> Result<impl Iterator<Item = (Key, Vec<u8>)>> {
    let mut sources: Vec<Box<dyn Iterator<Item = (Key, Value)>>> = Vec::new();

    let mem = mem
        .range(range.clone())
        .map(|(k, v)| (*k, v.clone()))
        .collect::<Vec<_>>();
    sources.push(Box::new(mem.into_iter()));

    for level in tables.iter() {
        for sst in level.values() {
            let iter = sst.range(&range)?;
            let iter = Box::new(iter) as Box<dyn Iterator<Item = (Key, Value)>>;

            sources.push(iter);
        }
    }

    let rows = Merged::new(sources).map(|(k, v)| (k, v.val.unwrap()));

    Ok(rows)
}

#[inline]
fn val_mem_use(val: &Value) -> usize {
    OVERHEAD + val.val.as_ref().map(Vec::len).unwrap_or(0)
}

// TODO: Write basic tests using mem-table
// 1. test put + delete works right
// 2. test delete of unknown key recorded
// 3. check memory usage calcs
