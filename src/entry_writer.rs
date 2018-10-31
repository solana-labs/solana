//! The `entry_writer` module helps implement the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bincode;
use entry::Entry;
use std::io::{self, BufRead, Error, ErrorKind, Write};
use std::mem::size_of;

pub struct EntryWriter {}

impl EntryWriter {
    fn write_entry<W: Write>(writer: &mut W, entry: &Entry) -> io::Result<()> {
        let entry_bytes =
            bincode::serialize(&entry).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

        let len = entry_bytes.len();
        let len_bytes =
            bincode::serialize(&len).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

        writer.write_all(&len_bytes[..])?;
        writer.write_all(&entry_bytes[..])?;
        writer.flush()
    }

    pub fn write_entries<W: Write, I>(writer: &mut W, entries: I) -> io::Result<()>
    where
        I: IntoIterator<Item = Entry>,
    {
        for entry in entries {
            Self::write_entry(writer, &entry)?;
        }
        Ok(())
    }
}

struct EntryReader<R: BufRead> {
    reader: R,
    entry_bytes: Vec<u8>,
}

impl<R: BufRead> Iterator for EntryReader<R> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<io::Result<Entry>> {
        let mut entry_len_bytes = [0u8; size_of::<usize>()];

        if self.reader.read_exact(&mut entry_len_bytes[..]).is_ok() {
            let entry_len = bincode::deserialize(&entry_len_bytes).unwrap();

            if entry_len > self.entry_bytes.len() {
                self.entry_bytes.resize(entry_len, 0);
            }

            if let Err(e) = self.reader.read_exact(&mut self.entry_bytes[..entry_len]) {
                Some(Err(e))
            } else {
                Some(
                    bincode::deserialize(&self.entry_bytes)
                        .map_err(|e| Error::new(ErrorKind::Other, e.to_string())),
                )
            }
        } else {
            None // EOF (probably)
        }
    }
}

/// Return an iterator for all the entries in the given file.
pub fn read_entries<R: BufRead>(reader: R) -> impl Iterator<Item = io::Result<Entry>> {
    EntryReader {
        reader,
        entry_bytes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::io::Cursor;

    /// Same as read_entries() but parsing a buffer and returning a vector.
    fn read_entries_from_buf(s: &[u8]) -> io::Result<Vec<Entry>> {
        let mut result = vec![];
        let reader = Cursor::new(s);
        for x in read_entries(reader) {
            trace!("entry... {:?}", x);
            result.push(x?);
        }
        Ok(result)
    }

    #[test]
    fn test_read_entries_from_buf() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let mint = Mint::new(1, dummy_leader_id, dummy_leader_tokens);
        let mut buf = vec![];
        EntryWriter::write_entries(&mut buf, mint.create_entries()).unwrap();
        let entries = read_entries_from_buf(&buf).unwrap();
        assert_eq!(entries, mint.create_entries());
    }
}
