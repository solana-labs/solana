use byteorder::{BigEndian, ByteOrder};

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::ops::Range;
use std::path::Path;

#[derive(Debug)]
pub struct RecordFile<T> {
    file: File,
    current_offset: u64,
    buf: RefCell<Vec<u8>>,
    _dummy: PhantomData<T>,
}

pub trait Record {
    const SIZE: usize;

    fn write(&self, buf: &mut [u8]);
    fn read(buf: &[u8]) -> Self;
}

#[derive(Debug)]
pub struct Records<'a, T> {
    rf: &'a RecordFile<T>,
    pos: u64,
    bounds: Range<u64>,
    done: bool,
}

impl<T: Record> RecordFile<T> {
    const _SIZE: usize = T::SIZE + 1;
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(RecordFile {
            buf: RefCell::new(vec![0u8; Self::_SIZE]),
            current_offset: 0,
            _dummy: PhantomData,
            file: OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(path)?,
        })
    }

    pub fn set(&mut self, idx: u64, elem: T) -> io::Result<()> {
        let mut buf = self.buf.borrow_mut();
        buf[0] = 1;
        elem.write(&mut buf[1..]);

        self.file.seek(SeekFrom::Start(Self::_SIZE as u64 * idx))?;
        self.file.write_all(&buf)
    }

    #[allow(dead_code)]
    pub fn is_present(&self, idx: u64) -> io::Result<bool> {
        let mut buf = self.buf.borrow_mut();
        let mut file = &self.file;
        file.seek(SeekFrom::Start(Self::_SIZE as u64 * idx))?;

        match file.read_exact(&mut buf) {
            Ok(_) => {}
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return Ok(false);
                }
            }
        };

        Ok(buf[0] != 0)
    }

    pub fn get(&self, idx: u64) -> io::Result<Option<T>> {
        let mut buf = self.buf.borrow_mut();
        let mut file = &self.file;
        file.seek(SeekFrom::Start(Self::_SIZE as u64 * idx))?;

        match file.read_exact(&mut buf) {
            Ok(_) => {}
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return Ok(None);
                } else {
                    return Err(e);
                }
            }
        };

        Ok(Some(T::read(&buf[1..])))
    }

    pub fn records(&self, range: Range<u64>) -> Records<T> {
        Records {
            pos: range.start,
            rf: self,
            bounds: range,
            done: false,
        }
    }
}

impl<'a, T: Record> Iterator for Records<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        loop {
            if self.done || self.pos < self.bounds.start || self.pos >= self.bounds.end {
                return None;
            }
            if let Ok(x) = self.rf.get(self.pos) {
                self.pos += 1;
                if x.is_some() {
                    return x;
                } else {
                    // hit EOF
                    self.done = true;
                    break;
                }
            } else {
                // real error
                break;
            }
        }

        None
    }
}

impl Record for u64 {
    const SIZE: usize = 8;

    fn read(data: &[u8]) -> u64 {
        BigEndian::read_u64(data)
    }

    fn write(&self, buf: &mut [u8]) {
        BigEndian::write_u64(buf, *self);
    }
}
