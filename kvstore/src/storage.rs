use crate::error::Result;
use crate::mapper::{Kind, Mapper};
use crate::sstable::{Key, Merged, SSTable, Value};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::mem;

/// Wrapper over a BTreeMap<`Key`, `Value`> that does basic accounting of memory usage
/// (Doesn't include BTreeMap internal stuff, can't reliably account for that without
/// using special data-structures or depending on unstable implementation details of `std`)
#[derive(Debug)]
pub struct MemTable {
    pub mem_size: usize,
    pub values: BTreeMap<Key, Value>,
}

impl MemTable {
    /// Memory over-head per record. Size of the key + size of commit ID.
    pub const OVERHEAD_PER_RECORD: usize = mem::size_of::<Key>() + mem::size_of::<i64>();

    pub fn new(values: BTreeMap<Key, Value>) -> MemTable {
        let mem_size = values.values().fold(0, |acc, elem| {
            acc + Self::OVERHEAD_PER_RECORD + opt_bytes_memory(&elem.val)
        });
        MemTable { mem_size, values }
    }

    pub fn put(&mut self, key: &Key, commit: i64, data: &[u8]) {
        let value = Value {
            ts: commit,
            val: Some(data.to_vec()),
        };

        self.mem_size += data.len();
        match self.values.entry(*key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                self.mem_size += Self::OVERHEAD_PER_RECORD;
            }
            Entry::Occupied(mut entry) => {
                let old = entry.insert(value);
                self.mem_size -= opt_bytes_memory(&old.val);
            }
        }
    }

    pub fn delete(&mut self, key: &Key, commit: i64) {
        let value = Value {
            ts: commit,
            val: None,
        };

        match self.values.entry(*key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                self.mem_size += Self::OVERHEAD_PER_RECORD;
            }
            Entry::Occupied(mut entry) => {
                let old = entry.insert(value);
                self.mem_size -= opt_bytes_memory(&old.val);
            }
        }
    }
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

impl Default for MemTable {
    fn default() -> MemTable {
        MemTable {
            values: BTreeMap::new(),
            mem_size: 0,
        }
    }
}

#[inline]
fn opt_bytes_memory(bytes: &Option<Vec<u8>>) -> usize {
    bytes.as_ref().map(Vec::len).unwrap_or(0)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::gen;

    const COMMIT: i64 = -1;

    #[test]
    fn test_put_calc() {
        const DATA_SIZE: usize = 16;

        let mut table = MemTable::default();

        for (key, data) in gen::pairs(DATA_SIZE).take(1024) {
            table.put(&key, COMMIT, &data);
        }

        let expected_size = 1024 * (DATA_SIZE + MemTable::OVERHEAD_PER_RECORD);
        assert_eq!(table.mem_size, expected_size);
    }

    #[test]
    fn test_delete_calc() {
        const DATA_SIZE: usize = 32;

        let mut table = MemTable::default();
        let input = gen::pairs(DATA_SIZE).take(1024).collect::<Vec<_>>();

        for (key, data) in &input {
            table.put(key, COMMIT, data);
        }

        for (key, _) in input.iter().rev().take(512) {
            table.delete(key, COMMIT);
        }

        let expected_size =
            512 * (DATA_SIZE + MemTable::OVERHEAD_PER_RECORD) + 512 * MemTable::OVERHEAD_PER_RECORD;
        assert_eq!(table.mem_size, expected_size);

        // Deletes of things not in the memory table must be recorded
        for key in gen::keys().take(512) {
            table.delete(&key, COMMIT);
        }

        let expected_size = expected_size + 512 * MemTable::OVERHEAD_PER_RECORD;
        assert_eq!(table.mem_size, expected_size);
    }

    #[test]
    fn test_put_order_irrelevant() {
        let (mut table_1, mut table_2) = (MemTable::default(), MemTable::default());
        let big_input: Vec<_> = gen::pairs(1024).take(128).collect();
        let small_input: Vec<_> = gen::pairs(16).take(128).collect();

        for (key, data) in big_input.iter().chain(small_input.iter()) {
            table_1.put(key, COMMIT, data);
        }

        let iter = big_input
            .iter()
            .rev()
            .zip(small_input.iter().rev())
            .enumerate();

        for (i, ((big_key, big_data), (small_key, small_data))) in iter {
            if i % 2 == 0 {
                table_2.put(big_key, COMMIT, big_data);
                table_2.put(small_key, COMMIT, small_data);
            } else {
                table_2.put(small_key, COMMIT, small_data);
                table_2.put(big_key, COMMIT, big_data);
            }
        }

        assert_eq!(table_1.mem_size, table_2.mem_size);
        assert_eq!(table_1.values, table_2.values);
    }

    #[test]
    fn test_delete_order_irrelevant() {
        let (mut table_1, mut table_2) = (MemTable::default(), MemTable::default());
        let big_input: Vec<_> = gen::pairs(1024).take(128).collect();
        let small_input: Vec<_> = gen::pairs(16).take(128).collect();

        for (key, data) in big_input.iter().chain(small_input.iter()) {
            table_1.put(key, COMMIT, data);
            table_2.put(key, COMMIT, data);
        }

        let iter = big_input
            .iter()
            .rev()
            .take(64)
            .chain(small_input.iter().rev().take(64))
            .map(|(key, _)| key);

        for key in iter {
            table_1.delete(key, COMMIT);
        }

        let iter = big_input
            .iter()
            .rev()
            .take(64)
            .zip(small_input.iter().rev().take(64))
            .map(|((key, _), (key2, _))| (key, key2))
            .enumerate();

        for (i, (big_key, small_key)) in iter {
            if i % 2 == 0 {
                table_2.delete(big_key, COMMIT);
                table_2.delete(small_key, COMMIT);
            } else {
                table_2.delete(small_key, COMMIT);
                table_2.delete(big_key, COMMIT);
            }
        }

        assert_eq!(table_1.mem_size, table_2.mem_size);
        assert_eq!(table_1.values, table_2.values);
    }
}
