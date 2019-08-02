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
            signature: Default::default(),
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
        let data: Vec<u8> = vec![];
        let overhead = serialized_size(&Signature::default()).unwrap()
            + serialized_size(&FirstDataShredHeader::default()).unwrap()
            + serialized_size(&0usize).unwrap()
            + serialized_size(&data).unwrap();
        let size = MAX_DGRAM_SIZE - overhead as usize;
        FirstDataShred {
            header: Default::default(),
            payload: vec![0; size],
        }
    }
}

impl Default for DataShred {
    fn default() -> Self {
        let data: Vec<u8> = vec![];
        let overhead = serialized_size(&Signature::default()).unwrap()
            + serialized_size(&DataShredHeader::default()).unwrap()
            + serialized_size(&0usize).unwrap()
            + serialized_size(&data).unwrap();
        let size = MAX_DGRAM_SIZE - overhead as usize;
        DataShred {
            header: Default::default(),
            payload: vec![0; size],
        }
    }
}

impl Default for CodingShred {
    fn default() -> Self {
        let data: Vec<u8> = vec![];
        let overhead = serialized_size(&Signature::default()).unwrap()
            + serialized_size(&CodingShredHeader::default()).unwrap()
            + serialized_size(&0usize).unwrap()
            + serialized_size(&data).unwrap();
        let size = MAX_DGRAM_SIZE - overhead as usize;
        CodingShred {
            header: Default::default(),
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
                Some(self.parent.map_or(
                    { SignedShred::new(Shred::Data(self.new_data_shred())) },
                    |parent| SignedShred::new(Shred::FirstInSlot(self.new_first_shred(parent))),
                ))
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
    pub fn new(slot: u64, parent: Option<u64>, fec_ratio: u64, signer: &Arc<Keypair>) -> Self {
        Shredder {
            slot,
            index: 0xff,
            parent,
            _fec_ratio: fec_ratio,
            signer: signer.clone(),
            shreds: vec![],
            active_shred: None,
            active_offset: 0,
        }
    }

    pub fn finalize_shred(&mut self, mut shred: SignedShred) {
        shred.sign(&self.signer.clone());
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
    use std::mem::size_of;
    use std::mem::size_of_val;

    #[test]
    fn test_sizes_for_shreds() {
        info!(
            "Sizeof ShredCommonHeader is {:?}",
            size_of::<ShredCommonHeader>()
        );
        info!(
            "Sizeof DataShredHeader is {:?}",
            size_of::<DataShredHeader>()
        );
        info!(
            "Sizeof FirstDataShredHeader is {:?}",
            size_of::<FirstDataShredHeader>()
        );
        info!(
            "Sizeof CodingShredHeader is {:?}",
            size_of::<CodingShredHeader>()
        );
        info!("Sizeof FirstDataShred is {:?}", size_of::<FirstDataShred>());
        info!("Sizeof DataShred is {:?}", size_of::<DataShred>());
        info!("Sizeof CodingShred is {:?}", size_of::<CodingShred>());
        info!("Sizeof Shred is {:?}", size_of::<Shred>());
        let first_shred = FirstDataShred::default();
        info!(
            "Sizeof DataShred_payload is {:?}",
            size_of_val(&first_shred.payload)
        );
    }

    #[test]
    fn test_serialize_shreds() {
        let keypair = Arc::new(Keypair::new());
        let mut shredder = Shredder::new(0x123456789abcdef0, Some(5), 0, &keypair);

        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_shred, None);
        assert_eq!(shredder.active_offset, 0);

        // Write some data to shred. Not enough to create a signed shred
        let data: Vec<u8> = (0..25).collect();
        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shreds.is_empty());
        assert_ne!(shredder.active_shred, None);
        assert_eq!(shredder.active_offset, 25);

        assert_eq!(shredder.write(&data).unwrap(), data.len());
        assert!(shredder.shreds.is_empty());
        assert_eq!(shredder.active_offset, 50);

        let data: Vec<_> = (0..1500).collect();
        let data: Vec<u8> = data.iter().map(|x| *x as u8).collect();
        let offset = shredder.write(&data).unwrap();
        assert_ne!(offset, data.len());
        assert!(!shredder.shreds.is_empty());
        assert_ne!(shredder.active_shred, None);
        assert_eq!(shredder.active_offset, 0);

        let shred = shredder.shreds.pop().unwrap();
        assert_matches!(shred.shred, Shred::FirstInSlot(_));

        let pdu = bincode::serialize(&shred).unwrap();
        info!("Len: {}", pdu.len());
        info!("{:?}", pdu);

        let deserialized_shred: SignedShred =
            bincode::deserialize(&pdu).expect("Failed in deserializing the PDU");

        assert_eq!(shred, deserialized_shred);

        shredder.write(&data[offset..]).unwrap();
        assert_matches!(shredder.active_shred.unwrap().shred, Shred::Data(_));
    }
}
