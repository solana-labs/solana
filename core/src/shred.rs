//! The `shred` module defines data structures and methods to pull MTU sized data frames from the network.
use bincode::serialized_size;
use core::borrow::BorrowMut;
use serde::{Deserialize, Serialize};
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use std::io::Write;
use std::sync::{Arc, RwLock};
use std::{cmp, io};

pub type SharedShred = Arc<RwLock<SignedShred>>;
pub type SharedShreds = Vec<SharedShred>;
pub type Shreds = Vec<SignedShred>;

// Assume 1500 bytes MTU size
// (subtract 8 bytes of UDP header + 20 bytes ipv4 OR 40 bytes ipv6 header)
pub const MAX_DGRAM_SIZE: usize = (1500 - 48);

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct SignedShred {
    pub signature: Signature,
    pub shred: Shred,
}

impl SignedShred {
    fn new(shred: Shred) -> Self {
        SignedShred {
            signature: Signature::default(),
            shred,
        }
    }

    pub fn sign(&mut self, keypair: &Keypair) {
        let data = bincode::serialize(&self.shred).expect("Failed to serialize shred");
        self.signature = keypair.sign_message(&data);
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum Shred {
    FirstInSlot(FirstDataShred),
    FirstInFECSet(DataShred),
    Data(DataShred),
    LastInFECSetData(DataShred),
    LastInSlotData(DataShred),
    Coding(CodingShred),
    LastInFECSetCoding(CodingShred),
    LastInSlotCoding(CodingShred),
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct ShredCommonHeader {
    pub slot: u64,
    pub index: u32,
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct DataShredHeader {
    _reserved: CodingShredHeader,
    pub common_header: ShredCommonHeader,
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct FirstDataShredHeader {
    pub data_header: DataShredHeader,
    pub parent: u64,
}

#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct CodingShredHeader {
    pub common_header: ShredCommonHeader,
    pub num_data_shreds: u8,
    pub num_coding_shreds: u8,
    pub position: u8,
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
    pub payload: Vec<u8>,
}

impl Default for FirstDataShred {
    fn default() -> Self {
        let empty_shred = Shred::FirstInSlot(FirstDataShred {
            header: FirstDataShredHeader::default(),
            payload: vec![],
        });
        let size =
            MAX_DGRAM_SIZE - serialized_size(&SignedShred::new(empty_shred)).unwrap() as usize;
        FirstDataShred {
            header: FirstDataShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

impl Default for DataShred {
    fn default() -> Self {
        let empty_shred = Shred::Data(DataShred {
            header: DataShredHeader::default(),
            payload: vec![],
        });
        let size =
            MAX_DGRAM_SIZE - serialized_size(&SignedShred::new(empty_shred)).unwrap() as usize;
        DataShred {
            header: DataShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

impl Default for CodingShred {
    fn default() -> Self {
        let empty_shred = Shred::Coding(CodingShred {
            header: CodingShredHeader::default(),
            payload: vec![],
        });
        let size =
            MAX_DGRAM_SIZE - serialized_size(&SignedShred::new(empty_shred)).unwrap() as usize;
        CodingShred {
            header: CodingShredHeader::default(),
            payload: vec![0; size],
        }
    }
}

pub trait WriteAtOffset {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize;
}

impl WriteAtOffset for FirstDataShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize {
        let slice_len = cmp::min(self.payload.len().saturating_sub(offset), buf.len());
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        slice_len
    }
}

impl WriteAtOffset for DataShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize {
        let slice_len = cmp::min(self.payload.len().saturating_sub(offset), buf.len());
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        slice_len
    }
}

impl WriteAtOffset for CodingShred {
    fn write_at(&mut self, offset: usize, buf: &[u8]) -> usize {
        let slice_len = cmp::min(self.payload.len().saturating_sub(offset), buf.len());
        if slice_len > 0 {
            self.payload[offset..offset + slice_len].copy_from_slice(&buf[..slice_len]);
        }
        slice_len
    }
}

#[derive(Default)]
pub struct Shredder {
    slot: u64,
    index: u32,
    parent: Option<u64>,
    _fec_ratio: u64,
    signer: Arc<Keypair>,
    pub shreds: Shreds,
    pub active_shred: Option<SignedShred>,
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
                            SignedShred::new(Shred::FirstInSlot(self.new_first_shred(parent)))
                        })
                        .unwrap_or_else(||
                            // If parent slot is not provided, and since there's no existing shred,
                            // assume it's first shred in FEC block
                            SignedShred::new(Shred::FirstInFECSet(self.new_data_shred()))),
                )
            })
            .unwrap();

        let written = self.active_offset;
        let slice_len = match current_shred.shred.borrow_mut() {
            Shred::FirstInSlot(s) => s.write_at(written, buf),
            Shred::FirstInFECSet(s) => s.write_at(written, buf),
            Shred::Data(s) => s.write_at(written, buf),
            Shred::LastInFECSetData(s) => s.write_at(written, buf),
            Shred::LastInSlotData(s) => s.write_at(written, buf),
            Shred::Coding(s) => s.write_at(written, buf),
            Shred::LastInFECSetCoding(s) => s.write_at(written, buf),
            Shred::LastInSlotCoding(s) => s.write_at(written, buf),
        };

        let active_shred = if buf.len() > slice_len {
            self.finalize_shred(current_shred);
            // Continue generating more data shreds.
            // If the caller decides to finalize the FEC block or Slot, the data shred will
            // morph into appropriate shred accordingly
            SignedShred::new(Shred::Data(DataShred::default()))
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
        self.finalize_shred(current_shred);
        Ok(())
    }
}

impl Shredder {
    pub fn new(
        slot: u64,
        parent: Option<u64>,
        fec_ratio: u64,
        signer: &Arc<Keypair>,
        index: u32,
    ) -> Self {
        Shredder {
            slot,
            index,
            parent,
            _fec_ratio: fec_ratio,
            signer: signer.clone(),
            ..Shredder::default()
        }
    }

    pub fn finalize_shred(&mut self, mut shred: SignedShred) {
        shred.sign(&self.signer);
        self.shreds.push(shred);
        self.active_offset = 0;
        self.index += 1;
    }

    pub fn new_data_shred(&self) -> DataShred {
        let mut data_shred = DataShred::default();
        data_shred.header.common_header.slot = self.slot;
        data_shred.header.common_header.index = self.index;
        data_shred
    }

    pub fn new_first_shred(&self, parent: u64) -> FirstDataShred {
        let mut first_shred = FirstDataShred::default();
        first_shred.header.parent = parent;
        first_shred.header.data_header.common_header.slot = self.slot;
        first_shred.header.data_header.common_header.index = self.index;
        first_shred
    }

    fn make_final_data_shred(&mut self) -> DataShred {
        self.active_shred
            .take()
            .map_or(self.new_data_shred(), |current_shred| {
                match current_shred.shred {
                    Shred::FirstInSlot(s) => {
                        self.finalize_shred(SignedShred::new(Shred::FirstInSlot(s)));
                        self.new_data_shred()
                    }
                    Shred::FirstInFECSet(s) => s,
                    Shred::Data(s) => s,
                    Shred::LastInFECSetData(s) => s,
                    Shred::LastInSlotData(s) => s,
                    Shred::Coding(_) => self.new_data_shred(),
                    Shred::LastInFECSetCoding(_) => self.new_data_shred(),
                    Shred::LastInSlotCoding(_) => self.new_data_shred(),
                }
            })
    }

    pub fn finalize_fec_block(&mut self) {
        let final_shred = self.make_final_data_shred();
        self.finalize_shred(SignedShred::new(Shred::LastInFECSetData(final_shred)));
    }

    pub fn finalize_slot(&mut self) {
        let final_shred = self.make_final_data_shred();
        self.finalize_shred(SignedShred::new(Shred::LastInSlotData(final_shred)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_shredder() {
        let keypair = Arc::new(Keypair::new());
        let mut shredder = Shredder::new(0x123456789abcdef0, Some(5), 0, &keypair, 0);

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

        // Test2: Write enough data to create a shred (> MAX_DGRAM_SIZE)
        let data: Vec<_> = (0..MAX_DGRAM_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());
        // Assert that we have atleast one signed shred
        assert!(!shredder.shreds.is_empty());
        // Assert that a new active shred was also created
        assert_ne!(shredder.active_shred, None);
        // Assert that the new active shred was not populated
        assert_eq!(shredder.active_offset, 0);

        // Test3: Assert that the first shred in slot was created (since we gave a parent to shredder)
        let shred = shredder.shreds.pop().unwrap();
        assert_matches!(shred.shred, Shred::FirstInSlot(_));

        let pdu = bincode::serialize(&shred).unwrap();
        assert_eq!(pdu.len(), MAX_DGRAM_SIZE);
        info!("Len: {}", pdu.len());
        info!("{:?}", pdu);

        // Test4: Try deserialize the PDU and assert that it matches the original shred
        let deserialized_shred: SignedShred =
            bincode::deserialize(&pdu).expect("Failed in deserializing the PDU");
        assert_eq!(shred, deserialized_shred);

        // Test5: Write left over data, and assert that a data shred is being created
        shredder.write(&data[offset..]).unwrap();
        // assert_matches!(shredder.active_shred.unwrap().shred, Shred::Data(_));

        // It shouldn't generate a signed shred
        assert!(shredder.shreds.is_empty());

        // Test6: Let's finalize the FEC block. That should result in the current shred to morph into
        // a signed LastInFECSetData shred
        shredder.finalize_fec_block();

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be Last in FEC Set
        let shred = shredder.shreds.pop().unwrap();
        assert_matches!(shred.shred, Shred::LastInFECSetData(_));
        let pdu = bincode::serialize(&shred).unwrap();
        assert_eq!(pdu.len(), MAX_DGRAM_SIZE);

        // Test7: Let's write some more data to the shredder.
        // Now we should get a new FEC block
        let data: Vec<_> = (0..MAX_DGRAM_SIZE).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be FirstInFECSet
        let shred = shredder.shreds.pop().unwrap();
        assert_matches!(shred.shred, Shred::FirstInFECSet(_));
        let pdu = bincode::serialize(&shred).unwrap();
        assert_eq!(pdu.len(), MAX_DGRAM_SIZE);

        // Test8: Write more data to generate an intermediate data shred
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be a Data shred
        let shred = shredder.shreds.pop().unwrap();
        assert_matches!(shred.shred, Shred::Data(_));
        let pdu = bincode::serialize(&shred).unwrap();
        assert_eq!(pdu.len(), MAX_DGRAM_SIZE);

        // Test9: Write some data to shredder
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());

        // And, finish the slot
        shredder.finalize_slot();

        // We should have a new signed shred
        assert!(!shredder.shreds.is_empty());

        // Must be LastInSlot
        let shred = shredder.shreds.pop().unwrap();
        assert_matches!(shred.shred, Shred::LastInSlotData(_));
        let pdu = bincode::serialize(&shred).unwrap();
        assert_eq!(pdu.len(), MAX_DGRAM_SIZE);
    }
}
