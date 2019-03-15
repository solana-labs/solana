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

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    fn make_table(&self, kind: Kind, func: &mut FnMut(Writer, Writer)) -> Result<SSTable> {
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
