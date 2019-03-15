use crate::io_utils::{MemMap, SharedWriter, Writer};
use crate::mapper::{Kind, Mapper, RwLockExt};
use crate::sstable::SSTable;
use crate::Result;

use rand::{rngs::SmallRng, FromEntropy, Rng};

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

type Id = u32;
type TableMap = HashMap<Id, (Arc<RwLock<Vec<u8>>>, Arc<RwLock<Vec<u8>>>)>;
type Backing = Arc<RwLock<TableMap>>;

const BACKING_ERR_MSG: &str = "In-memory table lock poisoned; concurrency error";

#[derive(Debug)]
pub struct Memory {
    tables: Backing,
    compaction: Backing,
    garbage: Backing,
    meta: Arc<RwLock<Vec<u8>>>,
    rng: RwLock<SmallRng>,
}

impl Memory {
    pub fn new() -> Self {
        fn init_backing() -> Backing {
            Arc::new(RwLock::new(HashMap::new()))
        }
        Memory {
            tables: init_backing(),
            compaction: init_backing(),
            garbage: init_backing(),
            meta: Arc::new(RwLock::new(vec![])),
            rng: RwLock::new(SmallRng::from_entropy()),
        }
    }
}

impl Memory {
    #[inline]
    fn get_backing(&self, kind: Kind) -> &Backing {
        match kind {
            Kind::Active => &self.tables,
            Kind::Compaction => &self.compaction,
            Kind::Garbage => &self.garbage,
        }
    }
}

impl Mapper for Memory {
    fn make_table(&self, kind: Kind, func: &mut FnMut(Writer, Writer)) -> Result<SSTable> {
        let backing = self.get_backing(kind);
        let id = next_id();

        let (data, index) = backing.write_as(|tables| get_memory_writers_for(id, tables))?;
        func(data, index);

        backing.read_as(|map| get_table(id, map))
    }

    fn rotate_tables(&self) -> Result<()> {
        use std::mem::swap;

        let (mut active, mut compact, mut garbage) = (
            self.tables.write().expect(BACKING_ERR_MSG),
            self.compaction.write().expect(BACKING_ERR_MSG),
            self.garbage.write().expect(BACKING_ERR_MSG),
        );

        // compacted tables => active set
        swap(&mut active, &mut compact);
        // old active set => garbage
        garbage.extend(compact.drain());

        Ok(())
    }

    fn empty_trash(&self) -> Result<()> {
        self.garbage.write().expect(BACKING_ERR_MSG).clear();

        Ok(())
    }

    fn active_set(&self) -> Result<Vec<SSTable>> {
        let active = self.tables.read().expect(BACKING_ERR_MSG);

        let mut tables = Vec::with_capacity(active.len());
        for tref in active.keys() {
            let sst = get_table(*tref, &*active)?;
            tables.push(sst);
        }

        Ok(tables)
    }

    fn serialize_state_to(&self, _: &Path) -> Result<()> {
        Ok(())
    }

    fn load_state_from(&self, _: &Path) -> Result<()> {
        Ok(())
    }
}

fn get_memory_writers_for(id: Id, backing: &mut TableMap) -> Result<(Writer, Writer)> {
    let data_buf = Arc::new(RwLock::new(vec![]));
    let index_buf = Arc::new(RwLock::new(vec![]));

    backing.insert(id, (Arc::clone(&data_buf), Arc::clone(&index_buf)));

    let data_wtr = SharedWriter::new(data_buf);
    let index_wtr = SharedWriter::new(index_buf);

    let data = Writer::Mem(data_wtr);
    let index = Writer::Mem(index_wtr);

    Ok((data, index))
}

fn get_memmaps(id: Id, map: &TableMap) -> Result<(MemMap, MemMap)> {
    let entry = map
        .get(&id)
        .expect("Map should always be present, given a Id that's not destroyed");

    let data = MemMap::Mem(Arc::clone(&entry.0));
    let index = MemMap::Mem(Arc::clone(&entry.1));

    Ok((data, index))
}

fn get_table(id: Id, map: &TableMap) -> Result<SSTable> {
    let (data, index) = get_memmaps(id, map)?;
    let sst = SSTable::from_parts(Arc::new(data), Arc::new(index))?;

    Ok(sst)
}

#[inline]
fn next_id() -> Id {
    rand::thread_rng().gen()
}
