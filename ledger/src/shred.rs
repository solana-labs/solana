//! The `shred` module defines data structures and methods to pull MTU sized data frames from the network.
use crate::{
    entry::{create_ticks, Entry},
    erasure::Session,
};
use core::cell::RefCell;
use rayon::{
    iter::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator},
    slice::ParallelSlice,
    ThreadPool,
};
use serde::{Deserialize, Serialize};
use solana_metrics::datapoint_debug;
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    packet::PACKET_DATA_SIZE,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil, Signature},
};
use std::{sync::Arc, time::Instant};

/// The following constants are computed by hand, and hardcoded.
/// `test_shred_constants` ensures that the values are correct.
/// Constants are used over lazy_static for performance reasons.
pub const SIZE_OF_COMMON_SHRED_HEADER: usize = 77;
pub const SIZE_OF_DATA_SHRED_HEADER: usize = 3;
pub const SIZE_OF_CODING_SHRED_HEADER: usize = 6;
pub const SIZE_OF_SIGNATURE: usize = 64;
pub const SIZE_OF_DATA_SHRED_IGNORED_TAIL: usize =
    SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_CODING_SHRED_HEADER;
pub const SIZE_OF_DATA_SHRED_PAYLOAD: usize = PACKET_DATA_SIZE
    - SIZE_OF_COMMON_SHRED_HEADER
    - SIZE_OF_DATA_SHRED_HEADER
    - SIZE_OF_DATA_SHRED_IGNORED_TAIL;

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
pub const DATA_COMPLETE_SHRED: u8 = 0b0000_0010;

#[derive(Debug)]
pub enum ShredError {
    InvalidShredType,
    InvalidFecRate(f32), // FEC rate must be more than 0.0 and less than 1.0
    SlotTooLow { slot: Slot, parent_slot: Slot }, // "Current slot must be > Parent slot, but the difference must not be > u16::MAX
    Serialize(std::boxed::Box<bincode::ErrorKind>),
}

pub type Result<T> = std::result::Result<T, ShredError>;

impl std::convert::From<std::boxed::Box<bincode::ErrorKind>> for ShredError {
    fn from(e: std::boxed::Box<bincode::ErrorKind>) -> ShredError {
        ShredError::Serialize(e)
    }
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct ShredType(pub u8);
impl Default for ShredType {
    fn default() -> Self {
        ShredType(DATA_SHRED)
    }
}

/// A common header that is present in data and code shred headers
#[derive(Serialize, Clone, Deserialize, Default, PartialEq, Debug)]
pub struct ShredCommonHeader {
    pub signature: Signature,
    pub shred_type: ShredType,
    pub slot: Slot,
    pub index: u32,
}

/// The data shred header has parent offset and flags
#[derive(Serialize, Clone, Default, Deserialize, PartialEq, Debug)]
pub struct DataShredHeader {
    pub parent_offset: u16,
    pub flags: u8,
}

/// The coding shred header has FEC information
#[derive(Serialize, Clone, Default, Deserialize, PartialEq, Debug)]
pub struct CodingShredHeader {
    pub num_data_shreds: u16,
    pub num_coding_shreds: u16,
    pub position: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shred {
    pub common_header: ShredCommonHeader,
    pub data_header: DataShredHeader,
    pub coding_header: CodingShredHeader,
    pub payload: Vec<u8>,
}

impl Shred {
    fn deserialize_obj<'de, T>(index: &mut usize, size: usize, buf: &'de [u8]) -> bincode::Result<T>
    where
        T: Deserialize<'de>,
    {
        let ret = bincode::config()
            .limit(PACKET_DATA_SIZE as u64)
            .deserialize(&buf[*index..*index + size])?;
        *index += size;
        Ok(ret)
    }

    fn serialize_obj_into<'de, T>(
        index: &mut usize,
        size: usize,
        buf: &'de mut [u8],
        obj: &T,
    ) -> bincode::Result<()>
    where
        T: Serialize,
    {
        bincode::serialize_into(&mut buf[*index..*index + size], obj)?;
        *index += size;
        Ok(())
    }

    pub fn new_from_data(
        slot: Slot,
        index: u32,
        parent_offset: u16,
        data: Option<&[u8]>,
        is_last_data: bool,
        is_last_in_slot: bool,
    ) -> Self {
        let mut payload = vec![0; PACKET_DATA_SIZE];
        let mut common_header = ShredCommonHeader::default();
        common_header.slot = slot;
        common_header.index = index;

        let mut data_header = DataShredHeader::default();
        data_header.parent_offset = parent_offset;

        if is_last_data {
            data_header.flags |= DATA_COMPLETE_SHRED
        }

        if is_last_in_slot {
            data_header.flags |= LAST_SHRED_IN_SLOT
        }

        if let Some(data) = data {
            let mut start = 0;
            Self::serialize_obj_into(
                &mut start,
                SIZE_OF_COMMON_SHRED_HEADER,
                &mut payload,
                &common_header,
            )
            .expect("Failed to write header into shred buffer");
            Self::serialize_obj_into(
                &mut start,
                SIZE_OF_DATA_SHRED_HEADER,
                &mut payload,
                &data_header,
            )
            .expect("Failed to write data header into shred buffer");
            payload[start..start + data.len()].clone_from_slice(data);
        }

        Self {
            common_header,
            data_header,
            coding_header: CodingShredHeader::default(),
            payload,
        }
    }

    pub fn new_from_serialized_shred(payload: Vec<u8>) -> Result<Self> {
        let mut start = 0;
        let common_header: ShredCommonHeader =
            Self::deserialize_obj(&mut start, SIZE_OF_COMMON_SHRED_HEADER, &payload)?;

        let shred = if common_header.shred_type == ShredType(CODING_SHRED) {
            let coding_header: CodingShredHeader =
                Self::deserialize_obj(&mut start, SIZE_OF_CODING_SHRED_HEADER, &payload)?;
            Self {
                common_header,
                data_header: DataShredHeader::default(),
                coding_header,
                payload,
            }
        } else if common_header.shred_type == ShredType(DATA_SHRED) {
            let data_header: DataShredHeader =
                Self::deserialize_obj(&mut start, SIZE_OF_DATA_SHRED_HEADER, &payload)?;
            Self {
                common_header,
                data_header,
                coding_header: CodingShredHeader::default(),
                payload,
            }
        } else {
            return Err(ShredError::InvalidShredType);
        };

        Ok(shred)
    }

    pub fn new_empty_from_header(
        common_header: ShredCommonHeader,
        data_header: DataShredHeader,
        coding_header: CodingShredHeader,
    ) -> Self {
        let mut payload = vec![0; PACKET_DATA_SIZE];
        let mut start = 0;
        Self::serialize_obj_into(
            &mut start,
            SIZE_OF_COMMON_SHRED_HEADER,
            &mut payload,
            &common_header,
        )
        .expect("Failed to write header into shred buffer");
        if common_header.shred_type == ShredType(DATA_SHRED) {
            Self::serialize_obj_into(
                &mut start,
                SIZE_OF_DATA_SHRED_HEADER,
                &mut payload,
                &data_header,
            )
            .expect("Failed to write data header into shred buffer");
        } else if common_header.shred_type == ShredType(CODING_SHRED) {
            Self::serialize_obj_into(
                &mut start,
                SIZE_OF_CODING_SHRED_HEADER,
                &mut payload,
                &coding_header,
            )
            .expect("Failed to write data header into shred buffer");
        }
        Shred {
            common_header,
            data_header,
            coding_header,
            payload,
        }
    }

    pub fn new_empty_data_shred() -> Self {
        Self::new_empty_from_header(
            ShredCommonHeader::default(),
            DataShredHeader::default(),
            CodingShredHeader::default(),
        )
    }

    pub fn slot(&self) -> Slot {
        self.common_header.slot
    }

    pub fn parent(&self) -> Slot {
        if self.is_data() {
            self.common_header.slot - u64::from(self.data_header.parent_offset)
        } else {
            std::u64::MAX
        }
    }

    pub fn index(&self) -> u32 {
        self.common_header.index
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_index(&mut self, index: u32) {
        self.common_header.index = index
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_slot(&mut self, slot: Slot) {
        self.common_header.slot = slot
    }

    pub fn signature(&self) -> Signature {
        self.common_header.signature
    }

    pub fn seed(&self) -> [u8; 32] {
        let mut seed = [0; 32];
        let seed_len = seed.len();
        let sig = self.common_header.signature.as_ref();
        seed[0..seed_len].copy_from_slice(&sig[(sig.len() - seed_len)..]);
        seed
    }

    pub fn is_data(&self) -> bool {
        self.common_header.shred_type == ShredType(DATA_SHRED)
    }
    pub fn is_code(&self) -> bool {
        self.common_header.shred_type == ShredType(CODING_SHRED)
    }

    pub fn last_in_slot(&self) -> bool {
        if self.is_data() {
            self.data_header.flags & LAST_SHRED_IN_SLOT == LAST_SHRED_IN_SLOT
        } else {
            false
        }
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_last_in_slot(&mut self) {
        if self.is_data() {
            self.data_header.flags |= LAST_SHRED_IN_SLOT
        }
    }

    pub fn data_complete(&self) -> bool {
        if self.is_data() {
            self.data_header.flags & DATA_COMPLETE_SHRED == DATA_COMPLETE_SHRED
        } else {
            false
        }
    }

    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        self.signature()
            .verify(pubkey.as_ref(), &self.payload[SIZE_OF_SIGNATURE..])
    }
}

#[derive(Debug)]
pub struct Shredder {
    slot: Slot,
    parent_slot: Slot,
    fec_rate: f32,
    keypair: Arc<Keypair>,
    pub signing_coding_time: u128,
}

impl Shredder {
    pub fn new(
        slot: Slot,
        parent_slot: Slot,
        fec_rate: f32,
        keypair: Arc<Keypair>,
    ) -> Result<Self> {
        if fec_rate > 1.0 || fec_rate < 0.0 {
            Err(ShredError::InvalidFecRate(fec_rate))
        } else if slot < parent_slot || slot - parent_slot > u64::from(std::u16::MAX) {
            Err(ShredError::SlotTooLow { slot, parent_slot })
        } else {
            Ok(Self {
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

        let no_header_size = SIZE_OF_DATA_SHRED_PAYLOAD;
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

                        Shredder::sign_shred(&self.keypair, &mut shred);
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
                    Shredder::sign_shred(&self.keypair, &mut coding_shred);
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

    pub fn sign_shred(signer: &Keypair, shred: &mut Shred) {
        let signature = signer.sign_message(&shred.payload[SIZE_OF_SIGNATURE..]);
        bincode::serialize_into(&mut shred.payload[..SIZE_OF_SIGNATURE], &signature)
            .expect("Failed to generate serialized signature");
        shred.common_header.signature = signature;
    }

    pub fn new_coding_shred_header(
        slot: Slot,
        index: u32,
        num_data: usize,
        num_code: usize,
        position: usize,
    ) -> (ShredCommonHeader, CodingShredHeader) {
        let mut header = ShredCommonHeader::default();
        header.shred_type = ShredType(CODING_SHRED);
        header.index = index;
        header.slot = slot;
        (
            header,
            CodingShredHeader {
                num_data_shreds: num_data as u16,
                num_coding_shreds: num_code as u16,
                position: position as u16,
            },
        )
    }

    /// Generates coding shreds for the data shreds in the current FEC set
    pub fn generate_coding_shreds(
        slot: Slot,
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
            let start_index = data_shred_batch[0].common_header.index;

            // All information after coding shred field in a data shred is encoded
            let valid_data_len = PACKET_DATA_SIZE - SIZE_OF_DATA_SHRED_IGNORED_TAIL;
            let data_ptrs: Vec<_> = data_shred_batch
                .iter()
                .map(|data| &data.payload[..valid_data_len])
                .collect();

            // Create empty coding shreds, with correctly populated headers
            let mut coding_shreds = Vec::with_capacity(num_coding);
            (0..num_coding).for_each(|i| {
                let (header, coding_header) = Self::new_coding_shred_header(
                    slot,
                    start_index + i as u32,
                    num_data,
                    num_coding,
                    i,
                );
                let shred =
                    Shred::new_empty_from_header(header, DataShredHeader::default(), coding_header);
                coding_shreds.push(shred.payload);
            });

            // Grab pointers for the coding blocks
            let coding_block_offset = SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_CODING_SHRED_HEADER;
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
                .map(|(i, payload)| {
                    let (common_header, coding_header) = Self::new_coding_shred_header(
                        slot,
                        start_index + i as u32,
                        num_data,
                        num_coding,
                        i,
                    );
                    Shred {
                        common_header,
                        data_header: DataShredHeader::default(),
                        coding_header,
                        payload,
                    }
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
        slot: Slot,
    ) -> std::result::Result<Vec<Shred>, reed_solomon_erasure::Error> {
        let mut recovered_data = vec![];
        let fec_set_size = num_data + num_coding;

        if num_coding > 0 && shreds.len() < fec_set_size {
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

            let valid_data_len = PACKET_DATA_SIZE - SIZE_OF_DATA_SHRED_IGNORED_TAIL;
            let coding_block_offset = SIZE_OF_CODING_SHRED_HEADER + SIZE_OF_COMMON_SHRED_HEADER;
            let mut blocks: Vec<(&mut [u8], bool)> = shred_bufs
                .iter_mut()
                .enumerate()
                .map(|(position, x)| {
                    if position < num_data {
                        x[..valid_data_len].as_mut()
                    } else {
                        x[coding_block_offset..].as_mut()
                    }
                })
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
    pub fn deshred(shreds: &[Shred]) -> std::result::Result<Vec<u8>, reed_solomon_erasure::Error> {
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
        let valid_data_len = PACKET_DATA_SIZE - SIZE_OF_DATA_SHRED_IGNORED_TAIL;
        data_shred_bufs[..num_data]
            .iter()
            .flat_map(|data| {
                let offset = SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_DATA_SHRED_HEADER;
                data[offset..valid_data_len].iter()
            })
            .cloned()
            .collect()
    }
}

pub fn max_ticks_per_n_shreds(num_shreds: u64) -> u64 {
    let ticks = create_ticks(1, 0, Hash::default());
    max_entries_per_n_shred(&ticks[0], num_shreds)
}

pub fn max_entries_per_n_shred(entry: &Entry, num_shreds: u64) -> u64 {
    let shred_data_size = SIZE_OF_DATA_SHRED_PAYLOAD as u64;
    let vec_size = bincode::serialized_size(&vec![entry]).unwrap();
    let entry_size = bincode::serialized_size(entry).unwrap();
    let count_size = vec_size - entry_size;

    (shred_data_size * num_shreds - count_size) / entry_size
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use bincode::serialized_size;
    use matches::assert_matches;
    use solana_sdk::system_transaction;
    use std::collections::HashSet;
    use std::convert::TryInto;

    #[test]
    fn test_shred_constants() {
        assert_eq!(
            SIZE_OF_COMMON_SHRED_HEADER,
            serialized_size(&ShredCommonHeader::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_CODING_SHRED_HEADER,
            serialized_size(&CodingShredHeader::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_DATA_SHRED_HEADER,
            serialized_size(&DataShredHeader::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SIGNATURE,
            bincode::serialized_size(&Signature::default()).unwrap() as usize
        );
    }

    fn verify_test_data_shred(
        shred: &Shred,
        index: u32,
        slot: Slot,
        parent: Slot,
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

    fn verify_test_code_shred(shred: &Shred, index: u32, slot: Slot, pk: &Pubkey, verify: bool) {
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
            Shredder::new(slot, slot + 1, 1.00, keypair.clone()),
            Err(ShredError::SlotTooLow {
                slot: _,
                parent_slot: _,
            })
        );
        // Test that slot - parent cannot be > u16 MAX
        assert_matches!(
            Shredder::new(slot, slot - 1 - 0xffff, 1.00, keypair.clone()),
            Err(ShredError::SlotTooLow {
                slot: _,
                parent_slot: _,
            })
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
        let no_header_size = SIZE_OF_DATA_SHRED_PAYLOAD as u64;
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
            assert_eq!(shred.common_header.shred_type, ShredType(DATA_SHRED));
            let index = shred.common_header.index;
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
            let index = shred.common_header.index;
            assert_eq!(shred.common_header.shred_type, ShredType(CODING_SHRED));
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
            Err(ShredError::InvalidFecRate(_))
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
