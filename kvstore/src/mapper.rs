use crate::io_utils::Writer;
use crate::sstable::SSTable;
use crate::Result;

use std::path::Path;
use std::sync::RwLock;

mod disk;
mod memory;

pub use self::disk::Disk;
pub use self::memory::Memory;

pub trait Mapper: std::fmt::Debug + Send + Sync {
    fn make_table(&self, kind: Kind, func: &mut dyn FnMut(Writer, Writer)) -> Result<SSTable>;
    fn rotate_tables(&self) -> Result<()>;
    fn empty_trash(&self) -> Result<()>;
    fn active_set(&self) -> Result<Vec<SSTable>>;
    fn serialize_state_to(&self, path: &Path) -> Result<()>;
    fn load_state_from(&self, path: &Path) -> Result<()>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Kind {
    Active,
    Compaction,
    Garbage,
}

pub trait RwLockExt<T> {
    fn read_as<U, F: FnOnce(&T) -> U>(&self, f: F) -> U;
    fn write_as<U, F: FnOnce(&mut T) -> U>(&self, f: F) -> U;
    fn try_read_as<U, F: FnOnce(&T) -> U>(&self, f: F) -> U;
    fn try_write_as<U, F: FnOnce(&mut T) -> U>(&self, f: F) -> U;
}

impl<T> RwLockExt<T> for RwLock<T> {
    fn read_as<U, F: FnOnce(&T) -> U>(&self, f: F) -> U {
        f(&*self.read().unwrap())
    }
    fn write_as<U, F: FnOnce(&mut T) -> U>(&self, f: F) -> U {
        f(&mut *self.write().unwrap())
    }
    fn try_read_as<U, F: FnOnce(&T) -> U>(&self, f: F) -> U {
        f(&*self.try_read().unwrap())
    }
    fn try_write_as<U, F: FnOnce(&mut T) -> U>(&self, f: F) -> U {
        f(&mut *self.try_write().unwrap())
    }
}
