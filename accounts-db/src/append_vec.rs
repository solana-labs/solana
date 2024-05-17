//! Persistent storage for accounts.
//!
//! For more information, see:
//!
//! <https://docs.solanalabs.com/implemented-proposals/persistent-account-storage>

use {
    crate::{
        account_storage::meta::{
            AccountMeta, StoredAccountMeta, StoredMeta, StoredMetaWriteVersion,
        },
        accounts_file::{
            AccountsFileError, InternalsForArchive, MatchAccountOwnerError, Result, StorageAccess,
            StoredAccountsInfo, ALIGN_BOUNDARY_OFFSET,
        },
        accounts_hash::AccountHash,
        accounts_index::ZeroLamport,
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
    },
    std::{
        convert::TryFrom,
        fs::{remove_file, OpenOptions},
        io::{Seek, SeekFrom, Write},
        mem,
        path::{Path, PathBuf},
        ptr,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
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

    pub fn write_version(&self) -> StoredMetaWriteVersion {
        self.meta.write_version_obsolete
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
    MmapOnly(MmapMut),
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
}

lazy_static! {
    pub static ref APPEND_VEC_MMAPPED_FILES_OPEN: AtomicU64 = AtomicU64::default();
}

impl Drop for AppendVec {
    fn drop(&mut self) {
        APPEND_VEC_MMAPPED_FILES_OPEN.fetch_sub(1, Ordering::Relaxed);
        if let Err(_err) = remove_file(&self.path) {
            // promote this to panic soon.
            // disabled due to many false positive warnings while running tests.
            // blocked by rpc's upgrade to jsonrpc v17
            //error!("AppendVec failed to remove {}: {err}", &self.path.display());
            inc_new_counter_info!("append_vec_drop_fail", 1);
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
            backing: AppendVecFileBacking::MmapOnly(mmap),
            // This mutex forces append to be single threaded, but concurrent with reads
            // See UNSAFE usage in `append_ptr`
            append_lock: Mutex::new(()),
            current_len: AtomicUsize::new(initial_len),
            file_size: size as u64,
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
            AppendVecFileBacking::MmapOnly(mmap) => Ok(mmap.flush()?),
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
    pub(crate) fn reopen_as_readonly(&self) -> Option<Self> {
        // this is a no-op when we are already a mmap
        None
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
            // This info show the failing accountvec file path.  It helps debugging
            // the appendvec data corrupution issues related to recycling.
            return Err(AccountsFileError::AppendVecError(
                AppendVecError::IncorrectLayout(new.path.clone()),
            ));
        }

        Ok((new, num_accounts))
    }

    /// Creates an appendvec from file without performing sanitize checks or counting the number of accounts
    pub fn new_from_file_unchecked(
        path: impl Into<PathBuf>,
        current_len: usize,
        _storage_access: StorageAccess,
    ) -> Result<Self> {
        let path = path.into();
        let file_size = std::fs::metadata(&path)?.len();
        Self::sanitize_len_and_size(current_len, file_size as usize)?;

        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&path)?;

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
            backing: AppendVecFileBacking::MmapOnly(mmap),
            append_lock: Mutex::new(()),
            current_len: AtomicUsize::new(current_len),
            file_size,
        })
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
    fn get_slice(&self, offset: usize, size: usize) -> Option<(&[u8], usize)> {
        let (next, overflow) = offset.overflowing_add(size);
        if overflow || next > self.len() {
            return None;
        }
        match &self.backing {
            AppendVecFileBacking::MmapOnly(mmap) => {
                let data = &mmap[offset..next];
                let next = u64_align!(next);

                Some((
                    //UNSAFE: This unsafe creates a slice that represents a chunk of self.map memory
                    //The lifetime of this slice is tied to &self, since it points to self.map memory
                    unsafe { std::slice::from_raw_parts(data.as_ptr(), size) },
                    next,
                ))
            }
        }
    }

    /// Copy `len` bytes from `src` to the first 64-byte boundary after position `offset` of
    /// the internal buffer. Then update `offset` to the first byte after the copied data.
    fn append_ptr(&self, offset: &mut usize, src: *const u8, len: usize) {
        let pos = u64_align!(*offset);
        match &self.backing {
            AppendVecFileBacking::MmapOnly(mmap) => {
                let data = &mmap[pos..(pos + len)];
                //UNSAFE: This mut append is safe because only 1 thread can append at a time
                //Mutex<()> guarantees exclusive write access to the memory occupied in
                //the range.
                unsafe {
                    let dst = data.as_ptr() as *mut _;
                    ptr::copy(src, dst, len);
                };
                *offset = pos + len;
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
    fn get_type<T>(&self, offset: usize) -> Option<(&T, usize)> {
        let (data, next) = self.get_slice(offset, mem::size_of::<T>())?;
        let ptr = data.as_ptr().cast();
        //UNSAFE: The cast is safe because the slice is aligned and fits into the memory
        //and the lifetime of the &T is tied to self, which holds the underlying memory map
        Some((unsafe { &*ptr }, next))
    }

    /// calls `callback` with the stored account metadata for the account at `offset` if its data doesn't overrun
    /// the internal buffer. Otherwise return None.
    pub fn get_stored_account_meta_callback<Ret>(
        &self,
        offset: usize,
        mut callback: impl for<'local> FnMut(StoredAccountMeta<'local>) -> Ret,
    ) -> Option<Ret> {
        let (meta, next): (&StoredMeta, _) = self.get_type(offset)?;
        let (account_meta, next): (&AccountMeta, _) = self.get_type(next)?;
        let (hash, next): (&AccountHash, _) = self.get_type(next)?;
        let (data, next) = self.get_slice(next, meta.data_len as usize)?;
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

    /// return an `AccountSharedData` for an account at `offset`.
    /// This fn can efficiently return exactly what is needed by a caller.
    pub(crate) fn get_account_shared_data(&self, offset: usize) -> Option<AccountSharedData> {
        let (meta, next) = self.get_type::<StoredMeta>(offset)?;
        let (account_meta, next) = self.get_type::<AccountMeta>(next)?;
        let next = next + std::mem::size_of::<AccountHash>();
        let (data, _next) = self.get_slice(next, meta.data_len as usize)?;
        Some(AccountSharedData::create(
            account_meta.lamports,
            data.to_vec(),
            account_meta.owner,
            account_meta.executable,
            account_meta.rent_epoch,
        ))
    }

    /// note this fn can return account meta for an account whose fields have been truncated (ie. if `len` isn't long enough.)
    /// This fn doesn't even load the data_len field, so this fn does not know how big `len` needs to be.
    fn get_account_meta(&self, offset: usize) -> Option<&AccountMeta> {
        // Skip over StoredMeta data in the account
        let offset = offset.checked_add(mem::size_of::<StoredMeta>())?;
        // u64_align! does an unchecked add for alignment. Check that it won't cause an overflow.
        offset.checked_add(ALIGN_BOUNDARY_OFFSET - 1)?;
        let (account_meta, _): (&AccountMeta, _) = self.get_type(u64_align!(offset))?;
        Some(account_meta)
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
        let account_meta = self
            .get_account_meta(offset)
            .ok_or(MatchAccountOwnerError::UnableToLoad)?;
        if account_meta.lamports == 0 {
            Err(MatchAccountOwnerError::NoMatch)
        } else {
            owners
                .iter()
                .position(|entry| &account_meta.owner == entry)
                .ok_or(MatchAccountOwnerError::NoMatch)
        }
    }

    #[cfg(test)]
    pub fn get_account_test(
        &self,
        offset: usize,
    ) -> Option<(StoredMeta, solana_sdk::account::AccountSharedData)> {
        let sizes = self.get_account_sizes(&[offset]);
        let result = self.get_stored_account_meta_callback(offset, |r_callback| {
            let r2 = self.get_account_shared_data(offset);
            let r3 = self.get_account_meta(offset);
            assert!(accounts_equal(&r_callback, r2.as_ref().unwrap()));
            // r3 can return Some when r_callback and r2 do not
            assert!(r3.is_some());
            assert_eq!(sizes, vec![r_callback.stored_size()]);
            if let Some(r2) = r2.as_ref() {
                let meta = r3.unwrap();
                assert_eq!(meta.executable, r2.executable());
                assert_eq!(meta.owner, *r2.owner());
                assert_eq!(meta.lamports, r2.lamports());
                assert_eq!(meta.rent_epoch, r2.rent_epoch());
            }
            let meta = r_callback.meta().clone();
            Some((meta, r_callback.to_account_shared_data()))
        });
        if result.is_none() {
            assert!(self
                .get_stored_account_meta_callback(offset, |_| {})
                .is_none());
            assert!(self.get_account_shared_data(offset).is_none());
            // note that sometimes `get_account_meta` can return Some(..) // assert!(self.get_account_meta(offset).is_none());
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
    fn next_account_offset(start_offset: usize, stored_meta: &StoredMeta) -> AccountOffsets {
        let stored_size_unaligned = STORE_META_OVERHEAD + stored_meta.data_len as usize;
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
        let mut offset = 0;
        loop {
            let Some((stored_meta, next)) = self.get_type::<StoredMeta>(offset) else {
                // eof
                break;
            };
            let Some((account_meta, _)) = self.get_type::<AccountMeta>(next) else {
                // eof
                break;
            };
            if account_meta.lamports == 0 && stored_meta.pubkey == Pubkey::default() {
                // we passed the last useful account
                return;
            }
            let next = Self::next_account_offset(offset, stored_meta);
            if next.offset_to_end_of_data > self.len() {
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

    /// Iterate over all accounts and call `callback` with each account.
    #[allow(clippy::blocks_in_conditions)]
    pub fn scan_accounts(&self, mut callback: impl for<'local> FnMut(StoredAccountMeta<'local>)) {
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

    /// for each offset in `sorted_offsets`, get the size of the account. No other information is needed for the account.
    pub(crate) fn get_account_sizes(&self, sorted_offsets: &[usize]) -> Vec<usize> {
        let mut result = Vec::with_capacity(sorted_offsets.len());
        for &offset in sorted_offsets {
            let Some((stored_meta, _)) = self.get_type::<StoredMeta>(offset) else {
                break;
            };
            let next = Self::next_account_offset(offset, stored_meta);
            if next.offset_to_end_of_data > self.len() {
                // data doesn't fit, so don't include
                break;
            }
            result.push(next.stored_size_aligned);
        }
        result
    }

    /// iterate over all pubkeys and call `callback`.
    /// This iteration does not deserialize and populate each field in `StoredAccountMeta`.
    /// `data` is completely ignored, for example.
    /// Also, no references have to be maintained/returned from an iterator function.
    /// This fn can operate on a batch of data at once.
    pub(crate) fn scan_pubkeys(&self, mut callback: impl FnMut(&Pubkey)) {
        let mut offset = 0;
        loop {
            let Some((stored_meta, _)) = self.get_type::<StoredMeta>(offset) else {
                // eof
                break;
            };
            let next = Self::next_account_offset(offset, stored_meta);
            if next.offset_to_end_of_data > self.len() {
                // data doesn't fit, so don't include this pubkey
                break;
            }
            callback(&stored_meta.pubkey);
            offset = next.next_account_offset;
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
        let default_hash: Hash = Hash::default(); // [0_u8; 32];
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
                let meta_ptr = &stored_meta as *const StoredMeta;
                let account_meta_ptr = &account_meta as *const AccountMeta;
                let data_len = stored_meta.data_len as usize;
                let data_ptr = account.data().as_ptr();
                let hash_ptr = bytemuck::bytes_of(&default_hash).as_ptr();
                let ptrs = [
                    (meta_ptr as *const u8, mem::size_of::<StoredMeta>()),
                    (account_meta_ptr as *const u8, mem::size_of::<AccountMeta>()),
                    (hash_ptr, mem::size_of::<AccountHash>()),
                    (data_ptr, data_len),
                ];
                if let Some(start_offset) = self.append_ptrs_locked(&mut offset, &ptrs) {
                    offsets.push(start_offset)
                } else {
                    stop = true;
                }
            });
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

    /// true if this storage can possibly be appended to (independent of capacity check)
    pub(crate) fn can_append(&self) -> bool {
        // always can append to a mmapped append vec
        true
    }

    /// Returns the way to access this accounts file when archiving
    pub(crate) fn internals_for_archive(&self) -> InternalsForArchive {
        match &self.backing {
            AppendVecFileBacking::MmapOnly(mmap) => InternalsForArchive::Mmap(mmap),
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
            account::{Account, AccountSharedData, WritableAccount},
            clock::Slot,
            timing::duration_as_ms,
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
        trace!("append time: {} ms", duration_as_ms(&now.elapsed()),);

        let now = Instant::now();
        for _ in 0..size {
            let sample = thread_rng().gen_range(0..indexes.len());
            let account = create_test_account(sample + 1);
            assert_eq!(av.get_account_test(indexes[sample]).unwrap(), account);
        }
        trace!("random read time: {} ms", duration_as_ms(&now.elapsed()),);

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
        trace!(
            "sequential read time: {} ms",
            duration_as_ms(&now.elapsed()),
        );
    }

    #[test_case(StorageAccess::Mmap)]
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

    #[test_case(StorageAccess::Mmap)]
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
                    #[cfg(not(target_arch = "aarch64"))]
                    {
                        const FALSE: bool = false; // keep clippy happy
                        if *executable_bool == FALSE {
                            panic!("This didn't occur if this test passed.");
                        }
                    }
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
}
