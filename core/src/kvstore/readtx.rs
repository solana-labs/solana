use crate::kvstore::error::Result;
use crate::kvstore::sstable::{Key, SSTable, Value};
use crate::kvstore::storage;

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

#[derive(Debug)]
pub struct ReadTx {
    mem: Arc<BTreeMap<Key, Value>>,
    tables: Arc<[BTreeMap<Key, SSTable>]>,
}

impl ReadTx {
    pub fn new(mem: BTreeMap<Key, Value>, tables: Vec<BTreeMap<Key, SSTable>>) -> ReadTx {
        ReadTx {
            mem: Arc::new(mem),
            tables: Arc::from(tables.into_boxed_slice()),
        }
    }

    pub fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        storage::get(&self.mem, &*self.tables, key)
    }

    pub fn range(
        &self,
        range: RangeInclusive<Key>,
    ) -> Result<impl Iterator<Item = (Key, Vec<u8>)>> {
        storage::range(&self.mem, &*self.tables, range)
    }
}
