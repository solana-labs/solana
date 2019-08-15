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
    fn make_table(&self, kind: Kind, func: &mut dyn FnMut(Writer, Writer)) -> Result<SSTable> {
        let backing = self.get_backing(kind);
        let id = next_id();

        let (data, index) = backing.write_as(|tables| get_memory_writers_for(id, tables))?;
        func(data, index);

        backing.read_as(|map| get_table(id, map))
    }

    fn rotate_tables(&self) -> Result<()> {
        let (mut active, mut compaction, mut garbage) = (
            self.tables.write().expect(BACKING_ERR_MSG),
            self.compaction.write().expect(BACKING_ERR_MSG),
            self.garbage.write().expect(BACKING_ERR_MSG),
        );

        // old active set => garbage
        garbage.extend(active.drain());
        // compacted tables => new active set
        active.extend(compaction.drain());

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

#[cfg(test)]
mod test {
    use super::*;
    use crate::mapper::Kind;
    use crate::sstable::{Key, Value};
    use crate::test::gen;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::thread;

    const DATA_SIZE: usize = 128;

    #[test]
    fn test_table_management() {
        let mapper = Arc::new(Memory::new());
        let records: BTreeMap<_, _> = gen_records().take(1024).collect();

        let mut threads = vec![];
        let mut number_of_tables = 4;

        for kind in [Kind::Active, Kind::Garbage, Kind::Compaction].iter() {
            let records = records.clone();
            let mapper = Arc::clone(&mapper);

            let child = thread::spawn(move || {
                for _ in 0..number_of_tables {
                    mapper
                        .make_table(*kind, &mut |mut data_writer, mut index_writer| {
                            SSTable::create(
                                &mut records.iter(),
                                0,
                                &mut data_writer,
                                &mut index_writer,
                            );
                        })
                        .unwrap();
                }
            });

            number_of_tables *= 2;
            threads.push(child);
        }

        threads.into_iter().for_each(|child| child.join().unwrap());
        assert_eq!(mapper.tables.read().unwrap().len(), 4);
        assert_eq!(mapper.garbage.read().unwrap().len(), 8);
        assert_eq!(mapper.compaction.read().unwrap().len(), 16);

        mapper.empty_trash().unwrap();
        assert_eq!(mapper.garbage.read().unwrap().len(), 0);

        mapper.rotate_tables().unwrap();
        assert_eq!(mapper.tables.read().unwrap().len(), 16);
        assert_eq!(mapper.garbage.read().unwrap().len(), 4);
        assert!(mapper.compaction.read().unwrap().is_empty());

        let active_set = mapper.active_set().unwrap();
        assert_eq!(active_set.len(), 16);
    }

    #[test]
    fn test_no_state() {
        let tempdir = tempfile::tempdir().unwrap();
        let mapper = Arc::new(Memory::new());
        let records: BTreeMap<_, _> = gen_records().take(1024).collect();

        mapper
            .make_table(Kind::Active, &mut |mut data_writer, mut index_writer| {
                SSTable::create(&mut records.iter(), 0, &mut data_writer, &mut index_writer);
            })
            .unwrap();

        let state_path = tempdir.path().join("state");
        mapper.serialize_state_to(&state_path).unwrap();
        mapper.load_state_from(&state_path).unwrap();
        assert!(!state_path.exists());
    }

    fn gen_records() -> impl Iterator<Item = (Key, Value)> {
        gen::pairs(DATA_SIZE).map(|(key, data)| (key, Value::new(0, Some(data))))
    }

}
