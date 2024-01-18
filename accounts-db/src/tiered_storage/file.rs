use {
    bytemuck::{AnyBitPattern, NoUninit},
    std::{
        fs::{File, OpenOptions},
        io::{Read, Result as IoResult, Seek, SeekFrom, Write},
        mem,
        path::Path,
    },
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

    /// Writes `value` to the file.
    ///
    /// `value` must be plain ol' data.
    pub fn write_pod<T: NoUninit>(&self, value: &T) -> IoResult<usize> {
        // SAFETY: Since T is NoUninit, it does not contain any uninitialized bytes.
        unsafe { self.write_type(value) }
    }

    /// Writes `value` to the file.
    ///
    /// Prefer `write_pod` when possible, because `write_value` may cause
    /// undefined behavior if `value` contains uninitialized bytes.
    ///
    /// # Safety
    ///
    /// Caller must ensure casting T to bytes is safe.
    /// Refer to the Safety sections in std::slice::from_raw_parts()
    /// and bytemuck's Pod and NoUninit for more information.
    pub unsafe fn write_type<T>(&self, value: &T) -> IoResult<usize> {
        let ptr = value as *const _ as *const u8;
        let bytes = unsafe { std::slice::from_raw_parts(ptr, mem::size_of::<T>()) };
        self.write_bytes(bytes)
    }

    /// Reads a value of type `T` from the file.
    ///
    /// Type T must be plain ol' data.
    pub fn read_pod<T: NoUninit + AnyBitPattern>(&self, value: &mut T) -> IoResult<()> {
        // SAFETY: Since T is AnyBitPattern, it is safe to cast bytes to T.
        unsafe { self.read_type(value) }
    }

    /// Reads a value of type `T` from the file.
    ///
    /// Prefer `read_pod()` when possible, because `read_type()` may cause
    /// undefined behavior.
    ///
    /// # Safety
    ///
    /// Caller must ensure casting bytes to T is safe.
    /// Refer to the Safety sections in std::slice::from_raw_parts()
    /// and bytemuck's Pod and AnyBitPattern for more information.
    pub unsafe fn read_type<T>(&self, value: &mut T) -> IoResult<()> {
        let ptr = value as *mut _ as *mut u8;
        // SAFETY: The caller ensures it is safe to cast bytes to T,
        // we ensure the size is safe by querying T directly,
        // and Rust ensures ptr is aligned.
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
