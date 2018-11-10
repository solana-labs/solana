//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use bincode::{deserialize, serialize};
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use packet::Blob;
use result::{Error, Result};
use rocksdb::{ColumnFamily, Options, WriteBatch, DB};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;

pub trait LedgerColumnFamily {
    type ValueType: DeserializeOwned + Serialize;

    fn get(&self, db: &DB, key: &[u8]) -> Result<Option<Self::ValueType>> {
        let data_bytes = db.get_cf(self.handle(), key)?;

        if let Some(raw) = data_bytes {
            let result: Self::ValueType = deserialize(&raw)?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn get_bytes(&self, db: &DB, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let data_bytes = db.get_cf(self.handle(), key)?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put_bytes(&self, db: &DB, key: &[u8], serialized_value: &[u8]) -> Result<()> {
        db.put_cf(self.handle(), &key, &serialized_value)?;
        Ok(())
    }

    fn put(&self, db: &DB, key: &[u8], value: &Self::ValueType) -> Result<()> {
        let serialized = serialize(value)?;
        db.put_cf(self.handle(), &key, &serialized)?;
        Ok(())
    }

    fn handle(&self) -> ColumnFamily;
}

#[derive(Debug, Deserialize, Serialize, Eq, PartialEq)]
// The Meta column family
pub struct SlotMeta {
    pub consumed: u64,
    pub received: u64,
}

impl SlotMeta {
    fn new() -> Self {
        SlotMeta {
            consumed: 0,
            received: 0,
        }
    }
}

pub struct MetaCf {
    handle: ColumnFamily,
}

impl MetaCf {
    pub fn new(handle: ColumnFamily) -> Self {
        MetaCf { handle }
    }

    pub fn key(slot_height: u64) -> Vec<u8> {
        let mut key = vec![0u8; 8];
        LittleEndian::write_u64(&mut key[0..8], slot_height);
        key
    }
}

impl LedgerColumnFamily for MetaCf {
    type ValueType = SlotMeta;

    fn handle(&self) -> ColumnFamily {
        self.handle
    }
}

// The data column family
pub struct DataCf {
    handle: ColumnFamily,
}

impl DataCf {
    pub fn new(handle: ColumnFamily) -> Self {
        DataCf { handle }
    }

    pub fn key(slot_height: u64, index: u64) -> Vec<u8> {
        let mut key = vec![0u8; 16];
        LittleEndian::write_u64(&mut key[0..8], slot_height);
        LittleEndian::write_u64(&mut key[8..16], index);
        key
    }

    pub fn slot_height_from_key(key: &[u8]) -> Result<u64> {
        let mut rdr = io::Cursor::new(&key[0..8]);
        let height = rdr.read_u64::<LittleEndian>()?;
        Ok(height)
    }

    pub fn index_from_key(key: &[u8]) -> Result<u64> {
        let mut rdr = io::Cursor::new(&key[8..16]);
        let index = rdr.read_u64::<LittleEndian>()?;
        Ok(index)
    }
}

impl LedgerColumnFamily for DataCf {
    type ValueType = Vec<u8>;

    fn handle(&self) -> ColumnFamily {
        self.handle
    }
}

// The erasure column family
pub struct ErasureCf {
    handle: ColumnFamily,
}

impl ErasureCf {
    pub fn new(handle: ColumnFamily) -> Self {
        ErasureCf { handle }
    }

    pub fn key(slot_height: u64, index: u64) -> Vec<u8> {
        DataCf::key(slot_height, index)
    }
}

impl LedgerColumnFamily for ErasureCf {
    type ValueType = Vec<u8>;

    fn handle(&self) -> ColumnFamily {
        self.handle
    }
}

// ledger window
pub struct DbLedger {
    // Underlying database is automatically closed in the Drop implementation of DB
    db: DB,
    meta_cf: MetaCf,
    data_cf: DataCf,
    #[allow(dead_code)]
    erasure_cf: ErasureCf,
}

// TODO: Once we support a window that knows about different leader
// slots, change functions where this is used to take slot height
// as a variable argument
pub const DEFAULT_SLOT_HEIGHT: u64 = 0;
// Column family for metadata about a leader slot
pub const META_CF: &str = "meta";
// Column family for the data in a leader slot
pub const DATA_CF: &str = "data";
// Column family for erasure data
pub const ERASURE_CF: &str = "erasure";

impl DbLedger {
    // Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(ledger_path: &str) -> Result<Self> {
        // Use default database options
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);

        // Column family names
        let cfs = vec![META_CF, DATA_CF, ERASURE_CF];

        // Open the database
        let db = DB::open_cf(&options, ledger_path, &cfs)?;

        // Create the metadata column family
        let meta_cf_handle = db.cf_handle(META_CF).unwrap();
        let meta_cf = MetaCf::new(meta_cf_handle);

        // Create the data column family
        let data_cf_handle = db.cf_handle(DATA_CF).unwrap();
        let data_cf = DataCf::new(data_cf_handle);

        // Create the erasure column family
        let erasure_cf_handle = db.cf_handle(ERASURE_CF).unwrap();
        let erasure_cf = ErasureCf::new(erasure_cf_handle);

        Ok(DbLedger {
            db,
            meta_cf,
            data_cf,
            erasure_cf,
        })
    }

    pub fn write_blobs<'a, I>(&mut self, start_index: u64, blobs: &'a I) -> Result<()>
    where
        &'a I: IntoIterator<Item = &'a &'a Blob>,
    {
        for (blob, index) in blobs.into_iter().zip(start_index..) {
            let key = DataCf::key(DEFAULT_SLOT_HEIGHT, index);
            let size = blob.get_size()?;
            self.data_cf.put_bytes(&self.db, &key, &blob.data[..size])?;
        }
        Ok(())
    }

    pub fn process_new_blob(&self, key: &[u8], new_blob: &Blob) -> Result<Vec<Vec<u8>>> {
        let slot_height = DataCf::slot_height_from_key(key)?;
        let meta_key = MetaCf::key(slot_height);

        let mut should_write_meta = false;

        let mut meta = {
            if let Some(meta) = self.db.get_cf(self.meta_cf.handle(), &meta_key)? {
                deserialize(&meta)?
            } else {
                should_write_meta = true;
                SlotMeta::new()
            }
        };

        let mut index = DataCf::index_from_key(key)?;
        if index > meta.received {
            meta.received = index;
            should_write_meta = true;
        }

        let mut consumed_queue = vec![];
        let serialized_blob_data = new_blob.data;

        if meta.consumed == index {
            should_write_meta = true;
            meta.consumed += 1;
            consumed_queue.push(new_blob.data.to_vec());
            // Find the next consecutive block of blobs.
            // TODO: account for consecutive blocks that
            // span multiple slots
            loop {
                index += 1;
                let key = DataCf::key(slot_height, index);
                if let Some(blob_data) = self.data_cf.get(&self.db, &key)? {
                    consumed_queue.push(blob_data);
                    meta.consumed += 1;
                } else {
                    break;
                }
            }
        }

        // Atomic write both the metadata and the data
        let mut batch = WriteBatch::default();
        if should_write_meta {
            batch.put_cf(self.meta_cf.handle(), &meta_key, &serialize(&meta)?)?;
        }

        batch.put_cf(self.data_cf.handle(), key, &serialized_blob_data)?;
        self.db.write(batch)?;

        Ok(consumed_queue)
    }

    // Fill 'buf' with num_blobs or most number of consecutive
    // whole blobs that fit into buf.len()
    //
    // Return tuple of (number of blob read, total size of blobs read)
    pub fn get_blob_bytes(
        &mut self,
        start_index: u64,
        num_blobs: u64,
        buf: &mut [u8],
    ) -> Result<(u64, u64)> {
        let start_key = DataCf::key(DEFAULT_SLOT_HEIGHT, start_index);
        let mut db_iterator = self.db.raw_iterator_cf(self.data_cf.handle())?;
        db_iterator.seek(&start_key);
        let mut total_blobs = 0;
        let mut total_current_size = 0;
        for expected_index in start_index..start_index + num_blobs {
            if !db_iterator.valid() {
                if expected_index == start_index {
                    return Err(Error::IO(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Blob at start_index not found",
                    )));
                } else {
                    break;
                }
            }

            // Check key is the next sequential key based on
            // blob index
            let key = &db_iterator.key();

            if key.is_none() {
                break;
            }

            let key = key.as_ref().unwrap();

            let index = DataCf::index_from_key(key)?;

            if index != expected_index {
                break;
            }

            // Get the blob data
            let value = &db_iterator.value();

            if value.is_none() {
                break;
            }

            let value = value.as_ref().unwrap();
            let blob_data_len = value.len();

            if total_current_size + blob_data_len > buf.len() {
                break;
            }

            buf[total_current_size..total_current_size + value.len()].copy_from_slice(value);
            total_current_size += blob_data_len;
            total_blobs += 1;

            // TODO: Change this logic to support looking for data
            // that spans multiple leader slots, once we support
            // a window that knows about different leader slots
            db_iterator.next();
        }

        Ok((total_blobs, total_current_size as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::{get_tmp_ledger_path, make_tiny_test_entries, Block};
    use rocksdb::{Options, DB};

    #[test]
    fn test_put_get_simple() {
        let ledger_path = get_tmp_ledger_path("test_put_get_simple");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Test meta column family
        let meta = SlotMeta::new();
        let meta_key = MetaCf::key(0);
        ledger.meta_cf.put(&ledger.db, &meta_key, &meta).unwrap();
        let result = ledger
            .meta_cf
            .get(&ledger.db, &meta_key)
            .unwrap()
            .expect("Expected meta object to exist");

        assert_eq!(result, meta);

        // Test erasure column family
        let erasure = vec![1u8; 16];
        let erasure_key = ErasureCf::key(DEFAULT_SLOT_HEIGHT, 0);
        ledger
            .erasure_cf
            .put(&ledger.db, &erasure_key, &erasure)
            .unwrap();

        let result = ledger
            .erasure_cf
            .get(&ledger.db, &erasure_key)
            .unwrap()
            .expect("Expected erasure object to exist");

        assert_eq!(result, erasure);

        // Test data column family
        let data = vec![2u8; 16];
        let data_key = DataCf::key(DEFAULT_SLOT_HEIGHT, 0);
        ledger.data_cf.put(&ledger.db, &data_key, &data).unwrap();

        let result = ledger
            .data_cf
            .get(&ledger.db, &data_key)
            .unwrap()
            .expect("Expected data object to exist");

        assert_eq!(result, data);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        let _ignored = DB::destroy(&Options::default(), &ledger_path);
    }

    #[test]
    fn test_get_blobs_bytes() {
        let shared_blobs = make_tiny_test_entries(10).to_blobs();
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_get_blobs_bytes");
        let mut ledger = DbLedger::open(&ledger_path).unwrap();
        ledger.write_blobs(0, &blobs).unwrap();

        let mut buf = [0; 1024];
        let (num_blobs, bytes) = ledger.get_blob_bytes(0, 1, &mut buf).unwrap();
        let bytes = bytes as usize;
        assert_eq!(num_blobs, 1);
        {
            let blob_data = &buf[..bytes];
            assert_eq!(blob_data, &blobs[0].data[..bytes]);
        }

        let (num_blobs, bytes2) = ledger.get_blob_bytes(0, 2, &mut buf).unwrap();
        let bytes2 = bytes2 as usize;
        assert_eq!(num_blobs, 2);
        assert!(bytes2 > bytes);
        {
            let blob_data_1 = &buf[..bytes];
            assert_eq!(blob_data_1, &blobs[0].data[..bytes]);

            let blob_data_2 = &buf[bytes..bytes2];
            assert_eq!(blob_data_2, &blobs[1].data[..bytes2 - bytes]);
        }

        // buf size part-way into blob[1], should just return blob[0]
        let mut buf = vec![0; bytes + 1];
        let (num_blobs, bytes3) = ledger.get_blob_bytes(0, 2, &mut buf).unwrap();
        assert_eq!(num_blobs, 1);
        let bytes3 = bytes3 as usize;
        assert_eq!(bytes3, bytes);

        let mut buf = vec![0; bytes2 - 1];
        let (num_blobs, bytes4) = ledger.get_blob_bytes(0, 2, &mut buf).unwrap();
        assert_eq!(num_blobs, 1);
        let bytes4 = bytes4 as usize;
        assert_eq!(bytes4, bytes);

        let mut buf = vec![0; bytes * 2];
        let (num_blobs, bytes6) = ledger.get_blob_bytes(9, 1, &mut buf).unwrap();
        assert_eq!(num_blobs, 1);
        let bytes6 = bytes6 as usize;

        {
            let blob_data = &buf[..bytes6];
            assert_eq!(blob_data, &blobs[9].data[..bytes6]);
        }

        // Read out of range
        assert!(ledger.get_blob_bytes(20, 2, &mut buf).is_err());

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        let _ignored = DB::destroy(&Options::default(), &ledger_path);
    }

    #[test]
    fn test_process_new_blobs_basic() {
        let shared_blobs = make_tiny_test_entries(2).to_blobs();
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_process_new_blobs_basic");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert second blob, we're misisng the first blob, so should return nothing
        let result = ledger
            .process_new_blob(&DataCf::key(DEFAULT_SLOT_HEIGHT, 1), blobs[1])
            .unwrap();

        assert!(result.len() == 0);
        let meta = ledger
            .meta_cf
            .get(&ledger.db, &MetaCf::key(DEFAULT_SLOT_HEIGHT))
            .unwrap()
            .expect("Expected new metadata object to be created");
        assert!(meta.consumed == 0 && meta.received == 1);

        // Insert first blob, check for consecutive returned blobs
        let result = ledger
            .process_new_blob(&DataCf::key(DEFAULT_SLOT_HEIGHT, 0), blobs[0])
            .unwrap();

        assert!(result.len() == 2);
        let meta = ledger
            .meta_cf
            .get(&ledger.db, &MetaCf::key(DEFAULT_SLOT_HEIGHT))
            .unwrap()
            .expect("Expected new metadata object to exist");
        assert!(meta.consumed == 2 && meta.received == 1);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        let _ignored = DB::destroy(&Options::default(), &ledger_path);
    }

    #[test]
    fn test_process_new_blobs_multiple() {
        let num_blobs = 10;
        let shared_blobs = make_tiny_test_entries(num_blobs).to_blobs();
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_process_new_blobs_multiple");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert first blob, check for consecutive returned blobs
        for i in (0..num_blobs).rev() {
            let result = ledger
                .process_new_blob(&DataCf::key(DEFAULT_SLOT_HEIGHT, i as u64), blobs[0])
                .unwrap();

            let meta = ledger
                .meta_cf
                .get(&ledger.db, &MetaCf::key(DEFAULT_SLOT_HEIGHT))
                .unwrap()
                .expect("Expected metadata object to exist");
            if i != 0 {
                assert_eq!(result.len(), 0);
                assert!(meta.consumed == 0 && meta.received == num_blobs as u64 - 1);
            } else {
                assert_eq!(result.len(), num_blobs);
                assert!(meta.consumed == num_blobs as u64 && meta.received == num_blobs as u64 - 1);
            }
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        let _ignored = DB::destroy(&Options::default(), &ledger_path);
    }
}
