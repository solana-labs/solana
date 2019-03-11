use memmap::Mmap;

use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::sync::{Arc, RwLock};

const BACKING_ERR: &str = "In-memory table lock poisoned; concurrency error";

#[derive(Debug)]
pub enum MemMap {
    Disk(Mmap),
    Mem(Arc<RwLock<Vec<u8>>>),
}

#[derive(Debug)]
pub enum Writer {
    Disk(BufWriter<File>),
    Mem(SharedWriter),
}

#[derive(Debug)]
pub struct SharedWriter {
    buf: Arc<RwLock<Vec<u8>>>,
    pos: u64,
}

impl SharedWriter {
    pub fn new(buf: Arc<RwLock<Vec<u8>>>) -> SharedWriter {
        SharedWriter { buf, pos: 0 }
    }
}

impl Deref for MemMap {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            MemMap::Disk(mmap) => mmap.deref(),
            MemMap::Mem(vec) => {
                let buf = vec.read().expect(BACKING_ERR);
                let slice = buf.as_slice();

                // transmute lifetime. Relying on the RwLock + immutability for safety
                unsafe { std::mem::transmute(slice) }
            }
        }
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use std::cmp;

        let mut vec = self.buf.write().expect(BACKING_ERR);

        // Calc ranges
        let space_remaining = vec.len() - self.pos as usize;
        let copy_len = cmp::min(buf.len(), space_remaining);
        let copy_src_range = 0..copy_len;
        let append_src_range = copy_len..buf.len();
        let copy_dest_range = self.pos as usize..(self.pos as usize + copy_len);

        // Copy then append
        (&mut vec[copy_dest_range]).copy_from_slice(&buf[copy_src_range]);
        vec.extend_from_slice(&buf[append_src_range]);

        let written = buf.len();

        self.pos += written as u64;

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let _written = self.write(buf)?;
        Ok(())
    }
}

impl Seek for SharedWriter {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(new_pos) => new_pos,
            SeekFrom::Current(diff) => (self.pos as i64 + diff) as u64,
            SeekFrom::End(rpos) => (self.buf.read().expect(BACKING_ERR).len() as i64 + rpos) as u64,
        };

        Ok(self.pos)
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Writer::Disk(ref mut wtr) => wtr.write(buf),
            Writer::Mem(ref mut wtr) => wtr.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Writer::Disk(ref mut wtr) => {
                wtr.flush()?;
                wtr.get_mut().sync_data()?;
                Ok(())
            }
            Writer::Mem(ref mut wtr) => wtr.flush(),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            Writer::Disk(ref mut wtr) => wtr.write_all(buf),
            Writer::Mem(ref mut wtr) => wtr.write_all(buf),
        }
    }
}

impl Seek for Writer {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            Writer::Disk(ref mut wtr) => wtr.seek(pos),
            Writer::Mem(ref mut wtr) => wtr.seek(pos),
        }
    }
}
