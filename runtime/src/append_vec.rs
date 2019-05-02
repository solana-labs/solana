use memmap::MmapMut;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::mem;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

//Data is aligned at the next 64 byte offset. Without alignment loading the memory may
//crash on some architectures.
macro_rules! align_up {
    ($addr: expr, $align: expr) => {
        ($addr + ($align - 1)) & !($align - 1)
    };
}

/// StorageMeta contains enough context to recover the index from storage itself
#[derive(Clone, PartialEq, Debug)]
pub struct StorageMeta {
    /// global write version
    pub write_version: u64,
    /// key for the account
    pub pubkey: Pubkey,
    pub data_len: u64,
}

#[derive(Serialize, Deserialize, Clone, Default, Eq, PartialEq)]
pub struct AccountBalance {
    /// lamports in the account
    pub lamports: u64,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
}

/// References to Memory Mapped memory
/// The Account is stored separately from its data, so getting the actual account requires a clone
pub struct StoredAccount<'a> {
    pub meta: &'a StorageMeta,
    /// account data
    pub balance: &'a AccountBalance,
    pub data: &'a [u8],
}

impl<'a> StoredAccount<'a> {
    pub fn clone_account(&self) -> Account {
        Account {
            lamports: self.balance.lamports,
            owner: self.balance.owner,
            executable: self.balance.executable,
            data: self.data.to_vec(),
        }
    }
}

pub struct AppendVec {
    map: MmapMut,
    // This mutex forces append to be single threaded, but concurrent with reads
    append_offset: Mutex<usize>,
    current_len: AtomicUsize,
    file_size: u64,
}

impl AppendVec {
    #[allow(clippy::mutex_atomic)]
    pub fn new(file: &Path, create: bool, size: usize) -> Self {
        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(file)
            .expect("Unable to open data file");

        data.seek(SeekFrom::Start((size - 1) as u64)).unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        data.flush().unwrap();
        //UNSAFE: Required to create a Mmap
        let map = unsafe { MmapMut::map_mut(&data).expect("failed to map the data file") };

        AppendVec {
            map,
            // This mutex forces append to be single threaded, but concurrent with reads
            // See UNSAFE usage in `append_ptr`
            append_offset: Mutex::new(0),
            current_len: AtomicUsize::new(0),
            file_size: size as u64,
        }
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

    fn get_slice(&self, offset: usize, size: usize) -> Option<(&[u8], usize)> {
        let len = self.len();
        if len < offset + size {
            return None;
        }
        let data = &self.map[offset..offset + size];
        //Data is aligned at the next 64 byte offset. Without alignment loading the memory may
        //crash on some architectures.
        let next = align_up!(offset + size, mem::size_of::<u64>());
        Some((
            //UNSAFE: This unsafe creates a slice that represents a chunk of self.map memory
            //The lifetime of this slice is tied to &self, since it points to self.map memory
            unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, size) },
            next,
        ))
    }

    fn append_ptr(&self, offset: &mut usize, src: *const u8, len: usize) {
        //Data is aligned at the next 64 byte offset. Without alignment loading the memory may
        //crash on some architectures.
        let pos = align_up!(*offset as usize, mem::size_of::<u64>());
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
            //Data is aligned at the next 64 byte offset. Without alignment loading the memory may
            //crash on some architectures.
            end = align_up!(end, mem::size_of::<u64>());
            end += val.1;
        }

        if (self.file_size as usize) < end {
            return None;
        }

        //Data is aligned at the next 64 byte offset. Without alignment loading the memory may
        //crash on some architectures.
        let pos = align_up!(*offset, mem::size_of::<u64>());
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
        let (meta, next): (&'a StorageMeta, _) = self.get_type(offset)?;
        let (balance, next): (&'a AccountBalance, _) = self.get_type(next)?;
        let (data, next) = self.get_slice(next, meta.data_len as usize)?;
        Some((
            StoredAccount {
                meta,
                balance,
                data,
            },
            next,
        ))
    }
    pub fn get_account_test(&self, offset: usize) -> Option<(StorageMeta, Account)> {
        let stored = self.get_account(offset)?;
        let meta = stored.0.meta.clone();
        Some((meta, stored.0.clone_account()))
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
    pub fn append_accounts(&self, accounts: &[(StorageMeta, &Account)]) -> Vec<usize> {
        let mut offset = self.append_offset.lock().unwrap();
        let mut rv = vec![];
        for (storage_meta, account) in accounts {
            let meta_ptr = storage_meta as *const StorageMeta;
            let balance = AccountBalance {
                lamports: account.lamports,
                owner: account.owner,
                executable: account.executable,
            };
            let balance_ptr = &balance as *const AccountBalance;
            let data_len = storage_meta.data_len as usize;
            let data_ptr = account.data.as_ptr();
            let ptrs = [
                (meta_ptr as *const u8, mem::size_of::<StorageMeta>()),
                (balance_ptr as *const u8, mem::size_of::<AccountBalance>()),
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

    pub fn append_account(&self, storage_meta: StorageMeta, account: &Account) -> Option<usize> {
        self.append_accounts(&[(storage_meta, account)])
            .first()
            .cloned()
    }

    pub fn append_account_test(&self, data: &(StorageMeta, Account)) -> Option<usize> {
        self.append_account(data.0.clone(), &data.1)
    }
}

pub mod test_utils {
    use super::StorageMeta;
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
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn get_append_vec_path(path: &str) -> TempFile {
        let out_dir =
            std::env::var("OUT_DIR").unwrap_or_else(|_| "/tmp/append_vec_tests".to_string());
        let mut buf = PathBuf::new();
        let rand_string: String = thread_rng().sample_iter(&Alphanumeric).take(30).collect();
        buf.push(&format!("{}/{}{}", out_dir, path, rand_string));
        create_dir_all(out_dir).expect("Create directory failed");
        TempFile { path: buf }
    }

    pub fn create_test_account(sample: usize) -> (StorageMeta, Account) {
        let data_len = sample % 256;
        let mut account = Account::new(sample as u64, 0, &Pubkey::default());
        account.data = (0..data_len).map(|_| data_len as u8).collect();
        let storage_meta = StorageMeta {
            write_version: 0,
            pubkey: Pubkey::default(),
            data_len: data_len as u64,
        };
        (storage_meta, account)
    }
}

#[cfg(test)]
pub mod tests {
    use super::test_utils::*;
    use super::*;
    use log::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::timing::duration_as_ms;
    use std::time::Instant;

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
}
