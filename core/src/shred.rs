//! The `shred` module defines data structures and methods to pull MTU sized data frames from the network.
use crate::erasure::Session;
use bincode::serialized_size;
use core::borrow::BorrowMut;
use serde::{Deserialize, Serialize};
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use std::io::Write;
use std::sync::Arc;
use std::{cmp, io};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum Shred {
    FirstInSlot(FirstDataShred),
    FirstInFECSet(DataShred),
    Data(DataShred),
    LastInFECSet(DataShred),
    LastInSlot(DataShred),
    Coding(CodingShred),
}

/// A common header that is present at start of every shred
#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct ShredCommonHeader {
    pub signature: Signature,
    pub slot: u64,
    pub index: u32,
}

/// A common header that is present at start of every data shred
#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct DataShredHeader {
    _reserved: CodingShredHeader,
    pub common_header: ShredCommonHeader,
    pub data_type: u8,
}

/// The first data shred also has parent slot value in it
#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct FirstDataShredHeader {
    pub data_header: DataShredHeader,
    pub parent: u64,
}

/// The coding shred header has FEC information
#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct CodingShredHeader {
    pub common_header: ShredCommonHeader,
    pub num_data_shreds: u8,
    pub num_coding_shreds: u8,
    pub position: u8,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct FirstDataShred {
    pub header: FirstDataShredHeader,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct DataShred {
    pub header: DataShredHeader,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct CodingShred {
    pub header: CodingShredHeader,
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for FirstDataShred {
    fn default() -> Self {
        let size = PACKET_DATA_SIZE
            - serialized_size(&Shred::FirstInSlot(Self::empty_shred())).unwrap() as usize;
        FirstDataShred {
            header: FirstDataShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for DataShred {
    fn default() -> Self {
        let size =
            PACKET_DATA_SIZE - serialized_size(&Shred::Data(Self::empty_shred())).unwrap() as usize;
        DataShred {
            header: DataShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

/// Default shred is sized correctly to meet MTU/Packet size requirements
impl Default for CodingShred {
    fn default() -> Self {
        let size = PACKET_DATA_SIZE
            - serialized_size(&Shred::Coding(Self::empty_shred())).unwrap() as usize;
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
    /// Write at a particular offset in the shred
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize;
    /// Overhead of shred enum and headers
    fn overhead() -> usize;
    /// Utility function to create an empty shred
    fn empty_shred() -> Self;
}

impl ShredCommon for FirstDataShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize {
        let slice_len = cmp::min(self.payload.len().saturating_sub(offset), buf.len());
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        slice_len
    }

    fn overhead() -> usize {
        (bincode::serialized_size(&Shred::FirstInSlot(Self::empty_shred())).unwrap()
            - bincode::serialized_size(&vec![0u8; 0]).unwrap()) as usize
    }

    fn empty_shred() -> Self {
        FirstDataShred {
            header: FirstDataShredHeader::default(),
            payload: vec![],
        }
    }
}

impl ShredCommon for DataShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize {
        let slice_len = cmp::min(self.payload.len().saturating_sub(offset), buf.len());
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        slice_len
    }

    fn overhead() -> usize {
        (bincode::serialized_size(&Shred::Data(Self::empty_shred())).unwrap()
            - bincode::serialized_size(&vec![0u8; 0]).unwrap()) as usize
    }

    fn empty_shred() -> Self {
        DataShred {
            header: DataShredHeader::default(),
            payload: vec![],
        }
    }
}

impl ShredCommon for CodingShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize {
        let slice_len = cmp::min(self.header.payload.len().saturating_sub(offset), buf.len());
        if slice_len > 0 {
            self.header.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        slice_len
    }

    fn overhead() -> usize {
        bincode::serialized_size(&Shred::Coding(Self::empty_shred())).unwrap() as usize
    }

    fn empty_shred() -> Self {
        CodingShred {
            header: CodingShredHeader::default(),
        }
    }
}

#[derive(Default)]
pub struct Shredder {
    slot: u64,
    index: u32,
    parent: Option<u64>,
    fec_rate: f32,
    signer: Arc<Keypair>,
    pub shreds: Vec<Vec<u8>>,
    pub active_shred: Option<Shred>,
    pub active_offset: usize,
}

impl Write for Shredder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut current_shred = self
            .active_shred
            .take()
            .or_else(|| {
                Some(
                    self.parent
                        .take()
                        .map(|parent| {
                            // If parent slot is provided, assume it's first shred in slot
                            Shred::FirstInSlot(self.new_first_shred(parent))
                        })
                        .unwrap_or_else(||
                            // If parent slot is not provided, and since there's no existing shred,
                            // assume it's first shred in FEC block
                            Shred::FirstInFECSet(self.new_data_shred())),
                )
            })
            .unwrap();

        let written = self.active_offset;
        let slice_len = match current_shred.borrow_mut() {
            Shred::FirstInSlot(s) => s.write_at(written, buf),
            Shred::FirstInFECSet(s)
            | Shred::Data(s)
            | Shred::LastInFECSet(s)
            | Shred::LastInSlot(s) => s.write_at(written, buf),
            Shred::Coding(s) => s.write_at(written, buf),
        };

        let active_shred = if buf.len() > slice_len {
            self.finalize_data_shred(current_shred);
            // Continue generating more data shreds.
            // If the caller decides to finalize the FEC block or Slot, the data shred will
            // morph into appropriate shred accordingly
            Shred::Data(DataShred::default())
        } else {
            self.active_offset += slice_len;
            current_shred
        };

        self.active_shred = Some(active_shred);

        Ok(slice_len)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.active_shred.is_none() {
            return Ok(());
        }
        let current_shred = self.active_shred.take().unwrap();
        self.finalize_data_shred(current_shred);
        Ok(())
    }
}

impl Shredder {
    pub fn new(
        slot: u64,
        parent: Option<u64>,
        fec_rate: f32,
        signer: &Arc<Keypair>,
        index: u32,
    ) -> Self {
        Shredder {
            slot,
            index,
            parent,
            fec_rate,
            signer: signer.clone(),
            ..Shredder::default()
        }
    }

    /// Serialize the payload, sign it and store the signature in the shred
    /// Store the signed shred in the vector of shreds
    fn finalize_shred(&mut self, mut shred: Vec<u8>, signature_offset: usize) {
        let data_offset =
            signature_offset + bincode::serialized_size(&Signature::default()).unwrap() as usize;
        let signature = bincode::serialize(&self.signer.sign_message(&shred[data_offset..]))
            .expect("Failed to generate serialized signature");
        shred[signature_offset..signature_offset + signature.len()].copy_from_slice(&signature);
        self.shreds.push(shred);
    }

    /// Finalize a data shred. Update the shred index for the next shred
    fn finalize_data_shred(&mut self, shred: Shred) {
        let data = bincode::serialize(&shred).expect("Failed to serialize shred");

        self.finalize_shred(data, CodingShred::overhead());
        self.active_offset = 0;
        self.index += 1;
    }

    /// Creates a new data shred
    fn new_data_shred(&self) -> DataShred {
        let mut data_shred = DataShred::default();
        data_shred.header.common_header.slot = self.slot;
        data_shred.header.common_header.index = self.index;
        data_shred
    }

    /// Create a new data shred that's also first in the slot
    fn new_first_shred(&self, parent: u64) -> FirstDataShred {
        let mut first_shred = FirstDataShred::default();
        first_shred.header.parent = parent;
        first_shred.header.data_header.common_header.slot = self.slot;
        first_shred.header.data_header.common_header.index = self.index;
        first_shred
    }

    fn new_coding_shred(
        slot: u64,
        index: u32,
        num_data: usize,
        num_code: usize,
        position: usize,
    ) -> CodingShred {
        let mut coding_shred = CodingShred::default();
        coding_shred.header.common_header.slot = slot;
        coding_shred.header.common_header.index = index;
        coding_shred.header.num_data_shreds = num_data as u8;
        coding_shred.header.num_coding_shreds = num_code as u8;
        coding_shred.header.position = position as u8;
        coding_shred
    }

    /// Generates coding shreds for the data shreds in the current FEC set
    fn generate_coding_shreds(&mut self) {
        if self.fec_rate != 0.0 {
            let num_data = self.shreds.len();
            let num_coding = (self.fec_rate * num_data as f32) as usize;
            let session =
                Session::new(num_data, num_coding).expect("Failed to create erasure session");
            let start_index = self.index - num_data as u32;

            // All information after "reserved" field (coding shred header) in a data shred is encoded
            let coding_block_offset = CodingShred::overhead();
            let data_ptrs: Vec<_> = self
                .shreds
                .iter()
                .map(|data| &data[coding_block_offset..])
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

            // Offset of coding shred header in the Coding Shred (i.e. overhead of enum variant)
            let coding_header_offset = (serialized_size(&Shred::Coding(CodingShred::empty_shred()))
                .unwrap()
                - serialized_size(&CodingShred::empty_shred()).unwrap())
                as usize;

            // Finalize the coding blocks (sign and append to the shred list)
            coding_shreds
                .into_iter()
                .for_each(|code| self.finalize_shred(code, coding_header_offset))
        }
    }

    /// Create the final data shred for the current FEC set or slot
    /// If there's an active data shred, morph it into the final shred
    /// If the current active data shred is first in slot, finalize it and create a new shred
    fn make_final_data_shred(&mut self) -> DataShred {
        self.active_shred.take().map_or(
            self.new_data_shred(),
            |current_shred| match current_shred {
                Shred::FirstInSlot(s) => {
                    self.finalize_data_shred(Shred::FirstInSlot(s));
                    self.new_data_shred()
                }
                Shred::FirstInFECSet(s)
                | Shred::Data(s)
                | Shred::LastInFECSet(s)
                | Shred::LastInSlot(s) => s,
                Shred::Coding(_) => self.new_data_shred(),
            },
        )
    }

    /// Finalize the current FEC block, and generate coding shreds
    pub fn finalize_fec_block(&mut self) {
        let final_shred = self.make_final_data_shred();
        self.finalize_data_shred(Shred::LastInFECSet(final_shred));
        self.generate_coding_shreds();
    }

    /// Finalize the current slot (i.e. add last slot shred) and generate coding shreds
    pub fn finalize_slot(&mut self) {
        let final_shred = self.make_final_data_shred();
        self.finalize_data_shred(Shred::LastInSlot(final_shred));
        self.generate_coding_shreds();
    }

    fn fill_in_missing_shreds(
        shred: &Shred,
        num_data: usize,
        num_coding: usize,
        slot: u64,
        first_index: usize,
        expected_index: usize,
        present: &mut [bool],
    ) -> (Vec<Vec<u8>>, bool, usize) {
        let (index, first_shred_in_slot) = Self::get_shred_index(shred, num_data);

        let mut missing_blocks: Vec<Vec<u8>> = (expected_index..index)
            .map(|missing| {
                present[missing] = false;
                let missing_shred = if missing == 0 {
                    let mut data_shred = FirstDataShred::default();
                    data_shred.header.data_header.common_header.slot = slot;
                    data_shred.header.data_header.common_header.index = 0;
                    Shred::FirstInSlot(data_shred)
                } else if missing < first_index + num_data {
                    let mut data_shred = DataShred::default();
                    data_shred.header.common_header.slot = slot;
                    data_shred.header.common_header.index = missing as u32;
                    if index == first_index + num_data - 1 {
                        Shred::LastInFECSet(data_shred)
                    } else {
                        Shred::Data(data_shred)
                    }
                } else {
                    Shred::Coding(Self::new_coding_shred(
                        slot,
                        missing as u32,
                        num_data,
                        num_coding,
                        missing - first_index - num_data,
                    ))
                };
                //vec![0u8; PACKET_DATA_SIZE]
                bincode::serialize(&missing_shred).unwrap()
            })
            .collect();
        let shred_buf = bincode::serialize(shred).unwrap();
        missing_blocks.push(shred_buf);
        (missing_blocks, first_shred_in_slot, index)
    }

    /// Combines all shreds to recreate the original buffer
    /// If the shreds include coding shreds, and if not all shreds are present, it tries
    /// to reconstruct missing shreds using erasure
    /// Note: The shreds are expected to be sorted
    /// (lower to higher index, and data shreds before coding shreds)
    pub fn deshred(
        shreds: &[Shred],
    ) -> Result<(Vec<u8>, Vec<Shred>, Vec<Shred>), reed_solomon_erasure::Error> {
        // If coding is enabled, the last shred must be a coding shred.
        let (num_data, num_coding, first_index, slot) =
            if let Shred::Coding(code) = shreds.last().unwrap() {
                (
                    code.header.num_data_shreds as usize,
                    code.header.num_coding_shreds as usize,
                    code.header.common_header.index as usize - code.header.position as usize,
                    code.header.common_header.slot,
                )
            } else {
                (shreds.len(), 0, 0, 0)
            };

        let mut recovered_data_shreds = vec![];
        let mut recovered_code_shreds = vec![];
        let fec_set_size = num_data + num_coding;
        let (data_shred_bufs, first_shred) = if num_coding > 0 && shreds.len() < fec_set_size {
            let coding_block_offset = CodingShred::overhead();

            // Let's try recovering missing shreds using erasure
            let mut present = &mut vec![true; fec_set_size];
            let mut first_shred_in_slot = false;
            let mut next_expected_index = first_index;
            let mut shred_bufs: Vec<Vec<u8>> = shreds
                .iter()
                .flat_map(|shred| {
                    let (blocks, first_shred, last_index) = Self::fill_in_missing_shreds(
                        shred,
                        num_data,
                        num_coding,
                        slot,
                        first_index,
                        next_expected_index,
                        &mut present,
                    );
                    first_shred_in_slot |= first_shred;
                    next_expected_index = last_index + 1;
                    blocks
                })
                .collect();

            let session = Session::new(num_data, num_coding).unwrap();

            let mut blocks: Vec<&mut [u8]> = shred_bufs
                .iter_mut()
                .map(|x| x[coding_block_offset..].as_mut())
                .collect();
            session.decode_blocks(&mut blocks, &present)?;

            present.iter().enumerate().for_each(|(index, was_present)| {
                if !was_present {
                    let shred: Shred = bincode::deserialize(&shred_bufs[index]).unwrap();
                    if index < first_index + num_data {
                        // ToDo: Check if the last recovered data shred is also last in Slot. If so, it needs
                        //       to be morphed into the correct type
                        recovered_data_shreds.push(shred)
                    } else {
                        recovered_code_shreds.push(shred)
                    }
                }
            });
            (shred_bufs, first_shred_in_slot)
        } else {
            let (first_index, first_shred_in_slot) =
                Shredder::get_shred_index(shreds.first().unwrap(), num_data);

            let last_index = match shreds.last().unwrap() {
                Shred::LastInFECSet(s) | Shred::LastInSlot(s) => {
                    s.header.common_header.index as usize
                }
                _ => 0,
            };

            if shreds.len() + first_index != last_index {
                return Ok((vec![], vec![], vec![]));
            }

            let shred_bufs: Vec<Vec<u8>> = shreds
                .iter()
                .map(|shred| bincode::serialize(shred).unwrap())
                .collect();
            (shred_bufs, first_shred_in_slot)
        };

        Ok((
            Self::reassemble_payload(num_data, data_shred_bufs, first_shred),
            recovered_data_shreds,
            recovered_code_shreds,
        ))
    }

    fn get_shred_index(shred: &Shred, num_data: usize) -> (usize, bool) {
        let (first_index, first_shred_in_slot) = match shred {
            Shred::FirstInSlot(s) => (s.header.data_header.common_header.index as usize, true),
            Shred::FirstInFECSet(s)
            | Shred::Data(s)
            | Shred::LastInFECSet(s)
            | Shred::LastInSlot(s) => (s.header.common_header.index as usize, false),
            Shred::Coding(s) => (s.header.common_header.index as usize + num_data, false),
        };
        (first_index, first_shred_in_slot)
    }

    fn reassemble_payload(
        num_data: usize,
        data_shred_bufs: Vec<Vec<u8>>,
        first_shred: bool,
    ) -> Vec<u8> {
        data_shred_bufs[..num_data]
            .iter()
            .enumerate()
            .flat_map(|(i, data)| {
                let offset = if i == 0 && first_shred {
                    FirstDataShred::overhead()
                } else {
                    DataShred::overhead()
                };
                data[offset..].iter()
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
        let mut shredder = Shredder::new(0x123456789abcdef0, Some(5), 0.0, &keypair, 0);

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_shred, None);
        assert_eq!(shredder.active_offset, 0);

        // Test0: Write some data to shred. Not enough to create a signed shred
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shreds.is_empty());
        assert_ne!(shredder.active_shred, None);
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
        // Assert that a new active shred was also created
        assert_ne!(shredder.active_shred, None);
        // Assert that the new active shred was not populated
        assert_eq!(shredder.active_offset, 0);

        let data_offset = CodingShred::overhead()
            + bincode::serialized_size(&Signature::default()).unwrap() as usize;

        // Test3: Assert that the first shred in slot was created (since we gave a parent to shredder)
        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        info!("Len: {}", shred.len());
        info!("{:?}", shred);

        // Test4: Try deserialize the PDU and assert that it matches the original shred
        let deserialized_shred: Shred =
            bincode::deserialize(&shred).expect("Failed in deserializing the PDU");
        assert_matches!(deserialized_shred, Shred::FirstInSlot(_));
        if let Shred::FirstInSlot(data) = deserialized_shred {
            assert!(data
                .header
                .data_header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        // Test5: Write left over data, and assert that a data shred is being created
        shredder.write(&data[offset..]).unwrap();

        // It shouldn't generate a signed shred
        assert!(shredder.shreds.is_empty());

        // Test6: Let's finalize the FEC block. That should result in the current shred to morph into
        // a signed LastInFECSetData shred
        shredder.finalize_fec_block();

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be Last in FEC Set
        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::LastInFECSet(_));
        if let Shred::LastInFECSet(data) = deserialized_shred {
            assert!(data
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        // Test7: Let's write some more data to the shredder.
        // Now we should get a new FEC block
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be FirstInFECSet
        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::FirstInFECSet(_));
        if let Shred::FirstInFECSet(data) = deserialized_shred {
            assert!(data
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        // Test8: Write more data to generate an intermediate data shred
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be a Data shred
        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        if let Shred::Data(data) = deserialized_shred {
            assert!(data
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        // Test9: Write some data to shredder
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());

        // And, finish the slot
        shredder.finalize_slot();

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be LastInSlot
        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);

        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::LastInSlot(_));
        if let Shred::LastInSlot(data) = deserialized_shred {
            assert!(data
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }
    }

    #[test]
    fn test_data_and_code_shredder() {
        let keypair = Arc::new(Keypair::new());
        let mut shredder = Shredder::new(0x123456789abcdef0, Some(5), 1.0, &keypair, 0);

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_shred, None);
        assert_eq!(shredder.active_offset, 0);

        // Write enough data to create a shred (> PACKET_DATA_SIZE)
        let data: Vec<_> = (0..PACKET_DATA_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let _ = shredder.write(&data).unwrap();
        let _ = shredder.write(&data).unwrap();

        // We should have 2 shreds now
        assert_eq!(shredder.shreds.len(), 2);

        shredder.finalize_fec_block();

        let data_offset = CodingShred::overhead()
            + bincode::serialized_size(&Signature::default()).unwrap() as usize;

        // Finalize must have created 1 final data shred and 3 coding shreds
        // assert_eq!(shredder.shreds.len(), 6);
        let shred = shredder.shreds.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::FirstInSlot(_));
        if let Shred::FirstInSlot(data) = deserialized_shred {
            assert!(data
                .header
                .data_header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        let shred = shredder.shreds.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Data(_));
        if let Shred::Data(data) = deserialized_shred {
            assert!(data
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        let shred = shredder.shreds.remove(0);
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::LastInFECSet(_));
        if let Shred::LastInFECSet(data) = deserialized_shred {
            assert!(data
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[data_offset..]));
        }

        let coding_data_offset =
            (serialized_size(&Shred::Coding(CodingShred::empty_shred())).unwrap()
                - serialized_size(&CodingShred::empty_shred()).unwrap()
                + serialized_size(&Signature::default()).unwrap()) as usize as usize;

        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Coding(_));
        if let Shred::Coding(code) = deserialized_shred {
            assert!(code
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[coding_data_offset..]));
        }

        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Coding(_));
        if let Shred::Coding(code) = deserialized_shred {
            assert!(code
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[coding_data_offset..]));
        }

        let shred = shredder.shreds.pop().unwrap();
        assert_eq!(shred.len(), PACKET_DATA_SIZE);
        let deserialized_shred: Shred = bincode::deserialize(&shred).unwrap();
        assert_matches!(deserialized_shred, Shred::Coding(_));
        if let Shred::Coding(code) = deserialized_shred {
            assert!(code
                .header
                .common_header
                .signature
                .verify(keypair.pubkey().as_ref(), &shred[coding_data_offset..]));
        }
    }
}
