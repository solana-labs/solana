//! The `shred` module defines data structures and methods to pull MTU sized data frames from the network.
use crate::erasure::Session;
use crate::result;
use crate::result::Error;
use bincode::serialized_size;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use std::io::{Error as IOError, ErrorKind, Write};
use std::sync::Arc;
use std::{cmp, io};

lazy_static! {
    static ref SIZE_OF_CODING_SHRED_HEADER: usize =
        { serialized_size(&CodingShredHeader::default()).unwrap() as usize };
    static ref SIZE_OF_DATA_SHRED_HEADER: usize =
        { serialized_size(&DataShredHeader::default()).unwrap() as usize };
    static ref SIZE_OF_EMPTY_CODING_SHRED: usize =
        { serialized_size(&CodingShred::empty_shred()).unwrap() as usize };
    static ref SIZE_OF_EMPTY_DATA_SHRED: usize =
        { serialized_size(&DataShred::empty_shred()).unwrap() as usize };
    static ref SIZE_OF_SIGNATURE: usize =
        { bincode::serialized_size(&Signature::default()).unwrap() as usize };
    static ref SIZE_OF_EMPTY_VEC: usize =
        { bincode::serialized_size(&vec![0u8; 0]).unwrap() as usize };
    static ref SIZE_OF_SHRED_TYPE: usize = { bincode::serialized_size(&0u8).unwrap() as usize };
}

/// The constants that define if a shred is data or coding
pub const DATA_SHRED: u8 = 0b1010_0101;
pub const CODING_SHRED: u8 = 0b0101_1010;

#[derive(Clone, Debug, PartialEq)]
pub struct Shred {
    pub headers: DataShredHeader,
    pub payload: Vec<u8>,
}

impl Shred {
    fn new(header: DataShredHeader, shred_buf: Vec<u8>) -> Self {
        Shred {
            headers: header,
            payload: shred_buf,
        }
    }

    pub fn new_from_serialized_shred(shred_buf: Vec<u8>) -> result::Result<Self> {
        let shred_type: u8 = bincode::deserialize(&shred_buf[..*SIZE_OF_SHRED_TYPE])?;
        let header = if shred_type == CODING_SHRED {
            let end = *SIZE_OF_CODING_SHRED_HEADER;
            let mut header = DataShredHeader::default();
            header.common_header.header = bincode::deserialize(&shred_buf[..end])?;
            header
        } else {
            let end = *SIZE_OF_DATA_SHRED_HEADER;
            bincode::deserialize(&shred_buf[..end])?
        };
        Ok(Self::new(header, shred_buf))
    }

    pub fn new_empty_from_header(headers: DataShredHeader) -> Self {
        let mut payload = vec![0; PACKET_DATA_SIZE];
        let mut wr = io::Cursor::new(&mut payload[..*SIZE_OF_DATA_SHRED_HEADER]);
        bincode::serialize_into(&mut wr, &headers).expect("Failed to serialize shred");
        Shred { headers, payload }
    }

    fn header(&self) -> &ShredCommonHeader {
        if self.is_data() {
            &self.headers.data_header
        } else {
            &self.headers.common_header.header.coding_header
        }
    }

    pub fn header_mut(&mut self) -> &mut ShredCommonHeader {
        if self.is_data() {
            &mut self.headers.data_header
        } else {
            &mut self.headers.common_header.header.coding_header
        }
    }

    pub fn slot(&self) -> u64 {
        self.header().slot
    }

    pub fn parent(&self) -> u64 {
        if self.is_data() {
            self.headers.data_header.slot - u64::from(self.headers.parent_offset)
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
        self.headers.common_header.header.shred_type == DATA_SHRED
    }

    pub fn last_in_slot(&self) -> bool {
        if self.is_data() {
            self.headers.flags & LAST_SHRED_IN_SLOT == LAST_SHRED_IN_SLOT
        } else {
            false
        }
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_last_in_slot(&mut self) {
        if self.is_data() {
            self.headers.flags |= LAST_SHRED_IN_SLOT
        }
    }

    pub fn data_complete(&self) -> bool {
        if self.is_data() {
            self.headers.flags & DATA_COMPLETE_SHRED == DATA_COMPLETE_SHRED
        } else {
            false
        }
    }

    pub fn coding_params(&self) -> Option<(u16, u16, u16)> {
        if !self.is_data() {
            let header = &self.headers.common_header.header;
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
            CodingShred::overhead()
        } else {
            *SIZE_OF_SHRED_TYPE
        } + *SIZE_OF_SIGNATURE;
        self.signature()
            .verify(pubkey.as_ref(), &self.payload[signed_payload_offset..])
    }
}

/// This limit comes from reed solomon library, but unfortunately they don't have
/// a public constant defined for it.
const MAX_DATA_SHREDS_PER_FEC_BLOCK: u32 = 16;

/// Based on rse benchmarks, the optimal erasure config uses 16 data shreds and 4 coding shreds
pub const RECOMMENDED_FEC_RATE: f32 = 0.25;

const LAST_SHRED_IN_SLOT: u8 = 0b0000_0001;
const DATA_COMPLETE_SHRED: u8 = 0b0000_0010;

/// A common header that is present at start of every shred
#[derive(Serialize, Clone, Deserialize, Default, PartialEq, Debug)]
pub struct ShredCommonHeader {
    pub signature: Signature,
    pub slot: u64,
    pub index: u32,
}

/// A common header that is present at start of every data shred
#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct DataShredHeader {
    pub common_header: CodingShred,
    pub data_header: ShredCommonHeader,
    pub parent_offset: u16,
    pub flags: u8,
}

/// The coding shred header has FEC information
#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct CodingShredHeader {
    pub shred_type: u8,
    pub coding_header: ShredCommonHeader,
    pub num_data_shreds: u16,
    pub num_coding_shreds: u16,
    pub position: u16,
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct DataShred {
    pub header: DataShredHeader,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct CodingShred {
    pub header: CodingShredHeader,
    pub payload: Vec<u8>,
}

impl Default for DataShredHeader {
    fn default() -> Self {
        DataShredHeader {
            common_header: CodingShred {
                header: CodingShredHeader {
                    shred_type: DATA_SHRED,
                    ..CodingShredHeader::default()
                },
                payload: vec![],
            },
            data_header: ShredCommonHeader::default(),
            parent_offset: 0,
            flags: 0,
        }
    }
}

impl Default for CodingShredHeader {
    fn default() -> Self {
        CodingShredHeader {
            shred_type: CODING_SHRED,
            coding_header: ShredCommonHeader::default(),
            num_data_shreds: 0,
            num_coding_shreds: 0,
            position: 0,
        }
    }
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for DataShred {
    fn default() -> Self {
        let size = PACKET_DATA_SIZE - *SIZE_OF_EMPTY_DATA_SHRED;
        DataShred {
            header: DataShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for CodingShred {
    fn default() -> Self {
        let size = PACKET_DATA_SIZE - *SIZE_OF_EMPTY_CODING_SHRED;
        CodingShred {
            header: CodingShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

/// Common trait implemented by all types of shreds
pub trait ShredCommon {
    /// Write at a particular offset in the shred. Returns amount written and leftover capacity
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> (usize, usize);
    /// Overhead of shred enum and headers
    fn overhead() -> usize;
    /// Utility function to create an empty shred
    fn empty_shred() -> Self;
}

impl ShredCommon for DataShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> (usize, usize) {
        let mut capacity = self.payload.len().saturating_sub(offset);
        let slice_len = cmp::min(capacity, buf.len());
        capacity -= slice_len;
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        (slice_len, capacity)
    }

    fn overhead() -> usize {
        *SIZE_OF_EMPTY_DATA_SHRED - *SIZE_OF_EMPTY_VEC
    }

    fn empty_shred() -> Self {
        DataShred {
            header: DataShredHeader::default(),
            payload: vec![],
        }
    }
}

impl ShredCommon for CodingShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> (usize, usize) {
        let mut capacity = self.payload.len().saturating_sub(offset);
        let slice_len = cmp::min(capacity, buf.len());
        capacity -= slice_len;
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        (slice_len, capacity)
    }

    fn overhead() -> usize {
        *SIZE_OF_EMPTY_CODING_SHRED
    }

    fn empty_shred() -> Self {
        CodingShred {
            header: CodingShredHeader::default(),
            payload: vec![],
        }
    }
}

#[derive(Debug)]
pub struct Shredder {
    slot: u64,
    pub index: u32,
    fec_set_index: u32,
    parent_offset: u16,
    fec_rate: f32,
    signer: Arc<Keypair>,
    pub shreds: Vec<Shred>,
    fec_set_shred_start: usize,
    active_shred: DataShred,
    active_offset: usize,
}

impl Write for Shredder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.active_offset;
        let (slice_len, capacity) = self.active_shred.write_at(written, buf);

        if buf.len() > slice_len || capacity == 0 {
            self.finalize_data_shred();
        } else {
            self.active_offset += slice_len;
        }

        if self.index - self.fec_set_index >= MAX_DATA_SHREDS_PER_FEC_BLOCK {
            self.sign_unsigned_shreds_and_generate_codes();
        }

        Ok(slice_len)
    }

    fn flush(&mut self) -> io::Result<()> {
        unimplemented!()
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct RecoveryResult {
    pub recovered_data: Vec<Shred>,
    pub recovered_code: Vec<Shred>,
}

impl Shredder {
    pub fn new(
        slot: u64,
        parent: u64,
        fec_rate: f32,
        signer: &Arc<Keypair>,
        index: u32,
    ) -> result::Result<Self> {
        if fec_rate > 1.0 || fec_rate < 0.0 {
            Err(Error::IO(IOError::new(
                ErrorKind::Other,
                format!(
                    "FEC rate {:?} must be more than 0.0 and less than 1.0",
                    fec_rate
                ),
            )))
        } else if slot < parent || slot - parent > u64::from(std::u16::MAX) {
            Err(Error::IO(IOError::new(
                ErrorKind::Other,
                format!(
                    "Current slot {:?} must be > Parent slot {:?}, but the difference must not be > {:?}",
                    slot, parent, std::u16::MAX
                ),
            )))
        } else {
            let mut data_shred = DataShred::default();
            data_shred.header.data_header.slot = slot;
            data_shred.header.data_header.index = index;
            data_shred.header.parent_offset = (slot - parent) as u16;
            let active_shred = data_shred;
            Ok(Shredder {
                slot,
                index,
                fec_set_index: index,
                parent_offset: (slot - parent) as u16,
                fec_rate,
                signer: signer.clone(),
                shreds: vec![],
                fec_set_shred_start: 0,
                active_shred,
                active_offset: 0,
            })
        }
    }

    fn sign_shred(signer: &Arc<Keypair>, shred_info: &mut Shred, signature_offset: usize) {
        let data_offset = signature_offset + *SIZE_OF_SIGNATURE;
        let signature = signer.sign_message(&shred_info.payload[data_offset..]);
        let serialized_signature =
            bincode::serialize(&signature).expect("Failed to generate serialized signature");
        shred_info.payload[signature_offset..signature_offset + serialized_signature.len()]
            .copy_from_slice(&serialized_signature);
        shred_info.header_mut().signature = signature;
    }

    fn sign_unsigned_shreds_and_generate_codes(&mut self) {
        let signature_offset = CodingShred::overhead();
        let signer = self.signer.clone();
        self.shreds[self.fec_set_shred_start..]
            .iter_mut()
            .for_each(|d| Self::sign_shred(&signer, d, signature_offset));
        let unsigned_coding_shred_start = self.shreds.len();

        self.generate_coding_shreds();
        let signature_offset = *SIZE_OF_SHRED_TYPE;
        self.shreds[unsigned_coding_shred_start..]
            .iter_mut()
            .for_each(|d| Self::sign_shred(&signer, d, signature_offset));
        self.fec_set_shred_start = self.shreds.len();
    }

    /// Finalize a data shred. Update the shred index for the next shred
    fn finalize_data_shred(&mut self) {
        let mut data = Vec::with_capacity(PACKET_DATA_SIZE);
        bincode::serialize_into(&mut data, &self.active_shred).expect("Failed to serialize shred");

        self.active_offset = 0;
        self.index += 1;

        let mut shred = self.new_data_shred();
        std::mem::swap(&mut shred, &mut self.active_shred);
        let shred_info = Shred::new(shred.header, data);
        self.shreds.push(shred_info);
    }

    /// Creates a new data shred
    fn new_data_shred(&self) -> DataShred {
        let mut data_shred = DataShred::default();
        data_shred.header.data_header.slot = self.slot;
        data_shred.header.data_header.index = self.index;
        data_shred.header.parent_offset = self.parent_offset;
        data_shred
    }

    pub fn new_coding_shred(
        slot: u64,
        index: u32,
        num_data: usize,
        num_code: usize,
        position: usize,
    ) -> CodingShred {
        let mut coding_shred = CodingShred::default();
        coding_shred.header.coding_header.slot = slot;
        coding_shred.header.coding_header.index = index;
        coding_shred.header.num_data_shreds = num_data as u16;
        coding_shred.header.num_coding_shreds = num_code as u16;
        coding_shred.header.position = position as u16;
        coding_shred
    }

    /// Generates coding shreds for the data shreds in the current FEC set
    fn generate_coding_shreds(&mut self) {
        if self.fec_rate != 0.0 {
            let num_data = (self.index - self.fec_set_index) as usize;
            // always generate at least 1 coding shred even if the fec_rate doesn't allow it
            let num_coding = 1.max((self.fec_rate * num_data as f32) as usize);
            let session =
                Session::new(num_data, num_coding).expect("Failed to create erasure session");
            let start_index = self.index - num_data as u32;

            // All information after coding shred field in a data shred is encoded
            let coding_block_offset = CodingShred::overhead();
            let data_ptrs: Vec<_> = self.shreds[self.fec_set_shred_start..]
                .iter()
                .map(|data| &data.payload[coding_block_offset..])
                .collect();

            // Create empty coding shreds, with correctly populated headers
            let mut coding_shreds = Vec::with_capacity(num_coding);
            (0..num_coding).for_each(|i| {
                let shred = bincode::serialize(&Self::new_coding_shred(
                    self.slot,
                    start_index + i as u32,
                    num_data,
                    num_coding,
                    i,
                ))
                .unwrap();
                coding_shreds.push(shred);
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
            coding_shreds.into_iter().enumerate().for_each(|(i, code)| {
                let mut header = DataShredHeader::default();
                header.common_header.header.shred_type = CODING_SHRED;
                header.common_header.header.coding_header.index = start_index + i as u32;
                header.common_header.header.coding_header.slot = self.slot;
                header.common_header.header.num_coding_shreds = num_coding as u16;
                header.common_header.header.num_data_shreds = num_data as u16;
                header.common_header.header.position = i as u16;
                let shred_info = Shred::new(header, code);
                self.shreds.push(shred_info);
            });
            self.fec_set_index = self.index;
        }
    }

    /// Create the final data shred for the current FEC set or slot
    /// If there's an active data shred, morph it into the final shred
    /// If the current active data shred is first in slot, finalize it and create a new shred
    fn make_final_data_shred(&mut self, last_in_slot: u8) {
        if self.active_shred.header.data_header.index == 0 {
            self.finalize_data_shred();
        }
        self.active_shred.header.flags |= DATA_COMPLETE_SHRED;
        if last_in_slot == LAST_SHRED_IN_SLOT {
            self.active_shred.header.flags |= LAST_SHRED_IN_SLOT;
        }
        self.finalize_data_shred();
        self.sign_unsigned_shreds_and_generate_codes();
    }

    /// Finalize the current FEC block, and generate coding shreds
    pub fn finalize_data(&mut self) {
        self.make_final_data_shred(0);
    }

    /// Finalize the current slot (i.e. add last slot shred) and generate coding shreds
    pub fn finalize_slot(&mut self) {
        self.make_final_data_shred(LAST_SHRED_IN_SLOT);
    }

    fn fill_in_missing_shreds(
        shred: &Shred,
        num_data: usize,
        num_coding: usize,
        slot: u64,
        first_index: usize,
        expected_index: usize,
        present: &mut [bool],
    ) -> (Vec<Vec<u8>>, usize) {
        let index = Self::get_shred_index(shred, num_data);

        // The index of current shred must be within the range of shreds that are being
        // recovered
        if !(first_index..first_index + num_data + num_coding).contains(&index) {
            return (vec![], index);
        }

        let missing_blocks: Vec<Vec<u8>> = (expected_index..index)
            .map(|missing| {
                present[missing.saturating_sub(first_index)] = false;
                Shredder::new_empty_missing_shred(num_data, num_coding, slot, first_index, missing)
            })
            .collect();
        (missing_blocks, index)
    }

    fn new_empty_missing_shred(
        num_data: usize,
        num_coding: usize,
        slot: u64,
        first_index: usize,
        missing: usize,
    ) -> Vec<u8> {
        if missing < first_index + num_data {
            let mut data_shred = DataShred::default();
            data_shred.header.data_header.slot = slot;
            data_shred.header.data_header.index = missing as u32;
            bincode::serialize(&data_shred).unwrap()
        } else {
            bincode::serialize(&Self::new_coding_shred(
                slot,
                missing.saturating_sub(num_data) as u32,
                num_data,
                num_coding,
                missing - first_index - num_data,
            ))
            .unwrap()
        }
    }

    pub fn try_recovery(
        shreds: Vec<Shred>,
        num_data: usize,
        num_coding: usize,
        first_index: usize,
        slot: u64,
    ) -> Result<RecoveryResult, reed_solomon_erasure::Error> {
        let mut recovered_data = vec![];
        let mut recovered_code = vec![];
        let fec_set_size = num_data + num_coding;
        if num_coding > 0 && shreds.len() < fec_set_size {
            let coding_block_offset = CodingShred::overhead();

            // Let's try recovering missing shreds using erasure
            let mut present = &mut vec![true; fec_set_size];
            let mut next_expected_index = first_index;
            let mut shred_bufs: Vec<Vec<u8>> = shreds
                .into_iter()
                .flat_map(|shred| {
                    let (mut blocks, last_index) = Self::fill_in_missing_shreds(
                        &shred,
                        num_data,
                        num_coding,
                        slot,
                        first_index,
                        next_expected_index,
                        &mut present,
                    );
                    blocks.push(shred.payload);
                    next_expected_index = last_index + 1;
                    blocks
                })
                .collect();

            // Insert any other missing shreds after the last shred we have received in the
            // current FEC block
            let mut pending_shreds: Vec<Vec<u8>> = (next_expected_index
                ..first_index + fec_set_size)
                .map(|missing| {
                    present[missing.saturating_sub(first_index)] = false;
                    Self::new_empty_missing_shred(num_data, num_coding, slot, first_index, missing)
                })
                .collect();
            shred_bufs.append(&mut pending_shreds);

            if shred_bufs.len() != fec_set_size {
                Err(reed_solomon_erasure::Error::TooFewShardsPresent)?;
            }

            let session = Session::new(num_data, num_coding).unwrap();

            let mut blocks: Vec<&mut [u8]> = shred_bufs
                .iter_mut()
                .map(|x| x[coding_block_offset..].as_mut())
                .collect();
            session.decode_blocks(&mut blocks, &present)?;

            let mut num_drained = 0;
            present
                .iter()
                .enumerate()
                .for_each(|(position, was_present)| {
                    if !was_present {
                        let drain_this = position - num_drained;
                        let shred_buf = shred_bufs.remove(drain_this);
                        num_drained += 1;
                        if let Ok(shred) = Shred::new_from_serialized_shred(shred_buf) {
                            let shred_index = shred.index() as usize;
                            // Valid shred must be in the same slot as the original shreds
                            if shred.slot() == slot {
                                // Data shreds are "positioned" at the start of the iterator. First num_data
                                // shreds are expected to be the data shreds.
                                if position < num_data
                                    && (first_index..first_index + num_data).contains(&shred_index)
                                {
                                    // Also, a valid data shred must be indexed between first_index and first+num_data index
                                    recovered_data.push(shred)
                                } else if (first_index..first_index + num_coding)
                                    .contains(&shred_index)
                                {
                                    // A valid coding shred must be indexed between first_index and first+num_coding index
                                    recovered_code.push(shred)
                                }
                            }
                        }
                    }
                });
        }

        Ok(RecoveryResult {
            recovered_data,
            recovered_code,
        })
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
                Err(reed_solomon_erasure::Error::TooFewDataShards)?;
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
                let offset = *SIZE_OF_EMPTY_DATA_SHRED;
                data[offset as usize..].iter()
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_test_data_shred(
        shred: &Shred,
        index: u32,
        slot: u64,
        parent: u64,
        pk: &Pubkey,
        verify: bool,
    ) {
        assert_eq!(shred.payload.len(), PACKET_DATA_SIZE);
        assert!(shred.is_data());
        assert_eq!(shred.index(), index);
        assert_eq!(shred.slot(), slot);
        assert_eq!(shred.parent(), parent);
        assert_eq!(verify, shred.verify(pk));
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
        assert_matches!(Shredder::new(slot, slot + 1, 1.001, &keypair, 0), Err(_));
        // Test that slot - parent cannot be > u16 MAX
        assert_matches!(
            Shredder::new(slot, slot - 1 - 0xffff, 1.001, &keypair, 0),
            Err(_)
        );

        let mut shredder =
            Shredder::new(slot, slot - 5, 0.0, &keypair, 0).expect("Failed in creating shredder");

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 0);

        assert!(DataShred::overhead() < PACKET_DATA_SIZE);
        assert!(CodingShred::overhead() < PACKET_DATA_SIZE);

        // Test0: Write some data to shred. Not enough to create a signed shred
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 25);

        // Test1: Write some more data to shred. Not enough to create a signed shred
        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 50);

        // Test2: Write enough data to create a shred (> PACKET_DATA_SIZE)
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());
        // Assert that we have atleast one signed shred
        assert!(!shredder.shreds.is_empty());
        // Assert that the new active shred was not populated
        assert_eq!(shredder.active_offset, 0);

        // Test3: Assert that the first shred in slot was created (since we gave a parent to shredder)
        let shred = &shredder.shreds[0];
        // Test4: assert that it matches the original shred
        // The shreds are not signed yet, as the data is not finalized
        verify_test_data_shred(&shred, 0, slot, slot - 5, &keypair.pubkey(), false);

        let seed0 = shred.seed();
        // Test that same seed is generated for a given shred
        assert_eq!(seed0, shred.seed());

        // Test5: Write left over data, and assert that a data shred is being created
        shredder.write(&data[offset..]).unwrap();

        // Test6: Let's finalize the FEC block. That should result in the current shred to morph into
        // a signed LastInFECBlock shred
        shredder.finalize_data();

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be Last in FEC Set
        let shred = &shredder.shreds[1];
        verify_test_data_shred(&shred, 1, slot, slot - 5, &keypair.pubkey(), true);

        // Test that same seed is NOT generated for two different shreds
        assert_ne!(seed0, shred.seed());

        // Test7: Let's write some more data to the shredder.
        // Now we should get a new FEC block
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        let shred = &shredder.shreds[2];
        verify_test_data_shred(&shred, 2, slot, slot - 5, &keypair.pubkey(), false);

        // Test8: Write more data to generate an intermediate data shred
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be a Data shred
        let shred = &shredder.shreds[3];
        verify_test_data_shred(&shred, 3, slot, slot - 5, &keypair.pubkey(), false);

        // Test9: Write some data to shredder
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());

        // And, finish the slot
        shredder.finalize_slot();

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be LastInSlot
        let shred = &shredder.shreds[4];
        verify_test_data_shred(&shred, 4, slot, slot - 5, &keypair.pubkey(), true);
    }

    #[test]
    fn test_small_data_shredder() {
        let keypair = Arc::new(Keypair::new());

        let slot = 0x123456789abcdef0;
        let mut shredder =
            Shredder::new(slot, slot - 5, 0.0, &keypair, 0).expect("Failed in creating shredder");

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..25).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();

        // We should have 0 shreds now
        assert_eq!(shredder.shreds.len(), 0);

        shredder.finalize_data();

        // We should have 1 shred now
        assert_eq!(shredder.shreds.len(), 2);

        let shred = shredder.shreds.remove(0);
        verify_test_data_shred(&shred, 0, slot, slot - 5, &keypair.pubkey(), true);

        let shred = shredder.shreds.remove(0);
        verify_test_data_shred(&shred, 1, slot, slot - 5, &keypair.pubkey(), true);

        let mut shredder = Shredder::new(0x123456789abcdef0, slot - 5, 0.0, &keypair, 2)
            .expect("Failed in creating shredder");

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..25).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();

        // We should have 0 shreds now
        assert_eq!(shredder.shreds.len(), 0);

        shredder.finalize_data();

        // We should have 1 shred now (LastInFECBlock)
        assert_eq!(shredder.shreds.len(), 1);
        let shred = shredder.shreds.remove(0);
        verify_test_data_shred(&shred, 2, slot, slot - 5, &keypair.pubkey(), true);
    }

    #[test]
    fn test_data_and_code_shredder() {
        let keypair = Arc::new(Keypair::new());

        let slot = 0x123456789abcdef0;
        // Test that FEC rate cannot be > 1.0
        assert_matches!(Shredder::new(slot, slot - 5, 1.001, &keypair, 0), Err(_));

        let mut shredder = Shredder::new(0x123456789abcdef0, slot - 5, 1.0, &keypair, 0)
            .expect("Failed in creating shredder");

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 0);

        // Write enough data to create a shred (> PACKET_DATA_SIZE)
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();
        let _ = shredder.write(&data).unwrap();

        // We should have 2 shreds now
        assert_eq!(shredder.shreds.len(), 2);

        shredder.finalize_data();

        // Finalize must have created 1 final data shred and 3 coding shreds
        // assert_eq!(shredder.shreds.len(), 6);
        let shred = shredder.shreds.remove(0);
        verify_test_data_shred(&shred, 0, slot, slot - 5, &keypair.pubkey(), true);

        let shred = shredder.shreds.remove(0);
        verify_test_data_shred(&shred, 1, slot, slot - 5, &keypair.pubkey(), true);

        let shred = shredder.shreds.remove(0);
        verify_test_data_shred(&shred, 2, slot, slot - 5, &keypair.pubkey(), true);

        let shred = shredder.shreds.remove(0);
        verify_test_code_shred(&shred, 0, slot, &keypair.pubkey(), true);

        let shred = shredder.shreds.remove(0);
        verify_test_code_shred(&shred, 1, slot, &keypair.pubkey(), true);

        let shred = shredder.shreds.remove(0);
        verify_test_code_shred(&shred, 2, slot, &keypair.pubkey(), true);
    }

    #[test]
    fn test_recovery_and_reassembly() {
        let keypair = Arc::new(Keypair::new());
        let slot = 0x123456789abcdef0;
        let mut shredder =
            Shredder::new(slot, slot - 5, 1.0, &keypair, 0).expect("Failed in creating shredder");

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..4000).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let mut offset = shredder.write(&data).unwrap();
        let approx_shred_payload_size = offset;
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();

        // We should have some shreds now
        assert_eq!(
            shredder.shreds.len(),
            data.len() / approx_shred_payload_size
        );
        assert_eq!(offset, data.len());

        shredder.finalize_data();

        // We should have 10 shreds now (one additional final shred, and equal number of coding shreds)
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shreds.len(), expected_shred_count);

        let shred_infos = shredder.shreds.clone();

        // Test0: Try recovery/reassembly with only data shreds, but not all data shreds. Hint: should fail
        assert_matches!(
            Shredder::try_recovery(
                shred_infos[..3].to_vec(),
                expected_shred_count / 2,
                expected_shred_count / 2,
                0,
                slot
            ),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );

        // Test1: Try recovery/reassembly with only data shreds. Hint: should work
        let result = Shredder::try_recovery(
            shred_infos[..4].to_vec(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);
        assert!(result.recovered_data.is_empty());
        assert!(!result.recovered_code.is_empty());
        let result = Shredder::deshred(&shred_infos[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test2: Try recovery/reassembly with missing data shreds + coding shreds. Hint: should work
        let mut shred_info: Vec<Shred> = shredder
            .shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 == 0 { Some(b.clone()) } else { None })
            .collect();

        let mut result = Shredder::try_recovery(
            shred_info.clone(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 1 and 3 were missing
        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(&recovered_shred, 1, slot, slot - 5, &keypair.pubkey(), true);
        shred_info.insert(1, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(&recovered_shred, 3, slot, slot - 5, &keypair.pubkey(), true);
        shred_info.insert(3, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 5, 7 were missing
        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 1, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 1)));

        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 3, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 3)));

        let result = Shredder::deshred(&shred_info[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test3: Try recovery/reassembly with 3 missing data shreds + 2 coding shreds. Hint: should work
        let mut shred_info: Vec<Shred> = shredder
            .shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let mut result = Shredder::try_recovery(
            shred_info.clone(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 0, 2 were missing
        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(&recovered_shred, 0, slot, slot - 5, &keypair.pubkey(), true);
        shred_info.insert(0, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(&recovered_shred, 2, slot, slot - 5, &keypair.pubkey(), true);
        shred_info.insert(2, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 4, 6 were missing
        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 0, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 0)));

        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 2, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 2)));

        let result = Shredder::deshred(&shred_info[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test4: Try recovery/reassembly full slot with 3 missing data shreds + 2 coding shreds. Hint: should work
        let mut shredder =
            Shredder::new(slot, slot - 5, 1.0, &keypair, 0).expect("Failed in creating shredder");

        let mut offset = shredder.write(&data).unwrap();
        let approx_shred_payload_size = offset;
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();

        // We should have some shreds now
        assert_eq!(
            shredder.shreds.len(),
            data.len() / approx_shred_payload_size
        );
        assert_eq!(offset, data.len());

        shredder.finalize_slot();

        // We should have 10 shreds now (one additional final shred, and equal number of coding shreds)
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shreds.len(), expected_shred_count);

        let mut shred_info: Vec<Shred> = shredder
            .shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let mut result = Shredder::try_recovery(
            shred_info.clone(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 0, 2 were missing
        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(&recovered_shred, 0, slot, slot - 5, &keypair.pubkey(), true);
        shred_info.insert(0, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(&recovered_shred, 2, slot, slot - 5, &keypair.pubkey(), true);
        shred_info.insert(2, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 4, 6 were missing
        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 0, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 0)));

        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 2, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 2)));

        let result = Shredder::deshred(&shred_info[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test5: Try recovery/reassembly with 3 missing data shreds + 3 coding shreds. Hint: should fail
        let shreds: Vec<Shred> = shredder
            .shreds
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if (i < 4 && i % 2 != 0) || (i >= 4 && i % 2 == 0) {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(shreds.len(), 4);
        assert_matches!(
            Shredder::deshred(&shreds),
            Err(reed_solomon_erasure::Error::TooFewDataShards)
        );

        // Test6: Try recovery/reassembly with non zero index full slot with 3 missing data shreds + 2 coding shreds. Hint: should work
        let mut shredder =
            Shredder::new(slot, slot - 5, 1.0, &keypair, 25).expect("Failed in creating shredder");

        let mut offset = shredder.write(&data).unwrap();
        let approx_shred_payload_size = offset;
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();
        offset += shredder.write(&data[offset..]).unwrap();

        // We should have some shreds now
        assert_eq!(
            shredder.shreds.len(),
            data.len() / approx_shred_payload_size
        );
        assert_eq!(offset, data.len());

        shredder.finalize_slot();

        // We should have 10 shreds now (one additional final shred, and equal number of coding shreds)
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shreds.len(), expected_shred_count);

        let mut shred_info: Vec<Shred> = shredder
            .shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let mut result = Shredder::try_recovery(
            shred_info.clone(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            25,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 0, 2 were missing
        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(
            &recovered_shred,
            25,
            slot,
            slot - 5,
            &keypair.pubkey(),
            true,
        );
        shred_info.insert(0, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        verify_test_data_shred(
            &recovered_shred,
            27,
            slot,
            slot - 5,
            &keypair.pubkey(),
            true,
        );
        shred_info.insert(2, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 4, 6 were missing
        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 25, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 0)));

        let recovered_shred = result.recovered_code.remove(0);
        verify_test_code_shred(&recovered_shred, 27, slot, &keypair.pubkey(), false);
        assert_eq!(recovered_shred.coding_params(), Some((4, 4, 2)));

        let result = Shredder::deshred(&shred_info[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test7: Try recovery/reassembly with incorrect slot. Hint: does not recover any shreds
        let result = Shredder::try_recovery(
            shred_info.clone(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            25,
            slot + 1,
        )
        .unwrap();
        assert!(result.recovered_data.is_empty());

        // Test8: Try recovery/reassembly with incorrect index. Hint: does not recover any shreds
        assert_matches!(
            Shredder::try_recovery(
                shred_info.clone(),
                expected_shred_count / 2,
                expected_shred_count / 2,
                15,
                slot,
            ),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );

        // Test9: Try recovery/reassembly with incorrect index. Hint: does not recover any shreds
        assert_matches!(
            Shredder::try_recovery(
                shred_info,
                expected_shred_count / 2,
                expected_shred_count / 2,
                35,
                slot,
            ),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );
    }

    #[test]
    fn test_multi_fec_block_coding() {
        let keypair = Arc::new(Keypair::new());
        let slot = 0x123456789abcdef0;
        let mut shredder =
            Shredder::new(slot, slot - 5, 1.0, &keypair, 0).expect("Failed in creating shredder");

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..MAX_DATA_SHREDS_PER_FEC_BLOCK * 1200 * 3).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let mut offset = shredder.write(&data).unwrap();
        let approx_shred_payload_size = offset;
        while offset < data.len() {
            offset += shredder.write(&data[offset..]).unwrap();
        }

        // We should have some shreds now
        assert!(shredder.shreds.len() > data.len() / approx_shred_payload_size);
        assert_eq!(offset, data.len());

        shredder.finalize_data();
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shreds.len(), expected_shred_count);

        let mut index = 0;

        while index < shredder.shreds.len() {
            let num_data_shreds = cmp::min(
                MAX_DATA_SHREDS_PER_FEC_BLOCK as usize,
                (shredder.shreds.len() - index) / 2,
            );
            let coding_start = index + num_data_shreds;
            shredder.shreds[index..coding_start]
                .iter()
                .for_each(|s| assert!(s.is_data()));
            index = coding_start + num_data_shreds;
            shredder.shreds[coding_start..index]
                .iter()
                .for_each(|s| assert!(!s.is_data()));
        }
    }
}
