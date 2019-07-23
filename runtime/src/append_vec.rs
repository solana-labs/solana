use bincode::{deserialize_from, serialize_into, serialized_size};
use memmap::MmapMut;
use serde::{Deserialize, Serialize};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::fmt;
use std::fs::{create_dir_all, remove_file, OpenOptions};
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::mem;
use std::path::{Path, PathBuf};
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

#[derive(Serialize, Deserialize, Clone, Debug, Default, Eq, PartialEq)]
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
#[derive(PartialEq, Debug)]
pub struct StoredAccount<'a> {
    pub meta: &'a StorageMeta,
    /// account data
    pub balance: &'a AccountBalance,
    pub data: &'a [u8],
    pub offset: usize,
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

#[derive(Debug)]
#[allow(clippy::mutex_atomic)]
pub struct AppendVec {
    path: PathBuf,
    map: MmapMut,
    // This mutex forces append to be single threaded, but concurrent with reads
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
        if create {
            let _ignored = remove_file(file);
            if let Some(parent) = file.parent() {
                create_dir_all(parent).expect("Create directory failed");
            }
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
        let map = unsafe { MmapMut::map_mut(&data).expect("failed to map the data file") };

        AppendVec {
            path: file.to_path_buf(),
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
                offset,
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

#[allow(clippy::mutex_atomic)]
impl Serialize for AppendVec {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::Error;
        let len = serialized_size(&self.path).unwrap()
            + std::mem::size_of::<u64>() as u64
            + std::mem::size_of::<u64>() as u64
            + std::mem::size_of::<usize>() as u64;
        let mut buf = vec![0u8; len as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        self.map.flush().map_err(Error::custom)?;
        serialize_into(&mut wr, &self.path).map_err(Error::custom)?;
        serialize_into(&mut wr, &(self.current_len.load(Ordering::Relaxed) as u64))
            .map_err(Error::custom)?;
        serialize_into(&mut wr, &self.file_size).map_err(Error::custom)?;
        let offset = *self.append_offset.lock().unwrap();
        serialize_into(&mut wr, &offset).map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }
}

struct AppendVecVisitor;

impl<'a> serde::de::Visitor<'a> for AppendVecVisitor {
    type Value = AppendVec;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting AppendVec")
    }

    #[allow(clippy::mutex_atomic)]
    fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        use serde::de::Error;
        let mut rd = Cursor::new(&data[..]);
        let path: PathBuf = deserialize_from(&mut rd).map_err(Error::custom)?;
        let current_len: u64 = deserialize_from(&mut rd).map_err(Error::custom)?;
        let file_size: u64 = deserialize_from(&mut rd).map_err(Error::custom)?;
        let offset: usize = deserialize_from(&mut rd).map_err(Error::custom)?;

        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&path)
            .map_err(|e| Error::custom(e.to_string()))?;

        let map = unsafe { MmapMut::map_mut(&data).map_err(|e| Error::custom(e.to_string()))? };
        Ok(AppendVec {
            path,
            map,
            append_offset: Mutex::new(offset),
            current_len: AtomicUsize::new(current_len as usize),
            file_size,
        })
    }
}

impl<'de> Deserialize<'de> for AppendVec {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(AppendVecVisitor)
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

    #[test]
    fn test_append_vec_serialize() {
        let path = get_append_vec_path("test_append_serialize");
        let av: AppendVec = AppendVec::new(&Path::new(&path.path).join("0"), true, 1024 * 1024);
        let account1 = create_test_account(1);
        let index1 = av.append_account_test(&account1).unwrap();
        assert_eq!(index1, 0);
        assert_eq!(av.get_account_test(index1).unwrap(), account1);

        let account2 = create_test_account(2);
        let index2 = av.append_account_test(&account2).unwrap();
        assert_eq!(av.get_account_test(index2).unwrap(), account2);
        assert_eq!(av.get_account_test(index1).unwrap(), account1);

        let mut buf = vec![0u8; serialized_size(&av).unwrap() as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        serialize_into(&mut writer, &av).unwrap();

        let mut reader = Cursor::new(&mut buf[..]);
        let dav: AppendVec = deserialize_from(&mut reader).unwrap();

        assert_eq!(dav.get_account_test(index2).unwrap(), account2);
        assert_eq!(dav.get_account_test(index1).unwrap(), account1);
        drop(dav);

        // dropping dav above blows away underlying file's directory entry,
        //   which is what we're testing next.
        let mut reader = Cursor::new(&mut buf[..]);
        let dav: Result<AppendVec, Box<bincode::ErrorKind>> = deserialize_from(&mut reader);
        assert!(dav.is_err());
    }
}
