//! Persistent storage for accounts.
//!
//! For more information, see:
//!
//! <https://docs.solanalabs.com/implemented-proposals/persistent-account-storage>

use {
    crate::{
        account_storage::meta::{AccountMeta, StoredAccountMeta, StoredMeta},
        accounts_file::{
            AccountsFileError, InternalsForArchive, MatchAccountOwnerError, Result, StorageAccess,
            StoredAccountsInfo, ALIGN_BOUNDARY_OFFSET,
        },
        accounts_hash::AccountHash,
        accounts_index::ZeroLamport,
        buffered_reader::{BufferedReader, BufferedReaderStatus},
        file_io::read_into_buffer,
        storable_accounts::StorableAccounts,
        u64_align,
    },
    log::*,
    memmap2::MmapMut,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        hash::Hash,
        pubkey::Pubkey,
        stake_history::Epoch,
        system_instruction::MAX_PERMITTED_DATA_LENGTH,
    },
    std::{
        convert::TryFrom,
        fs::{remove_file, File, OpenOptions},
        io::{Seek, SeekFrom, Write},
        mem,
        path::{Path, PathBuf},
        ptr,
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            Mutex,
        },
    },
    thiserror::Error,
};

pub mod test_utils;
#[cfg(test)]
use solana_sdk::account::accounts_equal;

/// size of the fixed sized fields in an append vec
/// we need to add data len and align it to get the actual stored size
pub const STORE_META_OVERHEAD: usize = 136;

// Ensure the STORE_META_OVERHEAD constant remains accurate
const _: () = assert!(
    STORE_META_OVERHEAD
        == mem::size_of::<StoredMeta>()
            + mem::size_of::<AccountMeta>()
            + mem::size_of::<AccountHash>()
);

/// Returns the size this item will take to store plus possible alignment padding bytes before the next entry.
/// fixed-size portion of per-account data written
/// plus 'data_len', aligned to next boundary
pub fn aligned_stored_size(data_len: usize) -> usize {
    u64_align!(STORE_META_OVERHEAD + data_len)
}

pub const MAXIMUM_APPEND_VEC_FILE_SIZE: u64 = 16 * 1024 * 1024 * 1024; // 16 GiB

#[derive(Error, Debug)]
/// An enum for AppendVec related errors.
pub enum AppendVecError {
    #[error("too small file size {0} for AppendVec")]
    FileSizeTooSmall(usize),

    #[error("too large file size {0} for AppendVec")]
    FileSizeTooLarge(usize),

    #[error("incorrect layout/length/data in the appendvec at path {}", .0.display())]
    IncorrectLayout(PathBuf),

    #[error("offset ({0}) is larger than file size ({1})")]
    OffsetOutOfBounds(usize, usize),
}

/// A slice whose contents are known to be valid.
/// The slice contains no undefined bytes.
#[derive(Debug, Copy, Clone)]
pub(crate) struct ValidSlice<'a>(&'a [u8]);

impl<'a> ValidSlice<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self(data)
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(all(unix, test))]
    pub(crate) fn slice(&self) -> &[u8] {
        self.0
    }
}

/// References to account data stored elsewhere. Getting an `Account` requires cloning
/// (see `StoredAccountMeta::clone_account()`).
#[derive(PartialEq, Eq, Debug)]
pub struct AppendVecStoredAccountMeta<'append_vec> {
    pub meta: &'append_vec StoredMeta,
    /// account data
    pub account_meta: &'append_vec AccountMeta,
    pub(crate) data: &'append_vec [u8],
    pub(crate) offset: usize,
    pub(crate) stored_size: usize,
    pub(crate) hash: &'append_vec AccountHash,
}

impl<'append_vec> AppendVecStoredAccountMeta<'append_vec> {
    pub fn pubkey(&self) -> &'append_vec Pubkey {
        &self.meta.pubkey
    }

    pub fn hash(&self) -> &'append_vec AccountHash {
        self.hash
    }

    pub fn stored_size(&self) -> usize {
        self.stored_size
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn data(&self) -> &'append_vec [u8] {
        self.data
    }

    pub fn data_len(&self) -> u64 {
        self.meta.data_len
    }

    pub fn meta(&self) -> &StoredMeta {
        self.meta
    }

    pub(crate) fn sanitize(&self) -> bool {
        self.sanitize_executable() && self.sanitize_lamports()
    }

    fn sanitize_executable(&self) -> bool {
        // Sanitize executable to ensure higher 7-bits are cleared correctly.
        self.ref_executable_byte() & !1 == 0
    }

    fn sanitize_lamports(&self) -> bool {
        // Sanitize 0 lamports to ensure to be same as AccountSharedData::default()
        self.account_meta.lamports != 0
            || self.to_account_shared_data() == AccountSharedData::default()
    }

    fn ref_executable_byte(&self) -> &u8 {
        // Use extra references to avoid value silently clamped to 1 (=true) and 0 (=false)
        // Yes, this really happens; see test_new_from_file_crafted_executable
        let executable_bool: &bool = &self.account_meta.executable;
        let executable_bool_ptr = ptr::from_ref(executable_bool);
        // UNSAFE: Force to interpret mmap-backed bool as u8 to really read the actual memory content
        let executable_byte: &u8 = unsafe { &*(executable_bool_ptr.cast()) };
        executable_byte
    }
}

impl<'append_vec> ReadableAccount for AppendVecStoredAccountMeta<'append_vec> {
    fn lamports(&self) -> u64 {
        self.account_meta.lamports
    }
    fn data(&self) -> &'append_vec [u8] {
        self.data()
    }
    fn owner(&self) -> &'append_vec Pubkey {
        &self.account_meta.owner
    }
    fn executable(&self) -> bool {
        self.account_meta.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.account_meta.rent_epoch
    }
}

/// info from an entry useful for building an index
pub(crate) struct IndexInfo {
    /// size of entry, aligned to next u64
    /// This matches the return of `get_account`
    pub stored_size_aligned: usize,
    /// info on the entry
    pub index_info: IndexInfoInner,
}

/// info from an entry useful for building an index
pub(crate) struct IndexInfoInner {
    /// offset to this entry
    pub offset: usize,
    pub pubkey: Pubkey,
    pub lamports: u64,
    pub rent_epoch: Epoch,
    pub executable: bool,
    pub data_len: u64,
}

/// offsets to help navigate the persisted format of `AppendVec`
#[derive(Debug)]
struct AccountOffsets {
    /// offset to the end of the &[u8] data
    offset_to_end_of_data: usize,
    /// offset to the next account. This will be aligned.
    next_account_offset: usize,
    /// # of bytes (aligned) to store this account, including variable sized data
    stored_size_aligned: usize,
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug)]
enum AppendVecFileBacking {
    /// A file-backed block of memory that is used to store the data for each appended item.
    Mmap(Mmap),
    /// This was opened as a read only file
    #[cfg_attr(not(unix), allow(dead_code))]
    File(File),
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug)]
struct Mmap {
    mmap: MmapMut,
    /// Flags if the mmap is dirty or not.
    /// Since fastboot requires that all mmaps are flushed to disk, be smart about it.
    /// AppendVecs are (almost) always write-once.  The common case is that an AppendVec
    /// will only need to be flushed once.  This avoids unnecessary syscalls/kernel work
    /// when nothing in the AppendVec has changed.
    is_dirty: AtomicBool,
}

/// A thread-safe, file-backed block of memory used to store `Account` instances. Append operations
/// are serialized such that only one thread updates the internal `append_lock` at a time. No
/// restrictions are placed on reading. That is, one may read items from one thread while another
/// is appending new items.
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug)]
pub struct AppendVec {
    /// The file path where the data is stored.
    path: PathBuf,

    /// access the file data
    backing: AppendVecFileBacking,

    /// A lock used to serialize append operations.
    append_lock: Mutex<()>,

    /// The number of bytes used to store items, not the number of items.
    current_len: AtomicUsize,

    /// The number of bytes available for storing items.
    file_size: u64,

    /// if true, remove file when dropped
    remove_file_on_drop: AtomicBool,
}

const PAGE_SIZE: u64 = 4 * 1024;
/// big enough for 3x the largest account size
const SCAN_BUFFER_SIZE: usize =
    page_align((STORE_META_OVERHEAD as u64 + MAX_PERMITTED_DATA_LENGTH) * 3) as usize;
const fn page_align(size: u64) -> u64 {
    (size + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1)
}

/// Buffer size to use when scanning *without* needing account data
///
/// When scanning without needing account data, it is desirable to only read the account metadata
/// and skip over the account data.  In theory, we could read a single account's metadata at a time,
/// then skip ahead to the next account, entirely bypassing the account's data.  However this comes
/// at the cost of requiring one syscall per scanning each account, which is expensive.  Ideally
/// we'd like to use the fewest syscalls and also read the least amount of extraneous account data.
/// As a compromise, we use a much smaller buffer, yet still large enough to amortize syscall cost.
///
/// On mnb, the overwhelming majority of accounts are token accounts, which use 165 bytes of data.
/// Including storage overhead and alignment, that's 304 bytes per account.
/// Per slot, *with* rent rewrites, we store 1,200 to 1,500 accounts.  With a 64 KiB buffer, we'd
/// be able to hold about 215 accounts, so there would not be many syscalls needed to scan
/// the file.  Since we also expect some larger accounts, this will also avoid reading/copying
/// large account data.  This should be a decent starting value, and can be modified over time.
#[cfg_attr(feature = "dev-context-only-utils", qualifier_attr::qualifiers(pub))]
const SCAN_BUFFER_SIZE_WITHOUT_DATA: usize = 1 << 16;

lazy_static! {
    pub static ref APPEND_VEC_MMAPPED_FILES_OPEN: AtomicU64 = AtomicU64::default();
    pub static ref APPEND_VEC_MMAPPED_FILES_DIRTY: AtomicU64 = AtomicU64::default();
    pub static ref APPEND_VEC_OPEN_AS_FILE_IO: AtomicU64 = AtomicU64::default();
}

impl Drop for AppendVec {
    fn drop(&mut self) {
        APPEND_VEC_MMAPPED_FILES_OPEN.fetch_sub(1, Ordering::Relaxed);
        match &self.backing {
            AppendVecFileBacking::Mmap(mmap_only) => {
                if mmap_only.is_dirty.load(Ordering::Acquire) {
                    APPEND_VEC_MMAPPED_FILES_DIRTY.fetch_sub(1, Ordering::Relaxed);
                }
            }
            AppendVecFileBacking::File(_) => {
                APPEND_VEC_OPEN_AS_FILE_IO.fetch_sub(1, Ordering::Relaxed);
            }
        }
        if self.remove_file_on_drop.load(Ordering::Acquire) {
            // If we're reopening in readonly mode, we don't delete the file. See
            // AppendVec::reopen_as_readonly.
            if let Err(_err) = remove_file(&self.path) {
                // promote this to panic soon.
                // disabled due to many false positive warnings while running tests.
                // blocked by rpc's upgrade to jsonrpc v17
                //error!("AppendVec failed to remove {}: {err}", &self.path.display());
                inc_new_counter_info!("append_vec_drop_fail", 1);
            }
        }
    }
}

impl AppendVec {
    pub fn new(file: impl Into<PathBuf>, create: bool, size: usize) -> Self {
        let file = file.into();
        let initial_len = 0;
        AppendVec::sanitize_len_and_size(initial_len, size).unwrap();

        if create {
            let _ignored = remove_file(&file);
        }

        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&file)
            .map_err(|e| {
                panic!(
                    "Unable to {} data file {} in current dir({:?}): {:?}",
                    if create { "create" } else { "open" },
                    file.display(),
                    std::env::current_dir(),
                    e
                );
            })
            .unwrap();

        // Theoretical performance optimization: write a zero to the end of
        // the file so that we won't have to resize it later, which may be
        // expensive.
        data.seek(SeekFrom::Start((size - 1) as u64)).unwrap();
        data.write_all(&[0]).unwrap();
        data.rewind().unwrap();
        data.flush().unwrap();

        //UNSAFE: Required to create a Mmap
        let mmap = unsafe { MmapMut::map_mut(&data) };
        let mmap = mmap.unwrap_or_else(|e| {
            error!(
                "Failed to map the data file (size: {}): {}.\n
                    Please increase sysctl vm.max_map_count or equivalent for your platform.",
                size, e
            );
            std::process::exit(1);
        });
        APPEND_VEC_MMAPPED_FILES_OPEN.fetch_add(1, Ordering::Relaxed);

        AppendVec {
            path: file,
            backing: AppendVecFileBacking::Mmap(Mmap {
                mmap,
                is_dirty: AtomicBool::new(false),
            }),
            // This mutex forces append to be single threaded, but concurrent with reads
            // See UNSAFE usage in `append_ptr`
            append_lock: Mutex::new(()),
            current_len: AtomicUsize::new(initial_len),
            file_size: size as u64,
            remove_file_on_drop: AtomicBool::new(true),
        }
    }

    fn sanitize_len_and_size(current_len: usize, file_size: usize) -> Result<()> {
        if file_size == 0 {
            Err(AccountsFileError::AppendVecError(
                AppendVecError::FileSizeTooSmall(file_size),
            ))
        } else if usize::try_from(MAXIMUM_APPEND_VEC_FILE_SIZE)
            .map(|max| file_size > max)
            .unwrap_or(true)
        {
            Err(AccountsFileError::AppendVecError(
                AppendVecError::FileSizeTooLarge(file_size),
            ))
        } else if current_len > file_size {
            Err(AccountsFileError::AppendVecError(
                AppendVecError::OffsetOutOfBounds(current_len, file_size),
            ))
        } else {
            Ok(())
        }
    }

    pub fn flush(&self) -> Result<()> {
        match &self.backing {
            AppendVecFileBacking::Mmap(mmap_only) => {
                // Check to see if the mmap is actually dirty before flushing.
                if mmap_only.is_dirty.load(Ordering::Acquire) {
                    mmap_only.mmap.flush()?;
                    mmap_only.is_dirty.store(false, Ordering::Release);
                    APPEND_VEC_MMAPPED_FILES_DIRTY.fetch_sub(1, Ordering::Relaxed);
                }
                Ok(())
            }
            // File also means read only, so nothing to flush.
            AppendVecFileBacking::File(_file) => Ok(()),
        }
    }

    pub fn reset(&self) {
        // This mutex forces append to be single threaded, but concurrent with reads
        // See UNSAFE usage in `append_ptr`
        let _lock = self.append_lock.lock().unwrap();
        self.current_len.store(0, Ordering::Release);
    }

    /// when we can use file i/o as opposed to mmap, this is the trigger to tell us
    /// that no more appending will occur and we can close the initial mmap.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn reopen_as_readonly(&self) -> Option<Self> {
        #[cfg(not(unix))]
        // must open as mmmap on non-unix
        return None;

        #[cfg(unix)]
        match &self.backing {
            // already a file, so already read-only
            AppendVecFileBacking::File(_file) => None,
            AppendVecFileBacking::Mmap(_mmap) => {
                // we are a map, so re-open as a file
                self.flush().expect("flush must succeed");
                // we are re-opening the file, so don't remove the file on disk when the old mmapped one is dropped
                self.remove_file_on_drop.store(false, Ordering::Release);

                // The file should have already been sanitized. Don't need to check when we open the file again.
                AppendVec::new_from_file_unchecked(
                    self.path.clone(),
                    self.len(),
                    StorageAccess::File,
                )
                .ok()
            }
        }
    }

    /// how many more bytes can be stored in this append vec
    pub fn remaining_bytes(&self) -> u64 {
        self.capacity()
            .saturating_sub(u64_align!(self.len()) as u64)
    }

    pub fn len(&self) -> usize {
        self.current_len.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> u64 {
        self.file_size
    }

    pub fn new_from_file(
        path: impl Into<PathBuf>,
        current_len: usize,
        storage_access: StorageAccess,
    ) -> Result<(Self, usize)> {
        let path = path.into();
        let new = Self::new_from_file_unchecked(path, current_len, storage_access)?;

        let (sanitized, num_accounts) = new.sanitize_layout_and_length();
        if !sanitized {
            return Err(AccountsFileError::AppendVecError(
                AppendVecError::IncorrectLayout(new.path.clone()),
            ));
        }

        Ok((new, num_accounts))
    }

    /// Creates an appendvec from file without performing sanitize checks or counting the number of accounts
    #[cfg_attr(not(unix), allow(unused_variables))]
    pub fn new_from_file_unchecked(
        path: impl Into<PathBuf>,
        current_len: usize,
        storage_access: StorageAccess,
    ) -> Result<Self> {
        let path = path.into();
        let file_size = std::fs::metadata(&path)?.len();
        Self::sanitize_len_and_size(current_len, file_size as usize)?;

        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&path)?;

        #[cfg(unix)]
        // we must use mmap on non-linux
        if storage_access == StorageAccess::File {
            APPEND_VEC_MMAPPED_FILES_OPEN.fetch_add(1, Ordering::Relaxed);
            APPEND_VEC_OPEN_AS_FILE_IO.fetch_add(1, Ordering::Relaxed);

            return Ok(AppendVec {
                path,
                backing: AppendVecFileBacking::File(data),
                append_lock: Mutex::new(()),
                current_len: AtomicUsize::new(current_len),
                file_size,
                remove_file_on_drop: AtomicBool::new(true),
            });
        }

        let mmap = unsafe {
            let result = MmapMut::map_mut(&data);
            if result.is_err() {
                // for vm.max_map_count, error is: {code: 12, kind: Other, message: "Cannot allocate memory"}
                info!("memory map error: {:?}. This may be because vm.max_map_count is not set correctly.", result);
            }
            result?
        };
        APPEND_VEC_MMAPPED_FILES_OPEN.fetch_add(1, Ordering::Relaxed);

        Ok(AppendVec {
            path,
            backing: AppendVecFileBacking::Mmap(Mmap {
                mmap,
                is_dirty: AtomicBool::new(false),
            }),
            append_lock: Mutex::new(()),
            current_len: AtomicUsize::new(current_len),
            file_size,
            remove_file_on_drop: AtomicBool::new(true),
        })
    }

    /// Opens the AppendVec at `path` for use by `store-tool`
    #[cfg(feature = "dev-context-only-utils")]
    pub fn new_for_store_tool(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let file_size = std::fs::metadata(&path)?.len();
        Self::new_from_file_unchecked(path, file_size as usize, StorageAccess::default())
    }

    fn sanitize_layout_and_length(&self) -> (bool, usize) {
        // This discards allocated accounts immediately after check at each loop iteration.
        //
        // This code should not reuse AppendVec.accounts() method as the current form or
        // extend it to be reused here because it would allow attackers to accumulate
        // some measurable amount of memory needlessly.
        let mut num_accounts = 0;
        let mut matches = true;
        let mut last_offset = 0;
        self.scan_accounts(|account| {
            if !matches || !account.sanitize() {
                matches = false;
                return;
            }
            last_offset = account.offset() + account.stored_size();
            num_accounts += 1;
        });
        if !matches {
            return (false, num_accounts);
        }
        let aligned_current_len = u64_align!(self.current_len.load(Ordering::Acquire));

        (last_offset == aligned_current_len, num_accounts)
    }

    /// Get a reference to the data at `offset` of `size` bytes if that slice
    /// doesn't overrun the internal buffer. Otherwise return None.
    /// Also return the offset of the first byte after the requested data that
    /// falls on a 64-byte boundary.
    fn get_slice(slice: ValidSlice, offset: usize, size: usize) -> Option<(&[u8], usize)> {
        // SAFETY: Wrapping math is safe here because if `end` does wrap, the Range
        // parameter to `.get()` will be invalid, and `.get()` will correctly return None.
        let end = offset.wrapping_add(size);
        slice
            .0
            .get(offset..end)
            .map(|subslice| (subslice, u64_align!(end)))
    }

    /// Copy `len` bytes from `src` to the first 64-byte boundary after position `offset` of
    /// the internal buffer. Then update `offset` to the first byte after the copied data.
    fn append_ptr(&self, offset: &mut usize, src: *const u8, len: usize) {
        let pos = u64_align!(*offset);
        match &self.backing {
            AppendVecFileBacking::Mmap(mmap_only) => {
                let data = &mmap_only.mmap[pos..(pos + len)];
                //UNSAFE: This mut append is safe because only 1 thread can append at a time
                //Mutex<()> guarantees exclusive write access to the memory occupied in
                //the range.
                unsafe {
                    let dst = data.as_ptr() as *mut _;
                    ptr::copy(src, dst, len);
                };
                *offset = pos + len;
            }
            AppendVecFileBacking::File(_file) => {
                unimplemented!();
            }
        }
    }

    /// Copy each value in `vals`, in order, to the first 64-byte boundary after position `offset`.
    /// If there is sufficient space, then update `offset` and the internal `current_len` to the
    /// first byte after the copied data and return the starting position of the copied data.
    /// Otherwise return None and leave `offset` unchanged.
    fn append_ptrs_locked(&self, offset: &mut usize, vals: &[(*const u8, usize)]) -> Option<usize> {
        let mut end = *offset;
        for val in vals {
            end = u64_align!(end);
            end += val.1;
        }

        if (self.file_size as usize) < end {
            return None;
        }

        let pos = u64_align!(*offset);
        for val in vals {
            self.append_ptr(offset, val.0, val.1)
        }
        self.current_len.store(*offset, Ordering::Release);
        Some(pos)
    }

    /// Return a reference to the type at `offset` if its data doesn't overrun the internal buffer.
    /// Otherwise return None. Also return the offset of the first byte after the requested data
    /// that falls on a 64-byte boundary.
    fn get_type<T>(slice: ValidSlice, offset: usize) -> Option<(&T, usize)> {
        let (data, next) = Self::get_slice(slice, offset, mem::size_of::<T>())?;
        let ptr = data.as_ptr().cast();
        //UNSAFE: The cast is safe because the slice is aligned and fits into the memory
        //and the lifetime of the &T is tied to self, which holds the underlying memory map
        Some((unsafe { &*ptr }, next))
    }

    /// MmapMut could have more capacity than `len()` knows is valid.
    /// Return the subset of `mmap` that is known to be valid.
    /// This allows comparisons against the slice len.
    fn get_valid_slice_from_mmap<'a>(&self, mmap: &'a MmapMut) -> ValidSlice<'a> {
        ValidSlice(&mmap[..self.len()])
    }

    /// calls `callback` with the stored account metadata for the account at `offset` if its data doesn't overrun
    /// the internal buffer. Otherwise return None.
    pub fn get_stored_account_meta_callback<Ret>(
        &self,
        offset: usize,
        mut callback: impl for<'local> FnMut(StoredAccountMeta<'local>) -> Ret,
    ) -> Option<Ret> {
        match &self.backing {
            AppendVecFileBacking::Mmap(Mmap { mmap, .. }) => {
                let slice = self.get_valid_slice_from_mmap(mmap);
                let (meta, next): (&StoredMeta, _) = Self::get_type(slice, offset)?;
                let (account_meta, next): (&AccountMeta, _) = Self::get_type(slice, next)?;
                let (hash, next): (&AccountHash, _) = Self::get_type(slice, next)?;
                let (data, next) = Self::get_slice(slice, next, meta.data_len as usize)?;
                let stored_size = next - offset;
                Some(callback(StoredAccountMeta::AppendVec(
                    AppendVecStoredAccountMeta {
                        meta,
                        account_meta,
                        data,
                        offset,
                        stored_size,
                        hash,
                    },
                )))
            }
            AppendVecFileBacking::File(file) => {
                // 4096 was just picked to be a single page size
                let mut buf = [0u8; PAGE_SIZE as usize];
                let bytes_read = read_into_buffer(file, self.len(), offset, &mut buf).ok()?;
                let valid_bytes = ValidSlice(&buf[..bytes_read]);
                let (meta, next): (&StoredMeta, _) = Self::get_type(valid_bytes, 0)?;
                let (account_meta, next): (&AccountMeta, _) = Self::get_type(valid_bytes, next)?;
                let (hash, next): (&AccountHash, _) = Self::get_type(valid_bytes, next)?;
                let data_len = meta.data_len;
                let remaining_bytes_for_data = bytes_read - next;
                Some(if remaining_bytes_for_data >= data_len as usize {
                    // we already read enough data to load this account
                    let (data, next) = Self::get_slice(valid_bytes, next, meta.data_len as usize)?;
                    let stored_size = next;
                    let account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
                        meta,
                        account_meta,
                        data,
                        offset,
                        stored_size,
                        hash,
                    });
                    callback(account)
                } else {
                    // not enough was read from file to get `data`
                    assert!(data_len <= MAX_PERMITTED_DATA_LENGTH, "{data_len}");
                    let mut data = vec![0u8; data_len as usize];
                    // instead, we could piece together what we already read here. Maybe we just needed 1 more byte.
                    // Note here `next` is a 0-based offset from the beginning of this account.
                    let bytes_read =
                        read_into_buffer(file, self.len(), offset + next, &mut data).ok()?;
                    if bytes_read < data_len as usize {
                        // eof or otherwise couldn't read all the data
                        return None;
                    }
                    let stored_size = aligned_stored_size(data_len as usize);
                    let account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
                        meta,
                        account_meta,
                        data: &data[..],
                        offset,
                        stored_size,
                        hash,
                    });
                    callback(account)
                })
            }
        }
    }

    /// return an `AccountSharedData` for an account at `offset`.
    /// This fn can efficiently return exactly what is needed by a caller.
    /// This is on the critical path of tx processing for accounts not in the read or write caches.
    pub(crate) fn get_account_shared_data(&self, offset: usize) -> Option<AccountSharedData> {
        match &self.backing {
            AppendVecFileBacking::Mmap(_) => self
                .get_stored_account_meta_callback(offset, |account| {
                    account.to_account_shared_data()
                }),
            AppendVecFileBacking::File(file) => {
                let mut buf = [0u8; PAGE_SIZE as usize];
                let bytes_read = read_into_buffer(file, self.len(), offset, &mut buf).ok()?;
                let valid_bytes = ValidSlice(&buf[..bytes_read]);
                let (meta, next): (&StoredMeta, _) = Self::get_type(valid_bytes, 0)?;
                let (account_meta, next): (&AccountMeta, _) = Self::get_type(valid_bytes, next)?;
                let (hash, next): (&AccountHash, _) = Self::get_type(valid_bytes, next)?;
                let data_len = meta.data_len;
                let remaining_bytes_for_data = bytes_read - next;
                Some(if remaining_bytes_for_data >= data_len as usize {
                    // we already read enough data to load this account
                    let (data, next) = Self::get_slice(valid_bytes, next, meta.data_len as usize)?;
                    let stored_size = next;
                    let account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
                        meta,
                        account_meta,
                        data,
                        offset,
                        stored_size,
                        hash,
                    });
                    // data is within `buf`, so just allocate a new vec for data
                    account.to_account_shared_data()
                } else {
                    // not enough was read from file to get `data`
                    assert!(data_len <= MAX_PERMITTED_DATA_LENGTH, "{data_len}");
                    let mut data = vec![0u8; data_len as usize];
                    // Note here `next` is a 0-based offset from the beginning of this account.
                    let bytes_read =
                        read_into_buffer(file, self.len(), offset + next, &mut data).ok()?;
                    if bytes_read < data_len as usize {
                        // eof or otherwise couldn't read all the data
                        return None;
                    }
                    AccountSharedData::create(
                        account_meta.lamports,
                        data,
                        account_meta.owner,
                        account_meta.executable,
                        account_meta.rent_epoch,
                    )
                })
            }
        }
    }

    /// Return Ok(index_of_matching_owner) if the account owner at `offset` is one of the pubkeys in `owners`.
    /// Return Err(MatchAccountOwnerError::NoMatch) if the account has 0 lamports or the owner is not one of
    /// the pubkeys in `owners`.
    /// Return Err(MatchAccountOwnerError::UnableToLoad) if the `offset` value causes a data overrun.
    pub fn account_matches_owners(
        &self,
        offset: usize,
        owners: &[Pubkey],
    ) -> std::result::Result<usize, MatchAccountOwnerError> {
        self.get_stored_account_meta_callback(offset, |stored_account_meta| {
            if stored_account_meta.lamports() == 0 {
                Err(MatchAccountOwnerError::NoMatch)
            } else {
                owners
                    .iter()
                    .position(|entry| stored_account_meta.owner() == entry)
                    .ok_or(MatchAccountOwnerError::NoMatch)
            }
        })
        .unwrap_or(Err(MatchAccountOwnerError::UnableToLoad))
    }

    #[cfg(test)]
    pub fn get_account_test(
        &self,
        offset: usize,
    ) -> Option<(StoredMeta, solana_sdk::account::AccountSharedData)> {
        let sizes = self.get_account_sizes(&[offset]);
        let result = self.get_stored_account_meta_callback(offset, |r_callback| {
            let r2 = self.get_account_shared_data(offset);
            assert!(accounts_equal(&r_callback, r2.as_ref().unwrap()));
            assert_eq!(sizes, vec![r_callback.stored_size()]);
            let meta = r_callback.meta().clone();
            Some((meta, r_callback.to_account_shared_data()))
        });
        if result.is_none() {
            assert!(self
                .get_stored_account_meta_callback(offset, |_| {})
                .is_none());
            assert!(self.get_account_shared_data(offset).is_none());
            // it has different rules for checking len and returning None
            assert!(sizes.is_empty());
        }
        result.flatten()
    }

    /// Returns the path to the file where the data is stored
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// help with the math of offsets when navigating the on-disk layout in an AppendVec.
    /// data is at the end of each account and is variable sized
    /// the next account is then aligned on a 64 bit boundary.
    /// With these helpers, we can skip over reading some of the data depending on what the caller wants.
    ///
    /// *Safety* - The caller must ensure that the `stored_meta.data_len` won't overflow the calculation.
    fn next_account_offset(start_offset: usize, stored_meta: &StoredMeta) -> AccountOffsets {
        let stored_size_unaligned = STORE_META_OVERHEAD
            .checked_add(stored_meta.data_len as usize)
            .expect("stored size cannot overflow");
        let stored_size_aligned = u64_align!(stored_size_unaligned);
        let offset_to_end_of_data = start_offset + stored_size_unaligned;
        let next_account_offset = start_offset + stored_size_aligned;

        AccountOffsets {
            next_account_offset,
            offset_to_end_of_data,
            stored_size_aligned,
        }
    }

    /// Iterate over all accounts and call `callback` with `IndexInfo` for each.
    /// This fn can help generate an index of the data in this storage.
    pub(crate) fn scan_index(&self, mut callback: impl FnMut(IndexInfo)) {
        // self.len() is an atomic load, so only do it once
        let self_len = self.len();
        match &self.backing {
            AppendVecFileBacking::Mmap(Mmap { mmap, .. }) => {
                let mut offset = 0;
                let slice = self.get_valid_slice_from_mmap(mmap);
                loop {
                    let Some((stored_meta, next)) = Self::get_type::<StoredMeta>(slice, offset)
                    else {
                        // eof
                        break;
                    };
                    let Some((account_meta, _)) = Self::get_type::<AccountMeta>(slice, next) else {
                        // eof
                        break;
                    };
                    if account_meta.lamports == 0 && stored_meta.pubkey == Pubkey::default() {
                        // we passed the last useful account
                        break;
                    }
                    let next = Self::next_account_offset(offset, stored_meta);
                    if next.offset_to_end_of_data > self_len {
                        // data doesn't fit, so don't include this account
                        break;
                    }
                    callback(IndexInfo {
                        index_info: {
                            IndexInfoInner {
                                pubkey: stored_meta.pubkey,
                                lamports: account_meta.lamports,
                                offset,
                                data_len: stored_meta.data_len,
                                executable: account_meta.executable,
                                rent_epoch: account_meta.rent_epoch,
                            }
                        },
                        stored_size_aligned: next.stored_size_aligned,
                    });
                    offset = next.next_account_offset;
                }
            }
            AppendVecFileBacking::File(file) => {
                let buffer_size = std::cmp::min(SCAN_BUFFER_SIZE, self_len);
                let mut reader =
                    BufferedReader::new(buffer_size, self_len, file, STORE_META_OVERHEAD);
                while reader.read().ok() == Some(BufferedReaderStatus::Success) {
                    let (offset, bytes) = reader.get_offset_and_data();
                    let (stored_meta, next) = Self::get_type::<StoredMeta>(bytes, 0).unwrap();
                    let (account_meta, _) = Self::get_type::<AccountMeta>(bytes, next).unwrap();
                    if account_meta.lamports == 0 && stored_meta.pubkey == Pubkey::default() {
                        // we passed the last useful account
                        break;
                    }
                    let next = Self::next_account_offset(offset, stored_meta);
                    if next.offset_to_end_of_data > self_len {
                        // data doesn't fit, so don't include this account
                        break;
                    }
                    callback(IndexInfo {
                        index_info: {
                            IndexInfoInner {
                                pubkey: stored_meta.pubkey,
                                lamports: account_meta.lamports,
                                offset,
                                data_len: stored_meta.data_len,
                                executable: account_meta.executable,
                                rent_epoch: account_meta.rent_epoch,
                            }
                        },
                        stored_size_aligned: next.stored_size_aligned,
                    });
                    reader.advance_offset(next.stored_size_aligned);
                }
            }
        }
    }

    /// Iterate over all accounts and call `callback` with each account.
    #[allow(clippy::blocks_in_conditions)]
    pub fn scan_accounts(&self, mut callback: impl for<'local> FnMut(StoredAccountMeta<'local>)) {
        match &self.backing {
            AppendVecFileBacking::Mmap(_mmap) => {
                let mut offset = 0;
                while self
                    .get_stored_account_meta_callback(offset, |account| {
                        offset += account.stored_size();
                        if account.is_zero_lamport() && account.pubkey() == &Pubkey::default() {
                            // we passed the last useful account
                            return false;
                        }

                        callback(account);
                        true
                    })
                    .unwrap_or_default()
                {}
            }
            AppendVecFileBacking::File(file) => {
                let mut reader =
                    BufferedReader::new(SCAN_BUFFER_SIZE, self.len(), file, STORE_META_OVERHEAD);
                while reader.read().ok() == Some(BufferedReaderStatus::Success) {
                    let (offset, bytes_subset) = reader.get_offset_and_data();
                    let (meta, next): (&StoredMeta, _) = Self::get_type(bytes_subset, 0).unwrap();
                    let (account_meta, next): (&AccountMeta, _) =
                        Self::get_type(bytes_subset, next).unwrap();
                    let (hash, next): (&AccountHash, _) =
                        Self::get_type(bytes_subset, next).unwrap();
                    let data_len = meta.data_len;
                    if bytes_subset.len() - next >= data_len as usize {
                        // we already read enough data to load this account
                        let data = &bytes_subset.0[next..(next + data_len as usize)];
                        let stored_size = u64_align!(next + (data_len as usize));
                        let account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
                            meta,
                            account_meta,
                            data,
                            offset,
                            stored_size,
                            hash,
                        });
                        callback(account);
                        reader.advance_offset(stored_size);
                    } else {
                        // fall through and read the whole account again. we need refs for StoredMeta and data.
                        reader.set_required_data_len(
                            STORE_META_OVERHEAD.saturating_add(data_len as usize),
                        )
                    }
                }
            }
        }
    }

    /// for each offset in `sorted_offsets`, get the size of the account. No other information is needed for the account.
    pub(crate) fn get_account_sizes(&self, sorted_offsets: &[usize]) -> Vec<usize> {
        // self.len() is an atomic load, so only do it once
        let self_len = self.len();
        let mut account_sizes = Vec::with_capacity(sorted_offsets.len());
        match &self.backing {
            AppendVecFileBacking::Mmap(Mmap { mmap, .. }) => {
                let slice = self.get_valid_slice_from_mmap(mmap);
                for &offset in sorted_offsets {
                    let Some((stored_meta, _)) = Self::get_type::<StoredMeta>(slice, offset) else {
                        break;
                    };
                    let next = Self::next_account_offset(offset, stored_meta);
                    if next.offset_to_end_of_data > self_len {
                        // data doesn't fit, so don't include
                        break;
                    }
                    account_sizes.push(next.stored_size_aligned);
                }
            }
            AppendVecFileBacking::File(file) => {
                let mut buffer = [0u8; mem::size_of::<StoredMeta>()];
                for &offset in sorted_offsets {
                    let Some(bytes_read) =
                        read_into_buffer(file, self_len, offset, &mut buffer).ok()
                    else {
                        break;
                    };
                    let bytes = ValidSlice(&buffer[..bytes_read]);
                    let Some((stored_meta, _)) = Self::get_type::<StoredMeta>(bytes, 0) else {
                        break;
                    };
                    let next = Self::next_account_offset(offset, stored_meta);
                    if next.offset_to_end_of_data > self_len {
                        // data doesn't fit, so don't include
                        break;
                    }
                    account_sizes.push(next.stored_size_aligned);
                }
            }
        }
        account_sizes
    }

    /// iterate over all pubkeys and call `callback`.
    /// This iteration does not deserialize and populate each field in `StoredAccountMeta`.
    /// `data` is completely ignored, for example.
    /// Also, no references have to be maintained/returned from an iterator function.
    /// This fn can operate on a batch of data at once.
    pub fn scan_pubkeys(&self, mut callback: impl FnMut(&Pubkey)) {
        // self.len() is an atomic load, so only do it once
        let self_len = self.len();
        match &self.backing {
            AppendVecFileBacking::Mmap(Mmap { mmap, .. }) => {
                let mut offset = 0;
                let slice = self.get_valid_slice_from_mmap(mmap);
                loop {
                    let Some((stored_meta, _)) = Self::get_type::<StoredMeta>(slice, offset) else {
                        // eof
                        break;
                    };
                    let next = Self::next_account_offset(offset, stored_meta);
                    if next.offset_to_end_of_data > self_len {
                        // data doesn't fit, so don't include this pubkey
                        break;
                    }
                    callback(&stored_meta.pubkey);
                    offset = next.next_account_offset;
                }
            }
            AppendVecFileBacking::File(file) => {
                let buffer_size = std::cmp::min(SCAN_BUFFER_SIZE_WITHOUT_DATA, self_len);
                let mut reader =
                    BufferedReader::new(buffer_size, self_len, file, STORE_META_OVERHEAD);
                while reader.read().ok() == Some(BufferedReaderStatus::Success) {
                    let (offset, bytes) = reader.get_offset_and_data();
                    let (stored_meta, _) = Self::get_type::<StoredMeta>(bytes, 0).unwrap();
                    let next = Self::next_account_offset(offset, stored_meta);
                    if next.offset_to_end_of_data > self.len() {
                        // data doesn't fit, so don't include this pubkey
                        break;
                    }
                    callback(&stored_meta.pubkey);
                    // since we only needed to read the pubkey, skip ahead to the next account
                    reader.advance_offset(next.stored_size_aligned);
                }
            }
        }
    }

    /// Copy each account metadata, account and hash to the internal buffer.
    /// If there is no room to write the first entry, None is returned.
    /// Otherwise, returns the starting offset of each account metadata.
    /// Plus, the final return value is the offset where the next entry would be appended.
    /// So, return.len() is 1 + (number of accounts written)
    /// After each account is appended, the internal `current_len` is updated
    /// and will be available to other threads.
    pub fn append_accounts<'a>(
        &self,
        accounts: &impl StorableAccounts<'a>,
        skip: usize,
    ) -> Option<StoredAccountsInfo> {
        let _lock = self.append_lock.lock().unwrap();
        let default_hash = Hash::default();
        let mut offset = self.len();
        let len = accounts.len();
        // Here we have `len - skip` number of accounts.  The +1 extra capacity
        // is for storing the aligned offset of the last-plus-one entry,
        // which is used to compute the size of the last stored account.
        let offsets_len = len - skip + 1;
        let mut offsets = Vec::with_capacity(offsets_len);
        let mut stop = false;
        for i in skip..len {
            if stop {
                break;
            }
            accounts.account_default_if_zero_lamport(i, |account| {
                let account_meta = AccountMeta {
                    lamports: account.lamports(),
                    owner: *account.owner(),
                    rent_epoch: account.rent_epoch(),
                    executable: account.executable(),
                };

                let stored_meta = StoredMeta {
                    pubkey: *account.pubkey(),
                    data_len: account.data().len() as u64,
                    write_version_obsolete: 0,
                };
                let stored_meta_ptr = ptr::from_ref(&stored_meta).cast();
                let account_meta_ptr = ptr::from_ref(&account_meta).cast();
                let hash_ptr = bytemuck::bytes_of(&default_hash).as_ptr();
                let data_ptr = account.data().as_ptr();
                let ptrs = [
                    (stored_meta_ptr, mem::size_of::<StoredMeta>()),
                    (account_meta_ptr, mem::size_of::<AccountMeta>()),
                    (hash_ptr, mem::size_of::<AccountHash>()),
                    (data_ptr, stored_meta.data_len as usize),
                ];
                if let Some(start_offset) = self.append_ptrs_locked(&mut offset, &ptrs) {
                    offsets.push(start_offset)
                } else {
                    stop = true;
                }
            });
        }

        match &self.backing {
            AppendVecFileBacking::Mmap(mmap_only) => {
                if !offsets.is_empty() {
                    // If we've actually written to the AppendVec, make sure we mark it as dirty.
                    // This ensures we properly flush it later.
                    // As an optimization to reduce unnecessary cache line invalidations,
                    // only write the `is_dirty` atomic if currently *not* dirty.
                    // (This also ensures the 'dirty counter' datapoint is correct.)
                    if !mmap_only.is_dirty.load(Ordering::Acquire) {
                        mmap_only.is_dirty.store(true, Ordering::Release);
                        APPEND_VEC_MMAPPED_FILES_DIRTY.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            AppendVecFileBacking::File(_) => {}
        }

        (!offsets.is_empty()).then(|| {
            // The last entry in the offsets needs to be the u64 aligned `offset`, because that's
            // where the *next* entry will begin to be stored.
            // This is used to compute the size of the last stored account; make sure to remove
            // it afterwards!
            offsets.push(u64_align!(offset));
            let size = offsets.windows(2).map(|offset| offset[1] - offset[0]).sum();
            offsets.pop();

            StoredAccountsInfo { offsets, size }
        })
    }

    pub(crate) fn can_append(&self) -> bool {
        match &self.backing {
            AppendVecFileBacking::File(_file) => false,
            AppendVecFileBacking::Mmap(_mmap) => true,
        }
    }

    /// Returns the way to access this accounts file when archiving
    pub(crate) fn internals_for_archive(&self) -> InternalsForArchive {
        match &self.backing {
            AppendVecFileBacking::File(_file) => InternalsForArchive::FileIo(self.path()),
            // note this returns the entire mmap slice, even bytes that we consider invalid
            AppendVecFileBacking::Mmap(Mmap { mmap, .. }) => InternalsForArchive::Mmap(mmap),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::{test_utils::*, *},
        assert_matches::assert_matches,
        memoffset::offset_of,
        rand::{thread_rng, Rng},
        solana_sdk::{
            account::{Account, AccountSharedData},
            clock::Slot,
        },
        std::{mem::ManuallyDrop, time::Instant},
        test_case::test_case,
    };

    impl AppendVec {
        pub(crate) fn set_current_len_for_tests(&self, len: usize) {
            self.current_len.store(len, Ordering::Release);
        }

        fn append_account_test(&self, data: &(StoredMeta, AccountSharedData)) -> Option<usize> {
            let slot_ignored = Slot::MAX;
            let accounts = [(&data.0.pubkey, &data.1)];
            let slice = &accounts[..];
            let storable_accounts = (slot_ignored, slice);

            self.append_accounts(&storable_accounts, 0)
                .map(|res| res.offsets[0])
        }
    }

    impl StoredAccountMeta<'_> {
        pub(crate) fn ref_executable_byte(&self) -> &u8 {
            match self {
                Self::AppendVec(av) => av.ref_executable_byte(),
                // Tests currently only cover AppendVec.
                Self::Hot(_) => unreachable!(),
            }
        }
    }

    impl AppendVecStoredAccountMeta<'_> {
        fn set_data_len_unsafe(&self, new_data_len: u64) {
            // UNSAFE: cast away & (= const ref) to &mut to force to mutate append-only (=read-only) AppendVec
            unsafe {
                #[allow(invalid_reference_casting)]
                ptr::write(
                    std::mem::transmute::<*const u64, *mut u64>(&self.meta.data_len),
                    new_data_len,
                );
            }
        }

        fn get_executable_byte(&self) -> u8 {
            let executable_bool: bool = self.executable();
            // UNSAFE: Force to interpret mmap-backed bool as u8 to really read the actual memory content
            let executable_byte: u8 = unsafe { std::mem::transmute::<bool, u8>(executable_bool) };
            executable_byte
        }

        fn set_executable_as_byte(&self, new_executable_byte: u8) {
            // UNSAFE: Force to interpret mmap-backed &bool as &u8 to write some crafted value;
            unsafe {
                #[allow(invalid_reference_casting)]
                ptr::write(
                    std::mem::transmute::<*const bool, *mut u8>(&self.account_meta.executable),
                    new_executable_byte,
                );
            }
        }
    }

    // Hash is [u8; 32], which has no alignment
    static_assertions::assert_eq_align!(u64, StoredMeta, AccountMeta);

    #[test]
    fn test_account_meta_default() {
        let def1 = AccountMeta::default();
        let def2 = AccountMeta::from(&Account::default());
        assert_eq!(&def1, &def2);
        let def2 = AccountMeta::from(&AccountSharedData::default());
        assert_eq!(&def1, &def2);
        let def2 = AccountMeta::from(Some(&AccountSharedData::default()));
        assert_eq!(&def1, &def2);
        let none: Option<&AccountSharedData> = None;
        let def2 = AccountMeta::from(none);
        assert_eq!(&def1, &def2);
    }

    #[test]
    fn test_account_meta_non_default() {
        let def1 = AccountMeta {
            lamports: 1,
            owner: Pubkey::new_unique(),
            executable: true,
            rent_epoch: 3,
        };
        let def2_account = Account {
            lamports: def1.lamports,
            owner: def1.owner,
            executable: def1.executable,
            rent_epoch: def1.rent_epoch,
            data: Vec::new(),
        };
        let def2 = AccountMeta::from(&def2_account);
        assert_eq!(&def1, &def2);
        let def2 = AccountMeta::from(&AccountSharedData::from(def2_account.clone()));
        assert_eq!(&def1, &def2);
        let def2 = AccountMeta::from(Some(&AccountSharedData::from(def2_account)));
        assert_eq!(&def1, &def2);
    }

    #[test]
    #[should_panic(expected = "AppendVecError(FileSizeTooSmall(0))")]
    fn test_append_vec_new_bad_size() {
        let path = get_append_vec_path("test_append_vec_new_bad_size");
        let _av = AppendVec::new(&path.path, true, 0);
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_append_vec_new_from_file_bad_size(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_append_vec_new_from_file_bad_size");
        let path = &file.path;

        let _data = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .expect("create a test file for mmap");

        let result = AppendVec::new_from_file(path, 0, storage_access);
        assert_matches!(result, Err(ref message) if message.to_string().contains("too small file size 0 for AppendVec"));
    }

    #[test]
    fn test_append_vec_sanitize_len_and_size_too_small() {
        const LEN: usize = 0;
        const SIZE: usize = 0;
        let result = AppendVec::sanitize_len_and_size(LEN, SIZE);
        assert_matches!(result, Err(ref message) if message.to_string().contains("too small file size 0 for AppendVec"));
    }

    #[test]
    fn test_append_vec_sanitize_len_and_size_maximum() {
        const LEN: usize = 0;
        const SIZE: usize = 16 * 1024 * 1024 * 1024;
        let result = AppendVec::sanitize_len_and_size(LEN, SIZE);
        assert_matches!(result, Ok(_));
    }

    #[test]
    fn test_append_vec_sanitize_len_and_size_too_large() {
        const LEN: usize = 0;
        const SIZE: usize = 16 * 1024 * 1024 * 1024 + 1;
        let result = AppendVec::sanitize_len_and_size(LEN, SIZE);
        assert_matches!(result, Err(ref message) if message.to_string().contains("too large file size 17179869185 for AppendVec"));
    }

    #[test]
    fn test_append_vec_sanitize_len_and_size_full_and_same_as_current_len() {
        const LEN: usize = 1024 * 1024;
        const SIZE: usize = 1024 * 1024;
        let result = AppendVec::sanitize_len_and_size(LEN, SIZE);
        assert_matches!(result, Ok(_));
    }

    #[test]
    fn test_append_vec_sanitize_len_and_size_larger_current_len() {
        const LEN: usize = 1024 * 1024 + 1;
        const SIZE: usize = 1024 * 1024;
        let result = AppendVec::sanitize_len_and_size(LEN, SIZE);
        assert_matches!(result, Err(ref message) if message.to_string().contains("is larger than file size (1048576)"));
    }

    #[test]
    fn test_append_vec_one() {
        let path = get_append_vec_path("test_append");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let account = create_test_account(0);
        let index = av.append_account_test(&account).unwrap();
        assert_eq!(av.get_account_test(index).unwrap(), account);
        truncate_and_test(av, index);
    }

    /// truncate `av` and make sure that we fail to get an account. This verifies that the eof
    /// code is working correctly.
    fn truncate_and_test(av: AppendVec, index: usize) {
        // truncate the hash, 1 byte at a time
        let hash = std::mem::size_of::<AccountHash>();
        for _ in 0..hash {
            av.current_len.fetch_sub(1, Ordering::Relaxed);
            assert_eq!(av.get_account_test(index), None);
        }
        // truncate 1 byte into the AccountMeta
        av.current_len.fetch_sub(1, Ordering::Relaxed);
        assert_eq!(av.get_account_test(index), None);
    }

    #[test]
    fn test_append_vec_one_with_data() {
        let path = get_append_vec_path("test_append");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let data_len = 1;
        let account = create_test_account(data_len);
        let index = av.append_account_test(&account).unwrap();
        // make the append vec 1 byte too short. we should get `None` since the append vec was truncated
        assert_eq!(
            STORE_META_OVERHEAD + data_len,
            av.current_len.load(Ordering::Relaxed)
        );
        assert_eq!(av.get_account_test(index).unwrap(), account);
        truncate_and_test(av, index);
    }

    #[test]
    fn test_remaining_bytes() {
        let path = get_append_vec_path("test_append");
        let sz = 1024 * 1024;
        let sz64 = sz as u64;
        let av = AppendVec::new(&path.path, true, sz);
        assert_eq!(av.capacity(), sz64);
        assert_eq!(av.remaining_bytes(), sz64);

        // append first account, an u64 aligned account (136 bytes)
        let mut av_len = 0;
        let account = create_test_account(0);
        av.append_account_test(&account).unwrap();
        av_len += STORE_META_OVERHEAD;
        assert_eq!(av.capacity(), sz64);
        assert_eq!(av.remaining_bytes(), sz64 - (STORE_META_OVERHEAD as u64));
        assert_eq!(av.len(), av_len);

        // append second account, a *not* u64 aligned account (137 bytes)
        let account = create_test_account(1);
        let account_storage_len = STORE_META_OVERHEAD + 1;
        av_len += account_storage_len;
        av.append_account_test(&account).unwrap();
        assert_eq!(av.capacity(), sz64);
        assert_eq!(av.len(), av_len);
        let alignment_bytes = u64_align!(av_len) - av_len; // bytes used for alignment (7 bytes)
        assert_eq!(alignment_bytes, 7);
        assert_eq!(av.remaining_bytes(), sz64 - u64_align!(av_len) as u64);

        // append third account, a *not* u64 aligned account (137 bytes)
        let account = create_test_account(1);
        av.append_account_test(&account).unwrap();
        let account_storage_len = STORE_META_OVERHEAD + 1;
        av_len += alignment_bytes; // bytes used for alignment at the end of previous account
        av_len += account_storage_len;
        assert_eq!(av.capacity(), sz64);
        assert_eq!(av.len(), av_len);
        assert_eq!(av.remaining_bytes(), sz64 - u64_align!(av_len) as u64);
    }

    #[test]
    fn test_append_vec_data() {
        let path = get_append_vec_path("test_append_data");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let account = create_test_account(5);
        let index = av.append_account_test(&account).unwrap();
        assert_eq!(av.get_account_test(index).unwrap(), account);
        let account1 = create_test_account(6);
        let index1 = av.append_account_test(&account1).unwrap();
        assert_eq!(av.get_account_test(index).unwrap(), account);
        assert_eq!(av.get_account_test(index1).unwrap(), account1);
    }

    #[test]
    fn test_account_matches_owners() {
        let path = get_append_vec_path("test_append_data");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let owners: Vec<Pubkey> = (0..2).map(|_| Pubkey::new_unique()).collect();

        let mut account = create_test_account(5);
        account.1.set_owner(owners[0]);
        let index = av.append_account_test(&account).unwrap();
        assert_eq!(av.account_matches_owners(index, &owners), Ok(0));

        let mut account1 = create_test_account(6);
        account1.1.set_owner(owners[1]);
        let index1 = av.append_account_test(&account1).unwrap();
        assert_eq!(av.account_matches_owners(index1, &owners), Ok(1));
        assert_eq!(av.account_matches_owners(index, &owners), Ok(0));

        let mut account2 = create_test_account(6);
        account2.1.set_owner(Pubkey::new_unique());
        let index2 = av.append_account_test(&account2).unwrap();
        assert_eq!(
            av.account_matches_owners(index2, &owners),
            Err(MatchAccountOwnerError::NoMatch)
        );

        // tests for overflow
        assert_eq!(
            av.account_matches_owners(usize::MAX - mem::size_of::<StoredMeta>(), &owners),
            Err(MatchAccountOwnerError::UnableToLoad)
        );

        assert_eq!(
            av.account_matches_owners(
                usize::MAX - mem::size_of::<StoredMeta>() - mem::size_of::<AccountMeta>() + 1,
                &owners
            ),
            Err(MatchAccountOwnerError::UnableToLoad)
        );
    }

    impl AppendVec {
        /// return how many accounts in the storage
        fn accounts_count(&self) -> usize {
            let mut count = 0;
            self.scan_accounts(|_| {
                count += 1;
            });
            count
        }
    }

    #[test]
    fn test_append_vec_append_many() {
        let path = get_append_vec_path("test_append_many");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let size = 1000;
        let mut indexes = vec![];
        let now = Instant::now();
        let mut sizes = vec![];
        for sample in 0..size {
            // sample + 1 is so sample = 0 won't be used.
            // sample = 0 produces default account with default pubkey
            let account = create_test_account(sample + 1);
            sizes.push(aligned_stored_size(account.1.data().len()));
            let pos = av.append_account_test(&account).unwrap();
            assert_eq!(av.get_account_test(pos).unwrap(), account);
            indexes.push(pos);
            assert_eq!(sizes, av.get_account_sizes(&indexes));
        }
        trace!("append time: {} ms", now.elapsed().as_millis());

        let now = Instant::now();
        for _ in 0..size {
            let sample = thread_rng().gen_range(0..indexes.len());
            let account = create_test_account(sample + 1);
            assert_eq!(av.get_account_test(indexes[sample]).unwrap(), account);
        }
        trace!("random read time: {} ms", now.elapsed().as_millis());

        let now = Instant::now();
        assert_eq!(indexes.len(), size);
        assert_eq!(indexes[0], 0);
        let mut sample = 0;
        assert_eq!(av.accounts_count(), size);
        av.scan_accounts(|v| {
            let account = create_test_account(sample + 1);
            let recovered = v.to_account_shared_data();
            assert_eq!(recovered, account.1);
            sample += 1;
        });
        trace!("sequential read time: {} ms", now.elapsed().as_millis());
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_new_from_file_crafted_zero_lamport_account(storage_access: StorageAccess) {
        // This test verifies that when we sanitize on load, that we fail sanitizing if we load an account with zero lamports that does not have all default value fields.
        // This test writes an account with zero lamports, but with 3 bytes of data. On load, it asserts that load fails.
        // It used to be possible to use the append vec api to write an account to an append vec with zero lamports, but with non-default values for other account fields.
        // This will no longer be possible. Thus, to implement the write portion of this test would require additional test-only parameters to public apis or otherwise duplicating code paths.
        // So, the sanitizing on load behavior can be tested by capturing [u8] that would be created if such a write was possible (as it used to be).
        // The contents of [u8] written by an append vec cannot easily or reasonably change frequently since it has released a long time.
        /*
            solana_logger::setup();
            // uncomment this code to generate the invalid append vec that will fail on load
            let file = get_append_vec_path("test_append");
            let path = &file.path;
            let mut av = AppendVec::new(path, true, 256);
            av.set_no_remove_on_drop();

            let pubkey = solana_sdk::pubkey::new_rand();
            let owner = Pubkey::default();
            let data_len = 3_u64;
            let mut account = AccountSharedData::new(0, data_len as usize, &owner);
            account.set_data(b"abc".to_vec());
            let stored_meta = StoredMeta {
                write_version: 0,
                pubkey,
                data_len,
            };
            let account_with_meta = (stored_meta, account);
            let index = av.append_account_test(&account_with_meta).unwrap();
            assert_eq!(av.get_account_test(index).unwrap(), account_with_meta);

            av.flush().unwrap();
            let accounts_len = av.len();
            drop(av);
            // read file and log out as [u8]
            use std::fs::File;
            use std::io::BufReader;
            use std::io::Read;
            let f = File::open(path).unwrap();
            let mut reader = BufReader::new(f);
            let mut buffer = Vec::new();
            reader.read_to_end(&mut buffer).unwrap();
            error!("{:?}", buffer);
        */

        // create an invalid append vec file using known bytes
        let file = get_append_vec_path("test_append_bytes");
        let path = &file.path;

        let accounts_len = 139;
        {
            let append_vec_data = [
                0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 192, 118, 150, 1, 185, 209, 118,
                82, 154, 222, 172, 202, 110, 26, 218, 140, 143, 96, 61, 43, 212, 73, 203, 7, 190,
                88, 80, 222, 110, 114, 67, 254, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 98, 99, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];

            let f = std::fs::File::create(path).unwrap();
            let mut writer = std::io::BufWriter::new(f);
            writer.write_all(append_vec_data.as_slice()).unwrap();
        }

        let result = AppendVec::new_from_file(path, accounts_len, storage_access);
        assert_matches!(result, Err(ref message) if message.to_string().contains("incorrect layout/length/data"));
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_new_from_file_crafted_data_len(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_new_from_file_crafted_data_len");
        let path = &file.path;
        let accounts_len = {
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let av = ManuallyDrop::new(AppendVec::new(path, true, 1024 * 1024));

            let crafted_data_len = 1;

            av.append_account_test(&create_test_account(10)).unwrap();

            av.get_stored_account_meta_callback(0, |account| {
                let StoredAccountMeta::AppendVec(account) = account else {
                    panic!("StoredAccountMeta can only be AppendVec in this test.");
                };
                account.set_data_len_unsafe(crafted_data_len);
                assert_eq!(account.data_len(), crafted_data_len);

                // Reload accounts and observe crafted_data_len
                av.get_stored_account_meta_callback(0, |account| {
                    assert_eq!(account.data_len() as u64, crafted_data_len);
                });
            });

            av.flush().unwrap();
            av.len()
        };
        let result = AppendVec::new_from_file(path, accounts_len, storage_access);
        assert_matches!(result, Err(ref message) if message.to_string().contains("incorrect layout/length/data"));
    }

    #[test]
    fn test_append_vec_reset() {
        let file = get_append_vec_path("test_append_vec_reset");
        let path = &file.path;
        let av = AppendVec::new(path, true, 1024 * 1024);
        av.append_account_test(&create_test_account(10)).unwrap();

        assert!(!av.is_empty());
        av.reset();
        assert_eq!(av.len(), 0);
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_append_vec_flush(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_append_vec_flush");
        let path = &file.path;
        let accounts_len = {
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let av = ManuallyDrop::new(AppendVec::new(path, true, 1024 * 1024));
            av.append_account_test(&create_test_account(10)).unwrap();
            av.len()
        };

        let (av, num_account) =
            AppendVec::new_from_file(path, accounts_len, storage_access).unwrap();
        av.flush().unwrap();
        assert_eq!(num_account, 1);
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_append_vec_reopen_as_readonly(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_append_vec_flush");
        let path = &file.path;
        let accounts_len = {
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let av = ManuallyDrop::new(AppendVec::new(path, true, 1024 * 1024));
            av.append_account_test(&create_test_account(10)).unwrap();
            av.len()
        };
        let (av, _) = AppendVec::new_from_file(path, accounts_len, storage_access).unwrap();
        let reopen = av.reopen_as_readonly();
        if storage_access == StorageAccess::File {
            assert!(reopen.is_none());
        } else {
            assert!(reopen.is_some());
        }
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_new_from_file_too_large_data_len(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_new_from_file_too_large_data_len");
        let path = &file.path;
        let accounts_len = {
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let av = ManuallyDrop::new(AppendVec::new(path, true, 1024 * 1024));

            let too_large_data_len = u64::MAX;
            av.append_account_test(&create_test_account(10)).unwrap();

            av.get_stored_account_meta_callback(0, |account| {
                let StoredAccountMeta::AppendVec(account) = account else {
                    panic!("StoredAccountMeta can only be AppendVec in this test.");
                };
                account.set_data_len_unsafe(too_large_data_len);
                assert_eq!(account.data_len(), too_large_data_len);
            })
            .unwrap();

            // Reload accounts and observe no account with bad offset
            assert!(av
                .get_stored_account_meta_callback(0, |_| {
                    panic!("unexpected");
                })
                .is_none());
            av.flush().unwrap();
            av.len()
        };
        let result = AppendVec::new_from_file(path, accounts_len, storage_access);
        assert_matches!(result, Err(ref message) if message.to_string().contains("incorrect layout/length/data"));
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_new_from_file_crafted_executable(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_new_from_crafted_executable");
        let path = &file.path;
        let accounts_len = {
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let av = ManuallyDrop::new(AppendVec::new(path, true, 1024 * 1024));
            av.append_account_test(&create_test_account(10)).unwrap();
            let offset_1 = {
                let mut executable_account = create_test_account(10);
                executable_account.1.set_executable(true);
                av.append_account_test(&executable_account).unwrap()
            };

            let crafted_executable = u8::MAX - 1;

            // reload accounts
            // ensure false is 0u8 and true is 1u8 actually
            av.get_stored_account_meta_callback(0, |account| {
                assert_eq!(*account.ref_executable_byte(), 0);
                let StoredAccountMeta::AppendVec(account) = account else {
                    panic!("StoredAccountMeta can only be AppendVec in this test.");
                };
                account.set_executable_as_byte(crafted_executable);
            })
            .unwrap();
            av.get_stored_account_meta_callback(offset_1, |account| {
                assert_eq!(*account.ref_executable_byte(), 1);
            })
            .unwrap();

            // reload crafted accounts
            av.get_stored_account_meta_callback(0, |account| {
                let StoredAccountMeta::AppendVec(account) = account else {
                    panic!("StoredAccountMeta can only be AppendVec in this test.");
                };

                // upper 7-bits are not 0, so sanitization should fail
                assert!(!account.sanitize_executable());

                // we can observe crafted value by ref
                {
                    let executable_bool: &bool = &account.account_meta.executable;
                    // Depending on use, *executable_bool can be truthy or falsy due to direct memory manipulation
                    // assert_eq! thinks *executable_bool is equal to false but the if condition thinks it's not, contradictorily.
                    assert!(!*executable_bool);
                    assert_eq!(*account.ref_executable_byte(), crafted_executable);
                }

                // we can NOT observe crafted value by value
                {
                    let executable_bool: bool = account.executable();
                    assert!(!executable_bool);
                    assert_eq!(account.get_executable_byte(), 0); // Wow, not crafted_executable!
                }
            })
            .unwrap();

            av.flush().unwrap();
            av.len()
        };
        let result = AppendVec::new_from_file(path, accounts_len, storage_access);
        assert_matches!(result, Err(ref message) if message.to_string().contains("incorrect layout/length/data"));
    }

    #[test]
    fn test_type_layout() {
        assert_eq!(offset_of!(StoredMeta, write_version_obsolete), 0x00);
        assert_eq!(offset_of!(StoredMeta, data_len), 0x08);
        assert_eq!(offset_of!(StoredMeta, pubkey), 0x10);
        assert_eq!(mem::size_of::<StoredMeta>(), 0x30);

        assert_eq!(offset_of!(AccountMeta, lamports), 0x00);
        assert_eq!(offset_of!(AccountMeta, rent_epoch), 0x08);
        assert_eq!(offset_of!(AccountMeta, owner), 0x10);
        assert_eq!(offset_of!(AccountMeta, executable), 0x30);
        assert_eq!(mem::size_of::<AccountMeta>(), 0x38);
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_get_account_shared_data_from_truncated_file(storage_access: StorageAccess) {
        let file = get_append_vec_path("test_get_account_shared_data_from_truncated_file");
        let path = &file.path;

        {
            // Set up a test account with data_len larger than PAGE_SIZE (i.e.
            // AppendVec internal buffer size is PAGESIZE).
            let data_len: usize = 2 * PAGE_SIZE as usize;
            let account = create_test_account_with(data_len);
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let av = ManuallyDrop::new(AppendVec::new(path, true, aligned_stored_size(data_len)));
            av.append_account_test(&account).unwrap();
            av.flush().unwrap();
        }

        // Truncate the AppendVec to PAGESIZE. This will cause get_account* to fail to load the account.
        let truncated_accounts_len: usize = PAGE_SIZE as usize;
        let av = AppendVec::new_from_file_unchecked(path, truncated_accounts_len, storage_access)
            .unwrap();
        let account = av.get_account_shared_data(0);
        assert!(account.is_none()); // Expect None to be returned.

        let result = av.get_stored_account_meta_callback(0, |_| true);
        assert!(result.is_none()); // Expect None to be returned.
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_get_account_sizes(storage_access: StorageAccess) {
        const NUM_ACCOUNTS: usize = 37;
        let pubkeys: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(NUM_ACCOUNTS)
            .collect();

        let mut rng = thread_rng();
        let mut accounts = Vec::with_capacity(pubkeys.len());
        let mut stored_sizes = Vec::with_capacity(pubkeys.len());
        for _ in &pubkeys {
            let lamports = rng.gen();
            let data_len = rng.gen_range(0..MAX_PERMITTED_DATA_LENGTH) as usize;
            let account = AccountSharedData::new(lamports, data_len, &Pubkey::default());
            accounts.push(account);
            stored_sizes.push(aligned_stored_size(data_len));
        }
        let accounts = accounts;
        let stored_sizes = stored_sizes;
        let total_stored_size = stored_sizes.iter().sum();

        let temp_file = get_append_vec_path("test_get_account_sizes");
        let account_offsets = {
            let append_vec = AppendVec::new(&temp_file.path, true, total_stored_size);
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let append_vec = ManuallyDrop::new(append_vec);
            let slot = 77; // the specific slot does not matter
            let storable_accounts: Vec<_> = std::iter::zip(&pubkeys, &accounts).collect();
            let stored_accounts_info = append_vec
                .append_accounts(&(slot, storable_accounts.as_slice()), 0)
                .unwrap();
            append_vec.flush().unwrap();
            stored_accounts_info.offsets
        };

        // now open the append vec with the given storage access method
        // then get the account sizes to ensure they are correct
        let (append_vec, _) =
            AppendVec::new_from_file(&temp_file.path, total_stored_size, storage_access).unwrap();

        let account_sizes = append_vec.get_account_sizes(account_offsets.as_slice());
        assert_eq!(account_sizes, stored_sizes);
    }

    /// A helper function for testing different scenario for scan_*.
    ///
    /// `modify_fn` is used to (optionally) modify the append vec before checks are performed.
    /// `check_fn` performs the check for the scan.
    fn test_scan_helper(
        storage_access: StorageAccess,
        modify_fn: impl Fn(&PathBuf, usize) -> usize,
        check_fn: impl Fn(&AppendVec, &[Pubkey], &[usize], &[AccountSharedData]),
    ) {
        const NUM_ACCOUNTS: usize = 37;
        let pubkeys: Vec<_> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
            .take(NUM_ACCOUNTS)
            .collect();

        let mut rng = thread_rng();
        let mut accounts = Vec::with_capacity(pubkeys.len());
        let mut total_stored_size = 0;
        for _ in &pubkeys {
            let lamports = rng.gen();
            let data_len = rng.gen_range(0..MAX_PERMITTED_DATA_LENGTH) as usize;
            let account = AccountSharedData::new(lamports, data_len, &Pubkey::default());
            accounts.push(account);
            total_stored_size += aligned_stored_size(data_len);
        }
        let accounts = accounts;
        let total_stored_size = total_stored_size;

        let temp_file = get_append_vec_path("test_scan");
        let account_offsets = {
            // wrap AppendVec in ManuallyDrop to ensure we do not remove the backing file when dropped
            let append_vec =
                ManuallyDrop::new(AppendVec::new(&temp_file.path, true, total_stored_size));
            let slot = 42; // the specific slot does not matter
            let storable_accounts: Vec<_> = std::iter::zip(&pubkeys, &accounts).collect();
            let stored_accounts_info = append_vec
                .append_accounts(&(slot, storable_accounts.as_slice()), 0)
                .unwrap();
            append_vec.flush().unwrap();
            stored_accounts_info.offsets
        };

        let total_stored_size = modify_fn(&temp_file.path, total_stored_size);
        // now open the append vec with the given storage access method
        // then perform the scan and check it is correct
        let append_vec = ManuallyDrop::new(
            AppendVec::new_from_file_unchecked(&temp_file.path, total_stored_size, storage_access)
                .unwrap(),
        );

        check_fn(&append_vec, &pubkeys, &account_offsets, &accounts);
    }

    /// A helper fn to test `scan_pubkeys`.
    fn test_scan_pubkeys_helper(
        storage_access: StorageAccess,
        modify_fn: impl Fn(&PathBuf, usize) -> usize,
    ) {
        test_scan_helper(
            storage_access,
            modify_fn,
            |append_vec, pubkeys, _account_offsets, _accounts| {
                let mut i = 0;
                append_vec.scan_pubkeys(|pubkey| {
                    assert_eq!(pubkey, pubkeys.get(i).unwrap());
                    i += 1;
                });
                assert_eq!(i, pubkeys.len());
            },
        )
    }

    /// Test `scan_pubkey` for a valid account storage.
    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_scan_pubkeys(storage_access: StorageAccess) {
        test_scan_pubkeys_helper(storage_access, |_, size| size);
    }

    /// Test `scan_pubkey` for storage with incomplete account meta data.
    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_scan_pubkeys_incomplete_data(storage_access: StorageAccess) {
        test_scan_pubkeys_helper(storage_access, |path, size| {
            // Append 1 byte of data at the end of the storage file to simulate
            // incomplete account's meta data.
            let mut f = OpenOptions::new()
                .read(true)
                .append(true)
                .open(path)
                .unwrap();
            f.write_all(&[0xFF]).unwrap();
            size + 1
        });
    }

    /// Test `scan_pubkey` for storage which is missing the last account data
    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_scan_pubkeys_missing_account_data(storage_access: StorageAccess) {
        test_scan_pubkeys_helper(storage_access, |path, size| {
            let fake_stored_meta = StoredMeta {
                write_version_obsolete: 0,
                data_len: 100,
                pubkey: solana_sdk::pubkey::new_rand(),
            };
            let fake_account_meta = AccountMeta {
                lamports: 100,
                rent_epoch: 10,
                owner: solana_sdk::pubkey::new_rand(),
                executable: false,
            };

            let stored_meta_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    (&fake_stored_meta as *const StoredMeta) as *const u8,
                    mem::size_of::<StoredMeta>(),
                )
            };
            let account_meta_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    (&fake_account_meta as *const AccountMeta) as *const u8,
                    mem::size_of::<AccountMeta>(),
                )
            };

            let mut f = OpenOptions::new()
                .read(true)
                .append(true)
                .open(path)
                .unwrap();

            f.write_all(stored_meta_slice).unwrap();
            f.write_all(account_meta_slice).unwrap();

            size + mem::size_of::<StoredMeta>() + mem::size_of::<AccountMeta>()
        });
    }

    /// A helper fn to test scan_index
    fn test_scan_index_helper(
        storage_access: StorageAccess,
        modify_fn: impl Fn(&PathBuf, usize) -> usize,
    ) {
        test_scan_helper(
            storage_access,
            modify_fn,
            |append_vec, pubkeys, account_offsets, accounts| {
                let mut i = 0;
                append_vec.scan_index(|index_info| {
                    let pubkey = pubkeys.get(i).unwrap();
                    let account = accounts.get(i).unwrap();
                    let offset = account_offsets.get(i).unwrap();

                    assert_eq!(
                        index_info.stored_size_aligned,
                        aligned_stored_size(account.data().len()),
                    );
                    assert_eq!(index_info.index_info.offset, *offset);
                    assert_eq!(index_info.index_info.pubkey, *pubkey);
                    assert_eq!(index_info.index_info.lamports, account.lamports());
                    assert_eq!(index_info.index_info.rent_epoch, account.rent_epoch());
                    assert_eq!(index_info.index_info.executable, account.executable());
                    assert_eq!(index_info.index_info.data_len, account.data().len() as u64);

                    i += 1;
                });
                assert_eq!(i, accounts.len());
            },
        )
    }

    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_scan_index(storage_access: StorageAccess) {
        test_scan_index_helper(storage_access, |_, size| size);
    }

    /// Test `scan_index` for storage with incomplete account meta data.
    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_scan_index_incomplete_data(storage_access: StorageAccess) {
        test_scan_index_helper(storage_access, |path, size| {
            // Append 1 byte of data at the end of the storage file to simulate
            // incomplete account's meta data.
            let mut f = OpenOptions::new()
                .read(true)
                .append(true)
                .open(path)
                .unwrap();
            f.write_all(&[0xFF]).unwrap();
            size + 1
        });
    }

    /// Test `scan_index` for storage which is missing the last account data
    #[test_case(StorageAccess::Mmap)]
    #[test_case(StorageAccess::File)]
    fn test_scan_index_missing_account_data(storage_access: StorageAccess) {
        test_scan_index_helper(storage_access, |path, size| {
            let fake_stored_meta = StoredMeta {
                write_version_obsolete: 0,
                data_len: 100,
                pubkey: solana_sdk::pubkey::new_rand(),
            };
            let fake_account_meta = AccountMeta {
                lamports: 100,
                rent_epoch: 10,
                owner: solana_sdk::pubkey::new_rand(),
                executable: false,
            };

            let stored_meta_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    (&fake_stored_meta as *const StoredMeta) as *const u8,
                    mem::size_of::<StoredMeta>(),
                )
            };
            let account_meta_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    (&fake_account_meta as *const AccountMeta) as *const u8,
                    mem::size_of::<AccountMeta>(),
                )
            };

            let mut f = OpenOptions::new()
                .read(true)
                .append(true)
                .open(path)
                .unwrap();

            f.write_all(stored_meta_slice).unwrap();
            f.write_all(account_meta_slice).unwrap();

            size + mem::size_of::<StoredMeta>() + mem::size_of::<AccountMeta>()
        });
    }
}
