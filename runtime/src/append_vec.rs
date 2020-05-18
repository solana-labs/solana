use memmap::MmapMut;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    account::Account,
    clock::{Epoch, Slot},
    hash::Hash,
    pubkey::Pubkey,
};
use std::{
    fs::{remove_file, OpenOptions},
    io,
    io::{Seek, SeekFrom, Write},
    mem,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    sync::Mutex,
};

//Data placement should be aligned at the next boundary. Without alignment accessing the memory may
//crash on some architectures.
const ALIGN_BOUNDARY_OFFSET: usize = mem::size_of::<u64>();
macro_rules! u64_align {
    ($addr: expr) => {
        ($addr + (ALIGN_BOUNDARY_OFFSET - 1)) & !(ALIGN_BOUNDARY_OFFSET - 1)
    };
}

const MAXIMUM_APPEND_VEC_FILE_SIZE: usize = 16 * 1024 * 1024 * 1024; // 16 GiB

/// Meta contains enough context to recover the index from storage itself
/// This struct will be backed by mmaped and snapshotted data files.
/// So the data layout must be stable and consistent across the entire cluster!
#[derive(Clone, PartialEq, Debug)]
pub struct StoredMeta {
    /// global write version
    pub write_version: u64,
    /// key for the account
    pub pubkey: Pubkey,
    pub data_len: u64,
}

/// This struct will be backed by mmaped and snapshotted data files.
/// So the data layout must be stable and consistent across the entire cluster!
#[derive(Serialize, Deserialize, Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountMeta {
    /// lamports in the account
    pub lamports: u64,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
    /// the epoch at which this account will next owe rent
    pub rent_epoch: Epoch,
}

/// References to Memory Mapped memory
/// The Account is stored separately from its data, so getting the actual account requires a clone
#[derive(PartialEq, Debug)]
pub struct StoredAccount<'a> {
    pub meta: &'a StoredMeta,
    /// account data
    pub account_meta: &'a AccountMeta,
    pub data: &'a [u8],
    pub offset: usize,
    pub hash: &'a Hash,
}

impl<'a> StoredAccount<'a> {
    pub fn clone_account(&self) -> Account {
        Account {
            lamports: self.account_meta.lamports,
            owner: self.account_meta.owner,
            executable: self.account_meta.executable,
            rent_epoch: self.account_meta.rent_epoch,
            data: self.data.to_vec(),
        }
    }

    fn sanitize(&self) -> bool {
        self.sanitize_executable() && self.sanitize_lamports()
    }

    fn sanitize_executable(&self) -> bool {
        // Sanitize executable to ensure higher 7-bits are cleared correctly.
        self.ref_executable_byte() & !1 == 0
    }

    fn sanitize_lamports(&self) -> bool {
        // Sanitize 0 lamports to ensure to be same as Account::default()
        self.account_meta.lamports != 0 || self.clone_account() == Account::default()
    }

    fn ref_executable_byte(&self) -> &u8 {
        // Use extra references to avoid value silently clamped to 1 (=true) and 0 (=false)
        // Yes, this really happens; see test_set_file_crafted_executable
        let executable_bool: &bool = &self.account_meta.executable;
        // UNSAFE: Force to interpret mmap-backed bool as u8 to really read the actual memory content
        let executable_byte: &u8 = unsafe { &*(executable_bool as *const bool as *const u8) };
        executable_byte
    }
}

#[derive(Debug)]
#[allow(clippy::mutex_atomic)]
pub struct AppendVec {
    path: PathBuf,
    map: MmapMut,
    // This mutex forces append to be single threaded, but concurrent with reads
    #[allow(clippy::mutex_atomic)]
    append_offset: Mutex<usize>,
    current_len: AtomicUsize,
    file_size: u64,
}

impl Drop for AppendVec {
    fn drop(&mut self) {
        let _ignored = remove_file(&self.path);
    }
}

impl AppendVec {
    #[allow(clippy::mutex_atomic)]
    pub fn new(file: &Path, create: bool, size: usize) -> Self {
        let initial_len = 0;
        AppendVec::sanitize_len_and_size(initial_len, size).unwrap();

        if create {
            let _ignored = remove_file(file);
        }

        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(file)
            .map_err(|e| {
                let mut msg = format!("in current dir {:?}\n", std::env::current_dir());
                for ancestor in file.ancestors() {
                    msg.push_str(&format!(
                        "{:?} is {:?}\n",
                        ancestor,
                        std::fs::metadata(ancestor)
                    ));
                }
                panic!(
                    "{}Unable to {} data file {}, err {:?}",
                    msg,
                    if create { "create" } else { "open" },
                    file.display(),
                    e
                );
            })
            .unwrap();

        data.seek(SeekFrom::Start((size - 1) as u64)).unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        data.flush().unwrap();
        //UNSAFE: Required to create a Mmap
        let map = unsafe { MmapMut::map_mut(&data) };
        let map =
            map.unwrap_or_else(|e| panic!("failed to map the data file (size: {}): {}", size, e));

        AppendVec {
            path: file.to_path_buf(),
            map,
            // This mutex forces append to be single threaded, but concurrent with reads
            // See UNSAFE usage in `append_ptr`
            append_offset: Mutex::new(initial_len),
            current_len: AtomicUsize::new(initial_len),
            file_size: size as u64,
        }
    }

    #[allow(clippy::mutex_atomic)]
    pub(crate) fn new_empty_map(current_len: usize) -> Self {
        let map = MmapMut::map_anon(1).expect("failed to map the data file");

        AppendVec {
            path: PathBuf::from(String::default()),
            map,
            append_offset: Mutex::new(current_len),
            current_len: AtomicUsize::new(current_len),
            file_size: 0, // will be filled by set_file()
        }
    }

    fn sanitize_len_and_size(current_len: usize, file_size: usize) -> io::Result<()> {
        if file_size == 0 {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("too small file size {} for AppendVec", file_size),
            ))
        } else if file_size > MAXIMUM_APPEND_VEC_FILE_SIZE {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("too large file size {} for AppendVec", file_size),
            ))
        } else if current_len > file_size {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("current_len is larger than file size ({})", file_size),
            ))
        } else {
            Ok(())
        }
    }

    pub fn flush(&self) -> io::Result<()> {
        self.map.flush()
    }

    #[allow(clippy::mutex_atomic)]
    pub fn reset(&self) {
        // This mutex forces append to be single threaded, but concurrent with reads
        // See UNSAFE usage in `append_ptr`
        let mut offset = self.append_offset.lock().unwrap();
        self.current_len.store(0, Ordering::Relaxed);
        *offset = 0;
    }

    pub fn len(&self) -> usize {
        self.current_len.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> u64 {
        self.file_size
    }

    // Get the file path relative to the top level accounts directory
    pub fn get_relative_path<P: AsRef<Path>>(append_vec_path: P) -> Option<PathBuf> {
        append_vec_path.as_ref().file_name().map(PathBuf::from)
    }

    pub fn new_relative_path(slot: Slot, id: usize) -> PathBuf {
        PathBuf::from(&format!("{}.{}", slot, id))
    }

    #[allow(clippy::mutex_atomic)]
    pub fn set_file<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        // this AppendVec must not hold actual file;
        assert_eq!(self.file_size, 0);

        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&path)?;

        let current_len = self.current_len.load(Ordering::Relaxed);
        assert_eq!(current_len, *self.append_offset.lock().unwrap());

        let file_size = std::fs::metadata(&path)?.len();
        AppendVec::sanitize_len_and_size(current_len, file_size as usize)?;

        let map = unsafe { MmapMut::map_mut(&data)? };

        self.file_size = file_size;
        self.path = path.as_ref().to_path_buf();
        self.map = map;

        if !self.sanitize_layout_and_length() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "incorrect layout/length/data",
            ));
        }

        Ok(())
    }

    fn sanitize_layout_and_length(&self) -> bool {
        let mut offset = 0;

        // This discards allocated accounts immediately after check at each loop iteration.
        //
        // This code should not reuse AppendVec.accounts() method as the current form or
        // extend it to be reused here because it would allow attackers to accumulate
        // some measurable amount of memory needlessly.
        while let Some((account, next_offset)) = self.get_account(offset) {
            if !account.sanitize() {
                return false;
            }
            offset = next_offset;
        }
        let aligned_current_len = u64_align!(self.current_len.load(Ordering::Relaxed));

        offset == aligned_current_len
    }

    fn get_slice(&self, offset: usize, size: usize) -> Option<(&[u8], usize)> {
        let (next, overflow) = offset.overflowing_add(size);
        if overflow || next > self.len() {
            return None;
        }
        let data = &self.map[offset..next];
        let next = u64_align!(next);

        Some((
            //UNSAFE: This unsafe creates a slice that represents a chunk of self.map memory
            //The lifetime of this slice is tied to &self, since it points to self.map memory
            unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, size) },
            next,
        ))
    }

    fn append_ptr(&self, offset: &mut usize, src: *const u8, len: usize) {
        let pos = u64_align!(*offset);
        let data = &self.map[pos..(pos + len)];
        //UNSAFE: This mut append is safe because only 1 thread can append at a time
        //Mutex<append_offset> guarantees exclusive write access to the memory occupied in
        //the range.
        unsafe {
            let dst = data.as_ptr() as *mut u8;
            std::ptr::copy(src, dst, len);
        };
        *offset = pos + len;
    }

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
        self.current_len.store(*offset, Ordering::Relaxed);
        Some(pos)
    }

    fn get_type<'a, T>(&self, offset: usize) -> Option<(&'a T, usize)> {
        let (data, next) = self.get_slice(offset, mem::size_of::<T>())?;
        let ptr: *const T = data.as_ptr() as *const T;
        //UNSAFE: The cast is safe because the slice is aligned and fits into the memory
        //and the lifetime of he &T is tied to self, which holds the underlying memory map
        Some((unsafe { &*ptr }, next))
    }

    pub fn get_account<'a>(&'a self, offset: usize) -> Option<(StoredAccount<'a>, usize)> {
        let (meta, next): (&'a StoredMeta, _) = self.get_type(offset)?;
        let (account_meta, next): (&'a AccountMeta, _) = self.get_type(next)?;
        let (hash, next): (&'a Hash, _) = self.get_type(next)?;
        let (data, next) = self.get_slice(next, meta.data_len as usize)?;
        Some((
            StoredAccount {
                meta,
                account_meta,
                data,
                offset,
                hash,
            },
            next,
        ))
    }
    pub fn get_account_test(&self, offset: usize) -> Option<(StoredMeta, Account)> {
        let (stored_account, _) = self.get_account(offset)?;
        let meta = stored_account.meta.clone();
        Some((meta, stored_account.clone_account()))
    }

    pub fn get_path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn accounts<'a>(&'a self, mut start: usize) -> Vec<StoredAccount<'a>> {
        let mut accounts = vec![];
        while let Some((account, next)) = self.get_account(start) {
            accounts.push(account);
            start = next;
        }
        accounts
    }

    #[allow(clippy::mutex_atomic)]
    pub fn append_accounts(
        &self,
        accounts: &[(StoredMeta, &Account)],
        hashes: &[Hash],
    ) -> Vec<usize> {
        let mut offset = self.append_offset.lock().unwrap();
        let mut rv = Vec::with_capacity(accounts.len());
        for ((stored_meta, account), hash) in accounts.iter().zip(hashes) {
            let meta_ptr = stored_meta as *const StoredMeta;
            let account_meta = AccountMeta {
                lamports: account.lamports,
                owner: account.owner,
                executable: account.executable,
                rent_epoch: account.rent_epoch,
            };
            let account_meta_ptr = &account_meta as *const AccountMeta;
            let data_len = stored_meta.data_len as usize;
            let data_ptr = account.data.as_ptr();
            let hash_ptr = hash.as_ref().as_ptr();
            let ptrs = [
                (meta_ptr as *const u8, mem::size_of::<StoredMeta>()),
                (account_meta_ptr as *const u8, mem::size_of::<AccountMeta>()),
                (hash_ptr as *const u8, mem::size_of::<Hash>()),
                (data_ptr, data_len),
            ];
            if let Some(res) = self.append_ptrs_locked(&mut offset, &ptrs) {
                rv.push(res)
            } else {
                break;
            }
        }
        rv
    }

    pub fn append_account(
        &self,
        storage_meta: StoredMeta,
        account: &Account,
        hash: Hash,
    ) -> Option<usize> {
        self.append_accounts(&[(storage_meta, account)], &[hash])
            .first()
            .cloned()
    }
}

pub mod test_utils {
    use super::StoredMeta;
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::pubkey::Pubkey;
    use std::fs::create_dir_all;
    use std::path::PathBuf;

    pub struct TempFile {
        pub path: PathBuf,
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let mut path = PathBuf::new();
            std::mem::swap(&mut path, &mut self.path);
            let _ignored = std::fs::remove_file(path);
        }
    }

    pub fn get_append_vec_dir() -> String {
        std::env::var("FARF_DIR").unwrap_or_else(|_| "farf/append_vec_tests".to_string())
    }

    pub fn get_append_vec_path(path: &str) -> TempFile {
        let out_dir = get_append_vec_dir();
        let rand_string: String = thread_rng().sample_iter(&Alphanumeric).take(30).collect();
        let dir = format!("{}/{}", out_dir, rand_string);
        let mut buf = PathBuf::new();
        buf.push(&format!("{}/{}", dir, path));
        create_dir_all(dir).expect("Create directory failed");
        TempFile { path: buf }
    }

    pub fn create_test_account(sample: usize) -> (StoredMeta, Account) {
        let data_len = sample % 256;
        let mut account = Account::new(sample as u64, 0, &Pubkey::default());
        account.data = (0..data_len).map(|_| data_len as u8).collect();
        let stored_meta = StoredMeta {
            write_version: 0,
            pubkey: Pubkey::default(),
            data_len: data_len as u64,
        };
        (stored_meta, account)
    }
}

#[cfg(test)]
pub mod tests {
    use super::test_utils::*;
    use super::*;
    use assert_matches::assert_matches;
    use log::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::timing::duration_as_ms;
    use std::time::Instant;

    impl AppendVec {
        fn append_account_test(&self, data: &(StoredMeta, Account)) -> Option<usize> {
            self.append_account(data.0.clone(), &data.1, Hash::default())
        }
    }

    impl<'a> StoredAccount<'a> {
        #[allow(clippy::cast_ref_to_mut)]
        fn set_data_len_unsafe(&self, new_data_len: u64) {
            // UNSAFE: cast away & (= const ref) to &mut to force to mutate append-only (=read-only) AppendVec
            unsafe {
                *(&self.meta.data_len as *const u64 as *mut u64) = new_data_len;
            }
        }

        fn get_executable_byte(&self) -> u8 {
            let executable_bool: bool = self.account_meta.executable;
            // UNSAFE: Force to interpret mmap-backed bool as u8 to really read the actual memory content
            let executable_byte: u8 = unsafe { std::mem::transmute::<bool, u8>(executable_bool) };
            executable_byte
        }

        #[allow(clippy::cast_ref_to_mut)]
        fn set_executable_as_byte(&self, new_executable_byte: u8) {
            // UNSAFE: Force to interpret mmap-backed &bool as &u8 to write some crafted value;
            unsafe {
                *(&self.account_meta.executable as *const bool as *mut u8) = new_executable_byte;
            }
        }
    }

    #[test]
    #[should_panic(expected = "too small file size 0 for AppendVec")]
    fn test_append_vec_new_bad_size() {
        let path = get_append_vec_path("test_append_vec_new_bad_size");
        let _av = AppendVec::new(&path.path, true, 0);
    }

    #[test]
    fn test_append_vec_set_file_bad_size() {
        let file = get_append_vec_path("test_append_vec_set_file_bad_size");
        let path = &file.path;
        let mut av = AppendVec::new_empty_map(0);
        assert_eq!(av.accounts(0).len(), 0);

        let _data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .expect("create a test file for mmap");

        let result = av.set_file(path);
        assert_matches!(result, Err(ref message) if message.to_string() == *"too small file size 0 for AppendVec");
    }

    #[test]
    fn test_append_vec_sanitize_len_and_size_too_small() {
        const LEN: usize = 0;
        const SIZE: usize = 0;
        let result = AppendVec::sanitize_len_and_size(LEN, SIZE);
        assert_matches!(result, Err(ref message) if message.to_string() == *"too small file size 0 for AppendVec");
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
        assert_matches!(result, Err(ref message) if message.to_string() == *"too large file size 17179869185 for AppendVec");
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
        assert_matches!(result, Err(ref message) if message.to_string() == *"current_len is larger than file size (1048576)");
    }

    #[test]
    fn test_append_vec_one() {
        let path = get_append_vec_path("test_append");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let account = create_test_account(0);
        let index = av.append_account_test(&account).unwrap();
        assert_eq!(av.get_account_test(index).unwrap(), account);
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
    fn test_append_vec_append_many() {
        let path = get_append_vec_path("test_append_many");
        let av = AppendVec::new(&path.path, true, 1024 * 1024);
        let size = 1000;
        let mut indexes = vec![];
        let now = Instant::now();
        for sample in 0..size {
            let account = create_test_account(sample);
            let pos = av.append_account_test(&account).unwrap();
            assert_eq!(av.get_account_test(pos).unwrap(), account);
            indexes.push(pos)
        }
        trace!("append time: {} ms", duration_as_ms(&now.elapsed()),);

        let now = Instant::now();
        for _ in 0..size {
            let sample = thread_rng().gen_range(0, indexes.len());
            let account = create_test_account(sample);
            assert_eq!(av.get_account_test(indexes[sample]).unwrap(), account);
        }
        trace!("random read time: {} ms", duration_as_ms(&now.elapsed()),);

        let now = Instant::now();
        assert_eq!(indexes.len(), size);
        assert_eq!(indexes[0], 0);
        let mut accounts = av.accounts(indexes[0]);
        assert_eq!(accounts.len(), size);
        for (sample, v) in accounts.iter_mut().enumerate() {
            let account = create_test_account(sample);
            let recovered = v.clone_account();
            assert_eq!(recovered, account.1)
        }
        trace!(
            "sequential read time: {} ms",
            duration_as_ms(&now.elapsed()),
        );
    }

    #[test]
    fn test_relative_path() {
        let relative_path = AppendVec::new_relative_path(0, 2);
        let full_path = Path::new("/tmp").join(&relative_path);
        assert_eq!(
            relative_path,
            AppendVec::get_relative_path(full_path).unwrap()
        );
    }

    #[test]
    fn test_set_file_crafted_zero_lamport_account() {
        let file = get_append_vec_path("test_append");
        let path = &file.path;
        let mut av = AppendVec::new(&path, true, 1024 * 1024);

        let pubkey = Pubkey::new_rand();
        let owner = Pubkey::default();
        let data_len = 3 as u64;
        let mut account = Account::new(0, data_len as usize, &owner);
        account.data = b"abc".to_vec();
        let stored_meta = StoredMeta {
            write_version: 0,
            pubkey,
            data_len,
        };
        let account_with_meta = (stored_meta, account);
        let index = av.append_account_test(&account_with_meta).unwrap();
        assert_eq!(av.get_account_test(index).unwrap(), account_with_meta);

        av.flush().unwrap();
        av.file_size = 0;
        let result = av.set_file(path);
        assert_matches!(result, Err(ref message) if message.to_string() == *"incorrect layout/length/data");
    }

    #[test]
    fn test_set_file_crafted_data_len() {
        let file = get_append_vec_path("test_set_file_crafted_data_len");
        let path = &file.path;
        let mut av = AppendVec::new(&path, true, 1024 * 1024);

        let crafted_data_len = 1;

        av.append_account_test(&create_test_account(10)).unwrap();

        let accounts = av.accounts(0);
        let account = accounts.first().unwrap();
        account.set_data_len_unsafe(crafted_data_len);
        assert_eq!(account.meta.data_len, crafted_data_len);

        // Reload accoutns and observe crafted_data_len
        let accounts = av.accounts(0);
        let account = accounts.first().unwrap();
        assert_eq!(account.meta.data_len, crafted_data_len);

        av.flush().unwrap();
        av.file_size = 0;
        let result = av.set_file(path);
        assert_matches!(result, Err(ref message) if message.to_string() == *"incorrect layout/length/data");
    }

    #[test]
    fn test_set_file_too_large_data_len() {
        let file = get_append_vec_path("test_set_file_too_large_data_len");
        let path = &file.path;
        let mut av = AppendVec::new(&path, true, 1024 * 1024);

        let too_large_data_len = u64::max_value();
        av.append_account_test(&create_test_account(10)).unwrap();

        let accounts = av.accounts(0);
        let account = accounts.first().unwrap();
        account.set_data_len_unsafe(too_large_data_len);
        assert_eq!(account.meta.data_len, too_large_data_len);

        // Reload accounts and observe no account with bad offset
        let accounts = av.accounts(0);
        assert_matches!(accounts.first(), None);

        av.flush().unwrap();
        av.file_size = 0;
        let result = av.set_file(path);
        assert_matches!(result, Err(ref message) if message.to_string() == *"incorrect layout/length/data");
    }

    #[test]
    fn test_set_file_crafted_executable() {
        let file = get_append_vec_path("test_set_file_crafted_executable");
        let path = &file.path;
        let mut av = AppendVec::new(&path, true, 1024 * 1024);
        av.append_account_test(&create_test_account(10)).unwrap();
        {
            let mut executable_account = create_test_account(10);
            executable_account.1.executable = true;
            av.append_account_test(&executable_account).unwrap();
        }

        // reload accounts
        let accounts = av.accounts(0);

        // ensure false is 0u8 and true is 1u8 actually
        assert_eq!(*accounts[0].ref_executable_byte(), 0);
        assert_eq!(*accounts[1].ref_executable_byte(), 1);

        let account = &accounts[0];
        let crafted_executable = u8::max_value() - 1;

        account.set_executable_as_byte(crafted_executable);

        // reload crafted accounts
        let accounts = av.accounts(0);
        let account = accounts.first().unwrap();

        // we can observe crafted value by ref
        {
            let executable_bool: &bool = &account.account_meta.executable;
            // Depending on use, *executable_bool can be truthy or falsy due to direct memory manipulation
            // assert_eq! thinks *exeutable_bool is equal to false but the if condition thinks it's not, contradictly.
            assert_eq!(*executable_bool, false);
            const FALSE: bool = false; // keep clippy happy
            if *executable_bool == FALSE {
                panic!("This didn't occur if this test passed.");
            }
            assert_eq!(*account.ref_executable_byte(), crafted_executable);
        }

        // we can NOT observe crafted value by value
        {
            let executable_bool: bool = account.account_meta.executable;
            assert_eq!(executable_bool, false);
            assert_eq!(account.get_executable_byte(), 0); // Wow, not crafted_executable!
        }

        av.flush().unwrap();
        av.file_size = 0;
        let result = av.set_file(path);
        assert_matches!(result, Err(ref message) if message.to_string() == *"incorrect layout/length/data");
    }
}
