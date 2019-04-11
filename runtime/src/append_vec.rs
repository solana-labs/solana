use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use memmap::{Mmap, MmapMut};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::fs::{File, OpenOptions};
use std::io::{Error, ErrorKind, Read, Result, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::mem;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const SIZEOF_U64: usize = mem::size_of::<u64>();

macro_rules! align_up {
    ($addr: expr, $align: expr) => {
        ($addr + ($align - 1)) & !($align - 1)
    };
}

pub struct AppendVec<T> {
    data: File,
    mmap: Mmap,
    current_offset: AtomicUsize,
    mmap_mut: Mutex<MmapMut>,
    file_size: u64,
    inc_size: u64,
    phantom: PhantomData<T>,
}

fn get_account_size_static() -> usize {
    mem::size_of::<Account>() - mem::size_of::<Vec<u8>>()
}

pub fn get_serialized_size(account: &Account) -> usize {
    get_account_size_static() + account.data.len()
}

pub fn serialize_account(dst_slice: &mut [u8], account: &Account, len: usize) {
    let mut at = 0;

    write_u64(&mut at, dst_slice, len as u64);
    write_u64(&mut at, dst_slice, account.lamports);
    write_bytes(&mut at, dst_slice, &account.data);
    write_bytes(&mut at, dst_slice, account.owner.as_ref());
    write_bytes(&mut at, dst_slice, &[account.executable as u8]);
}

fn read_bytes(at: &mut usize, dst_slice: &mut [u8], src_slice: &[u8], len: usize) {
    let data = &src_slice[*at..*at + len];
    (&data[..]).read_exact(&mut dst_slice[..]).unwrap();
    *at += len;
}

fn write_bytes(at: &mut usize, dst_slice: &mut [u8], src_slice: &[u8]) {
    let data = &mut dst_slice[*at..*at + src_slice.len()];
    (&mut data[..]).write_all(&src_slice).unwrap();
    *at += src_slice.len();
}

fn read_u64(at: &mut usize, src_slice: &[u8]) -> u64 {
    let data = &src_slice[*at..*at + mem::size_of::<u64>()];
    *at += mem::size_of::<u64>();
    (&data[..]).read_u64::<LittleEndian>().unwrap()
}

fn write_u64(at: &mut usize, dst_slice: &mut [u8], value: u64) {
    let data = &mut dst_slice[*at..*at + mem::size_of::<u64>()];
    (&mut data[..]).write_u64::<LittleEndian>(value).unwrap();
    *at += mem::size_of::<u64>();
}

pub fn deserialize_account(
    src_slice: &[u8],
    index: usize,
    current_offset: usize,
) -> Result<Account> {
    let mut at = index;

    let size = read_u64(&mut at, &src_slice);
    let len = size as usize;
    assert!(current_offset >= at + len);

    let lamports = read_u64(&mut at, &src_slice);

    let data_len = len - get_account_size_static();
    let mut data = vec![0; data_len];
    read_bytes(&mut at, &mut data, &src_slice, data_len);

    let mut pubkey = vec![0; mem::size_of::<Pubkey>()];
    read_bytes(&mut at, &mut pubkey, &src_slice, mem::size_of::<Pubkey>());
    let owner = Pubkey::new(&pubkey);

    let mut exec = vec![0; mem::size_of::<bool>()];
    read_bytes(&mut at, &mut exec, &src_slice, mem::size_of::<bool>());
    let executable: bool = exec[0] != 0;

    Ok(Account {
        lamports,
        data,
        owner,
        executable,
    })
}

impl<T> AppendVec<T>
where
    T: Default,
{
    pub fn new(path: &Path, create: bool, size: u64, inc: u64) -> Self {
        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(path)
            .expect("Unable to open data file");

        data.seek(SeekFrom::Start(size)).unwrap();
        data.write_all(&[0]).unwrap();
        data.seek(SeekFrom::Start(0)).unwrap();
        data.flush().unwrap();
        let mmap = unsafe { Mmap::map(&data).expect("failed to map the data file") };
        let mmap_mut = unsafe { MmapMut::map_mut(&data).expect("failed to map the data file") };

        AppendVec {
            data,
            mmap,
            current_offset: AtomicUsize::new(0),
            mmap_mut: Mutex::new(mmap_mut),
            file_size: size,
            inc_size: inc,
            phantom: PhantomData,
        }
    }

    pub fn reset(&mut self) {
        let _mmap_mut = self.mmap_mut.lock().unwrap();
        self.current_offset.store(0, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get(&self, index: u64) -> &T {
        let offset = self.current_offset.load(Ordering::Relaxed);
        let at = index as usize;
        assert!(offset >= at + mem::size_of::<T>());
        let data = &self.mmap[at..at + mem::size_of::<T>()];
        let ptr = data.as_ptr() as *const T;
        let x: Option<&T> = unsafe { ptr.as_ref() };
        x.unwrap()
    }

    #[allow(dead_code)]
    pub fn grow_file(&mut self) -> Result<()> {
        if self.inc_size == 0 {
            return Err(Error::new(ErrorKind::WriteZero, "Grow not supported"));
        }
        let mut mmap_mut = self.mmap_mut.lock().unwrap();
        let index = self.current_offset.load(Ordering::Relaxed) + mem::size_of::<T>();
        if index as u64 + self.inc_size < self.file_size {
            // grow was already called
            return Ok(());
        }
        let end = self.file_size + self.inc_size;
        drop(mmap_mut.to_owned());
        drop(self.mmap.to_owned());
        self.data.seek(SeekFrom::Start(end))?;
        self.data.write_all(&[0])?;
        self.mmap = unsafe { Mmap::map(&self.data)? };
        *mmap_mut = unsafe { MmapMut::map_mut(&self.data)? };
        self.file_size = end;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn append(&self, val: T) -> Option<u64> {
        let mmap_mut = self.mmap_mut.lock().unwrap();
        let index = self.current_offset.load(Ordering::Relaxed);

        if (self.file_size as usize) < index + mem::size_of::<T>() {
            return None;
        }

        let data = &mmap_mut[index..(index + mem::size_of::<T>())];
        unsafe {
            let ptr = data.as_ptr() as *mut T;
            std::ptr::write(ptr, val)
        };
        self.current_offset
            .fetch_add(mem::size_of::<T>(), Ordering::Relaxed);
        Some(index as u64)
    }

    pub fn get_account(&self, index: u64) -> Result<Account> {
        let index = index as usize;
        deserialize_account(
            &self.mmap[..],
            index,
            self.current_offset.load(Ordering::Relaxed),
        )
    }

    pub fn append_account(&self, account: &Account) -> Option<u64> {
        let mut mmap_mut = self.mmap_mut.lock().unwrap();
        let data_at = align_up!(
            self.current_offset.load(Ordering::Relaxed),
            mem::size_of::<u64>()
        );
        let len = get_serialized_size(account);

        if (self.file_size as usize) < data_at + len + SIZEOF_U64 {
            return None;
        }

        serialize_account(
            &mut mmap_mut[data_at..data_at + len + SIZEOF_U64],
            &account,
            len,
        );

        self.current_offset
            .store(data_at + len + SIZEOF_U64, Ordering::Relaxed);
        Some(data_at as u64)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use log::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::timing::{duration_as_ms, duration_as_s};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    const START_SIZE: u64 = 4 * 1024 * 1024;
    const INC_SIZE: u64 = 1 * 1024 * 1024;

    #[test]
    fn test_append_vec() {
        let path = Path::new("append_vec");
        let av = AppendVec::new(path, true, START_SIZE, INC_SIZE);
        let val: u64 = 5;
        let index = av.append(val).unwrap();
        assert_eq!(*av.get(index), val);
        let val1 = val + 1;
        let index1 = av.append(val1).unwrap();
        assert_eq!(*av.get(index), val);
        assert_eq!(*av.get(index1), val1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_append_vec_account() {
        let path = Path::new("append_vec_account");
        let av: AppendVec<Account> = AppendVec::new(path, true, START_SIZE, INC_SIZE);
        let v1 = vec![1u8; 32];
        let mut account1 = Account {
            lamports: 1,
            data: v1,
            owner: Pubkey::default(),
            executable: false,
        };
        let index1 = av.append_account(&account1).unwrap();
        assert_eq!(index1, 0);
        assert_eq!(av.get_account(index1).unwrap(), account1);

        let v2 = vec![4u8; 32];
        let mut account2 = Account {
            lamports: 1,
            data: v2,
            owner: Pubkey::default(),
            executable: false,
        };
        let index2 = av.append_account(&account2).unwrap();
        let mut len = get_serialized_size(&account1) + SIZEOF_U64 as usize;
        assert_eq!(index2, len as u64);
        assert_eq!(av.get_account(index2).unwrap(), account2);
        assert_eq!(av.get_account(index1).unwrap(), account1);

        account2.data.iter_mut().for_each(|e| *e *= 2);
        let index3 = av.append_account(&account2).unwrap();
        len += get_serialized_size(&account2) + SIZEOF_U64 as usize;
        assert_eq!(index3, len as u64);
        assert_eq!(av.get_account(index3).unwrap(), account2);

        account1.data.extend([1, 2, 3, 4, 5, 6].iter().cloned());
        let index4 = av.append_account(&account1).unwrap();
        len += get_serialized_size(&account2) + SIZEOF_U64 as usize;
        assert_eq!(index4, len as u64);
        assert_eq!(av.get_account(index4).unwrap(), account1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_grow_append_vec() {
        let path = Path::new("grow");
        let mut av = AppendVec::new(path, true, START_SIZE, INC_SIZE);
        let mut val = [5u64; 32];
        let size = 100_000;
        let mut offsets = vec![0; size];

        let now = Instant::now();
        for index in 0..size {
            if let Some(offset) = av.append(val) {
                offsets[index] = offset;
            } else {
                assert!(av.grow_file().is_ok());
                if let Some(offset) = av.append(val) {
                    offsets[index] = offset;
                } else {
                    assert!(false);
                }
            }
            val[0] += 1;
        }
        info!(
            "time: {} ms {} / s",
            duration_as_ms(&now.elapsed()),
            ((mem::size_of::<[u64; 32]>() * size) as f32) / duration_as_s(&now.elapsed()),
        );

        let now = Instant::now();
        let num_reads = 100_000;
        for _ in 0..num_reads {
            let index = thread_rng().gen_range(0, size);
            assert_eq!(av.get(offsets[index])[0], (index + 5) as u64);
        }
        info!(
            "time: {} ms {} / s",
            duration_as_ms(&now.elapsed()),
            (num_reads as f32) / duration_as_s(&now.elapsed()),
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn random_atomic_change() {
        let path = Path::new("random");
        let mut vec = AppendVec::<AtomicUsize>::new(path, true, START_SIZE, INC_SIZE);
        let size = 1_000;
        for k in 0..size {
            if vec.append(AtomicUsize::new(k)).is_none() {
                assert!(vec.grow_file().is_ok());
                assert!(vec.append(AtomicUsize::new(0)).is_some());
            }
        }
        let index = thread_rng().gen_range(0, size as u64);
        let atomic1 = vec.get(index * mem::size_of::<AtomicUsize>() as u64);
        let current1 = atomic1.load(Ordering::Relaxed);
        assert_eq!(current1, index as usize);
        let next = current1 + 1;
        let index = vec.append(AtomicUsize::new(next)).unwrap();
        let atomic2 = vec.get(index);
        let current2 = atomic2.load(Ordering::Relaxed);
        assert_eq!(current2, next);
        std::fs::remove_file(path).unwrap();
    }
}
