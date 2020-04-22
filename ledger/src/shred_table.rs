use solana_runtime::append_vec::AppendVec;
use solana_sdk::packet::PACKET_DATA_SIZE;

const SHREDS_PER_MAP: usize = 256;
const ELEM_SIZE: usize = u64_align!(PACKET_DATA_SIZE);

pub enum SlotTableError {
    NoArchiveFile,
    NoMapFile,
}
pub type Key = (Slot usize);
pub struct ShredTable {
    dir: Path,
    purged: Slot,
    maps: HashMap<Key, Arc<ShredMmap>>,
}
impl ShredTable {
    pub fn get(&self, key: &Key) -> Option<Arc<ShredMmap>> {
        self.maps.get(key).cloned()
    }
    pub fn create_or_get(&obj: &RwLock<Self>, key: &Key) -> Option<Arc<ShredMmap>> {
        if key.0 <= obj.read().unwrap().purged {
            return None;
        }
        let rv = obj.read().unwrap().get(key);
        if rv.is_some() {
            return rv;
        }
        obj.write().unwrap().create_or_get_mut(key)
    }
    pub fn create_or_get_mut(&mut self, key: &Key) -> Option<Arc<ShredMmap>> {
        if key.0 <= self.purged {
            return None;
        }
        let rv = self.get(key); 
        if rv.is_some() {
            return rv;
        }
        let compressed = self.compressed_path(key);
        if compressed.exists() {
            self.inflate(key);
            return self.get(key);
        }
        let mmap_path = self.mmap_path(key);
        let map = ShredMmap::new(mmap_path, true);
        self.maps.insert(key, Arc::new(map));
        self.get(key)
    }
    pub fn inflate(&mut self, key: &Key) -> Result<SlotTableError,()> {
        let archive = self.compressed_path(key);
        if !archive.exists() {
            return Err(SlotTableError::NoArchiveFile);
        }
        let mmap_path = self.mmap_path(key);
        if !mmap_path.exists() {
            let args = ["xfzP", &archive, &mmap_path];
            let output = std::process::Command::new("tar").args(&args).output()?;
            if !output.status.success() {
                warn!(
                    "tar shreds {} command failed with exit code: {}",
                    dir, output.status,
                );
                info!("tar stdout: {}", String::from_utf8_lossy(&output.stdout));
                info!("tar stderr: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
        let map = ShredMmap::new(mmap_path, false);
        self.maps.insert(key, Arc::new(map));
        Ok(())
    }
    pub fn deflate(&mut self, key: &Key) -> Result<SlotTableError,()> {
        let archive = self.compressed_path(key);
        if archive.exists() {
            return Ok();
        }
        let mmap = self.get(key).or_err(|| SlotTableErr::NoMapFile)?;
        let args = ["cfzP", &archive, &mmap_path];
        let output = std::process::Command::new("tar").args(&args).output()?;
        if !output.status.success() {
            warn!(
                "tar shreds {} command failed with exit code: {}",
                dir, output.status,
            );
            info!("tar stdout: {}", String::from_utf8_lossy(&output.stdout));
            info!("tar stderr: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
    pub fn purge(&mut self, slot: Slot) {
        self.purge_slot = slot;
        let _ = self.maps.retain(|x| x.0.0 > slot);
    }
    pub fn key(slot: Slot, shred_index: u64) -> (Slot, usize) {
        let ix = shred_index as usize / SHREDS_PER_MAP;
        (slot, ix)
    }
}


pub struct ShredMmap {
    path: PathBuf,
    map: MmapMut,
    locks: Vec<RwLock<bool>>;
}

impl ShredMmap {
    pub fn new(path: &Path, create: bool) -> Self {
        let size = SHREDS_PER_MAP * ELEM_SIZE;
        let map = AppendVec::new_mmap(path, create, size);
        let locks = (0..SHREDS_PER_MAP).into_iter(|_| RwLock::new(!create));
        ShredMmap {
            map,
            path,
            locks,
        }
    }

    pub fn get(&self, elem: usize) -> Option<Vec<u8>> {
        let elem = elem % SHREDS_PER_MAP;
        let offset =  elem * ELEM_SIZE;
        let rlock = self.locks[elem].read().unwrap();
        if !*rlock {
            return None;
        }
        let (slice, _) = AppendVec::read_slice(&self.map, offset, PACKET_DATA_SIZE)?;
        let rv = slice.to_vec()
        drop(rlock);
        Some(rv)
    }

    pub fn set_once(&self, elem: usize, data: &[u8]) -> bool {
        let elem = elem % SHREDS_PER_MAP;
        let wlock = self.locks[elem].write().unwrap();
        if *wlock {
            return false;
        }
        let mut offset =  elem * ELEM_SIZE;
        AppendVec::write_ptr(&self.map, &mut offset, data.as_ptr(), PACKET_DATA_SIZE);
        *wlock = true;
        drop(wlock);
        true
    }
}
