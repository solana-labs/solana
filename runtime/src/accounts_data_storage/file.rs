use {
    memmap2::MmapMut,
    std::{
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        mem,
        path::Path,
    },
};

#[derive(Debug)]
pub struct AccountsDataStorageFile {
    pub file: File,
    pub map: MmapMut,
}

impl AccountsDataStorageFile {
    pub fn new(file_path: &Path, create: bool) -> Self {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(file_path)
            .map_err(|e| {
                panic!(
                    "Unable to {} data file {} in current dir({:?}): {:?}",
                    if create { "create" } else { "open" },
                    file_path.display(),
                    std::env::current_dir(),
                    e
                );
            })
            .unwrap();
        let map = unsafe { MmapMut::map_mut(&file).unwrap() };
        Self { file, map }
    }

    pub fn write_type<T>(&self, value: &T) -> Result<usize, std::io::Error> {
        unsafe {
            let ptr =
                std::slice::from_raw_parts((value as *const T) as *const u8, mem::size_of::<T>());
            (&self.file).write_all(ptr)?;
        }
        Ok(std::mem::size_of::<T>())
    }

    pub fn read_type<T>(&self, value: &mut T) -> Result<(), std::io::Error> {
        unsafe {
            // TODO(yhchiang): this requires alignment
            let ptr =
                std::slice::from_raw_parts_mut((value as *mut T) as *mut u8, mem::size_of::<T>());
            (&self.file).read_exact(ptr)?;
        }
        Ok(())
    }

    pub fn seek(&self, offset: u64) -> Result<u64, std::io::Error> {
        (&self.file).seek(SeekFrom::Start(offset))
    }

    pub fn seek_from_end(&self, offset: i64) -> Result<u64, std::io::Error> {
        (&self.file).seek(SeekFrom::End(offset))
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> Result<usize, std::io::Error> {
        (&self.file).write_all(bytes)?;

        Ok(bytes.len())
    }

    pub fn read_bytes(&self, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        (&self.file).read_exact(buffer)?;

        Ok(())
    }
}
