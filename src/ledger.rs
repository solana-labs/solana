//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use bincode::{self, deserialize, deserialize_from, serialize_into, serialized_size};
use entry::Entry;
use hash::Hash;
//use log::Level::Trace;
use packet::{self, SharedBlob, BLOB_DATA_SIZE};
use rayon::prelude::*;
use result::{Error, Result};
use std::collections::VecDeque;
use std::fs::{create_dir_all, remove_dir_all, File, OpenOptions};
use std::io::prelude::*;
use std::io::{self, Cursor, Seek, SeekFrom};
use std::mem::size_of;
use std::path::Path;
use transaction::Transaction;

//
// A persistent ledger is 2 files:
//  ledger_path/ --+
//                 +-- index <== an array of u64 offsets into data,
//                 |               each offset points to the first bytes
//                 |               of a u64 that contains the length of
//                 |               the entry.  To make the code smaller,
//                 |               index[0] is set to 0, TODO: this field
//                 |               could later be used for other stuff...
//                 +-- data  <== concatenated instances of
//                                    u64 length
//                                    entry data
//
// When opening a ledger, we have the ability to "audit" it, which means we need
//  to pick which file to use as "truth", and correct the other file as
//  necessary, if possible.
//
// The protocol for writing the ledger is to append to the data file first, the
//   index file 2nd.  If the writing node is interupted while appending to the
//   ledger, there are some possibilities we need to cover:
//
//     1. a partial write of data, which might be a partial write of length
//          or a partial write entry data
//     2. a partial or missing write to index for that entry
//
// There is also the possibility of "unsynchronized" reading of the ledger
//   during transfer across nodes via rsync (or whatever).  In this case, if the
//   transfer of the data file is done before the transfer of the index file,
//   it's likely that the index file will be far ahead of the data file in time.
//
// The quickest and most reliable strategy for recovery is therefore to treat
//   the data file as nearest to the "truth".
//
// The logic for "recovery/audit" is to open index and read backwards from the
//   last u64-aligned entry to get to where index and data agree (i.e. where a
//   successful deserialization of an entry can be performed), then truncate
//   both files to this syncrhonization point.
//

// ledger window
#[derive(Debug)]
pub struct LedgerWindow {
    index: File,
    data: File,
}

// use a CONST because there's a cast, and we don't want "sizeof::<u64> as u64"...
const SIZEOF_U64: u64 = size_of::<u64>() as u64;

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
fn err_bincode_to_io(e: Box<bincode::ErrorKind>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

fn entry_at(file: &mut File, at: u64) -> io::Result<Entry> {
    file.seek(SeekFrom::Start(at))?;

    let len = deserialize_from(file.take(SIZEOF_U64)).map_err(err_bincode_to_io)?;
    //trace!("entry_at({}) len: {}", at, len);

    deserialize_from(file.take(len)).map_err(err_bincode_to_io)
}

fn next_entry(file: &mut File) -> io::Result<Entry> {
    let len = deserialize_from(file.take(SIZEOF_U64)).map_err(err_bincode_to_io)?;
    deserialize_from(file.take(len)).map_err(err_bincode_to_io)
}

fn u64_at(file: &mut File, at: u64) -> io::Result<u64> {
    file.seek(SeekFrom::Start(at))?;
    deserialize_from(file.take(SIZEOF_U64)).map_err(err_bincode_to_io)
}

impl LedgerWindow {
    // opens a Ledger in directory, provides "infinite" window
    pub fn new(ledger_path: &str) -> io::Result<Self> {
        let ledger_path = Path::new(&ledger_path);

        recover_ledger(ledger_path)?;

        let index = File::open(ledger_path.join("index"))?;
        let data = File::open(ledger_path.join("data"))?;

        Ok(LedgerWindow { index, data })
    }

    pub fn get_entry(&mut self, index: u64) -> io::Result<Entry> {
        let offset = u64_at(&mut self.index, index * SIZEOF_U64)?;
        entry_at(&mut self.data, offset)
    }
}

pub fn verify_ledger(ledger_path: &str, recover: bool) -> io::Result<()> {
    let ledger_path = Path::new(&ledger_path);

    if recover {
        recover_ledger(ledger_path)?;
    }

    let mut index = File::open(ledger_path.join("index"))?;
    let mut data = File::open(ledger_path.join("data"))?;

    let index_len = index.metadata()?.len();

    if index_len % SIZEOF_U64 != 0 {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "expected back-to-back entries",
        ))?;
    }

    let mut last_data_offset = 0;
    let mut index_offset = 0;
    let mut data_read = 0;
    let mut last_len = 0;

    while index_offset < index_len {
        let data_offset = u64_at(&mut index, index_offset)?;

        if last_data_offset + last_len != data_offset {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "expected back-to-back entries",
            ))?;
        }

        let entry = entry_at(&mut data, data_offset)?;
        last_len = serialized_size(&entry).map_err(err_bincode_to_io)? + SIZEOF_U64;
        last_data_offset = data_offset;

        data_read += last_len;
        index_offset += SIZEOF_U64;
    }
    if data_read != data.metadata()?.len() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "garbage on end of data file",
        ))?;
    }
    Ok(())
}

fn recover_ledger(ledger_path: &Path) -> io::Result<()> {
    let mut index = OpenOptions::new()
        .write(true)
        .read(true)
        .open(ledger_path.join("index"))?;

    let mut data = OpenOptions::new()
        .write(true)
        .read(true)
        .open(ledger_path.join("data"))?;

    // first, truncate to a multiple of SIZEOF_U64
    let len = index.metadata()?.len();

    if len % SIZEOF_U64 != 0 {
        //trace!("recover: trimming index len to {}", len - len % SIZEOF_U64);
        index.set_len(len - (len % SIZEOF_U64))?;
    }

    // next, pull index offsets off one at a time until the last one points
    //   to a valid entry deserialization offset...
    loop {
        let len = index.metadata()?.len();
        //trace!("recover: index len:{}", len);

        // should never happen
        if len < SIZEOF_U64 {
            //trace!("recover: error index len {} too small", len);

            Err(io::Error::new(io::ErrorKind::Other, "empty ledger index"))?;
        }

        let offset = u64_at(&mut index, len - SIZEOF_U64)?;
        //trace!("recover: offset[{}]: {}", (len / SIZEOF_U64) - 1, offset);

        match entry_at(&mut data, offset) {
            Ok(entry) => {
                //trace!("recover: entry[{}]: {:?}", (len / SIZEOF_U64) - 1, entry);

                let entry_len = serialized_size(&entry).map_err(err_bincode_to_io)?;

                //trace!("recover: entry_len: {}", entry_len);

                // now trim data file to size...
                data.set_len(offset + SIZEOF_U64 + entry_len)?;

                //trace!(
                //    "recover: trimmed data file to {}",
                //    offset + SIZEOF_U64 + entry_len
                //);

                break; // all good
            }
            Err(_err) => {
                //trace!(
                //    "recover: no entry recovered at {} {}",
                //    offset,
                //    _err.to_string()
                //);
                index.set_len(len - SIZEOF_U64)?;
            }
        }
    }
    //if log_enabled!(Trace) {
    //    let num_entries = index.metadata()?.len() / SIZEOF_U64;
    //    trace!("recover: done. {} entries", num_entries);
    //}

    // flush everything to disk...
    index.sync_all()?;
    data.sync_all()
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
    // opens or creates a LedgerWriter in ledger_path directory
    pub fn new(ledger_path: &str, create: bool) -> io::Result<Self> {
        let ledger_path = Path::new(&ledger_path);

        if create {
            let _ignored = remove_dir_all(ledger_path);
            create_dir_all(ledger_path)?;
        } else {
            recover_ledger(ledger_path)?;
        }
        let index = OpenOptions::new()
            .create(create)
            .append(true)
            .open(ledger_path.join("index"))?;

        //if log_enabled!(Trace) {
        //    let len = index.metadata()?.len();
        //    trace!("LedgerWriter::new: index fp:{}", len);
        //}

        let data = OpenOptions::new()
            .create(create)
            .append(true)
            .open(ledger_path.join("data"))?;

        //if log_enabled!(Trace) {
        //    let len = data.metadata()?.len();
        //    trace!("LedgerWriter::new: data fp:{}", len);
        //}

        Ok(LedgerWriter { index, data })
    }

    pub fn write_entry(&mut self, entry: &Entry) -> io::Result<()> {
        let len = serialized_size(&entry).map_err(err_bincode_to_io)?;

        serialize_into(&mut self.data, &len).map_err(err_bincode_to_io)?;
        //if log_enabled!(Trace) {
        //    let offset = self.data.seek(SeekFrom::Current(0))?;
        //    trace!("write_entry: after len data fp:{}", offset);
        //}

        serialize_into(&mut self.data, &entry).map_err(err_bincode_to_io)?;
        //if log_enabled!(Trace) {
        //    let offset = self.data.seek(SeekFrom::Current(0))?;
        //    trace!("write_entry: after entry data fp:{}", offset);
        //}

        //self.data.sync_data()?;

        let offset = self.data.seek(SeekFrom::Current(0))? - len - SIZEOF_U64;
        //trace!("write_entry: offset:{} len:{}", offset, len);

        serialize_into(&mut self.index, &offset).map_err(err_bincode_to_io)

        //if log_enabled!(Trace) {
        //    let offset = self.index.seek(SeekFrom::Current(0))?;
        //    trace!("write_entry: end index fp:{}", offset);
        //}

        //self.index.sync_data()
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
pub fn read_ledger(ledger_path: &str) -> io::Result<impl Iterator<Item = io::Result<Entry>>> {
    let ledger_path = Path::new(&ledger_path);

    recover_ledger(ledger_path)?;

    let data = File::open(ledger_path.join("data"))?;

    Ok(LedgerReader { data })
}

///// copy ledger is doesn't fix up the "from" ledger
//pub fn copy_ledger(from: &str, to: &str) -> io::Result<()> {
//    let mut to = LedgerWriter::new(to, true)?;
//
//    let from = Path::new(&from);
//
//    // for a copy, we read "readonly" from data
//    let data = File::open(from.join("data"))?;
//
//    for entry in (LedgerReader { data }) {
//        let entry = entry?;
//        to.write_entry(&entry)?;
//    }
//    Ok(())
//}

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

        format!("/tmp/tmp-ledger-{}-{}", name, keypair.pubkey())
    }

    #[test]
    fn test_verify_slice() {
        use logger;
        logger::setup();
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

    fn make_tiny_test_entries(num: usize) -> Vec<Entry> {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let keypair = KeyPair::new();

        let mut id = one;
        let mut num_hashes = 0;
        (0..num)
            .map(|_| {
                Entry::new_mut(
                    &mut id,
                    &mut num_hashes,
                    vec![Transaction::new_timestamp(&keypair, Utc::now(), one)],
                    false,
                )
            })
            .collect()
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
        use logger;
        logger::setup();
        let entries = make_test_entries();

        let blob_recycler = BlobRecycler::default();
        let mut blob_q = VecDeque::new();
        entries.to_blobs(&blob_recycler, &mut blob_q);

        assert_eq!(reconstruct_entries_from_blobs(blob_q).unwrap(), entries);
    }

    #[test]
    fn test_bad_blobs_attack() {
        use logger;
        logger::setup();
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
        use logger;
        logger::setup();
        let ledger_path = tmp_ledger_path("test_ledger_reader_writer");
        let entries = make_tiny_test_entries(10);

        let mut writer = LedgerWriter::new(&ledger_path, true).unwrap();
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

    fn truncated_last_entry(ledger_path: &str, entries: Vec<Entry>) {
        let mut writer = LedgerWriter::new(&ledger_path, true).unwrap();
        writer.write_entries(entries).unwrap();
        let len = writer.data.seek(SeekFrom::Current(0)).unwrap();
        writer.data.set_len(len - 4).unwrap();
    }

    fn garbage_on_data(ledger_path: &str, entries: Vec<Entry>) {
        let mut writer = LedgerWriter::new(&ledger_path, true).unwrap();
        writer.write_entries(entries).unwrap();
        writer.data.write_all(b"hi there!").unwrap();
    }

    fn read_ledger_check(ledger_path: &str, entries: Vec<Entry>, len: usize) {
        let read_entries = read_ledger(&ledger_path).unwrap();
        let mut i = 0;

        for entry in read_entries {
            assert_eq!(entry.unwrap(), entries[i]);
            i += 1;
        }
        assert_eq!(i, len);
    }

    fn ledger_window_check(ledger_path: &str, entries: Vec<Entry>, len: usize) {
        let mut window = LedgerWindow::new(&ledger_path).unwrap();
        for i in 0..len {
            let entry = window.get_entry(i as u64);
            assert_eq!(entry.unwrap(), entries[i]);
        }
    }

    #[test]
    fn test_recover_ledger() {
        use logger;
        logger::setup();

        let entries = make_tiny_test_entries(10);
        let ledger_path = tmp_ledger_path("test_recover_ledger");

        // truncate data file, tests recover inside read_ledger_check()
        truncated_last_entry(&ledger_path, entries.clone());
        read_ledger_check(&ledger_path, entries.clone(), entries.len() - 1);

        // truncate data file, tests recover inside LedgerWindow::new()
        truncated_last_entry(&ledger_path, entries.clone());
        ledger_window_check(&ledger_path, entries.clone(), entries.len() - 1);

        // restore last entry, tests recover_ledger() inside LedgerWriter::new()
        truncated_last_entry(&ledger_path, entries.clone());
        let mut writer = LedgerWriter::new(&ledger_path, false).unwrap();
        writer.write_entry(&entries[entries.len() - 1]).unwrap();
        read_ledger_check(&ledger_path, entries.clone(), entries.len());
        ledger_window_check(&ledger_path, entries.clone(), entries.len());

        // make it look like data is newer in time, check reader...
        garbage_on_data(&ledger_path, entries.clone());
        read_ledger_check(&ledger_path, entries.clone(), entries.len());

        // make it look like data is newer in time, check window...
        garbage_on_data(&ledger_path, entries.clone());
        ledger_window_check(&ledger_path, entries.clone(), entries.len());

        // make it look like data is newer in time, check writer...
        garbage_on_data(&ledger_path, entries[..entries.len() - 1].to_vec());
        let mut writer = LedgerWriter::new(&ledger_path, false).unwrap();
        writer.write_entry(&entries[entries.len() - 1]).unwrap();
        read_ledger_check(&ledger_path, entries.clone(), entries.len());
        ledger_window_check(&ledger_path, entries.clone(), entries.len());
        let _ignored = remove_dir_all(&ledger_path);
    }

    #[test]
    fn test_verify_ledger() {
        use logger;
        logger::setup();

        let entries = make_tiny_test_entries(10);
        let ledger_path = tmp_ledger_path("test_verify_ledger");
        let mut writer = LedgerWriter::new(&ledger_path, true).unwrap();
        writer.write_entries(entries.clone()).unwrap();

        assert!(verify_ledger(&ledger_path, false).is_ok());
        let _ignored = remove_dir_all(&ledger_path);
    }

    //    #[test]
    //    fn test_copy_ledger() {
    //        use logger;
    //        logger::setup();
    //
    //        let from = tmp_ledger_path("test_ledger_copy_from");
    //        let entries = make_tiny_test_entries(10);
    //
    //        let mut writer = LedgerWriter::new(&from, true).unwrap();
    //        writer.write_entries(entries.clone()).unwrap();
    //
    //        let to = tmp_ledger_path("test_ledger_copy_to");
    //
    //        copy_ledger(&from, &to).unwrap();
    //
    //        let mut read_entries = vec![];
    //        for x in read_ledger(&to).unwrap() {
    //            let entry = x.unwrap();
    //            trace!("entry... {:?}", entry);
    //            read_entries.push(entry);
    //        }
    //        assert_eq!(read_entries, entries);
    //
    //        std::fs::remove_dir_all(from).unwrap();
    //        std::fs::remove_dir_all(to).unwrap();
    //    }

}
