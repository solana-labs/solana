use byteorder::{BigEndian, ByteOrder};

use crc::crc32;

use memmap::Mmap;

use std::cmp;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
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

#[derive(Debug)]
pub struct CRCWriter<W: Write> {
    wtr: W,
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
}

#[derive(Debug)]
pub struct CRCReader<R: Read> {
    rdr: R,
    buf: Vec<u8>,
    pos: usize,
    chunk_size: usize,
}

impl<W: Write> CRCWriter<W> {
    pub fn new(inner: W, chunk_size: usize) -> CRCWriter<W> {
        if chunk_size <= 8 {
            panic!("chunk_size must be > 8");
        }

        CRCWriter {
            wtr: inner,
            buf: vec![0; chunk_size],
            pos: 0,
            cap: chunk_size - 8,
        }
    }

    #[allow(dead_code)]
    pub fn into_inner(mut self) -> io::Result<W> {
        self.flush()?;
        Ok(self.wtr)
    }

    pub fn get_ref(&self) -> &W {
        &self.wtr
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }
}

impl<R: Read> CRCReader<R> {
    pub fn new(inner: R, chunk_size: usize) -> CRCReader<R> {
        if chunk_size <= 8 {
            panic!("chunk_size must be > 8");
        }

        CRCReader {
            rdr: inner,
            buf: vec![0; chunk_size - 8],
            pos: chunk_size,
            chunk_size,
        }
    }

    #[allow(dead_code)]
    pub fn into_inner(self) -> R {
        self.rdr
    }
}

impl SharedWriter {
    pub fn new(buf: Arc<RwLock<Vec<u8>>>) -> SharedWriter {
        SharedWriter { buf, pos: 0 }
    }
}

impl<R> CRCReader<R>
where
    R: Read,
{
    fn load_block(&mut self) -> io::Result<()> {
        self.buf.clear();
        self.pos = 0;

        let mut blk_buf = vec![0; self.chunk_size];
        let mut bpos = 0;

        while bpos < self.chunk_size {
            let bytes_read = self.rdr.read(&mut blk_buf[bpos..])?;
            if bytes_read == 0 {
                break;
            }
            bpos += bytes_read
        }

        if bpos < self.chunk_size {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        assert_eq!(bpos, self.chunk_size);

        let stored_digest = BigEndian::read_u32(&blk_buf[0..4]);
        let payload_len = BigEndian::read_u32(&blk_buf[4..8]) as usize;
        if payload_len + 8 > blk_buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CRCReader: invalid block size",
            ));
        }
        let payload = &blk_buf[8..8 + payload_len];
        let computed_digest = crc32::checksum_ieee(&blk_buf[4..8 + payload_len]);

        if computed_digest != stored_digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CRCReader: CRC validation failed",
            ));
        }

        self.buf.extend_from_slice(payload);

        Ok(())
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

impl<W> Write for CRCWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;

        while written < buf.len() {
            let batch_len = (&mut self.buf[8 + self.pos..]).write(&buf[written..])?;

            self.pos += batch_len;
            written += batch_len;

            if self.pos >= self.cap {
                self.flush()?;
            }
        }

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        BigEndian::write_u32(&mut self.buf[4..8], self.pos as u32);
        let total_len = self.pos + 8;

        // crc over length + payload
        let digest = crc32::checksum_ieee(&self.buf[4..total_len]);

        BigEndian::write_u32(&mut self.buf[0..4], digest);
        self.wtr.write_all(&self.buf)?;

        self.pos = 0;
        Ok(())
    }
}

impl<R> Read for CRCReader<R>
where
    R: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut bytes_read = 0;
        let mut write_pos = 0;

        while write_pos < buf.len() {
            if self.pos >= self.buf.len() {
                self.load_block()?;
            }

            let bytes_available = self.buf.len() - self.pos;
            let space_remaining = buf.len() - write_pos;
            let copy_len = cmp::min(bytes_available, space_remaining);

            (&mut buf[write_pos..write_pos + copy_len])
                .copy_from_slice(&self.buf[self.pos..self.pos + copy_len]);

            bytes_read += copy_len;
            write_pos += copy_len;
            self.pos += copy_len;
        }

        Ok(bytes_read)
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_crc_write() {
        let block_sizes = &[256, 512, 1024, 2048];
        let byte_counts = &[8, 128, 1024, 1024 * 8];

        for &block_size in block_sizes {
            for &n_bytes in byte_counts {
                let bytes: Vec<_> = (0..n_bytes).map(|x| (x % 255) as u8).collect();
                let buf = Vec::new();

                let mut wtr = CRCWriter::new(buf, block_size);
                wtr.write_all(&bytes).unwrap();

                let buf = wtr.into_inner().unwrap();

                let space_per_block = block_size - 8;
                let n_full_blocks = n_bytes / space_per_block;
                let blocks_expected = n_full_blocks + (n_bytes % space_per_block != 0) as usize;
                let expected_len = blocks_expected * block_size;

                assert_eq!(buf.len(), expected_len);
                assert_eq!(&buf[8..16], &[0, 1, 2, 3, 4, 5, 6, 7]);
            }
        }
    }

    #[test]
    fn test_crc_io() {
        const BLK_SIZE: usize = 1024;
        let bytes: Vec<_> = (0..512 * 1024).map(|x| (x % 255) as u8).collect();
        let buf = Vec::new();

        let mut wtr = CRCWriter::new(buf, BLK_SIZE);
        wtr.write_all(&bytes).unwrap();

        let buf = wtr.into_inner().unwrap();
        assert_eq!(&buf[8..16], &[0, 1, 2, 3, 4, 5, 6, 7]);

        let mut rdr = CRCReader::new(&buf[..], BLK_SIZE);

        let mut retrieved = Vec::with_capacity(512 * 1024);
        let buffer = &mut [0; 1024];
        while let Ok(amt) = rdr.read(buffer) {
            if amt == 0 {
                break;
            }
            retrieved.extend_from_slice(&buffer[..amt]);
        }

        assert_eq!(&retrieved[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);

        assert_eq!(bytes.len(), retrieved.len());
        assert_eq!(bytes, retrieved);
    }

    #[test]
    fn test_crc_validation() {
        const BLK_SIZE: usize = 1024;
        let n_bytes = 512 * 1024;
        let bytes: Vec<_> = (0..n_bytes).map(|x| (x % 255) as u8).collect();
        let buf = Vec::new();

        let mut wtr = CRCWriter::new(buf, BLK_SIZE);
        wtr.write_all(&bytes).unwrap();

        let mut buf = wtr.into_inner().unwrap();
        buf[BLK_SIZE / 2] += 1;

        let mut rdr = CRCReader::new(&buf[..], BLK_SIZE);

        let mut retrieved = vec![];
        let res = rdr.read_to_end(&mut retrieved);
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_crc_size_mismatch() {
        const BLK_SIZE: usize = 1024;
        let n_bytes = 512 * 1024;
        let bytes: Vec<_> = (0..n_bytes).map(|x| (x % 255) as u8).collect();
        let buf = Vec::new();

        let mut wtr = CRCWriter::new(buf, BLK_SIZE);
        wtr.write_all(&bytes).unwrap();

        let mut buf = wtr.into_inner().unwrap();
        buf.drain((n_bytes - 512)..n_bytes);

        for &size_diff in &[100, 1, 25, BLK_SIZE - 9] {
            let mut rdr = CRCReader::new(&buf[..], BLK_SIZE - size_diff);

            let mut retrieved = vec![];
            let res = rdr.read_to_end(&mut retrieved);
            assert_eq!(res.unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
    }

    #[should_panic]
    #[test]
    fn test_crc_writer_invalid_chunk_size() {
        let _ = CRCWriter::new(Vec::new(), 8);
    }

    #[should_panic]
    #[test]
    fn test_crc_reader_invalid_chunk_size() {
        let _ = CRCReader::new(io::empty(), 8);
    }
}
