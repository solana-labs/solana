use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    mem,
    path::Path,
};

#[derive(Debug)]
pub struct TieredStorageFile(pub File);

impl TieredStorageFile {
    pub fn new_readonly(file_path: impl AsRef<Path>) -> Self {
        Self(
            OpenOptions::new()
                .read(true)
                .create(false)
                .open(&file_path)
                .unwrap_or_else(|e| {
                    panic!(
                        "[TieredStorageError] Unable to open {:?} as read-only: {:?}",
                        file_path.as_ref().display(),
                        e
                    );
                }),
        )
    }

    pub fn new_writable(file_path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Ok(Self(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(file_path)?,
        ))
    }

    pub fn write_type<T>(&self, value: &T) -> Result<usize, std::io::Error> {
        let ptr = value as *const _ as *const u8;
        let slice = unsafe { std::slice::from_raw_parts(ptr, mem::size_of::<T>()) };
        (&self.0).write_all(slice)?;

        Ok(std::mem::size_of::<T>())
    }

    pub fn read_type<T>(&self, value: &mut T) -> Result<(), std::io::Error> {
        let ptr = value as *mut _ as *mut u8;
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr, mem::size_of::<T>()) };
        (&self.0).read_exact(slice)?;

        Ok(())
    }

    pub fn seek(&self, offset: u64) -> Result<u64, std::io::Error> {
        (&self.0).seek(SeekFrom::Start(offset))
    }

    pub fn seek_from_end(&self, offset: i64) -> Result<u64, std::io::Error> {
        (&self.0).seek(SeekFrom::End(offset))
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> Result<usize, std::io::Error> {
        (&self.0).write_all(bytes)?;

        Ok(bytes.len())
    }

    pub fn read_bytes(&self, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        (&self.0).read_exact(buffer)?;

        Ok(())
    }
}
