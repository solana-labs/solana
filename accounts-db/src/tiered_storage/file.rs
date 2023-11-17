use std::{
    fs::{File, OpenOptions},
    io::{Read, Result as IoResult, Seek, SeekFrom, Write},
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
                .unwrap_or_else(|err| {
                    panic!(
                        "[TieredStorageError] Unable to open {} as read-only: {err}",
                        file_path.as_ref().display(),
                    );
                }),
        )
    }

    pub fn new_writable(file_path: impl AsRef<Path>) -> IoResult<Self> {
        Ok(Self(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(file_path)?,
        ))
    }

    pub fn write_type<T>(&self, value: &T) -> IoResult<usize> {
        let ptr = value as *const _ as *const u8;
        let bytes = unsafe { std::slice::from_raw_parts(ptr, mem::size_of::<T>()) };
        self.write_bytes(bytes)
    }

    pub fn read_type<T>(&self, value: &mut T) -> IoResult<()> {
        let ptr = value as *mut _ as *mut u8;
        let bytes = unsafe { std::slice::from_raw_parts_mut(ptr, mem::size_of::<T>()) };
        self.read_bytes(bytes)
    }

    pub fn seek(&self, offset: u64) -> IoResult<u64> {
        (&self.0).seek(SeekFrom::Start(offset))
    }

    pub fn seek_from_end(&self, offset: i64) -> IoResult<u64> {
        (&self.0).seek(SeekFrom::End(offset))
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> IoResult<usize> {
        (&self.0).write_all(bytes)?;

        Ok(bytes.len())
    }

    pub fn read_bytes(&self, buffer: &mut [u8]) -> IoResult<()> {
        (&self.0).read_exact(buffer)
    }
}
