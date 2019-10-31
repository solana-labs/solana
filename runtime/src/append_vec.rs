use bincode::{deserialize_from, serialize_into};
use memmap::MmapMut;
use serde::{Deserialize, Serialize};
use solana_sdk::{account::Account, clock::Epoch, hash::Hash, pubkey::Pubkey};
use std::{
    fmt,
    fs::{create_dir_all, remove_file, OpenOptions},
    io,
    io::{Cursor, Seek, SeekFrom, Write},
    mem,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    sync::Mutex,
};

//Data is aligned at the next 64 byte offset. Without alignment loading the memory may
//crash on some architectures.
macro_rules! align_up {
    ($addr: expr, $align: expr) => {
        ($addr + ($align - 1)) & !($align - 1)
    };
}

/// Meta contains enough context to recover the index from storage itself
#[derive(Clone, PartialEq, Debug)]
pub struct StoredMeta {
    /// global write version
    pub write_version: u64,
    /// key for the account
    pub pubkey: Pubkey,
    pub data_len: u64,
}

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
            hash: *self.hash,
        }
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

    pub fn new_relative_path(fork_id: u64, id: usize) -> PathBuf {
        PathBuf::from(&format!("{}.{}", fork_id, id))
    }

    #[allow(clippy::mutex_atomic)]
    pub fn set_file<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        self.path = path.as_ref().to_path_buf();
        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&path)?;

        let map = unsafe { MmapMut::map_mut(&data)? };
        self.map = map;
        Ok(())
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

#[allow(clippy::mutex_atomic)]
impl Serialize for AppendVec {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::Error;
        let len = std::mem::size_of::<usize>() as u64;
        let mut buf = vec![0u8; len as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &(self.current_len.load(Ordering::Relaxed) as u64))
            .map_err(Error::custom)?;
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
    // Note this does not initialize a valid Mmap in the AppendVec, needs to be done
    // externally
    fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        use serde::de::Error;
        let mut rd = Cursor::new(&data[..]);
        let current_len: usize = deserialize_from(&mut rd).map_err(Error::custom)?;
        let map = MmapMut::map_anon(1).map_err(|e| Error::custom(e.to_string()))?;
        Ok(AppendVec {
            path: PathBuf::from(String::default()),
            map,
            append_offset: Mutex::new(current_len),
            current_len: AtomicUsize::new(current_len),
            file_size: current_len as u64,
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

    impl AppendVec {
        fn append_account_test(&self, data: &(StoredMeta, Account)) -> Option<usize> {
            self.append_account(data.0.clone(), &data.1, Hash::default())
        }
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
}
