use crate::io_utils::{MemMap, Writer};
use crate::mapper::{Kind, Mapper, RwLockExt};
use crate::sstable::SSTable;
use crate::Result;

use memmap::Mmap;

use rand::{rngs::SmallRng, seq::SliceRandom, FromEntropy, Rng};

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Id {
    id: u32,
    kind: Kind,
}

#[derive(Debug)]
pub struct Disk {
    rng: RwLock<SmallRng>,
    mappings: RwLock<HashMap<Id, PathInfo>>,
    storage_dirs: RwLock<Vec<PathBuf>>,
}

impl Disk {
    pub fn single(dir: &Path) -> Self {
        Disk::new(&[dir])
    }

    pub fn new<P: AsRef<Path>>(storage_dirs: &[P]) -> Self {
        if storage_dirs.is_empty() {
            panic!("Disk Mapper requires at least one storage director");
        }

        let storage_dirs = storage_dirs
            .iter()
            .map(AsRef::as_ref)
            .map(Path::to_path_buf)
            .collect();

        Disk {
            storage_dirs: RwLock::new(storage_dirs),
            mappings: RwLock::new(HashMap::new()),
            rng: RwLock::new(SmallRng::from_entropy()),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PathInfo {
    pub data: PathBuf,
    pub index: PathBuf,
}

impl Disk {
    #[inline]
    fn choose_storage(&self) -> PathBuf {
        let mut rng = rand::thread_rng();
        let path = self
            .storage_dirs
            .read_as(|storage| storage.choose(&mut rng).unwrap().to_path_buf());
        if !path.exists() {
            fs::create_dir_all(&path).expect("couldn't create table storage directory");
        }

        path
    }

    #[inline]
    fn add_mapping(&self, tref: Id, paths: PathInfo) {
        let mut map = self.mappings.write().unwrap();
        map.insert(tref, paths);
    }
}

impl Mapper for Disk {
    fn make_table(&self, kind: Kind, func: &mut dyn FnMut(Writer, Writer)) -> Result<SSTable> {
        let storage = self.choose_storage();

        let id = next_id(kind);
        let paths = mk_paths(id, &storage);
        let (data, index) = mk_writers(&paths)?;

        func(data, index);

        self.add_mapping(id, paths.clone());

        let (data, index) = mk_maps(&paths)?;
        let sst = SSTable::from_parts(Arc::new(data), Arc::new(index))?;
        Ok(sst)
    }

    fn rotate_tables(&self) -> Result<()> {
        let mut map = self.mappings.write().unwrap();
        let mut new_map = HashMap::new();

        for (tref, paths) in map.drain() {
            let new_kind = match tref.kind {
                Kind::Active => Kind::Garbage,
                Kind::Compaction => Kind::Active,
                k => k,
            };
            let new_ref = next_id(new_kind);
            new_map.insert(new_ref, paths);
        }
        *map = new_map;

        Ok(())
    }

    fn empty_trash(&self) -> Result<()> {
        self.mappings.write_as(|map| {
            let to_rm = map
                .keys()
                .filter(|tref| tref.kind == Kind::Garbage)
                .cloned()
                .collect::<Vec<_>>();

            for tref in to_rm {
                let paths = map.remove(&tref).unwrap();
                fs::remove_file(&paths.index)?;
                fs::remove_file(&paths.data)?;
            }

            Ok(())
        })
    }

    fn active_set(&self) -> Result<Vec<SSTable>> {
        let map = self.mappings.read().unwrap();
        let active = map.iter().filter(|(tref, _)| tref.kind == Kind::Active);
        let mut vec = Vec::new();

        for (_, paths) in active {
            let (data, index): (MemMap, MemMap) = mk_maps(paths)?;
            let sst = SSTable::from_parts(Arc::new(data), Arc::new(index))?;

            vec.push(sst);
        }
        Ok(vec)
    }

    fn serialize_state_to(&self, path: &Path) -> Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        let wtr = BufWriter::new(file);

        self.mappings.read_as(|mappings| {
            self.storage_dirs
                .read_as(|storage| bincode::serialize_into(wtr, &(storage, mappings)))
        })?;

        Ok(())
    }

    fn load_state_from(&self, path: &Path) -> Result<()> {
        let rdr = BufReader::new(File::open(path)?);
        let (new_storage, new_mappings) = bincode::deserialize_from(rdr)?;

        self.storage_dirs.write_as(|storage| {
            self.mappings.write_as(|mappings| {
                *storage = new_storage;
                *mappings = new_mappings;
            })
        });

        Ok(())
    }
}

fn mk_writers(paths: &PathInfo) -> io::Result<(Writer, Writer)> {
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);

    let data = BufWriter::new(opts.open(&paths.data)?);
    let index = BufWriter::new(opts.open(&paths.index)?);

    Ok((Writer::Disk(data), Writer::Disk(index)))
}

fn mk_maps(paths: &PathInfo) -> io::Result<(MemMap, MemMap)> {
    let (data_file, index_file) = (File::open(&paths.data)?, File::open(&paths.index)?);
    let (data, index) = unsafe { (Mmap::map(&data_file)?, Mmap::map(&index_file)?) };
    Ok((MemMap::Disk(data), MemMap::Disk(index)))
}

fn mk_paths(tref: Id, dir: &Path) -> PathInfo {
    let (data_name, index_name) = mk_filenames(tref.id);
    PathInfo {
        data: dir.join(data_name),
        index: dir.join(index_name),
    }
}

#[inline]
fn mk_filenames(n: u32) -> (String, String) {
    let data = format!("{}.sstable", n,);
    let index = format!("{}.index", n,);
    (data, index)
}

#[inline]
fn next_id(kind: Kind) -> Id {
    Id {
        id: rand::thread_rng().gen(),
        kind,
    }
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
    use tempfile::tempdir;

    const DATA_SIZE: usize = 128;

    #[test]
    fn test_table_management() {
        let tempdir = tempdir().unwrap();
        let mapper = Arc::new(Disk::single(tempdir.path()));
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
        let count_kind = |kind, mapper: &Disk| {
            mapper
                .mappings
                .read()
                .unwrap()
                .keys()
                .filter(|id| id.kind == kind)
                .count()
        };
        assert_eq!(count_kind(Kind::Active, &mapper), 4);
        assert_eq!(count_kind(Kind::Garbage, &mapper), 8);
        assert_eq!(count_kind(Kind::Compaction, &mapper), 16);

        mapper.empty_trash().unwrap();
        assert_eq!(count_kind(Kind::Garbage, &mapper), 0);

        mapper.rotate_tables().unwrap();
        assert_eq!(count_kind(Kind::Active, &mapper), 16);
        assert_eq!(count_kind(Kind::Garbage, &mapper), 4);
        assert_eq!(count_kind(Kind::Compaction, &mapper), 0);

        let active_set = mapper.active_set().unwrap();
        assert_eq!(active_set.len(), 16);
    }

    #[test]
    fn test_state() {
        let tempdir = tempdir().unwrap();
        let dirs_1: Vec<_> = (0..4).map(|i| tempdir.path().join(i.to_string())).collect();
        let dirs_2: Vec<_> = (4..8).map(|i| tempdir.path().join(i.to_string())).collect();

        let mapper_1 = Arc::new(Disk::new(&dirs_1));
        let records: BTreeMap<_, _> = gen_records().take(1024).collect();

        for (i, &kind) in [Kind::Active, Kind::Compaction, Kind::Garbage]
            .iter()
            .enumerate()
        {
            for _ in 0..(i * 3) {
                mapper_1
                    .make_table(kind, &mut |mut data_writer, mut index_writer| {
                        SSTable::create(
                            &mut records.iter(),
                            0,
                            &mut data_writer,
                            &mut index_writer,
                        );
                    })
                    .unwrap();
            }
        }

        let state_path = tempdir.path().join("state");
        mapper_1.serialize_state_to(&state_path).unwrap();
        assert!(state_path.exists());

        let mapper_2 = Arc::new(Disk::new(&dirs_2));
        mapper_2.load_state_from(&state_path).unwrap();

        assert_eq!(
            &*mapper_1.mappings.read().unwrap(),
            &*mapper_2.mappings.read().unwrap()
        );
        assert_eq!(
            &*mapper_1.storage_dirs.read().unwrap(),
            &*mapper_2.storage_dirs.read().unwrap()
        );
    }

    fn gen_records() -> impl Iterator<Item = (Key, Value)> {
        gen::pairs(DATA_SIZE).map(|(key, data)| (key, Value::new(0, Some(data))))
    }

}
