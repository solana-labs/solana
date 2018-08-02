//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger.

use bincode::{self, deserialize, deserialize_from, serialize_into, serialized_size};
use entry::Entry;
use hash::Hash;
use packet::{self, SharedBlob, BLOB_DATA_SIZE};
use rayon::prelude::*;
use result::{Error, Result};
use std::collections::VecDeque;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::prelude::*;
use std::io::{self, Cursor, Seek, SeekFrom};
use std::mem::size_of;
use std::path::Path;
use transaction::Transaction;

// ledger window
#[derive(Debug)]
pub struct LedgerWindow {
    index: File,
    data: File,
}

// use a CONST because there's a cast, and we don't want "sizeof::<u64> as u64"...
const SIZEOF_U64: u64 = size_of::<u64>() as u64;
const SIZEOF_USIZE: u64 = size_of::<usize>() as u64;

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
fn err_bincode_to_io(e: Box<bincode::ErrorKind>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

fn entry_at(file: &mut File, at: u64) -> io::Result<Entry> {
    file.seek(SeekFrom::Start(at))?;

    let len = deserialize_from(file.take(SIZEOF_USIZE)).map_err(err_bincode_to_io)?;

    deserialize_from(file.take(len)).map_err(err_bincode_to_io)
}

fn next_entry(file: &mut File) -> io::Result<Entry> {
    let len = deserialize_from(file.take(SIZEOF_USIZE)).map_err(err_bincode_to_io)?;
    deserialize_from(file.take(len)).map_err(err_bincode_to_io)
}

fn u64_at(file: &mut File, at: u64) -> io::Result<u64> {
    file.seek(SeekFrom::Start(at))?;
    deserialize_from(file.take(SIZEOF_U64)).map_err(err_bincode_to_io)
}

impl LedgerWindow {
    // opens a Ledger in directory, provides "infinite" window
    pub fn new(directory: &str) -> io::Result<Self> {
        let directory = Path::new(&directory);

        let index = File::open(directory.join("index"))?;
        let data = File::open(directory.join("data"))?;

        Ok(LedgerWindow { index, data })
    }

    pub fn get_entry(&mut self, index: u64) -> io::Result<Entry> {
        let offset = u64_at(&mut self.index, index * SIZEOF_U64)?;
        entry_at(&mut self.data, offset)
    }
}

// TODO?? ... we could open the files on demand to support [], but today
//   LedgerWindow needs "&mut self"
//
//impl Index<u64> for LedgerWindow {
//    type Output = io::Result<Entry>;
//
//    fn index(&mut self, index: u64) -> &io::Result<Entry> {
//        match u64_at(&mut self.index, index * SIZEOF_U64) {
//            Ok(offset) => &entry_at(&mut self.data, offset),
//            Err(e) => &Err(e),
//        }
//    }
//}

#[derive(Debug)]
pub struct LedgerWriter {
    index: File,
    data: File,
}

impl LedgerWriter {
    // opens or creates a LedgerWriter in directory
    pub fn new(directory: &str) -> io::Result<Self> {
        let directory = Path::new(&directory);

        create_dir_all(directory)?;

        let index = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("index"))?;

        let data = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("data"))?;

        Ok(LedgerWriter { index, data })
    }

    pub fn write_entry(&mut self, entry: &Entry) -> io::Result<()> {
        let offset = self.data.seek(SeekFrom::Current(0))?;

        let len = serialized_size(&entry).map_err(err_bincode_to_io)?;

        serialize_into(&mut self.data, &len).map_err(err_bincode_to_io)?;
        serialize_into(&mut self.data, &entry).map_err(err_bincode_to_io)?;
        self.data.flush()?;

        serialize_into(&mut self.index, &offset).map_err(err_bincode_to_io)?;
        self.index.flush()
    }

    pub fn write_entries<I>(&mut self, entries: I) -> io::Result<()>
    where
        I: IntoIterator<Item = Entry>,
    {
        for entry in entries {
            self.write_entry(&entry)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct LedgerReader {
    index: File,
    data: File,
}

impl Iterator for LedgerReader {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<io::Result<Entry>> {
        match next_entry(&mut self.data) {
            Ok(entry) => Some(Ok(entry)),
            Err(_) => None,
        }
    }
}

/// Return an iterator for all the entries in the given file.
pub fn read_ledger(directory: &str) -> io::Result<impl Iterator<Item = io::Result<Entry>>> {
    let directory = Path::new(&directory);

    let index = File::open(directory.join("index"))?;
    let data = File::open(directory.join("data"))?;

    Ok(LedgerReader { index, data })
}

pub fn copy_ledger(from: &str, to: &str) -> io::Result<()> {
    let mut to = LedgerWriter::new(to)?;

    for entry in read_ledger(from)? {
        let entry = entry?;
        to.write_entry(&entry)?;
    }
    Ok(())
}

// a Block is a slice of Entries
pub trait Block {
    /// Verifies the hashes and counts of a slice of transactions are all consistent.
    fn verify(&self, start_hash: &Hash) -> bool;
    fn to_blobs(&self, blob_recycler: &packet::BlobRecycler, q: &mut VecDeque<SharedBlob>);
}

impl Block for [Entry] {
    fn verify(&self, start_hash: &Hash) -> bool {
        let genesis = [Entry::new_tick(0, start_hash)];
        let entry_pairs = genesis.par_iter().chain(self).zip(self);
        entry_pairs.all(|(x0, x1)| {
            let r = x1.verify(&x0.id);
            if !r {
                warn!(
                    "entry invalid!: {:?} num txs: {}",
                    x1.id,
                    x1.transactions.len()
                );
            }
            r
        })
    }

    fn to_blobs(&self, blob_recycler: &packet::BlobRecycler, q: &mut VecDeque<SharedBlob>) {
        for entry in self {
            let blob = blob_recycler.allocate();
            let pos = {
                let mut bd = blob.write().unwrap();
                let mut out = Cursor::new(bd.data_mut());
                serialize_into(&mut out, &entry).expect("failed to serialize output");
                out.position() as usize
            };
            assert!(pos <= BLOB_DATA_SIZE, "pos: {}", pos);
            blob.write().unwrap().set_size(pos);
            q.push_back(blob);
        }
    }
}

pub fn reconstruct_entries_from_blobs(blobs: VecDeque<SharedBlob>) -> Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::with_capacity(blobs.len());

    for blob in blobs {
        let entry = {
            let msg = blob.read().unwrap();
            let msg_size = msg.get_size()?;
            deserialize(&msg.data()[..msg_size])
        };

        match entry {
            Ok(entry) => entries.push(entry),
            Err(err) => {
                trace!("reconstruct_entry_from_blobs: {:?}", err);
                return Err(Error::Serialize(err));
            }
        }
    }
    Ok(entries)
}

/// Creates the next entries for given transactions, outputs
/// updates start_hash to id of last Entry, sets cur_hashes to 0
pub fn next_entries_mut(
    start_hash: &mut Hash,
    cur_hashes: &mut u64,
    transactions: Vec<Transaction>,
) -> Vec<Entry> {
    // TODO: find a magic number that works better than |  ?
    //                                                  V
    if transactions.is_empty() || transactions.len() == 1 {
        vec![Entry::new_mut(start_hash, cur_hashes, transactions, false)]
    } else {
        let mut start = 0;
        let mut entries = Vec::new();

        while start < transactions.len() {
            let mut chunk_end = transactions.len();
            let mut upper = chunk_end;
            let mut lower = start;
            let mut next = chunk_end; // be optimistic that all will fit

            // binary search for how many transactions will fit in an Entry (i.e. a BLOB)
            loop {
                debug!(
                    "chunk_end {}, upper {} lower {} next {} transactions.len() {}",
                    chunk_end,
                    upper,
                    lower,
                    next,
                    transactions.len()
                );
                if Entry::will_fit(transactions[start..chunk_end].to_vec()) {
                    next = (upper + chunk_end) / 2;
                    lower = chunk_end;
                    debug!(
                        "chunk_end {} fits, maybe too well? trying {}",
                        chunk_end, next
                    );
                } else {
                    next = (lower + chunk_end) / 2;
                    upper = chunk_end;
                    debug!("chunk_end {} doesn't fit! trying {}", chunk_end, next);
                }
                // same as last time
                if next == chunk_end {
                    debug!("converged on chunk_end {}", chunk_end);
                    break;
                }
                chunk_end = next;
            }
            entries.push(Entry::new_mut(
                start_hash,
                cur_hashes,
                transactions[start..chunk_end].to_vec(),
                transactions.len() - chunk_end > 0,
            ));
            start = chunk_end;
        }

        entries
    }
}

/// Creates the next Entries for given transactions
pub fn next_entries(
    start_hash: &Hash,
    cur_hashes: u64,
    transactions: Vec<Transaction>,
) -> Vec<Entry> {
    let mut id = *start_hash;
    let mut num_hashes = cur_hashes;
    next_entries_mut(&mut id, &mut num_hashes, transactions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialized_size;
    use chrono::prelude::*;
    use entry::{next_entry, Entry};
    use hash::hash;
    use packet::{BlobRecycler, BLOB_DATA_SIZE, PACKET_DATA_SIZE};
    use signature::{KeyPair, KeyPairUtil};
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use transaction::{Transaction, Vote};

    fn tmp_ledger_path(name: &str) -> String {
        let keypair = KeyPair::new();

        format!("farf/{}-{}", name, keypair.pubkey())
    }

    #[test]
    fn test_verify_slice() {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        assert!(vec![][..].verify(&zero)); // base case
        assert!(vec![Entry::new_tick(0, &zero)][..].verify(&zero)); // singleton case 1
        assert!(!vec![Entry::new_tick(0, &zero)][..].verify(&one)); // singleton case 2, bad
        assert!(vec![next_entry(&zero, 0, vec![]); 2][..].verify(&zero)); // inductive step

        let mut bad_ticks = vec![next_entry(&zero, 0, vec![]); 2];
        bad_ticks[1].id = one;
        assert!(!bad_ticks.verify(&zero)); // inductive step, bad
    }

    fn make_test_entries() -> Vec<Entry> {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let keypair = KeyPair::new();
        let tx0 = Transaction::new_vote(
            &keypair,
            Vote {
                version: 0,
                contact_info_version: 1,
            },
            one,
            1,
        );
        let tx1 = Transaction::new_timestamp(&keypair, Utc::now(), one);
        //
        // TODO: this magic number and the mix of transaction types
        //       is designed to fill up a Blob more or less exactly,
        //       to get near enough the the threshold that
        //       deserialization falls over if it uses the wrong size()
        //       parameter to index into blob.data()
        //
        // magic numbers -----------------+
        //                                |
        //                                V
        let mut transactions = vec![tx0; 362];
        transactions.extend(vec![tx1; 100]);
        next_entries(&zero, 0, transactions)
    }

    #[test]
    fn test_entries_to_blobs() {
        let entries = make_test_entries();

        let blob_recycler = BlobRecycler::default();
        let mut blob_q = VecDeque::new();
        entries.to_blobs(&blob_recycler, &mut blob_q);

        assert_eq!(reconstruct_entries_from_blobs(blob_q).unwrap(), entries);
    }

    #[test]
    fn test_bad_blobs_attack() {
        let blob_recycler = BlobRecycler::default();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        let blobs_q = packet::to_blobs(vec![(0, addr)], &blob_recycler).unwrap(); // <-- attack!
        assert!(reconstruct_entries_from_blobs(blobs_q).is_err());
    }

    #[test]
    fn test_next_entries() {
        use logger;
        logger::setup();
        let id = Hash::default();
        let next_id = hash(&id.as_ref());
        let keypair = KeyPair::new();
        let tx_small = Transaction::new_vote(
            &keypair,
            Vote {
                version: 0,
                contact_info_version: 2,
            },
            next_id,
            2,
        );
        let tx_large = Transaction::new(&keypair, keypair.pubkey(), 1, next_id);

        let tx_small_size = serialized_size(&tx_small).unwrap();
        let tx_large_size = serialized_size(&tx_large).unwrap();
        assert!(tx_small_size < tx_large_size);
        assert!(tx_large_size < PACKET_DATA_SIZE as u64);

        // NOTE: if Entry grows to larger than a transaction, the code below falls over
        let threshold = (BLOB_DATA_SIZE / PACKET_DATA_SIZE) - 1;

        // verify no split
        let transactions = vec![tx_small.clone(); threshold];
        let entries0 = next_entries(&id, 0, transactions.clone());
        assert_eq!(entries0.len(), 1);
        assert!(entries0.verify(&id));

        // verify the split with uniform transactions
        let transactions = vec![tx_small.clone(); threshold * 2];
        let entries0 = next_entries(&id, 0, transactions.clone());
        assert_eq!(entries0.len(), 2);
        assert!(entries0[0].has_more);
        assert!(!entries0[entries0.len() - 1].has_more);
        assert!(entries0.verify(&id));

        // verify the split with small transactions followed by large
        // transactions
        let mut transactions = vec![tx_small.clone(); BLOB_DATA_SIZE / (tx_small_size as usize)];
        let large_transactions = vec![tx_large.clone(); BLOB_DATA_SIZE / (tx_large_size as usize)];

        transactions.extend(large_transactions);

        let entries0 = next_entries(&id, 0, transactions.clone());
        assert!(entries0.len() > 2);
        assert!(entries0[0].has_more);
        assert!(!entries0[entries0.len() - 1].has_more);
        assert!(entries0.verify(&id));
    }

    #[test]
    fn test_ledger_reader_writer() {
        let ledger_path = tmp_ledger_path("test_ledger_reader_writer");
        let entries = make_test_entries();

        let mut writer = LedgerWriter::new(&ledger_path).unwrap();
        writer.write_entries(entries.clone()).unwrap();

        let mut read_entries = vec![];
        for x in read_ledger(&ledger_path).unwrap() {
            let entry = x.unwrap();
            trace!("entry... {:?}", entry);
            read_entries.push(entry);
        }
        assert_eq!(read_entries, entries);

        let mut window = LedgerWindow::new(&ledger_path).unwrap();

        for (i, entry) in entries.iter().enumerate() {
            let read_entry = window.get_entry(i as u64).unwrap();
            assert_eq!(*entry, read_entry);
        }
        assert!(window.get_entry(100).is_err());

        std::fs::remove_file(Path::new(&ledger_path).join("data")).unwrap();
        // empty data file should fall over
        assert!(LedgerWindow::new(&ledger_path).is_err());
        assert!(read_ledger(&ledger_path).is_err());

        std::fs::remove_dir_all(ledger_path).unwrap();
    }
    #[test]
    fn test_copy_ledger() {
        let from = tmp_ledger_path("test_ledger_copy_from");
        let entries = make_test_entries();

        let mut writer = LedgerWriter::new(&from).unwrap();
        writer.write_entries(entries.clone()).unwrap();

        let to = tmp_ledger_path("test_ledger_copy_to");

        copy_ledger(&from, &to).unwrap();

        let mut read_entries = vec![];
        for x in read_ledger(&to).unwrap() {
            let entry = x.unwrap();
            trace!("entry... {:?}", entry);
            read_entries.push(entry);
        }
        assert_eq!(read_entries, entries);

        std::fs::remove_dir_all(from).unwrap();
        std::fs::remove_dir_all(to).unwrap();
    }

}
