//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use bincode::{self, deserialize, deserialize_from, serialize_into, serialized_size};
use budget_instruction::Vote;
use budget_transaction::BudgetTransaction;
#[cfg(test)]
use chrono::prelude::Utc;
use entry::Entry;
#[cfg(test)]
use hash::hash;
use hash::Hash;
use log::Level::Trace;
use mint::Mint;
use packet::{SharedBlob, BLOB_DATA_SIZE};
use rayon::prelude::*;
use result::{Error, Result};
use signature::{Keypair, KeypairUtil};
use solana_program_interface::pubkey::Pubkey;
use std::fs::{create_dir_all, remove_dir_all, File, OpenOptions};
use std::io::prelude::*;
use std::io::{self, BufReader, BufWriter, Seek, SeekFrom};
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use transaction::Transaction;
use window::WINDOW_SIZE;

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
    index: BufReader<File>,
    data: BufReader<File>,
}

pub const LEDGER_DATA_FILE: &str = "data";
const LEDGER_INDEX_FILE: &str = "index";

// use a CONST because there's a cast, and we don't want "sizeof::<u64> as u64"...
const SIZEOF_U64: u64 = size_of::<u64>() as u64;

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
fn err_bincode_to_io(e: Box<bincode::ErrorKind>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

fn read_entry<A: Read>(file: &mut A, len: u64) -> io::Result<Entry> {
    deserialize_from(file.take(len)).map_err(err_bincode_to_io)
}

fn entry_at<A: Read + Seek>(file: &mut A, at: u64) -> io::Result<Entry> {
    let len = u64_at(file, at)?;

    read_entry(file, len)
}

fn next_entry<A: Read>(file: &mut A) -> io::Result<Entry> {
    let len = deserialize_from(file.take(SIZEOF_U64)).map_err(err_bincode_to_io)?;
    read_entry(file, len)
}

fn u64_at<A: Read + Seek>(file: &mut A, at: u64) -> io::Result<u64> {
    file.seek(SeekFrom::Start(at))?;
    deserialize_from(file.take(SIZEOF_U64)).map_err(err_bincode_to_io)
}

impl LedgerWindow {
    // opens a Ledger in directory, provides "infinite" window
    //
    pub fn open(ledger_path: &str) -> io::Result<Self> {
        let ledger_path = Path::new(&ledger_path);

        let index = File::open(ledger_path.join(LEDGER_INDEX_FILE))?;
        let index = BufReader::with_capacity((WINDOW_SIZE * SIZEOF_U64) as usize, index);
        let data = File::open(ledger_path.join(LEDGER_DATA_FILE))?;
        let data = BufReader::with_capacity(WINDOW_SIZE as usize * BLOB_DATA_SIZE, data);

        Ok(LedgerWindow { index, data })
    }

    pub fn get_entry(&mut self, index: u64) -> io::Result<Entry> {
        let offset = self.get_entry_offset(index)?;
        entry_at(&mut self.data, offset)
    }

    // Fill 'buf' with num_entries or most number of whole entries that fit into buf.len()
    //
    // Return tuple of (number of entries read, total size of entries read)
    pub fn get_entries_bytes(
        &mut self,
        start_index: u64,
        num_entries: u64,
        buf: &mut [u8],
    ) -> io::Result<(u64, u64)> {
        let start_offset = self.get_entry_offset(start_index)?;
        let mut total_entries = 0;
        let mut end_offset = 0;
        for i in 0..num_entries {
            let offset = self.get_entry_offset(start_index + i)?;
            let len = u64_at(&mut self.data, offset)?;
            let cur_end_offset = offset + len + SIZEOF_U64;
            if (cur_end_offset - start_offset) > buf.len() as u64 {
                break;
            }
            end_offset = cur_end_offset;
            total_entries += 1;
        }

        if total_entries == 0 {
            return Ok((0, 0));
        }

        let read_len = end_offset - start_offset;
        self.data.seek(SeekFrom::Start(start_offset))?;
        let fread_len = self.data.read(&mut buf[..read_len as usize])? as u64;
        if fread_len != read_len {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "entry read_len({}) doesn't match expected ({})",
                    fread_len, read_len
                ),
            ));
        }
        Ok((total_entries, read_len))
    }

    fn get_entry_offset(&mut self, index: u64) -> io::Result<u64> {
        u64_at(&mut self.index, index * SIZEOF_U64)
    }
}

pub fn verify_ledger(ledger_path: &str) -> io::Result<()> {
    let ledger_path = Path::new(&ledger_path);

    let index = File::open(ledger_path.join(LEDGER_INDEX_FILE))?;

    let index_len = index.metadata()?.len();

    if index_len % SIZEOF_U64 != 0 {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("index is not a multiple of {} bytes long", SIZEOF_U64),
        ))?;
    }
    let mut index = BufReader::with_capacity((WINDOW_SIZE * SIZEOF_U64) as usize, index);

    let data = File::open(ledger_path.join(LEDGER_DATA_FILE))?;
    let mut data = BufReader::with_capacity(WINDOW_SIZE as usize * BLOB_DATA_SIZE, data);

    let mut last_data_offset = 0;
    let mut index_offset = 0;
    let mut data_read = 0;
    let mut last_len = 0;
    let mut i = 0;

    while index_offset < index_len {
        let data_offset = u64_at(&mut index, index_offset)?;

        if last_data_offset + last_len != data_offset {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "at entry[{}], a gap or an overlap last_offset {} offset {} last_len {}",
                    i, last_data_offset, data_offset, last_len
                ),
            ))?;
        }

        match entry_at(&mut data, data_offset) {
            Err(e) => Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "entry[{}] deserialize() failed at offset {}, err: {}",
                    index_offset / SIZEOF_U64,
                    data_offset,
                    e.to_string(),
                ),
            ))?,
            Ok(entry) => {
                last_len = serialized_size(&entry).map_err(err_bincode_to_io)? + SIZEOF_U64
            }
        }

        last_data_offset = data_offset;
        data_read += last_len;
        index_offset += SIZEOF_U64;
        i += 1;
    }
    let data = data.into_inner();
    if data_read != data.metadata()?.len() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "garbage on end of data file",
        ))?;
    }
    Ok(())
}

fn recover_ledger(ledger_path: &str) -> io::Result<()> {
    let ledger_path = Path::new(ledger_path);
    let mut index = OpenOptions::new()
        .write(true)
        .read(true)
        .open(ledger_path.join(LEDGER_INDEX_FILE))?;

    let mut data = OpenOptions::new()
        .write(true)
        .read(true)
        .open(ledger_path.join(LEDGER_DATA_FILE))?;

    // first, truncate to a multiple of SIZEOF_U64
    let len = index.metadata()?.len();

    if len % SIZEOF_U64 != 0 {
        trace!("recover: trimming index len to {}", len - len % SIZEOF_U64);
        index.set_len(len - (len % SIZEOF_U64))?;
    }

    // next, pull index offsets off one at a time until the last one points
    //   to a valid entry deserialization offset...
    loop {
        let len = index.metadata()?.len();
        trace!("recover: index len:{}", len);

        // should never happen
        if len < SIZEOF_U64 {
            trace!("recover: error index len {} too small", len);

            Err(io::Error::new(io::ErrorKind::Other, "empty ledger index"))?;
        }

        let offset = u64_at(&mut index, len - SIZEOF_U64)?;
        trace!("recover: offset[{}]: {}", (len / SIZEOF_U64) - 1, offset);

        match entry_at(&mut data, offset) {
            Ok(entry) => {
                trace!("recover: entry[{}]: {:?}", (len / SIZEOF_U64) - 1, entry);

                let entry_len = serialized_size(&entry).map_err(err_bincode_to_io)?;

                trace!("recover: entry_len: {}", entry_len);

                // now trim data file to size...
                data.set_len(offset + SIZEOF_U64 + entry_len)?;

                trace!(
                    "recover: trimmed data file to {}",
                    offset + SIZEOF_U64 + entry_len
                );

                break; // all good
            }
            Err(_err) => {
                trace!(
                    "recover: no entry recovered at {} {}",
                    offset,
                    _err.to_string()
                );
                index.set_len(len - SIZEOF_U64)?;
            }
        }
    }
    if log_enabled!(Trace) {
        let num_entries = index.metadata()?.len() / SIZEOF_U64;
        trace!("recover: done. {} entries", num_entries);
    }

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
    index: BufWriter<File>,
    data: BufWriter<File>,
}

impl LedgerWriter {
    // recover and open the ledger for writing
    pub fn recover(ledger_path: &str) -> io::Result<Self> {
        recover_ledger(ledger_path)?;
        LedgerWriter::open(ledger_path, false)
    }

    // opens or creates a LedgerWriter in ledger_path directory
    pub fn open(ledger_path: &str, create: bool) -> io::Result<Self> {
        let ledger_path = Path::new(&ledger_path);

        if create {
            let _ignored = remove_dir_all(ledger_path);
            create_dir_all(ledger_path)?;
        }

        let index = OpenOptions::new()
            .create(create)
            .append(true)
            .open(ledger_path.join(LEDGER_INDEX_FILE))?;

        if log_enabled!(Trace) {
            let len = index.metadata()?.len();
            trace!("LedgerWriter::new: index fp:{}", len);
        }
        let index = BufWriter::new(index);

        let data = OpenOptions::new()
            .create(create)
            .append(true)
            .open(ledger_path.join(LEDGER_DATA_FILE))?;

        if log_enabled!(Trace) {
            let len = data.metadata()?.len();
            trace!("LedgerWriter::new: data fp:{}", len);
        }
        let data = BufWriter::new(data);

        Ok(LedgerWriter { index, data })
    }

    pub fn write_entry_noflush(&mut self, entry: &Entry) -> io::Result<()> {
        let len = serialized_size(&entry).map_err(err_bincode_to_io)?;

        serialize_into(&mut self.data, &len).map_err(err_bincode_to_io)?;
        if log_enabled!(Trace) {
            let offset = self.data.seek(SeekFrom::Current(0))?;
            trace!("write_entry: after len data fp:{}", offset);
        }

        serialize_into(&mut self.data, &entry).map_err(err_bincode_to_io)?;
        if log_enabled!(Trace) {
            let offset = self.data.seek(SeekFrom::Current(0))?;
            trace!("write_entry: after entry data fp:{}", offset);
        }

        let offset = self.data.seek(SeekFrom::Current(0))? - len - SIZEOF_U64;
        trace!("write_entry: offset:{} len:{}", offset, len);

        serialize_into(&mut self.index, &offset).map_err(err_bincode_to_io)?;

        if log_enabled!(Trace) {
            let offset = self.index.seek(SeekFrom::Current(0))?;
            trace!("write_entry: end index fp:{}", offset);
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.index.flush()?;
        self.data.flush()?;
        Ok(())
    }

    pub fn write_entry(&mut self, entry: &Entry) -> io::Result<()> {
        self.write_entry_noflush(&entry)?;
        self.flush()
    }

    pub fn write_entries<I>(&mut self, entries: I) -> io::Result<()>
    where
        I: IntoIterator<Item = Entry>,
    {
        for entry in entries {
            self.write_entry_noflush(&entry)?;
        }
        self.flush()
    }
}

#[derive(Debug)]
pub struct LedgerReader {
    data: BufReader<File>,
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
pub fn read_ledger(
    ledger_path: &str,
    recover: bool,
) -> io::Result<impl Iterator<Item = io::Result<Entry>>> {
    if recover {
        recover_ledger(ledger_path)?;
    }

    let ledger_path = Path::new(&ledger_path);
    let data = File::open(ledger_path.join(LEDGER_DATA_FILE))?;
    let data = BufReader::new(data);

    Ok(LedgerReader { data })
}

// a Block is a slice of Entries
pub trait Block {
    /// Verifies the hashes and counts of a slice of transactions are all consistent.
    fn verify(&self, start_hash: &Hash) -> bool;
    fn to_blobs(&self) -> Vec<SharedBlob>;
    fn to_blobs_with_id(&self, id: Pubkey, start_id: u64, addr: &SocketAddr) -> Vec<SharedBlob>;
    fn votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl Block for [Entry] {
    fn verify(&self, start_hash: &Hash) -> bool {
        let genesis = [Entry::new_tick(0, start_hash)];
        let entry_pairs = genesis.par_iter().chain(self).zip(self);
        entry_pairs.all(|(x0, x1)| {
            let r = x1.verify(&x0.id);
            if !r {
                warn!(
                    "entry invalid!: x0: {:?}, x1: {:?} num txs: {}",
                    x0.id,
                    x1.id,
                    x1.transactions.len()
                );
            }
            r
        })
    }

    fn to_blobs_with_id(&self, id: Pubkey, start_idx: u64, addr: &SocketAddr) -> Vec<SharedBlob> {
        self.iter()
            .enumerate()
            .map(|(i, entry)| entry.to_blob(Some(start_idx + i as u64), Some(id), Some(&addr)))
            .collect()
    }

    fn to_blobs(&self) -> Vec<SharedBlob> {
        let default_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        self.to_blobs_with_id(Pubkey::default(), 0, &default_addr)
    }

    fn votes(&self) -> Vec<(Pubkey, Vote, Hash)> {
        self.iter()
            .flat_map(|entry| {
                entry
                    .transactions
                    .iter()
                    .filter_map(BudgetTransaction::vote)
            }).collect()
    }
}

pub fn reconstruct_entries_from_blobs(blobs: Vec<SharedBlob>) -> Result<Vec<Entry>> {
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
/// updates start_hash to id of last Entry, sets num_hashes to 0
pub fn next_entries_mut(
    start_hash: &mut Hash,
    num_hashes: &mut u64,
    transactions: Vec<Transaction>,
) -> Vec<Entry> {
    // TODO: ?? find a number that works better than |?
    //                                               V
    if transactions.is_empty() || transactions.len() == 1 {
        vec![Entry::new_mut(start_hash, num_hashes, transactions)]
    } else {
        let mut chunk_start = 0;
        let mut entries = Vec::new();

        while chunk_start < transactions.len() {
            let mut chunk_end = transactions.len();
            let mut upper = chunk_end;
            let mut lower = chunk_start;
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
                if Entry::serialized_size(&transactions[chunk_start..chunk_end])
                    <= BLOB_DATA_SIZE as u64
                {
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
                num_hashes,
                transactions[chunk_start..chunk_end].to_vec(),
            ));
            chunk_start = chunk_end;
        }

        entries
    }
}

/// Creates the next Entries for given transactions
pub fn next_entries(
    start_hash: &Hash,
    num_hashes: u64,
    transactions: Vec<Transaction>,
) -> Vec<Entry> {
    let mut id = *start_hash;
    let mut num_hashes = num_hashes;
    next_entries_mut(&mut id, &mut num_hashes, transactions)
}

pub fn get_tmp_ledger_path(name: &str) -> String {
    use std::env;
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let keypair = Keypair::new();

    let path = format!("{}/tmp/ledger-{}-{}", out_dir, name, keypair.pubkey());

    // whack any possible collision
    let _ignored = remove_dir_all(&path);

    path
}

pub fn create_tmp_ledger_with_mint(name: &str, mint: &Mint) -> String {
    let path = get_tmp_ledger_path(name);

    let mut writer = LedgerWriter::open(&path, true).unwrap();
    writer.write_entries(mint.create_entries()).unwrap();

    path
}

pub fn create_tmp_genesis(name: &str, num: i64) -> (Mint, String) {
    let mint = Mint::new(num);
    let path = create_tmp_ledger_with_mint(name, &mint);

    (mint, path)
}

pub fn create_ticks(num_ticks: usize, mut hash: Hash) -> Vec<Entry> {
    let mut ticks = Vec::with_capacity(num_ticks as usize);
    for _ in 0..num_ticks {
        let new_tick = Entry::new(&hash, 1, vec![]);
        hash = new_tick.id;
        ticks.push(new_tick);
    }

    ticks
}

pub fn create_tmp_sample_ledger(
    name: &str,
    num_tokens: i64,
    num_ending_ticks: usize,
) -> (Mint, String, Vec<Entry>) {
    let mint = Mint::new(num_tokens);
    let path = get_tmp_ledger_path(name);

    // Create the entries
    let mut genesis = mint.create_entries();
    let ticks = create_ticks(num_ending_ticks, mint.last_id());
    genesis.extend(ticks);

    let mut writer = LedgerWriter::open(&path, true).unwrap();
    writer.write_entries(genesis.clone()).unwrap();

    (mint, path, genesis)
}

#[cfg(test)]
pub fn make_tiny_test_entries(num: usize) -> Vec<Entry> {
    let zero = Hash::default();
    let one = hash(&zero.as_ref());
    let keypair = Keypair::new();

    let mut id = one;
    let mut num_hashes = 0;
    (0..num)
        .map(|_| {
            Entry::new_mut(
                &mut id,
                &mut num_hashes,
                vec![Transaction::budget_new_timestamp(
                    &keypair,
                    keypair.pubkey(),
                    keypair.pubkey(),
                    Utc::now(),
                    one,
                )],
            )
        }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialized_size;
    use budget_instruction::Vote;
    use budget_transaction::BudgetTransaction;
    use entry::{next_entry, Entry};
    use hash::hash;
    use packet::{to_blobs, BLOB_DATA_SIZE, PACKET_DATA_SIZE};
    use signature::{Keypair, KeypairUtil};
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use transaction::Transaction;

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

    fn make_test_entries() -> Vec<Entry> {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let keypair = Keypair::new();
        let tx0 = Transaction::budget_new_vote(
            &keypair,
            Vote {
                version: 0,
                contact_info_version: 1,
            },
            one,
            1,
        );
        let tx1 = Transaction::budget_new_timestamp(
            &keypair,
            keypair.pubkey(),
            keypair.pubkey(),
            Utc::now(),
            one,
        );
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

        let blob_q = entries.to_blobs();

        assert_eq!(reconstruct_entries_from_blobs(blob_q).unwrap(), entries);
    }

    #[test]
    fn test_bad_blobs_attack() {
        use logger;
        logger::setup();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        let blobs_q = to_blobs(vec![(0, addr)]).unwrap(); // <-- attack!
        assert!(reconstruct_entries_from_blobs(blobs_q).is_err());
    }

    #[test]
    fn test_next_entries() {
        use logger;
        logger::setup();
        let id = Hash::default();
        let next_id = hash(&id.as_ref());
        let keypair = Keypair::new();
        let tx_small = Transaction::budget_new_vote(
            &keypair,
            Vote {
                version: 0,
                contact_info_version: 2,
            },
            next_id,
            2,
        );
        let tx_large = Transaction::budget_new(&keypair, keypair.pubkey(), 1, next_id);

        let tx_small_size = serialized_size(&tx_small).unwrap() as usize;
        let tx_large_size = serialized_size(&tx_large).unwrap() as usize;
        let entry_size = serialized_size(&Entry {
            num_hashes: 0,
            id: Hash::default(),
            transactions: vec![],
        }).unwrap() as usize;
        assert!(tx_small_size < tx_large_size);
        assert!(tx_large_size < PACKET_DATA_SIZE);

        let threshold = (BLOB_DATA_SIZE - entry_size) / tx_small_size;

        // verify no split
        let transactions = vec![tx_small.clone(); threshold];
        let entries0 = next_entries(&id, 0, transactions.clone());
        assert_eq!(entries0.len(), 1);
        assert!(entries0.verify(&id));

        // verify the split with uniform transactions
        let transactions = vec![tx_small.clone(); threshold * 2];
        let entries0 = next_entries(&id, 0, transactions.clone());
        assert_eq!(entries0.len(), 2);
        assert!(entries0.verify(&id));

        // verify the split with small transactions followed by large
        // transactions
        let mut transactions = vec![tx_small.clone(); BLOB_DATA_SIZE / tx_small_size];
        let large_transactions = vec![tx_large.clone(); BLOB_DATA_SIZE / tx_large_size];

        transactions.extend(large_transactions);

        let entries0 = next_entries(&id, 0, transactions.clone());
        assert!(entries0.len() >= 2);
        assert!(entries0.verify(&id));
    }

    #[test]
    fn test_ledger_reader_writer() {
        use logger;
        logger::setup();
        let ledger_path = get_tmp_ledger_path("test_ledger_reader_writer");
        let entries = make_tiny_test_entries(10);

        {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(entries.clone()).unwrap();
            // drops writer, flushes buffers
        }
        verify_ledger(&ledger_path).unwrap();

        let mut read_entries = vec![];
        for x in read_ledger(&ledger_path, true).unwrap() {
            let entry = x.unwrap();
            trace!("entry... {:?}", entry);
            read_entries.push(entry);
        }
        assert_eq!(read_entries, entries);

        let mut window = LedgerWindow::open(&ledger_path).unwrap();

        for (i, entry) in entries.iter().enumerate() {
            let read_entry = window.get_entry(i as u64).unwrap();
            assert_eq!(*entry, read_entry);
        }
        assert!(window.get_entry(100).is_err());

        std::fs::remove_file(Path::new(&ledger_path).join(LEDGER_DATA_FILE)).unwrap();
        // empty data file should fall over
        assert!(LedgerWindow::open(&ledger_path).is_err());
        assert!(read_ledger(&ledger_path, false).is_err());

        std::fs::remove_dir_all(ledger_path).unwrap();
    }

    fn truncated_last_entry(ledger_path: &str, entries: Vec<Entry>) {
        let len = {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(entries).unwrap();
            writer.data.seek(SeekFrom::Current(0)).unwrap()
        };
        verify_ledger(&ledger_path).unwrap();

        let data = OpenOptions::new()
            .write(true)
            .open(Path::new(&ledger_path).join(LEDGER_DATA_FILE))
            .unwrap();
        data.set_len(len - 4).unwrap();
    }

    fn garbage_on_data(ledger_path: &str, entries: Vec<Entry>) {
        let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
        writer.write_entries(entries).unwrap();
        writer.data.write_all(b"hi there!").unwrap();
    }

    fn read_ledger_check(ledger_path: &str, entries: Vec<Entry>, len: usize) {
        let read_entries = read_ledger(&ledger_path, true).unwrap();
        let mut i = 0;

        for entry in read_entries {
            assert_eq!(entry.unwrap(), entries[i]);
            i += 1;
        }
        assert_eq!(i, len);
    }

    fn ledger_window_check(ledger_path: &str, entries: Vec<Entry>, len: usize) {
        let mut window = LedgerWindow::open(&ledger_path).unwrap();
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
        let ledger_path = get_tmp_ledger_path("test_recover_ledger");

        // truncate data file, tests recover inside read_ledger_check()
        truncated_last_entry(&ledger_path, entries.clone());
        read_ledger_check(&ledger_path, entries.clone(), entries.len() - 1);

        // truncate data file, tests recover inside LedgerWindow::new()
        truncated_last_entry(&ledger_path, entries.clone());
        ledger_window_check(&ledger_path, entries.clone(), entries.len() - 1);

        // restore last entry, tests recover_ledger() inside LedgerWriter::new()
        truncated_last_entry(&ledger_path, entries.clone());
        // verify should fail at first
        assert!(verify_ledger(&ledger_path).is_err());
        {
            let mut writer = LedgerWriter::recover(&ledger_path).unwrap();
            writer.write_entry(&entries[entries.len() - 1]).unwrap();
        }
        // and be fine after recover()
        verify_ledger(&ledger_path).unwrap();

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
        assert!(verify_ledger(&ledger_path).is_err());
        {
            let mut writer = LedgerWriter::recover(&ledger_path).unwrap();
            writer.write_entry(&entries[entries.len() - 1]).unwrap();
        }
        verify_ledger(&ledger_path).unwrap();
        read_ledger_check(&ledger_path, entries.clone(), entries.len());
        ledger_window_check(&ledger_path, entries.clone(), entries.len());
        let _ignored = remove_dir_all(&ledger_path);
    }

    #[test]
    fn test_verify_ledger() {
        use logger;
        logger::setup();

        let entries = make_tiny_test_entries(10);
        let ledger_path = get_tmp_ledger_path("test_verify_ledger");
        {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(entries.clone()).unwrap();
        }
        // TODO more cases that make ledger_verify() fail
        // assert!(verify_ledger(&ledger_path).is_err());

        assert!(verify_ledger(&ledger_path).is_ok());
        let _ignored = remove_dir_all(&ledger_path);
    }

    #[test]
    fn test_get_entries_bytes() {
        use logger;
        logger::setup();
        let entries = make_tiny_test_entries(10);
        let ledger_path = get_tmp_ledger_path("test_raw_entries");
        {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(entries.clone()).unwrap();
        }

        let mut window = LedgerWindow::open(&ledger_path).unwrap();
        let mut buf = [0; 1024];
        let (num_entries, bytes) = window.get_entries_bytes(0, 1, &mut buf).unwrap();
        let bytes = bytes as usize;
        assert_eq!(num_entries, 1);
        let entry: Entry = deserialize(&buf[size_of::<u64>()..bytes]).unwrap();
        assert_eq!(entry, entries[0]);

        let (num_entries, bytes2) = window.get_entries_bytes(0, 2, &mut buf).unwrap();
        let bytes2 = bytes2 as usize;
        assert_eq!(num_entries, 2);
        assert!(bytes2 > bytes);
        for (i, ref entry) in entries.iter().enumerate() {
            info!("entry[{}] = {:?}", i, entry.id);
        }

        let entry: Entry = deserialize(&buf[size_of::<u64>()..bytes]).unwrap();
        assert_eq!(entry, entries[0]);

        let entry: Entry = deserialize(&buf[bytes + size_of::<u64>()..bytes2]).unwrap();
        assert_eq!(entry, entries[1]);

        // buf size part-way into entry[1], should just return entry[0]
        let mut buf = vec![0; bytes + size_of::<u64>() + 1];
        let (num_entries, bytes3) = window.get_entries_bytes(0, 2, &mut buf).unwrap();
        assert_eq!(num_entries, 1);
        let bytes3 = bytes3 as usize;
        assert_eq!(bytes3, bytes);

        let mut buf = vec![0; bytes2 - 1];
        let (num_entries, bytes4) = window.get_entries_bytes(0, 2, &mut buf).unwrap();
        assert_eq!(num_entries, 1);
        let bytes4 = bytes4 as usize;
        assert_eq!(bytes4, bytes);

        let mut buf = vec![0; bytes + size_of::<u64>() - 1];
        let (num_entries, bytes5) = window.get_entries_bytes(0, 2, &mut buf).unwrap();
        assert_eq!(num_entries, 1);
        let bytes5 = bytes5 as usize;
        assert_eq!(bytes5, bytes);

        let mut buf = vec![0; bytes * 2];
        let (num_entries, bytes6) = window.get_entries_bytes(9, 1, &mut buf).unwrap();
        assert_eq!(num_entries, 1);
        let bytes6 = bytes6 as usize;

        let entry: Entry = deserialize(&buf[size_of::<u64>()..bytes6]).unwrap();
        assert_eq!(entry, entries[9]);

        // Read out of range
        assert!(window.get_entries_bytes(20, 2, &mut buf).is_err());

        let _ignored = remove_dir_all(&ledger_path);
    }
}
