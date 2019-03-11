use crate::kvstore::error::Result;
use crate::kvstore::mapper::{Kind, Mapper};
use crate::kvstore::sstable::{Key, Merged, SSTable, Value};
use crate::kvstore::writelog::WriteLog;

use chrono::Utc;

use std::collections::BTreeMap;

type MemTable = BTreeMap<Key, Value>;

// Size of timestamp + size of key
const OVERHEAD: usize = 8 + 3 * 8;
const LOG_ERR: &str = "Write to log failed! Halting.";

#[derive(Debug)]
pub struct WriteState {
    pub commit: i64,
    pub log: WriteLog,
    pub values: MemTable,
    pub mem_size: usize,
}

impl WriteState {
    pub fn new(log: WriteLog, values: BTreeMap<Key, Value>) -> WriteState {
        let mem_size = values.values().fold(0, |acc, elem| acc + val_mem_use(elem));
        WriteState {
            commit: Utc::now().timestamp(),
            log,
            mem_size,
            values,
        }
    }

    pub fn put(&mut self, key: &Key, data: &[u8]) -> Result<()> {
        use std::collections::btree_map::Entry;
        let ts = self.commit;
        let value = Value {
            ts,
            val: Some(data.to_vec()),
        };
        self.log.log_put(key, ts, data).expect(LOG_ERR);

        self.mem_size += val_mem_use(&value);

        match self.values.entry(*key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => {
                let old = entry.insert(value);
                self.mem_size -= val_mem_use(&old);
            }
        }

        Ok(())
    }

    pub fn delete(&mut self, key: &Key) -> Result<()> {
        use std::collections::btree_map::Entry;
        let ts = self.commit;
        let value = Value { ts, val: None };

        self.log.log_delete(key, ts).expect(LOG_ERR);

        self.mem_size += val_mem_use(&value);

        match self.values.entry(*key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => {
                let old = entry.insert(value);
                self.mem_size -= val_mem_use(&old);
            }
        }

        Ok(())
    }

    pub fn reset(&mut self) -> Result<()> {
        self.values.clear();
        self.log.reset()?;
        self.mem_size = 0;
        Ok(())
    }
}

pub fn flush_table(
    mem: &MemTable,
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

pub fn get(mem: &MemTable, pages: &[BTreeMap<Key, SSTable>], key: &Key) -> Result<Option<Vec<u8>>> {
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
    mem: &MemTable,
    tables: &[BTreeMap<Key, SSTable>],
    range: std::ops::RangeInclusive<Key>,
) -> Result<impl Iterator<Item = (Key, Vec<u8>)>> {
    let mut sources: Vec<Box<dyn Iterator<Item = (Key, Value)>>> = Vec::new();

    let mem = mem
        .range(range.clone())
        .map(|(k, v)| (*k, v.clone()))
        .collect::<Vec<_>>();

    let mut disk = Vec::new();

    for level in tables.iter() {
        for sst in level.values() {
            let iter = sst.range(&range)?;
            let iter = Box::new(iter) as Box<dyn Iterator<Item = (Key, Value)>>;

            disk.push(iter);
        }
    }

    sources.push(Box::new(mem.into_iter()));
    sources.extend(disk);

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
