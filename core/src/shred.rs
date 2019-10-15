//! The `shred` module defines data structures and methods to pull MTU sized data frames from the network.
use crate::blocktree::BlocktreeError;
use crate::entry::create_ticks;
use crate::entry::Entry;
use crate::erasure::Session;
use crate::result;
use crate::result::Error;
use bincode::serialized_size;
use core::cell::RefCell;
use lazy_static::lazy_static;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator};
use rayon::slice::ParallelSlice;
use rayon::ThreadPool;
use serde::{Deserialize, Serialize};
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::hash::Hash;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use std::io;
use std::io::{Error as IOError, ErrorKind};
use std::sync::Arc;
use std::time::Instant;

lazy_static! {
    pub static ref SIZE_OF_CODING_SHRED_HEADER: usize =
        { serialized_size(&CodingShredHeader::default()).unwrap() as usize };
    pub static ref SIZE_OF_DATA_SHRED_HEADER: usize =
        { serialized_size(&DataShredHeader::default()).unwrap() as usize };
    pub static ref SIZE_OF_SHRED_HEADER: usize =
        { serialized_size(&ShredHeader::default()).unwrap() as usize };
    static ref SIZE_OF_SIGNATURE: usize =
        { bincode::serialized_size(&Signature::default()).unwrap() as usize };
    pub static ref SIZE_OF_SHRED_TYPE: usize =
        { bincode::serialized_size(&ShredType(DATA_SHRED)).unwrap() as usize };
}

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .build()
                    .unwrap()));

/// The constants that define if a shred is data or coding
pub const DATA_SHRED: u8 = 0b1010_0101;
pub const CODING_SHRED: u8 = 0b0101_1010;

/// This limit comes from reed solomon library, but unfortunately they don't have
/// a public constant defined for it.
pub const MAX_DATA_SHREDS_PER_FEC_BLOCK: u32 = 16;

/// Based on rse benchmarks, the optimal erasure config uses 16 data shreds and 4 coding shreds
pub const RECOMMENDED_FEC_RATE: f32 = 0.25;

const LAST_SHRED_IN_SLOT: u8 = 0b0000_0001;
const DATA_COMPLETE_SHRED: u8 = 0b0000_0010;

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct ShredType(u8);

/// A common header that is present in data and code shred headers
#[derive(Serialize, Clone, Deserialize, Default, PartialEq, Debug)]
pub struct ShredCommonHeader {
    pub signature: Signature,
    pub slot: u64,
    pub index: u32,
}

/// The data shred header has parent offset and flags
#[derive(Serialize, Clone, Default, Deserialize, PartialEq, Debug)]
pub struct DataShredHeader {
    pub common_header: ShredCommonHeader,
    pub parent_offset: u16,
    pub flags: u8,
}

/// The coding shred header has FEC information
#[derive(Serialize, Clone, Default, Deserialize, PartialEq, Debug)]
pub struct CodingShredHeader {
    pub common_header: ShredCommonHeader,
    pub num_data_shreds: u16,
    pub num_coding_shreds: u16,
    pub position: u16,
}

/// A common header that is present at start of every shred
#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct ShredHeader {
    pub shred_type: ShredType,
    pub coding_header: CodingShredHeader,
    pub data_header: DataShredHeader,
}

impl Default for ShredHeader {
    fn default() -> Self {
        ShredHeader {
            shred_type: ShredType(DATA_SHRED),
            coding_header: CodingShredHeader::default(),
            data_header: DataShredHeader::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shred {
    pub headers: ShredHeader,
    pub payload: Vec<u8>,
}

impl Shred {
    fn new(header: ShredHeader, shred_buf: Vec<u8>) -> Self {
        Shred {
            headers: header,
            payload: shred_buf,
        }
    }

    pub fn new_from_data(
        slot: u64,
        index: u32,
        parent_offset: u16,
        data: Option<&[u8]>,
        is_last_data: bool,
        is_last_in_slot: bool,
    ) -> Self {
        let mut shred_buf = vec![0; PACKET_DATA_SIZE];
        let mut header = ShredHeader::default();
        header.data_header.common_header.slot = slot;
        header.data_header.common_header.index = index;
        header.data_header.parent_offset = parent_offset;
        header.data_header.flags = 0;

        if is_last_data {
            header.data_header.flags |= DATA_COMPLETE_SHRED
        }

        if is_last_in_slot {
            header.data_header.flags |= LAST_SHRED_IN_SLOT
        }

        if let Some(data) = data {
            bincode::serialize_into(&mut shred_buf[..*SIZE_OF_SHRED_HEADER], &header)
                .expect("Failed to write header into shred buffer");
            shred_buf[*SIZE_OF_SHRED_HEADER..*SIZE_OF_SHRED_HEADER + data.len()]
                .clone_from_slice(data);
        }

        Self::new(header, shred_buf)
    }

    pub fn new_from_serialized_shred(shred_buf: Vec<u8>) -> result::Result<Self> {
        let shred_type: ShredType = bincode::deserialize(&shred_buf[..*SIZE_OF_SHRED_TYPE])?;
        let mut header = if shred_type == ShredType(CODING_SHRED) {
            let start = *SIZE_OF_SHRED_TYPE;
            let end = start + *SIZE_OF_CODING_SHRED_HEADER;
            let mut header = ShredHeader::default();
            header.coding_header = bincode::deserialize(&shred_buf[start..end])?;
            header
        } else if shred_type == ShredType(DATA_SHRED) {
            let start = *SIZE_OF_CODING_SHRED_HEADER + *SIZE_OF_SHRED_TYPE;
            let end = start + *SIZE_OF_DATA_SHRED_HEADER;
            let mut header = ShredHeader::default();
            header.data_header = bincode::deserialize(&shred_buf[start..end])?;
            header
        } else {
            return Err(Error::BlocktreeError(BlocktreeError::InvalidShredData(
                Box::new(bincode::ErrorKind::Custom("Invalid shred type".to_string())),
            )));
        };
        header.shred_type = shred_type;

        Ok(Self::new(header, shred_buf))
    }

    pub fn new_empty_from_header(headers: ShredHeader) -> Self {
        let mut payload = vec![0; PACKET_DATA_SIZE];
        let mut wr = io::Cursor::new(&mut payload[..*SIZE_OF_SHRED_HEADER]);
        bincode::serialize_into(&mut wr, &headers).expect("Failed to serialize shred");
        Shred { headers, payload }
    }

    pub fn new_empty_data_shred() -> Self {
        let mut payload = vec![0; PACKET_DATA_SIZE];
        payload[0] = DATA_SHRED;
        let headers = ShredHeader::default();
        Shred { headers, payload }
    }

    pub fn header(&self) -> &ShredCommonHeader {
        if self.is_data() {
            &self.headers.data_header.common_header
        } else {
            &self.headers.coding_header.common_header
        }
    }

    pub fn header_mut(&mut self) -> &mut ShredCommonHeader {
        if self.is_data() {
            &mut self.headers.data_header.common_header
        } else {
            &mut self.headers.coding_header.common_header
        }
    }

    pub fn slot(&self) -> u64 {
        self.header().slot
    }

    pub fn parent(&self) -> u64 {
        if self.is_data() {
            self.headers.data_header.common_header.slot
                - u64::from(self.headers.data_header.parent_offset)
        } else {
            std::u64::MAX
        }
    }

    pub fn index(&self) -> u32 {
        self.header().index
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_index(&mut self, index: u32) {
        self.header_mut().index = index
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_slot(&mut self, slot: u64) {
        self.header_mut().slot = slot
    }

    pub fn signature(&self) -> Signature {
        self.header().signature
    }

    pub fn seed(&self) -> [u8; 32] {
        let mut seed = [0; 32];
        let seed_len = seed.len();
        let sig = self.header().signature.as_ref();
        seed[0..seed_len].copy_from_slice(&sig[(sig.len() - seed_len)..]);
        seed
    }

    pub fn is_data(&self) -> bool {
        self.headers.shred_type == ShredType(DATA_SHRED)
    }

    pub fn last_in_slot(&self) -> bool {
        if self.is_data() {
            self.headers.data_header.flags & LAST_SHRED_IN_SLOT == LAST_SHRED_IN_SLOT
        } else {
            false
        }
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_last_in_slot(&mut self) {
        if self.is_data() {
            self.headers.data_header.flags |= LAST_SHRED_IN_SLOT
        }
    }

    pub fn data_complete(&self) -> bool {
        if self.is_data() {
            self.headers.data_header.flags & DATA_COMPLETE_SHRED == DATA_COMPLETE_SHRED
        } else {
            false
        }
    }

    pub fn coding_params(&self) -> Option<(u16, u16, u16)> {
        if !self.is_data() {
            let header = &self.headers.coding_header;
            Some((
                header.num_data_shreds,
                header.num_coding_shreds,
                header.position,
            ))
        } else {
            None
        }
    }

    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        let signed_payload_offset = if self.is_data() {
            *SIZE_OF_CODING_SHRED_HEADER + *SIZE_OF_SHRED_TYPE
        } else {
            *SIZE_OF_SHRED_TYPE
        } + *SIZE_OF_SIGNATURE;
        self.signature()
            .verify(pubkey.as_ref(), &self.payload[signed_payload_offset..])
    }
}

#[derive(Debug)]
pub struct Shredder {
    slot: u64,
    parent_slot: u64,
    fec_rate: f32,
    keypair: Arc<Keypair>,
    pub signing_coding_time: u128,
}

impl Shredder {
    pub fn new(
        slot: u64,
        parent_slot: u64,
        fec_rate: f32,
        keypair: Arc<Keypair>,
    ) -> result::Result<Self> {
        if fec_rate > 1.0 || fec_rate < 0.0 {
            Err(Error::IO(IOError::new(
                ErrorKind::Other,
                format!(
                    "FEC rate {:?} must be more than 0.0 and less than 1.0",
                    fec_rate
                ),
            )))
        } else if slot < parent_slot || slot - parent_slot > u64::from(std::u16::MAX) {
            Err(Error::IO(IOError::new(
                ErrorKind::Other,
                format!(
                    "Current slot {:?} must be > Parent slot {:?}, but the difference must not be > {:?}",
                    slot, parent_slot, std::u16::MAX
                ),
            )))
        } else {
            Ok(Shredder {
                slot,
                parent_slot,
                fec_rate,
                keypair,
                signing_coding_time: 0,
            })
        }
    }

    pub fn entries_to_shreds(
        &self,
        entries: &[Entry],
        is_last_in_slot: bool,
        next_shred_index: u32,
    ) -> (Vec<Shred>, Vec<Shred>, u32) {
        let now = Instant::now();
        let serialized_shreds =
            bincode::serialize(entries).expect("Expect to serialize all entries");
        let serialize_time = now.elapsed().as_millis();

        let no_header_size = PACKET_DATA_SIZE - *SIZE_OF_SHRED_HEADER;
        let num_shreds = (serialized_shreds.len() + no_header_size - 1) / no_header_size;
        let last_shred_index = next_shred_index + num_shreds as u32 - 1;

        // 1) Generate data shreds
        let data_shreds: Vec<Shred> = PAR_THREAD_POOL.with(|thread_pool| {
            thread_pool.borrow().install(|| {
                serialized_shreds
                    .par_chunks(no_header_size)
                    .enumerate()
                    .map(|(i, shred_data)| {
                        let shred_index = next_shred_index + i as u32;

                        let (is_last_data, is_last_in_slot) = {
                            if shred_index == last_shred_index {
                                (true, is_last_in_slot)
                            } else {
                                (false, false)
                            }
                        };

                        let mut shred = Shred::new_from_data(
                            self.slot,
                            shred_index,
                            (self.slot - self.parent_slot) as u16,
                            Some(shred_data),
                            is_last_data,
                            is_last_in_slot,
                        );

                        Shredder::sign_shred(
                            &self.keypair,
                            &mut shred,
                            *SIZE_OF_CODING_SHRED_HEADER + *SIZE_OF_SHRED_TYPE,
                        );
                        shred
                    })
                    .collect()
            })
        });

        // 2) Generate coding shreds
        let mut coding_shreds: Vec<_> = PAR_THREAD_POOL.with(|thread_pool| {
            thread_pool.borrow().install(|| {
                data_shreds
                    .par_chunks(MAX_DATA_SHREDS_PER_FEC_BLOCK as usize)
                    .flat_map(|shred_data_batch| {
                        Shredder::generate_coding_shreds(self.slot, self.fec_rate, shred_data_batch)
                    })
                    .collect()
            })
        });

        // 3) Sign coding shreds
        PAR_THREAD_POOL.with(|thread_pool| {
            thread_pool.borrow().install(|| {
                coding_shreds.par_iter_mut().for_each(|mut coding_shred| {
                    Shredder::sign_shred(&self.keypair, &mut coding_shred, *SIZE_OF_SHRED_TYPE);
                })
            })
        });

        let elapsed = now.elapsed().as_millis();

        datapoint_debug!(
            "shredding-stats",
            ("slot", self.slot as i64, i64),
            ("num_data_shreds", data_shreds.len() as i64, i64),
            ("num_coding_shreds", coding_shreds.len() as i64, i64),
            ("signing_coding", (elapsed - serialize_time) as i64, i64),
            ("serialzing", serialize_time as i64, i64),
        );

        (data_shreds, coding_shreds, last_shred_index + 1)
    }

    pub fn sign_shred(signer: &Arc<Keypair>, shred_info: &mut Shred, signature_offset: usize) {
        let data_offset = signature_offset + *SIZE_OF_SIGNATURE;
        let signature = signer.sign_message(&shred_info.payload[data_offset..]);
        let serialized_signature =
            bincode::serialize(&signature).expect("Failed to generate serialized signature");
        shred_info.payload[signature_offset..signature_offset + serialized_signature.len()]
            .copy_from_slice(&serialized_signature);
        shred_info.header_mut().signature = signature;
    }

    pub fn new_coding_shred_header(
        slot: u64,
        index: u32,
        num_data: usize,
        num_code: usize,
        position: usize,
    ) -> ShredHeader {
        let mut header = ShredHeader::default();
        header.shred_type = ShredType(CODING_SHRED);
        header.coding_header.common_header.index = index;
        header.coding_header.common_header.slot = slot;
        header.coding_header.num_coding_shreds = num_code as u16;
        header.coding_header.num_data_shreds = num_data as u16;
        header.coding_header.position = position as u16;
        header
    }

    /// Generates coding shreds for the data shreds in the current FEC set
    pub fn generate_coding_shreds(
        slot: u64,
        fec_rate: f32,
        data_shred_batch: &[Shred],
    ) -> Vec<Shred> {
        assert!(!data_shred_batch.is_empty());
        if fec_rate != 0.0 {
            let num_data = data_shred_batch.len();
            // always generate at least 1 coding shred even if the fec_rate doesn't allow it
            let num_coding = Self::calculate_num_coding_shreds(num_data as f32, fec_rate);
            let session =
                Session::new(num_data, num_coding).expect("Failed to create erasure session");
            let start_index = data_shred_batch[0].header().index;

            // All information after coding shred field in a data shred is encoded
            let coding_block_offset = *SIZE_OF_CODING_SHRED_HEADER + *SIZE_OF_SHRED_TYPE;
            let data_ptrs: Vec<_> = data_shred_batch
                .iter()
                .map(|data| &data.payload[coding_block_offset..])
                .collect();

            // Create empty coding shreds, with correctly populated headers
            let mut coding_shreds = Vec::with_capacity(num_coding);
            (0..num_coding).for_each(|i| {
                let header = Self::new_coding_shred_header(
                    slot,
                    start_index + i as u32,
                    num_data,
                    num_coding,
                    i,
                );
                let shred = Shred::new_empty_from_header(header);
                coding_shreds.push(shred.payload);
            });

            // Grab pointers for the coding blocks
            let mut coding_ptrs: Vec<_> = coding_shreds
                .iter_mut()
                .map(|buffer| &mut buffer[coding_block_offset..])
                .collect();

            // Create coding blocks
            session
                .encode(&data_ptrs, coding_ptrs.as_mut_slice())
                .expect("Failed in erasure encode");

            // append to the shred list
            coding_shreds
                .into_iter()
                .enumerate()
                .map(|(i, code)| {
                    let header = Self::new_coding_shred_header(
                        slot,
                        start_index + i as u32,
                        num_data,
                        num_coding,
                        i,
                    );
                    Shred::new(header, code)
                })
                .collect()
        } else {
            vec![]
        }
    }

    fn calculate_num_coding_shreds(num_data_shreds: f32, fec_rate: f32) -> usize {
        1.max((fec_rate * num_data_shreds) as usize)
    }

    fn fill_in_missing_shreds(
        num_data: usize,
        num_coding: usize,
        first_index_in_fec_set: usize,
        expected_index: usize,
        index_found: usize,
        present: &mut [bool],
    ) -> Vec<Vec<u8>> {
        let end_index = index_found.saturating_sub(1);
        // The index of current shred must be within the range of shreds that are being
        // recovered
        if !(first_index_in_fec_set..first_index_in_fec_set + num_data + num_coding)
            .contains(&end_index)
        {
            return vec![];
        }

        let missing_blocks: Vec<Vec<u8>> = (expected_index..index_found)
            .map(|missing| {
                present[missing.saturating_sub(first_index_in_fec_set)] = false;
                if missing < first_index_in_fec_set + num_data {
                    Shred::new_empty_data_shred().payload
                } else {
                    vec![0; PACKET_DATA_SIZE]
                }
            })
            .collect();
        missing_blocks
    }

    pub fn try_recovery(
        shreds: Vec<Shred>,
        num_data: usize,
        num_coding: usize,
        first_index: usize,
        slot: u64,
    ) -> Result<Vec<Shred>, reed_solomon_erasure::Error> {
        let mut recovered_data = vec![];
        let fec_set_size = num_data + num_coding;

        if num_coding > 0 && shreds.len() < fec_set_size {
            let coding_block_offset = *SIZE_OF_CODING_SHRED_HEADER + *SIZE_OF_SHRED_TYPE;

            // Let's try recovering missing shreds using erasure
            let mut present = &mut vec![true; fec_set_size];
            let mut next_expected_index = first_index;
            let mut shred_bufs: Vec<Vec<u8>> = shreds
                .into_iter()
                .flat_map(|shred| {
                    let index = Self::get_shred_index(&shred, num_data);
                    let mut blocks = Self::fill_in_missing_shreds(
                        num_data,
                        num_coding,
                        first_index,
                        next_expected_index,
                        index,
                        &mut present,
                    );
                    blocks.push(shred.payload);
                    next_expected_index = index + 1;
                    blocks
                })
                .collect();

            // Insert any other missing shreds after the last shred we have received in the
            // current FEC block
            let mut pending_shreds = Self::fill_in_missing_shreds(
                num_data,
                num_coding,
                first_index,
                next_expected_index,
                first_index + fec_set_size,
                &mut present,
            );

            shred_bufs.append(&mut pending_shreds);

            if shred_bufs.len() != fec_set_size {
                return Err(reed_solomon_erasure::Error::TooFewShardsPresent);
            }

            let session = Session::new(num_data, num_coding).unwrap();

            let mut blocks: Vec<(&mut [u8], bool)> = shred_bufs
                .iter_mut()
                .map(|x| x[coding_block_offset..].as_mut())
                .zip(present.clone())
                .collect();
            session.decode_blocks(&mut blocks)?;

            let mut num_drained = 0;
            present
                .iter()
                .enumerate()
                .for_each(|(position, was_present)| {
                    if !*was_present && position < num_data {
                        let drain_this = position - num_drained;
                        let shred_buf = shred_bufs.remove(drain_this);
                        num_drained += 1;
                        if let Ok(shred) = Shred::new_from_serialized_shred(shred_buf) {
                            let shred_index = shred.index() as usize;
                            // Valid shred must be in the same slot as the original shreds
                            if shred.slot() == slot {
                                // A valid data shred must be indexed between first_index and first+num_data index
                                if (first_index..first_index + num_data).contains(&shred_index) {
                                    recovered_data.push(shred)
                                }
                            }
                        }
                    }
                });
        }

        Ok(recovered_data)
    }

    /// Combines all shreds to recreate the original buffer
    pub fn deshred(shreds: &[Shred]) -> Result<Vec<u8>, reed_solomon_erasure::Error> {
        let num_data = shreds.len();
        let data_shred_bufs = {
            let first_index = shreds.first().unwrap().index() as usize;
            let last_shred = shreds.last().unwrap();
            let last_index = if last_shred.data_complete() || last_shred.last_in_slot() {
                last_shred.index() as usize
            } else {
                0
            };

            if num_data.saturating_add(first_index) != last_index.saturating_add(1) {
                return Err(reed_solomon_erasure::Error::TooFewDataShards);
            }

            shreds.iter().map(|shred| &shred.payload).collect()
        };

        Ok(Self::reassemble_payload(num_data, data_shred_bufs))
    }

    fn get_shred_index(shred: &Shred, num_data: usize) -> usize {
        if shred.is_data() {
            shred.index() as usize
        } else {
            shred.index() as usize + num_data
        }
    }

    fn reassemble_payload(num_data: usize, data_shred_bufs: Vec<&Vec<u8>>) -> Vec<u8> {
        data_shred_bufs[..num_data]
            .iter()
            .flat_map(|data| {
                let offset = *SIZE_OF_SHRED_HEADER;
                data[offset as usize..].iter()
            })
            .cloned()
            .collect()
    }
}

pub fn max_ticks_per_n_shreds(num_shreds: u64) -> u64 {
    let ticks = create_ticks(1, Hash::default());
    max_entries_per_n_shred(&ticks[0], num_shreds)
}

pub fn max_entries_per_n_shred(entry: &Entry, num_shreds: u64) -> u64 {
    let shred_data_size = (PACKET_DATA_SIZE - *SIZE_OF_SHRED_HEADER) as u64;
    let vec_size = bincode::serialized_size(&vec![entry]).unwrap();
    let entry_size = bincode::serialized_size(entry).unwrap();
    let count_size = vec_size - entry_size;

    (shred_data_size * num_shreds - count_size) / entry_size
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_sdk::system_transaction;
    use std::collections::HashSet;
    use std::convert::TryInto;

    fn verify_test_data_shred(
        shred: &Shred,
        index: u32,
        slot: u64,
        parent: u64,
        pk: &Pubkey,
        verify: bool,
        is_last_in_slot: bool,
        is_last_in_fec_set: bool,
    ) {
        assert_eq!(shred.payload.len(), PACKET_DATA_SIZE);
        assert!(shred.is_data());
        assert_eq!(shred.index(), index);
        assert_eq!(shred.slot(), slot);
        assert_eq!(shred.parent(), parent);
        assert_eq!(verify, shred.verify(pk));
        if is_last_in_slot {
            assert!(shred.last_in_slot());
        } else {
            assert!(!shred.last_in_slot());
        }
        if is_last_in_fec_set {
            assert!(shred.data_complete());
        } else {
            assert!(!shred.data_complete());
        }
    }

    fn verify_test_code_shred(shred: &Shred, index: u32, slot: u64, pk: &Pubkey, verify: bool) {
        assert_eq!(shred.payload.len(), PACKET_DATA_SIZE);
        assert!(!shred.is_data());
        assert_eq!(shred.index(), index);
        assert_eq!(shred.slot(), slot);
        assert_eq!(verify, shred.verify(pk));
    }

    #[test]
    fn test_data_shredder() {
        let keypair = Arc::new(Keypair::new());
        let slot = 0x123456789abcdef0;

        // Test that parent cannot be > current slot
        assert_matches!(
            Shredder::new(slot, slot + 1, 1.001, keypair.clone()),
            Err(_)
        );
        // Test that slot - parent cannot be > u16 MAX
        assert_matches!(
            Shredder::new(slot, slot - 1 - 0xffff, 1.001, keypair.clone()),
            Err(_)
        );

        let fec_rate = 0.25;
        let parent_slot = slot - 5;
        let shredder = Shredder::new(slot, parent_slot, fec_rate, keypair.clone())
            .expect("Failed in creating shredder");

        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let size = serialized_size(&entries).unwrap();
        let no_header_size = (PACKET_DATA_SIZE - *SIZE_OF_SHRED_HEADER) as u64;
        let num_expected_data_shreds = (size + no_header_size - 1) / no_header_size;
        let num_expected_coding_shreds =
            Shredder::calculate_num_coding_shreds(num_expected_data_shreds as f32, fec_rate);

        let start_index = 0;
        let (data_shreds, coding_shreds, next_index) =
            shredder.entries_to_shreds(&entries, true, start_index);
        assert_eq!(next_index as u64, num_expected_data_shreds);

        let mut data_shred_indexes = HashSet::new();
        let mut coding_shred_indexes = HashSet::new();
        for shred in data_shreds.iter() {
            assert_eq!(shred.headers.shred_type, ShredType(DATA_SHRED));
            let index = shred.headers.data_header.common_header.index;
            let is_last = index as u64 == num_expected_data_shreds - 1;
            verify_test_data_shred(
                shred,
                index,
                slot,
                parent_slot,
                &keypair.pubkey(),
                true,
                is_last,
                is_last,
            );
            assert!(!data_shred_indexes.contains(&index));
            data_shred_indexes.insert(index);
        }

        for shred in coding_shreds.iter() {
            let index = shred.headers.data_header.common_header.index;
            assert_eq!(shred.headers.shred_type, ShredType(CODING_SHRED));
            verify_test_code_shred(shred, index, slot, &keypair.pubkey(), true);
            assert!(!coding_shred_indexes.contains(&index));
            coding_shred_indexes.insert(index);
        }

        for i in start_index..start_index + num_expected_data_shreds as u32 {
            assert!(data_shred_indexes.contains(&i));
        }

        for i in start_index..start_index + num_expected_coding_shreds as u32 {
            assert!(coding_shred_indexes.contains(&i));
        }

        assert_eq!(data_shred_indexes.len() as u64, num_expected_data_shreds);
        assert_eq!(coding_shred_indexes.len(), num_expected_coding_shreds);

        // Test reassembly
        let deshred_payload = Shredder::deshred(&data_shreds).unwrap();
        let deshred_entries: Vec<Entry> = bincode::deserialize(&deshred_payload).unwrap();
        assert_eq!(entries, deshred_entries);
    }

    #[test]
    fn test_deserialize_shred_payload() {
        let keypair = Arc::new(Keypair::new());
        let slot = 1;

        let parent_slot = 0;
        let shredder = Shredder::new(slot, parent_slot, 0.0, keypair.clone())
            .expect("Failed in creating shredder");

        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let data_shreds = shredder.entries_to_shreds(&entries, true, 0).0;

        let deserialized_shred =
            Shred::new_from_serialized_shred(data_shreds.last().unwrap().payload.clone()).unwrap();
        assert_eq!(deserialized_shred, *data_shreds.last().unwrap());
    }

    #[test]
    fn test_data_and_code_shredder() {
        let keypair = Arc::new(Keypair::new());

        let slot = 0x123456789abcdef0;
        // Test that FEC rate cannot be > 1.0
        assert_matches!(
            Shredder::new(slot, slot - 5, 1.001, keypair.clone()),
            Err(_)
        );

        let shredder = Shredder::new(0x123456789abcdef0, slot - 5, 1.0, keypair.clone())
            .expect("Failed in creating shredder");

        // Create enough entries to make > 1 shred
        let num_entries = max_ticks_per_n_shreds(1) + 1;
        let entries: Vec<_> = (0..num_entries)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(&entries, true, 0);

        // Must have created an equal number of coding and data shreds
        assert_eq!(data_shreds.len(), coding_shreds.len());

        for (i, s) in data_shreds.iter().enumerate() {
            verify_test_data_shred(
                s,
                s.index(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                i == data_shreds.len() - 1,
                i == data_shreds.len() - 1,
            );
        }

        for s in coding_shreds {
            verify_test_code_shred(&s, s.index(), slot, &keypair.pubkey(), true);
        }
    }

    #[test]
    fn test_recovery_and_reassembly() {
        let keypair = Arc::new(Keypair::new());
        let slot = 0x123456789abcdef0;
        let shredder = Shredder::new(slot, slot - 5, 1.0, keypair.clone())
            .expect("Failed in creating shredder");

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
        let entry = Entry::new(&Hash::default(), 1, vec![tx0]);

        let num_data_shreds: usize = 5;
        let num_entries = max_entries_per_n_shred(&entry, num_data_shreds as u64);
        let entries: Vec<_> = (0..num_entries)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let serialized_entries = bincode::serialize(&entries).unwrap();
        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(&entries, true, 0);

        // We should have 10 shreds now, an equal number of coding shreds
        assert_eq!(data_shreds.len(), num_data_shreds);
        assert_eq!(coding_shreds.len(), num_data_shreds);

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();

        // Test0: Try recovery/reassembly with only data shreds, but not all data shreds. Hint: should fail
        assert_matches!(
            Shredder::try_recovery(
                data_shreds[..data_shreds.len() - 1].to_vec(),
                num_data_shreds,
                num_data_shreds,
                0,
                slot
            ),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );

        // Test1: Try recovery/reassembly with only data shreds. Hint: should work
        let recovered_data = Shredder::try_recovery(
            data_shreds[..].to_vec(),
            num_data_shreds,
            num_data_shreds,
            0,
            slot,
        )
        .unwrap();
        assert!(recovered_data.is_empty());

        // Test2: Try recovery/reassembly with missing data shreds + coding shreds. Hint: should work
        let mut shred_info: Vec<Shred> = all_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 == 0 { Some(b.clone()) } else { None })
            .collect();

        let mut recovered_data = Shredder::try_recovery(
            shred_info.clone(),
            num_data_shreds,
            num_data_shreds,
            0,
            slot,
        )
        .unwrap();

        assert_eq!(recovered_data.len(), 2); // Data shreds 1 and 3 were missing
        let recovered_shred = recovered_data.remove(0);
        verify_test_data_shred(
            &recovered_shred,
            1,
            slot,
            slot - 5,
            &keypair.pubkey(),
            true,
            false,
            false,
        );
        shred_info.insert(1, recovered_shred);

        let recovered_shred = recovered_data.remove(0);
        verify_test_data_shred(
            &recovered_shred,
            3,
            slot,
            slot - 5,
            &keypair.pubkey(),
            true,
            false,
            false,
        );
        shred_info.insert(3, recovered_shred);

        let result = Shredder::deshred(&shred_info[..num_data_shreds]).unwrap();
        assert!(result.len() >= serialized_entries.len());
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);

        // Test3: Try recovery/reassembly with 3 missing data shreds + 2 coding shreds. Hint: should work
        let mut shred_info: Vec<Shred> = all_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let recovered_data = Shredder::try_recovery(
            shred_info.clone(),
            num_data_shreds,
            num_data_shreds,
            0,
            slot,
        )
        .unwrap();

        assert_eq!(recovered_data.len(), 3); // Data shreds 0, 2, 4 were missing
        for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
            let index = i * 2;
            verify_test_data_shred(
                &recovered_shred,
                index.try_into().unwrap(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                recovered_shred.index() as usize == num_data_shreds - 1,
                recovered_shred.index() as usize == num_data_shreds - 1,
            );

            shred_info.insert(i * 2, recovered_shred);
        }

        let result = Shredder::deshred(&shred_info[..num_data_shreds]).unwrap();
        assert!(result.len() >= serialized_entries.len());
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);

        // Test4: Try reassembly with 2 missing data shreds, but keeping the last
        // data shred. Hint: should fail
        let shreds: Vec<Shred> = all_shreds[..num_data_shreds]
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if (i < 4 && i % 2 != 0) || i == num_data_shreds - 1 {
                    // Keep 1, 3, 4
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(shreds.len(), 3);
        assert_matches!(
            Shredder::deshred(&shreds),
            Err(reed_solomon_erasure::Error::TooFewDataShards)
        );

        // Test5: Try recovery/reassembly with non zero index full slot with 3 missing data shreds
        // and 2 missing coding shreds. Hint: should work
        let serialized_entries = bincode::serialize(&entries).unwrap();
        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(&entries, true, 25);

        // We should have 10 shreds now, an equal number of coding shreds
        assert_eq!(data_shreds.len(), num_data_shreds);
        assert_eq!(coding_shreds.len(), num_data_shreds);

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();

        let mut shred_info: Vec<Shred> = all_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let recovered_data = Shredder::try_recovery(
            shred_info.clone(),
            num_data_shreds,
            num_data_shreds,
            25,
            slot,
        )
        .unwrap();

        assert_eq!(recovered_data.len(), 3); // Data shreds 25, 27, 29 were missing
        for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
            let index = 25 + (i * 2);
            verify_test_data_shred(
                &recovered_shred,
                index.try_into().unwrap(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                index == 25 + num_data_shreds - 1,
                index == 25 + num_data_shreds - 1,
            );

            shred_info.insert(i * 2, recovered_shred);
        }

        let result = Shredder::deshred(&shred_info[..num_data_shreds]).unwrap();
        assert!(result.len() >= serialized_entries.len());
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);

        // Test6: Try recovery/reassembly with incorrect slot. Hint: does not recover any shreds
        let recovered_data = Shredder::try_recovery(
            shred_info.clone(),
            num_data_shreds,
            num_data_shreds,
            25,
            slot + 1,
        )
        .unwrap();
        assert!(recovered_data.is_empty());

        // Test7: Try recovery/reassembly with incorrect index. Hint: does not recover any shreds
        assert_matches!(
            Shredder::try_recovery(
                shred_info.clone(),
                num_data_shreds,
                num_data_shreds,
                15,
                slot,
            ),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );

        // Test8: Try recovery/reassembly with incorrect index. Hint: does not recover any shreds
        assert_matches!(
            Shredder::try_recovery(shred_info, num_data_shreds, num_data_shreds, 35, slot,),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );
    }

    #[test]
    fn test_multi_fec_block_coding() {
        let keypair = Arc::new(Keypair::new());
        let slot = 0x123456789abcdef0;
        let shredder = Shredder::new(slot, slot - 5, 1.0, keypair.clone())
            .expect("Failed in creating shredder");

        let num_fec_sets = 100;
        let num_data_shreds = (MAX_DATA_SHREDS_PER_FEC_BLOCK * num_fec_sets) as usize;
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
        let entry = Entry::new(&Hash::default(), 1, vec![tx0]);
        let num_entries = max_entries_per_n_shred(&entry, num_data_shreds as u64);

        let entries: Vec<_> = (0..num_entries)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let serialized_entries = bincode::serialize(&entries).unwrap();
        let (data_shreds, coding_shreds, next_index) =
            shredder.entries_to_shreds(&entries, true, 0);
        assert_eq!(next_index as usize, num_data_shreds);
        assert_eq!(data_shreds.len(), num_data_shreds);
        assert_eq!(coding_shreds.len(), num_data_shreds);

        for c in &coding_shreds {
            assert!(!c.is_data());
        }

        let mut all_shreds = vec![];
        for i in 0..num_fec_sets {
            let shred_start_index = (MAX_DATA_SHREDS_PER_FEC_BLOCK * i) as usize;
            let end_index = shred_start_index + MAX_DATA_SHREDS_PER_FEC_BLOCK as usize - 1;
            let fec_set_shreds = data_shreds[shred_start_index..=end_index]
                .iter()
                .cloned()
                .chain(coding_shreds[shred_start_index..=end_index].iter().cloned())
                .collect::<Vec<_>>();

            let mut shred_info: Vec<Shred> = fec_set_shreds
                .iter()
                .enumerate()
                .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
                .collect();

            let recovered_data = Shredder::try_recovery(
                shred_info.clone(),
                MAX_DATA_SHREDS_PER_FEC_BLOCK as usize,
                MAX_DATA_SHREDS_PER_FEC_BLOCK as usize,
                shred_start_index,
                slot,
            )
            .unwrap();

            for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
                let index = shred_start_index + (i * 2);
                verify_test_data_shred(
                    &recovered_shred,
                    index.try_into().unwrap(),
                    slot,
                    slot - 5,
                    &keypair.pubkey(),
                    true,
                    index == end_index,
                    index == end_index,
                );

                shred_info.insert(i * 2, recovered_shred);
            }

            all_shreds.extend(
                shred_info
                    .into_iter()
                    .take(MAX_DATA_SHREDS_PER_FEC_BLOCK as usize),
            );
        }

        let result = Shredder::deshred(&all_shreds[..]).unwrap();
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);
    }
}
