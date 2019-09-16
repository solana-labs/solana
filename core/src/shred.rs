//! The `shred` module defines data structures and methods to pull MTU sized data frames from the network.
use crate::erasure::Session;
use crate::result;
use crate::result::Error;
use bincode::serialized_size;
use core::borrow::BorrowMut;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use std::io::{Error as IOError, ErrorKind, Write};
use std::sync::Arc;
use std::{cmp, io};

lazy_static! {
    static ref SIZE_OF_EMPTY_CODING_SHRED: usize =
        { serialized_size(&CodingShred::empty_shred()).unwrap() as usize };
    static ref SIZE_OF_EMPTY_DATA_SHRED: usize =
        { serialized_size(&DataShred::empty_shred()).unwrap() as usize };
    static ref SIZE_OF_SHRED_CODING_SHRED: usize =
        { serialized_size(&Shred::Coding(CodingShred::empty_shred())).unwrap() as usize };
    static ref SIZE_OF_SHRED_DATA_SHRED: usize =
        { serialized_size(&Shred::Data(DataShred::empty_shred())).unwrap() as usize };
    static ref SIZE_OF_SIGNATURE: usize =
        { bincode::serialized_size(&Signature::default()).unwrap() as usize };
    static ref SIZE_OF_EMPTY_VEC: usize =
        { bincode::serialized_size(&vec![0u8; 0]).unwrap() as usize };
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct ShredMetaBuf {
    pub slot: u64,
    pub index: u32,
    pub data_shred: bool,
    pub shred_buf: Vec<u8>,
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub enum Shred {
    Data(DataShred),
    Coding(CodingShred),
}

/// This limit comes from reed solomon library, but unfortunately they don't have
/// a public constant defined for it.
const MAX_DATA_SHREDS_PER_FEC_BLOCK: u32 = 4;

const LAST_SHRED_IN_SLOT: u8 = 1;
const DATA_COMPLETE_SHRED: u8 = 2;

impl Shred {
    pub fn slot(&self) -> u64 {
        match self {
            Shred::Data(s) => s.header.common_header.slot,
            Shred::Coding(s) => s.header.common_header.slot,
        }
    }

    pub fn parent(&self) -> u64 {
        match self {
            Shred::Data(s) => s.header.common_header.slot - u64::from(s.header.parent_offset),
            Shred::Coding(_) => std::u64::MAX,
        }
    }

    pub fn set_slot(&mut self, slot: u64) {
        let parent = self.parent();
        match self {
            Shred::Data(s) => {
                s.header.common_header.slot = slot;
                s.header.parent_offset = (slot - parent) as u16;
            }
            Shred::Coding(s) => s.header.common_header.slot = slot,
        };
    }

    pub fn index(&self) -> u32 {
        match self {
            Shred::Data(s) => s.header.common_header.index,
            Shred::Coding(s) => s.header.common_header.index,
        }
    }

    pub fn set_index(&mut self, index: u32) {
        match self {
            Shred::Data(s) => s.header.common_header.index = index,
            Shred::Coding(s) => s.header.common_header.index = index,
        };
    }

    pub fn signature(&self) -> Signature {
        match self {
            Shred::Data(s) => s.header.common_header.signature,
            Shred::Coding(s) => s.header.common_header.signature,
        }
    }

    pub fn set_signature(&mut self, sig: Signature) {
        match self {
            Shred::Data(s) => s.header.common_header.signature = sig,
            Shred::Coding(s) => s.header.common_header.signature = sig,
        };
    }

    pub fn seed(&self) -> [u8; 32] {
        let mut seed = [0; 32];
        let seed_len = seed.len();
        let sig = match self {
            Shred::Data(s) => &s.header.common_header.signature,
            Shred::Coding(s) => &s.header.common_header.signature,
        }
        .as_ref();

        seed[0..seed_len].copy_from_slice(&sig[(sig.len() - seed_len)..]);
        seed
    }

    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        let shred = bincode::serialize(&self).unwrap();
        self.fast_verify(&shred, pubkey)
    }

    pub fn fast_verify(&self, shred_buf: &[u8], pubkey: &Pubkey) -> bool {
        let signed_payload_offset = match self {
            Shred::Data(_) => CodingShred::overhead(),
            Shred::Coding(_) => CodingShred::overhead() - *SIZE_OF_EMPTY_CODING_SHRED,
        } + *SIZE_OF_SIGNATURE;
        self.signature()
            .verify(pubkey.as_ref(), &shred_buf[signed_payload_offset..])
    }

    pub fn is_data(&self) -> bool {
        if let Shred::Coding(_) = self {
            false
        } else {
            true
        }
    }

    pub fn last_in_slot(&self) -> bool {
        match self {
            Shred::Data(s) => s.header.flags & LAST_SHRED_IN_SLOT == LAST_SHRED_IN_SLOT,
            Shred::Coding(_) => false,
        }
    }

    pub fn set_last_in_slot(&mut self) {
        match self {
            Shred::Data(s) => s.header.flags |= LAST_SHRED_IN_SLOT,
            Shred::Coding(_) => {}
        }
    }

    pub fn data_complete(&self) -> bool {
        match self {
            Shred::Data(s) => s.header.flags & DATA_COMPLETE_SHRED == DATA_COMPLETE_SHRED,
            Shred::Coding(_) => false,
        }
    }
}

/// A common header that is present at start of every shred
#[derive(Serialize, Clone, Deserialize, Default, PartialEq, Debug)]
pub struct ShredCommonHeader {
    pub signature: Signature,
    pub slot: u64,
    pub index: u32,
}

/// A common header that is present at start of every data shred
#[derive(Serialize, Clone, Deserialize, Default, PartialEq, Debug)]
pub struct DataShredHeader {
    _reserved: CodingShredHeader,
    pub common_header: ShredCommonHeader,
    pub parent_offset: u16,
    pub flags: u8,
}

/// The coding shred header has FEC information
#[derive(Serialize, Clone, Deserialize, Default, PartialEq, Debug)]
pub struct CodingShredHeader {
    pub common_header: ShredCommonHeader,
    pub num_data_shreds: u16,
    pub num_coding_shreds: u16,
    pub position: u16,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct DataShred {
    pub header: DataShredHeader,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct CodingShred {
    pub header: CodingShredHeader,
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for DataShred {
    fn default() -> Self {
        let size = PACKET_DATA_SIZE - *SIZE_OF_SHRED_DATA_SHRED;
        DataShred {
            header: DataShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for CodingShred {
    fn default() -> Self {
        let size = PACKET_DATA_SIZE - *SIZE_OF_SHRED_CODING_SHRED;
        CodingShred {
            header: CodingShredHeader {
                common_header: ShredCommonHeader::default(),
                num_data_shreds: 0,
                num_coding_shreds: 0,
                position: 0,
                payload: vec![0; size],
            },
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
        *SIZE_OF_SHRED_DATA_SHRED - *SIZE_OF_EMPTY_VEC
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
        let mut capacity = self.header.payload.len().saturating_sub(offset);
        let slice_len = cmp::min(capacity, buf.len());
        capacity -= slice_len;
        if slice_len > 0 {
            self.header.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        (slice_len, capacity)
    }

    fn overhead() -> usize {
        *SIZE_OF_SHRED_CODING_SHRED
    }

    fn empty_shred() -> Self {
        CodingShred {
            header: CodingShredHeader::default(),
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
    pub shred_tuples: Vec<(Shred, Vec<u8>)>,
    fec_set_shred_start: usize,
    active_shred: Shred,
    active_offset: usize,
}

impl Write for Shredder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.active_offset;
        let (slice_len, capacity) = match self.active_shred.borrow_mut() {
            Shred::Data(s) => s.write_at(written, buf),
            Shred::Coding(s) => s.write_at(written, buf),
        };

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

#[derive(Default, Debug, PartialEq)]
pub struct DeshredResult {
    pub payload: Vec<u8>,
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
            data_shred.header.common_header.slot = slot;
            data_shred.header.common_header.index = index;
            data_shred.header.parent_offset = (slot - parent) as u16;
            let active_shred = Shred::Data(data_shred);
            Ok(Shredder {
                slot,
                index,
                fec_set_index: index,
                parent_offset: (slot - parent) as u16,
                fec_rate,
                signer: signer.clone(),
                shred_tuples: vec![],
                fec_set_shred_start: 0,
                active_shred,
                active_offset: 0,
            })
        }
    }

    fn sign_shred(
        signer: &Arc<Keypair>,
        shred: &mut Shred,
        shred_buf: &mut [u8],
        signature_offset: usize,
    ) {
        let data_offset = signature_offset + *SIZE_OF_SIGNATURE;
        let signature = signer.sign_message(&shred_buf[data_offset..]);
        let serialized_signature =
            bincode::serialize(&signature).expect("Failed to generate serialized signature");
        shred.set_signature(signature);
        shred_buf[signature_offset..signature_offset + serialized_signature.len()]
            .copy_from_slice(&serialized_signature);
    }

    fn sign_unsigned_shreds_and_generate_codes(&mut self) {
        let signature_offset = CodingShred::overhead();
        let signer = self.signer.clone();
        self.shred_tuples[self.fec_set_shred_start..]
            .iter_mut()
            .for_each(|(s, d)| Self::sign_shred(&signer, s, d, signature_offset));
        let unsigned_coding_shred_start = self.shred_tuples.len();
        self.generate_coding_shreds();
        let coding_header_offset = *SIZE_OF_SHRED_CODING_SHRED - *SIZE_OF_EMPTY_CODING_SHRED;
        self.shred_tuples[unsigned_coding_shred_start..]
            .iter_mut()
            .for_each(|(s, d)| Self::sign_shred(&signer, s, d, coding_header_offset));
        self.fec_set_shred_start = self.shred_tuples.len();
    }

    /// Finalize a data shred. Update the shred index for the next shred
    fn finalize_data_shred(&mut self) {
        let mut data = vec![0; PACKET_DATA_SIZE];
        let mut wr = io::Cursor::new(&mut data[..]);
        bincode::serialize_into(&mut wr, &self.active_shred).expect("Failed to serialize shred");

        self.active_offset = 0;
        self.index += 1;

        let mut shred = Shred::Data(self.new_data_shred());
        std::mem::swap(&mut shred, &mut self.active_shred);
        self.shred_tuples.push((shred, data));
    }

    /// Creates a new data shred
    fn new_data_shred(&self) -> DataShred {
        let mut data_shred = DataShred::default();
        data_shred.header.common_header.slot = self.slot;
        data_shred.header.common_header.index = self.index;
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
        coding_shred.header.common_header.slot = slot;
        coding_shred.header.common_header.index = index;
        coding_shred.header.num_data_shreds = num_data as u16;
        coding_shred.header.num_coding_shreds = num_code as u16;
        coding_shred.header.position = position as u16;
        coding_shred
    }

    /// Generates coding shreds for the data shreds in the current FEC set
    fn generate_coding_shreds(&mut self) {
        if self.fec_rate != 0.0 {
            let num_data = (self.index - self.fec_set_index) as usize;
            let num_coding = (self.fec_rate * num_data as f32) as usize;
            let session =
                Session::new(num_data, num_coding).expect("Failed to create erasure session");
            let start_index = self.index - num_data as u32;

            // All information after "reserved" field (coding shred header) in a data shred is encoded
            let coding_block_offset = CodingShred::overhead();
            let data_ptrs: Vec<_> = self.shred_tuples[self.fec_set_shred_start..]
                .iter()
                .map(|(_, data)| &data[coding_block_offset..])
                .collect();

            // Create empty coding shreds, with correctly populated headers
            let mut coding_shreds = Vec::with_capacity(num_coding);
            (0..num_coding).for_each(|i| {
                let shred = bincode::serialize(&Shred::Coding(Self::new_coding_shred(
                    self.slot,
                    start_index + i as u32,
                    num_data,
                    num_coding,
                    i,
                )))
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
            coding_shreds.into_iter().for_each(|code| {
                let shred: Shred = bincode::deserialize(&code).unwrap();
                self.shred_tuples.push((shred, code));
            });
            self.fec_set_index = self.index;
        }
    }

    /// Create the final data shred for the current FEC set or slot
    /// If there's an active data shred, morph it into the final shred
    /// If the current active data shred is first in slot, finalize it and create a new shred
    fn make_final_data_shred(&mut self, last_in_slot: u8) {
        if self.active_shred.index() == 0 {
            self.finalize_data_shred();
        }
        self.active_shred = match self.active_shred.borrow_mut() {
            Shred::Data(s) => {
                s.header.flags |= DATA_COMPLETE_SHRED;
                if last_in_slot == LAST_SHRED_IN_SLOT {
                    s.header.flags |= LAST_SHRED_IN_SLOT;
                }
                Shred::Data(s.clone())
            }
            Shred::Coding(_) => unreachable!(),
        };
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
        shred: &ShredMetaBuf,
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
        let missing_shred = if missing < first_index + num_data {
            let mut data_shred = DataShred::default();
            data_shred.header.common_header.slot = slot;
            data_shred.header.common_header.index = missing as u32;
            Shred::Data(data_shred)
        } else {
            Shred::Coding(Self::new_coding_shred(
                slot,
                missing.saturating_sub(num_data) as u32,
                num_data,
                num_coding,
                missing - first_index - num_data,
            ))
        };
        bincode::serialize(&missing_shred).unwrap()
    }

    pub fn try_recovery(
        shreds: Vec<ShredMetaBuf>,
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
                    blocks.push(shred.shred_buf);
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

            present
                .iter()
                .enumerate()
                .for_each(|(position, was_present)| {
                    if !was_present {
                        let shred: Shred = bincode::deserialize(&shred_bufs[position]).unwrap();
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
                            } else if (first_index..first_index + num_coding).contains(&shred_index)
                            {
                                // A valid coding shred must be indexed between first_index and first+num_coding index
                                recovered_code.push(shred)
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

            let shred_bufs: Vec<Vec<u8>> = shreds
                .iter()
                .map(|shred| bincode::serialize(shred).unwrap())
                .collect();
            shred_bufs
        };

        Ok(Self::reassemble_payload(num_data, data_shred_bufs))
    }

    fn get_shred_index(shred: &ShredMetaBuf, num_data: usize) -> usize {
        if shred.data_shred {
            shred.index as usize
        } else {
            shred.index as usize + num_data
        }
    }

    fn reassemble_payload(num_data: usize, data_shred_bufs: Vec<Vec<u8>>) -> Vec<u8> {
        data_shred_bufs[..num_data]
            .iter()
            .flat_map(|data| {
                let offset = *SIZE_OF_SHRED_DATA_SHRED;
                data[offset as usize..].iter()
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 0);

        assert!(DataShred::overhead() < PACKET_DATA_SIZE);
        assert!(CodingShred::overhead() < PACKET_DATA_SIZE);

        // Test0: Write some data to shred. Not enough to create a signed shred
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 25);

        // Test1: Write some more data to shred. Not enough to create a signed shred
        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 50);

        // Test2: Write enough data to create a shred (> PACKET_DATA_SIZE)
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());
        // Assert that we have atleast one signed shred
        assert!(!shredder.shred_tuples.is_empty());
        // Assert that the new active shred was not populated
        assert_eq!(shredder.active_offset, 0);

        // Test3: Assert that the first shred in slot was created (since we gave a parent to shredder)
        let (_, shred) = &shredder.shred_tuples[0];
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        info!("Len: {}", shred.len());
        info!("{:?}", shred);

        // Test4: Try deserialize the PDU and assert that it matches the original shred
        let deserialized_shred: Shred =
            bincode::deserialize(&shred).expect("Failed in deserializing the PDU");
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 0);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        // The shreds are not signed yet, as the data is not finalized
        assert!(!deserialized_shred.verify(&keypair.pubkey()));
        let seed0 = deserialized_shred.seed();
        // Test that same seed is generated for a given shred
        assert_eq!(seed0, deserialized_shred.seed());

        // Test5: Write left over data, and assert that a data shred is being created
        shredder.write(&data[offset..]).unwrap();

        // Test6: Let's finalize the FEC block. That should result in the current shred to morph into
        // a signed LastInFECBlock shred
        shredder.finalize_data();

        // We should have a new signed shred
        assert!(!shredder.shred_tuples.is_empty());

        // Must be Last in FEC Set
        let (_, shred) = &shredder.shred_tuples[1];
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 1);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));
        // Test that same seed is NOT generated for two different shreds
        assert_ne!(seed0, deserialized_shred.seed());

        // Test7: Let's write some more data to the shredder.
        // Now we should get a new FEC block
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shred_tuples.is_empty());

        let (_, shred) = &shredder.shred_tuples[2];
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 2);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);

        // Test8: Write more data to generate an intermediate data shred
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shred_tuples.is_empty());

        // Must be a Data shred
        let (_, shred) = &shredder.shred_tuples[3];
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 3);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);

        // Test9: Write some data to shredder
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());

        // And, finish the slot
        shredder.finalize_slot();

        // We should have a new signed shred
        assert!(!shredder.shred_tuples.is_empty());

        // Must be LastInSlot
        let (_, shred) = &shredder.shred_tuples[4];
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 4);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
    }

    #[test]
    fn test_small_data_shredder() {
        let keypair = Arc::new(Keypair::new());

        let slot = 0x123456789abcdef0;
        let mut shredder =
            Shredder::new(slot, slot - 5, 0.0, &keypair, 0).expect("Failed in creating shredder");

        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..25).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();

        // We should have 0 shreds now
        assert_eq!(shredder.shred_tuples.len(), 0);

        shredder.finalize_data();

        // We should have 1 shred now
        assert_eq!(shredder.shred_tuples.len(), 2);

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 0);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 1);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let mut shredder = Shredder::new(0x123456789abcdef0, slot - 5, 0.0, &keypair, 2)
            .expect("Failed in creating shredder");

        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..25).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();

        // We should have 0 shreds now
        assert_eq!(shredder.shred_tuples.len(), 0);

        shredder.finalize_data();

        // We should have 1 shred now (LastInFECBlock)
        assert_eq!(shredder.shred_tuples.len(), 1);
        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 2);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));
    }

    #[test]
    fn test_data_and_code_shredder() {
        let keypair = Arc::new(Keypair::new());

        let slot = 0x123456789abcdef0;
        // Test that FEC rate cannot be > 1.0
        assert_matches!(Shredder::new(slot, slot - 5, 1.001, &keypair, 0), Err(_));

        let mut shredder = Shredder::new(0x123456789abcdef0, slot - 5, 1.0, &keypair, 0)
            .expect("Failed in creating shredder");

        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 0);

        // Write enough data to create a shred (> PACKET_DATA_SIZE)
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();
        let _ = shredder.write(&data).unwrap();

        // We should have 2 shreds now
        assert_eq!(shredder.shred_tuples.len(), 2);

        shredder.finalize_data();

        // Finalize must have created 1 final data shred and 3 coding shreds
        // assert_eq!(shredder.shreds.len(), 6);
        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 0);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 1);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        assert_eq!(deserialized_shred.index(), 2);
        assert_eq!(deserialized_shred.slot(), slot);
        assert_eq!(deserialized_shred.parent(), slot - 5);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Coding(_));
        assert_eq!(deserialized_shred.index(), 0);
        assert_eq!(deserialized_shred.slot(), slot);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Coding(_));
        assert_eq!(deserialized_shred.index(), 1);
        assert_eq!(deserialized_shred.slot(), slot);
        assert!(deserialized_shred.verify(&keypair.pubkey()));

        let (_, shred) = shredder.shred_tuples.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Coding(_));
        assert_eq!(deserialized_shred.index(), 2);
        assert_eq!(deserialized_shred.slot(), slot);
        assert!(deserialized_shred.verify(&keypair.pubkey()));
    }

    #[test]
    fn test_recovery_and_reassembly() {
        let keypair = Arc::new(Keypair::new());
        let slot = 0x123456789abcdef0;
        let mut shredder =
            Shredder::new(slot, slot - 5, 1.0, &keypair, 0).expect("Failed in creating shredder");

        assert!(shredder.shred_tuples.is_empty());
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
            shredder.shred_tuples.len(),
            data.len() / approx_shred_payload_size
        );
        assert_eq!(offset, data.len());

        shredder.finalize_data();

        // We should have 10 shreds now (one additional final shred, and equal number of coding shreds)
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shred_tuples.len(), expected_shred_count);

        let (shreds, shred_meta_bufs): (Vec<Shred>, Vec<ShredMetaBuf>) = shredder
            .shred_tuples
            .iter()
            .map(|(s, b)| {
                (
                    s.clone(),
                    ShredMetaBuf {
                        slot: s.slot(),
                        index: s.index(),
                        data_shred: s.is_data(),
                        shred_buf: b.clone(),
                    },
                )
            })
            .unzip();

        // Test0: Try recovery/reassembly with only data shreds, but not all data shreds. Hint: should fail
        assert_matches!(
            Shredder::try_recovery(
                shred_meta_bufs[..3].to_vec(),
                expected_shred_count / 2,
                expected_shred_count / 2,
                0,
                slot
            ),
            Err(reed_solomon_erasure::Error::TooFewShardsPresent)
        );

        // Test1: Try recovery/reassembly with only data shreds. Hint: should work
        let result = Shredder::try_recovery(
            shred_meta_bufs[..4].to_vec(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);
        assert!(result.recovered_data.is_empty());
        assert!(!result.recovered_code.is_empty());
        let result = Shredder::deshred(&shreds[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test2: Try recovery/reassembly with missing data shreds + coding shreds. Hint: should work
        let (mut shreds, shred_meta_bufs): (Vec<Shred>, Vec<ShredMetaBuf>) = shredder
            .shred_tuples
            .iter()
            .enumerate()
            .filter_map(|(i, (s, b))| {
                if i % 2 == 0 {
                    Some((
                        s.clone(),
                        ShredMetaBuf {
                            slot: s.slot(),
                            index: s.index(),
                            data_shred: s.is_data(),
                            shred_buf: b.clone(),
                        },
                    ))
                } else {
                    None
                }
            })
            .unzip();

        let mut result = Shredder::try_recovery(
            shred_meta_bufs,
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 1 and 3 were missing
        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 1);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(1, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 3);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(3, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 5, 7 were missing
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 1);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 1);
        }
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 3);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 3);
        }

        let result = Shredder::deshred(&shreds[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test3: Try recovery/reassembly with 3 missing data shreds + 2 coding shreds. Hint: should work
        let (mut shreds, shred_meta_bufs): (Vec<Shred>, Vec<ShredMetaBuf>) = shredder
            .shred_tuples
            .iter()
            .enumerate()
            .filter_map(|(i, (s, b))| {
                if i % 2 != 0 {
                    Some((
                        s.clone(),
                        ShredMetaBuf {
                            slot: s.slot(),
                            index: s.index(),
                            data_shred: s.is_data(),
                            shred_buf: b.clone(),
                        },
                    ))
                } else {
                    None
                }
            })
            .unzip();

        let mut result = Shredder::try_recovery(
            shred_meta_bufs,
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 0, 2 were missing
        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 0);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(0, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 2);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(2, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 4, 6 were missing
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 0);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 0);
        }
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 2);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 2);
        }

        let result = Shredder::deshred(&shreds[..4]).unwrap();
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
            shredder.shred_tuples.len(),
            data.len() / approx_shred_payload_size
        );
        assert_eq!(offset, data.len());

        shredder.finalize_slot();

        // We should have 10 shreds now (one additional final shred, and equal number of coding shreds)
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shred_tuples.len(), expected_shred_count);

        let (mut shreds, shred_meta_bufs): (Vec<Shred>, Vec<ShredMetaBuf>) = shredder
            .shred_tuples
            .iter()
            .enumerate()
            .filter_map(|(i, (s, b))| {
                if i % 2 != 0 {
                    Some((
                        s.clone(),
                        ShredMetaBuf {
                            slot: s.slot(),
                            index: s.index(),
                            data_shred: s.is_data(),
                            shred_buf: b.clone(),
                        },
                    ))
                } else {
                    None
                }
            })
            .unzip();

        let mut result = Shredder::try_recovery(
            shred_meta_bufs,
            expected_shred_count / 2,
            expected_shred_count / 2,
            0,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 0, 2 were missing
        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 0);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(0, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 2);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(2, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 4, 6 were missing
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 0);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 0);
        }
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 2);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 2);
        }

        let result = Shredder::deshred(&shreds[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test5: Try recovery/reassembly with 3 missing data shreds + 3 coding shreds. Hint: should fail
        let shreds: Vec<Shred> = shredder
            .shred_tuples
            .iter()
            .enumerate()
            .filter_map(|(i, (s, _))| {
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
            shredder.shred_tuples.len(),
            data.len() / approx_shred_payload_size
        );
        assert_eq!(offset, data.len());

        shredder.finalize_slot();

        // We should have 10 shreds now (one additional final shred, and equal number of coding shreds)
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shred_tuples.len(), expected_shred_count);

        let (mut shreds, shred_meta_bufs): (Vec<Shred>, Vec<ShredMetaBuf>) = shredder
            .shred_tuples
            .iter()
            .enumerate()
            .filter_map(|(i, (s, b))| {
                if i % 2 != 0 {
                    Some((
                        s.clone(),
                        ShredMetaBuf {
                            slot: s.slot(),
                            index: s.index(),
                            data_shred: s.is_data(),
                            shred_buf: b.clone(),
                        },
                    ))
                } else {
                    None
                }
            })
            .unzip();

        let mut result = Shredder::try_recovery(
            shred_meta_bufs.clone(),
            expected_shred_count / 2,
            expected_shred_count / 2,
            25,
            slot,
        )
        .unwrap();
        assert_ne!(RecoveryResult::default(), result);

        assert_eq!(result.recovered_data.len(), 2); // Data shreds 0, 2 were missing
        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 25);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(0, recovered_shred);

        let recovered_shred = result.recovered_data.remove(0);
        assert_matches!(recovered_shred, Shred::Data(_));
        assert_eq!(recovered_shred.index(), 27);
        assert_eq!(recovered_shred.slot(), slot);
        assert_eq!(recovered_shred.parent(), slot - 5);
        assert!(recovered_shred.verify(&keypair.pubkey()));
        shreds.insert(2, recovered_shred);

        assert_eq!(result.recovered_code.len(), 2); // Coding shreds 4, 6 were missing
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 0);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 25);
        }
        let recovered_shred = result.recovered_code.remove(0);
        if let Shred::Coding(code) = recovered_shred {
            assert_eq!(code.header.num_data_shreds, 4);
            assert_eq!(code.header.num_coding_shreds, 4);
            assert_eq!(code.header.position, 2);
            assert_eq!(code.header.common_header.slot, slot);
            assert_eq!(code.header.common_header.index, 27);
        }

        let result = Shredder::deshred(&shreds[..4]).unwrap();
        assert!(result.len() >= data.len());
        assert_eq!(data[..], result[..data.len()]);

        // Test7: Try recovery/reassembly with incorrect slot. Hint: does not recover any shreds
        let result = Shredder::try_recovery(
            shred_meta_bufs.clone(),
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
                shred_meta_bufs.clone(),
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
                shred_meta_bufs.clone(),
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

        assert!(shredder.shred_tuples.is_empty());
        assert_eq!(shredder.active_offset, 0);

        let data: Vec<_> = (0..MAX_DATA_SHREDS_PER_FEC_BLOCK * 1200 * 3).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let mut offset = shredder.write(&data).unwrap();
        let approx_shred_payload_size = offset;
        while offset < data.len() {
            offset += shredder.write(&data[offset..]).unwrap();
        }

        // We should have some shreds now
        assert!(shredder.shred_tuples.len() > data.len() / approx_shred_payload_size);
        assert_eq!(offset, data.len());

        shredder.finalize_data();
        let expected_shred_count = ((data.len() / approx_shred_payload_size) + 1) * 2;
        assert_eq!(shredder.shred_tuples.len(), expected_shred_count);

        let mut index = 0;

        while index < shredder.shred_tuples.len() {
            let num_data_shreds = cmp::min(
                MAX_DATA_SHREDS_PER_FEC_BLOCK as usize,
                (shredder.shred_tuples.len() - index) / 2,
            );
            let coding_start = index + num_data_shreds;
            shredder.shred_tuples[index..coding_start]
                .iter()
                .for_each(|(s, _)| assert!(s.is_data()));
            index = coding_start + num_data_shreds;
            shredder.shred_tuples[coding_start..index]
                .iter()
                .for_each(|(s, _)| assert!(!s.is_data()));
        }
    }
}
