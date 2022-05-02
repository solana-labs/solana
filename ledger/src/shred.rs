//! The `shred` module defines data structures and methods to pull MTU sized data frames from the
//! network. There are two types of shreds: data and coding. Data shreds contain entry information
//! while coding shreds provide redundancy to protect against dropped network packets (erasures).
//!
//! +---------------------------------------------------------------------------------------------+
//! | Data Shred                                                                                  |
//! +---------------------------------------------------------------------------------------------+
//! | common       | data       | payload                                                         |
//! | header       | header     |                                                                 |
//! |+---+---+---  |+---+---+---|+----------------------------------------------------------+----+|
//! || s | s | .   || p | f | s || data (ie ledger entries)                                 | r  ||
//! || i | h | .   || a | l | i ||                                                          | e  ||
//! || g | r | .   || r | a | z || See notes immediately after shred diagrams for an        | s  ||
//! || n | e |     || e | g | e || explanation of the "restricted" section in this payload  | t  ||
//! || a | d |     || n | s |   ||                                                          | r  ||
//! || t |   |     || t |   |   ||                                                          | i  ||
//! || u | t |     ||   |   |   ||                                                          | c  ||
//! || r | y |     || o |   |   ||                                                          | t  ||
//! || e | p |     || f |   |   ||                                                          | e  ||
//! ||   | e |     || f |   |   ||                                                          | d  ||
//! |+---+---+---  |+---+---+---+|----------------------------------------------------------+----+|
//! +---------------------------------------------------------------------------------------------+
//!
//! +---------------------------------------------------------------------------------------------+
//! | Coding Shred                                                                                |
//! +---------------------------------------------------------------------------------------------+
//! | common       | coding     | payload                                                         |
//! | header       | header     |                                                                 |
//! |+---+---+---  |+---+---+---+----------------------------------------------------------------+|
//! || s | s | .   || n | n | p || data (encoded data shred data)                                ||
//! || i | h | .   || u | u | o ||                                                               ||
//! || g | r | .   || m | m | s ||                                                               ||
//! || n | e |     ||   |   | i ||                                                               ||
//! || a | d |     || d | c | t ||                                                               ||
//! || t |   |     ||   |   | i ||                                                               ||
//! || u | t |     || s | s | o ||                                                               ||
//! || r | y |     || h | h | n ||                                                               ||
//! || e | p |     || r | r |   ||                                                               ||
//! ||   | e |     || e | e |   ||                                                               ||
//! ||   |   |     || d | d |   ||                                                               ||
//! |+---+---+---  |+---+---+---+|+--------------------------------------------------------------+|
//! +---------------------------------------------------------------------------------------------+
//!
//! Notes:
//! a) Coding shreds encode entire data shreds: both of the headers AND the payload.
//! b) Coding shreds require their own headers for identification and etc.
//! c) The erasure algorithm requires data shred and coding shred bytestreams to be equal in length.
//!
//! So, given a) - c), we must restrict data shred's payload length such that the entire coding
//! payload can fit into one coding shred / packet.

pub use crate::{
    shred_stats::{ProcessShredsStats, ShredFetchStats},
    shredder::Shredder,
};
use {
    crate::blockstore::MAX_DATA_SHREDS_PER_SLOT,
    bitflags::bitflags,
    num_enum::{IntoPrimitive, TryFromPrimitive},
    serde::{Deserialize, Serialize},
    solana_entry::entry::{create_ticks, Entry},
    solana_perf::packet::{limited_deserialize, Packet},
    solana_sdk::{
        clock::Slot,
        hash::{hashv, Hash},
        packet::PACKET_DATA_SIZE,
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
    },
    std::{fmt::Debug, io::Cursor, mem::size_of, ops::RangeInclusive},
    thiserror::Error,
};

pub type Nonce = u32;

/// The following constants are computed by hand, and hardcoded.
/// `test_shred_constants` ensures that the values are correct.
/// Constants are used over lazy_static for performance reasons.
const SIZE_OF_COMMON_SHRED_HEADER: usize = 83;
const SIZE_OF_DATA_SHRED_HEADER: usize = 5;
const SIZE_OF_CODING_SHRED_HEADER: usize = 6;
const SIZE_OF_SIGNATURE: usize = 64;
const SIZE_OF_SHRED_TYPE: usize = 1;
const SIZE_OF_SHRED_SLOT: usize = 8;
const SIZE_OF_SHRED_INDEX: usize = 4;
pub const SIZE_OF_NONCE: usize = 4;
const SIZE_OF_CODING_SHRED_HEADERS: usize =
    SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_CODING_SHRED_HEADER;
pub const SIZE_OF_DATA_SHRED_PAYLOAD: usize = PACKET_DATA_SIZE
    - SIZE_OF_COMMON_SHRED_HEADER
    - SIZE_OF_DATA_SHRED_HEADER
    - SIZE_OF_CODING_SHRED_HEADERS
    - SIZE_OF_NONCE;
const SHRED_DATA_OFFSET: usize = SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_DATA_SHRED_HEADER;
// DataShredHeader.size is sum of common-shred-header, data-shred-header and
// data.len(). Broadcast stage may create zero length data shreds when the
// previous slot was interrupted:
// https://github.com/solana-labs/solana/blob/2d4defa47/core/src/broadcast_stage/standard_broadcast_run.rs#L79
const DATA_SHRED_SIZE_RANGE: RangeInclusive<usize> =
    SHRED_DATA_OFFSET..=SHRED_DATA_OFFSET + SIZE_OF_DATA_SHRED_PAYLOAD;

const OFFSET_OF_SHRED_TYPE: usize = SIZE_OF_SIGNATURE;
const OFFSET_OF_SHRED_SLOT: usize = SIZE_OF_SIGNATURE + SIZE_OF_SHRED_TYPE;
const OFFSET_OF_SHRED_INDEX: usize = OFFSET_OF_SHRED_SLOT + SIZE_OF_SHRED_SLOT;
const SHRED_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE - SIZE_OF_NONCE;
// SIZE_OF_CODING_SHRED_HEADERS bytes at the end of data shreds
// is never used and is not part of erasure coding.
const ENCODED_PAYLOAD_SIZE: usize = SHRED_PAYLOAD_SIZE - SIZE_OF_CODING_SHRED_HEADERS;

pub const MAX_DATA_SHREDS_PER_FEC_BLOCK: u32 = 32;

bitflags! {
    #[derive(Default, Serialize, Deserialize)]
    pub struct ShredFlags:u8 {
        const SHRED_TICK_REFERENCE_MASK = 0b0011_1111;
        const DATA_COMPLETE_SHRED = 0b0100_0000;
        const LAST_SHRED_IN_SLOT = 0b1000_0000;
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    BincodeError(#[from] bincode::Error),
    #[error(transparent)]
    ErasureError(#[from] reed_solomon_erasure::Error),
    #[error("Invalid data shred index: {0}")]
    InvalidDataShredIndex(/*shred index:*/ u32),
    #[error("Invalid data size: {size}, payload: {payload}")]
    InvalidDataSize { size: u16, payload: usize },
    #[error("Invalid erasure shard index: {0:?}")]
    InvalidErasureShardIndex(/*headers:*/ Box<dyn Debug>),
    #[error("Invalid num coding shreds: {0}")]
    InvalidNumCodingShreds(u16),
    #[error("Invalid parent_offset: {parent_offset}, slot: {slot}")]
    InvalidParentOffset { slot: Slot, parent_offset: u16 },
    #[error("Invalid parent slot: {parent_slot}, slot: {slot}")]
    InvalidParentSlot { slot: Slot, parent_slot: Slot },
    #[error("Invalid payload size: {0}")]
    InvalidPayloadSize(/*payload size:*/ usize),
    #[error("Invalid shred type")]
    InvalidShredType,
}

#[repr(u8)]
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    PartialEq,
    AbiEnumVisitor,
    AbiExample,
    Deserialize,
    IntoPrimitive,
    Serialize,
    TryFromPrimitive,
)]
#[serde(into = "u8", try_from = "u8")]
pub enum ShredType {
    Data = 0b1010_0101,
    Code = 0b0101_1010,
}

/// A common header that is present in data and code shred headers
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
struct ShredCommonHeader {
    signature: Signature,
    shred_type: ShredType,
    slot: Slot,
    index: u32,
    version: u16,
    fec_set_index: u32,
}

/// The data shred header has parent offset and flags
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
struct DataShredHeader {
    parent_offset: u16,
    flags: ShredFlags,
    size: u16, // common shred header + data shred header + data
}

/// The coding shred header has FEC information
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
struct CodingShredHeader {
    num_data_shreds: u16,
    num_coding_shreds: u16,
    position: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shred {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    coding_header: CodingShredHeader,
    payload: Vec<u8>,
}

/// Tuple which uniquely identifies a shred should it exists.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ShredId(Slot, /*shred index:*/ u32, ShredType);

impl ShredId {
    pub(crate) fn new(slot: Slot, index: u32, shred_type: ShredType) -> ShredId {
        ShredId(slot, index, shred_type)
    }

    pub(crate) fn unwrap(&self) -> (Slot, /*shred index:*/ u32, ShredType) {
        (self.0, self.1, self.2)
    }
}

/// Tuple which identifies erasure coding set that the shred belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ErasureSetId(Slot, /*fec_set_index:*/ u32);

impl ErasureSetId {
    pub(crate) fn slot(&self) -> Slot {
        self.0
    }

    // Storage key for ErasureMeta in blockstore db.
    pub(crate) fn store_key(&self) -> (Slot, /*fec_set_index:*/ u64) {
        (self.0, u64::from(self.1))
    }
}

impl Shred {
    pub fn copy_to_packet(&self, packet: &mut Packet) {
        let len = self.payload.len();
        packet.data[..len].copy_from_slice(&self.payload[..]);
        packet.meta.size = len;
    }

    // TODO: Should this sanitize output?
    pub fn new_from_data(
        slot: Slot,
        index: u32,
        parent_offset: u16,
        data: &[u8],
        is_last_data: bool,
        is_last_in_slot: bool,
        reference_tick: u8,
        version: u16,
        fec_set_index: u32,
    ) -> Self {
        let mut payload = vec![0; SHRED_PAYLOAD_SIZE];
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_type: ShredType::Data,
            slot,
            index,
            version,
            fec_set_index,
        };

        let size = (data.len() + SIZE_OF_DATA_SHRED_HEADER + SIZE_OF_COMMON_SHRED_HEADER) as u16;
        let mut data_header = DataShredHeader {
            parent_offset,
            flags: unsafe {
                ShredFlags::from_bits_unchecked(
                    ShredFlags::SHRED_TICK_REFERENCE_MASK
                        .bits()
                        .min(reference_tick),
                )
            },
            size,
        };

        if is_last_data {
            data_header.flags |= ShredFlags::DATA_COMPLETE_SHRED
        }

        if is_last_in_slot {
            data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT
        }

        let mut cursor = Cursor::new(&mut payload[..]);
        bincode::serialize_into(&mut cursor, &common_header).unwrap();
        bincode::serialize_into(&mut cursor, &data_header).unwrap();
        // TODO: Need to check if data is too large!
        let offset = cursor.position() as usize;
        debug_assert_eq!(offset, SHRED_DATA_OFFSET);
        payload[offset..offset + data.len()].copy_from_slice(data);
        Self {
            common_header,
            data_header,
            coding_header: CodingShredHeader::default(),
            payload,
        }
    }

    pub fn new_from_serialized_shred(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = bincode::deserialize_from(&mut cursor)?;
        let (data_header, coding_header) = match common_header.shred_type {
            ShredType::Code => {
                let coding_header = bincode::deserialize_from(&mut cursor)?;
                // see: https://github.com/solana-labs/solana/pull/10109
                payload.truncate(SHRED_PAYLOAD_SIZE);
                (DataShredHeader::default(), coding_header)
            }
            ShredType::Data => {
                let data_header = bincode::deserialize_from(&mut cursor)?;
                // see: https://github.com/solana-labs/solana/pull/16602
                payload.resize(SHRED_PAYLOAD_SIZE, 0u8);
                (data_header, CodingShredHeader::default())
            }
        };
        let shred = Self {
            common_header,
            data_header,
            coding_header,
            payload,
        };
        shred.sanitize().map(|_| shred)
    }

    pub fn new_from_parity_shard(
        slot: Slot,
        index: u32,
        parity_shard: &[u8],
        fec_set_index: u32,
        num_data_shreds: u16,
        num_coding_shreds: u16,
        position: u16,
        version: u16,
    ) -> Self {
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_type: ShredType::Code,
            index,
            slot,
            version,
            fec_set_index,
        };
        let coding_header = CodingShredHeader {
            num_data_shreds,
            num_coding_shreds,
            position,
        };
        let mut payload = vec![0; SHRED_PAYLOAD_SIZE];
        let mut cursor = Cursor::new(&mut payload[..]);
        bincode::serialize_into(&mut cursor, &common_header).unwrap();
        bincode::serialize_into(&mut cursor, &coding_header).unwrap();
        // Tests may have an empty parity_shard.
        if !parity_shard.is_empty() {
            let offset = cursor.position() as usize;
            debug_assert_eq!(offset, SIZE_OF_CODING_SHRED_HEADERS);
            payload[offset..].copy_from_slice(parity_shard);
        }
        Shred {
            common_header,
            data_header: DataShredHeader::default(),
            coding_header,
            payload,
        }
    }

    /// Unique identifier for each shred.
    pub fn id(&self) -> ShredId {
        ShredId(self.slot(), self.index(), self.shred_type())
    }

    pub fn slot(&self) -> Slot {
        self.common_header.slot
    }

    pub fn parent(&self) -> Result<Slot, Error> {
        match self.shred_type() {
            ShredType::Data => {
                let slot = self.slot();
                let parent_offset = self.data_header.parent_offset;
                if parent_offset == 0 && slot != 0 {
                    return Err(Error::InvalidParentOffset {
                        slot,
                        parent_offset,
                    });
                }
                slot.checked_sub(Slot::from(parent_offset))
                    .ok_or(Error::InvalidParentOffset {
                        slot,
                        parent_offset,
                    })
            }
            ShredType::Code => Err(Error::InvalidShredType),
        }
    }

    pub fn index(&self) -> u32 {
        self.common_header.index
    }

    pub(crate) fn data(&self) -> Result<&[u8], Error> {
        match self.shred_type() {
            ShredType::Code => Err(Error::InvalidShredType),
            ShredType::Data => {
                let size = usize::from(self.data_header.size);
                if size > self.payload.len() || !DATA_SHRED_SIZE_RANGE.contains(&size) {
                    return Err(Error::InvalidDataSize {
                        size: self.data_header.size,
                        payload: self.payload.len(),
                    });
                }
                Ok(&self.payload[SHRED_DATA_OFFSET..size])
            }
        }
    }

    #[inline]
    pub fn payload(&self) -> &Vec<u8> {
        &self.payload
    }

    // Possibly trimmed payload;
    // Should only be used when storing shreds to blockstore.
    pub(crate) fn bytes_to_store(&self) -> &[u8] {
        match self.shred_type() {
            ShredType::Code => &self.payload,
            ShredType::Data => {
                // Payload will be padded out to SHRED_PAYLOAD_SIZE.
                // But only need to store the bytes within data_header.size.
                &self.payload[..self.data_header.size as usize]
            }
        }
    }

    // Possibly zero pads bytes stored in blockstore.
    pub(crate) fn resize_stored_shred(mut shred: Vec<u8>) -> Result<Vec<u8>, Error> {
        let shred_type = match shred.get(OFFSET_OF_SHRED_TYPE) {
            None => return Err(Error::InvalidPayloadSize(shred.len())),
            Some(shred_type) => match ShredType::try_from(*shred_type) {
                Err(_) => return Err(Error::InvalidShredType),
                Ok(shred_type) => shred_type,
            },
        };
        match shred_type {
            ShredType::Code => {
                if shred.len() != SHRED_PAYLOAD_SIZE {
                    return Err(Error::InvalidPayloadSize(shred.len()));
                }
                Ok(shred)
            }
            ShredType::Data => {
                if !(SHRED_DATA_OFFSET..SHRED_PAYLOAD_SIZE).contains(&shred.len()) {
                    return Err(Error::InvalidPayloadSize(shred.len()));
                }
                shred.resize(SHRED_PAYLOAD_SIZE, 0u8);
                Ok(shred)
            }
        }
    }

    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }

    pub fn fec_set_index(&self) -> u32 {
        self.common_header.fec_set_index
    }

    pub(crate) fn first_coding_index(&self) -> Option<u32> {
        match self.shred_type() {
            ShredType::Data => None,
            ShredType::Code => {
                let position = u32::from(self.coding_header.position);
                self.index().checked_sub(position)
            }
        }
    }

    // Returns true if the shred passes sanity checks.
    pub fn sanitize(&self) -> Result<(), Error> {
        if self.payload().len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        if self.erasure_shard_index().is_none() {
            let headers: Box<dyn Debug> = match self.shred_type() {
                ShredType::Data => Box::new((self.common_header, self.data_header)),
                ShredType::Code => Box::new((self.common_header, self.coding_header)),
            };
            return Err(Error::InvalidErasureShardIndex(headers));
        }
        match self.shred_type() {
            ShredType::Data => {
                if self.index() as usize >= MAX_DATA_SHREDS_PER_SLOT {
                    return Err(Error::InvalidDataShredIndex(self.index()));
                }
                let _parent = self.parent()?;
                let size = usize::from(self.data_header.size);
                if size > self.payload.len() || !DATA_SHRED_SIZE_RANGE.contains(&size) {
                    return Err(Error::InvalidDataSize {
                        size: self.data_header.size,
                        payload: self.payload.len(),
                    });
                }
            }
            ShredType::Code => {
                let num_coding_shreds = u32::from(self.coding_header.num_coding_shreds);
                if num_coding_shreds > 8 * MAX_DATA_SHREDS_PER_FEC_BLOCK {
                    return Err(Error::InvalidNumCodingShreds(
                        self.coding_header.num_coding_shreds,
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn version(&self) -> u16 {
        self.common_header.version
    }

    // Identifier for the erasure coding set that the shred belongs to.
    pub(crate) fn erasure_set(&self) -> ErasureSetId {
        ErasureSetId(self.slot(), self.fec_set_index())
    }

    // Returns the shard index within the erasure coding set.
    pub(crate) fn erasure_shard_index(&self) -> Option<usize> {
        match self.shred_type() {
            ShredType::Data => {
                let index = self.index().checked_sub(self.fec_set_index())?;
                usize::try_from(index).ok()
            }
            ShredType::Code => {
                // Assert that the last shred index in the erasure set does not
                // overshoot u32.
                self.fec_set_index().checked_add(u32::from(
                    self.coding_header.num_data_shreds.checked_sub(1)?,
                ))?;
                self.first_coding_index()?.checked_add(u32::from(
                    self.coding_header.num_coding_shreds.checked_sub(1)?,
                ))?;
                let num_data_shreds = usize::from(self.coding_header.num_data_shreds);
                let num_coding_shreds = usize::from(self.coding_header.num_coding_shreds);
                let position = usize::from(self.coding_header.position);
                let fec_set_size = num_data_shreds.checked_add(num_coding_shreds)?;
                let index = position.checked_add(num_data_shreds)?;
                (index < fec_set_size).then(|| index)
            }
        }
    }

    // Returns the portion of the shred's payload which is erasure coded.
    pub(crate) fn erasure_shard(self) -> Result<Vec<u8>, Error> {
        if self.payload.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let shred_type = self.shred_type();
        let mut shard = self.payload;
        match shred_type {
            ShredType::Data => {
                shard.resize(ENCODED_PAYLOAD_SIZE, 0u8);
            }
            ShredType::Code => {
                // SIZE_OF_CODING_SHRED_HEADERS bytes at the beginning of the
                // coding shreds contains the header and is not part of erasure
                // coding.
                shard.drain(..SIZE_OF_CODING_SHRED_HEADERS);
            }
        }
        Ok(shard)
    }

    // Like Shred::erasure_shard but returning a slice.
    pub(crate) fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        Ok(match self.shred_type() {
            ShredType::Data => &self.payload[..ENCODED_PAYLOAD_SIZE],
            ShredType::Code => &self.payload[SIZE_OF_CODING_SHRED_HEADERS..],
        })
    }

    pub fn set_index(&mut self, index: u32) {
        self.common_header.index = index;
        bincode::serialize_into(&mut self.payload[..], &self.common_header).unwrap();
    }

    pub fn set_slot(&mut self, slot: Slot) {
        self.common_header.slot = slot;
        bincode::serialize_into(&mut self.payload[..], &self.common_header).unwrap();
    }

    pub fn signature(&self) -> Signature {
        self.common_header.signature
    }

    pub fn sign(&mut self, keypair: &Keypair) {
        let signature = keypair.sign_message(&self.payload[SIZE_OF_SIGNATURE..]);
        bincode::serialize_into(&mut self.payload[..SIZE_OF_SIGNATURE], &signature)
            .expect("Failed to generate serialized signature");
        self.common_header.signature = signature;
    }

    pub fn seed(&self, leader_pubkey: Pubkey) -> [u8; 32] {
        hashv(&[
            &self.slot().to_le_bytes(),
            &self.index().to_le_bytes(),
            &leader_pubkey.to_bytes(),
        ])
        .to_bytes()
    }

    #[inline]
    pub fn shred_type(&self) -> ShredType {
        self.common_header.shred_type
    }

    pub fn is_data(&self) -> bool {
        self.shred_type() == ShredType::Data
    }
    pub fn is_code(&self) -> bool {
        self.shred_type() == ShredType::Code
    }

    pub fn last_in_slot(&self) -> bool {
        if self.is_data() {
            self.data_header
                .flags
                .contains(ShredFlags::LAST_SHRED_IN_SLOT)
        } else {
            false
        }
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_last_in_slot(&mut self) {
        if self.is_data() {
            self.data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT
        }
        let buffer = &mut self.payload[SIZE_OF_COMMON_SHRED_HEADER..];
        bincode::serialize_into(buffer, &self.data_header).unwrap();
    }

    #[cfg(test)]
    pub(crate) fn unset_data_complete(&mut self) {
        if self.is_data() {
            self.data_header
                .flags
                .remove(ShredFlags::DATA_COMPLETE_SHRED);
        }
        // Data header starts after the shred common header
        let buffer = &mut self.payload[SIZE_OF_COMMON_SHRED_HEADER..];
        bincode::serialize_into(buffer, &self.data_header).unwrap();
    }

    pub fn data_complete(&self) -> bool {
        if self.is_data() {
            self.data_header
                .flags
                .contains(ShredFlags::DATA_COMPLETE_SHRED)
        } else {
            false
        }
    }

    pub(crate) fn reference_tick(&self) -> u8 {
        if self.is_data() {
            self.data_header.flags & ShredFlags::SHRED_TICK_REFERENCE_MASK
        } else {
            ShredFlags::SHRED_TICK_REFERENCE_MASK
        }
        .bits()
    }

    // Get slot from a shred packet with partial deserialize
    pub fn get_slot_from_packet(p: &Packet) -> Option<Slot> {
        let slot_start = OFFSET_OF_SHRED_SLOT;
        let slot_end = slot_start + SIZE_OF_SHRED_SLOT;

        if slot_end > p.meta.size {
            return None;
        }

        limited_deserialize::<Slot>(&p.data[slot_start..slot_end]).ok()
    }

    pub(crate) fn reference_tick_from_data(data: &[u8]) -> u8 {
        let flags = data[SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_DATA_SHRED_HEADER
            - size_of::<u8>()
            - size_of::<u16>()];
        flags & ShredFlags::SHRED_TICK_REFERENCE_MASK.bits()
    }

    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        self.signature()
            .verify(pubkey.as_ref(), &self.payload[SIZE_OF_SIGNATURE..])
    }

    // Returns true if the erasure coding of the two shreds mismatch.
    pub(crate) fn erasure_mismatch(self: &Shred, other: &Shred) -> Result<bool, Error> {
        match (self.shred_type(), other.shred_type()) {
            (ShredType::Code, ShredType::Code) => {
                let CodingShredHeader {
                    num_data_shreds,
                    num_coding_shreds,
                    position: _,
                } = self.coding_header;
                Ok(num_coding_shreds != other.coding_header.num_coding_shreds
                    || num_data_shreds != other.coding_header.num_data_shreds
                    || self.first_coding_index() != other.first_coding_index())
            }
            _ => Err(Error::InvalidShredType),
        }
    }

    pub(crate) fn num_data_shreds(self: &Shred) -> Result<u16, Error> {
        match self.shred_type() {
            ShredType::Data => Err(Error::InvalidShredType),
            ShredType::Code => Ok(self.coding_header.num_data_shreds),
        }
    }

    pub(crate) fn num_coding_shreds(self: &Shred) -> Result<u16, Error> {
        match self.shred_type() {
            ShredType::Data => Err(Error::InvalidShredType),
            ShredType::Code => Ok(self.coding_header.num_coding_shreds),
        }
    }
}

// Get slot, index, and type from a packet with partial deserialize
pub fn get_shred_slot_index_type(
    p: &Packet,
    stats: &mut ShredFetchStats,
) -> Option<(Slot, u32, ShredType)> {
    let index_start = OFFSET_OF_SHRED_INDEX;
    let index_end = index_start + SIZE_OF_SHRED_INDEX;
    let slot_start = OFFSET_OF_SHRED_SLOT;
    let slot_end = slot_start + SIZE_OF_SHRED_SLOT;

    debug_assert!(index_end > slot_end);
    debug_assert!(index_end > OFFSET_OF_SHRED_TYPE);

    if index_end > p.meta.size {
        stats.index_overrun += 1;
        return None;
    }

    let index = match limited_deserialize::<u32>(&p.data[index_start..index_end]) {
        Ok(x) => x,
        Err(_e) => {
            stats.index_bad_deserialize += 1;
            return None;
        }
    };

    if index >= MAX_DATA_SHREDS_PER_SLOT as u32 {
        stats.index_out_of_bounds += 1;
        return None;
    }

    let slot = match limited_deserialize::<Slot>(&p.data[slot_start..slot_end]) {
        Ok(x) => x,
        Err(_e) => {
            stats.slot_bad_deserialize += 1;
            return None;
        }
    };

    let shred_type = match ShredType::try_from(p.data[OFFSET_OF_SHRED_TYPE]) {
        Err(_) => {
            stats.bad_shred_type += 1;
            return None;
        }
        Ok(shred_type) => shred_type,
    };
    Some((slot, index, shred_type))
}

pub fn max_ticks_per_n_shreds(num_shreds: u64, shred_data_size: Option<usize>) -> u64 {
    let ticks = create_ticks(1, 0, Hash::default());
    max_entries_per_n_shred(&ticks[0], num_shreds, shred_data_size)
}

pub fn max_entries_per_n_shred(
    entry: &Entry,
    num_shreds: u64,
    shred_data_size: Option<usize>,
) -> u64 {
    let shred_data_size = shred_data_size.unwrap_or(SIZE_OF_DATA_SHRED_PAYLOAD) as u64;
    let vec_size = bincode::serialized_size(&vec![entry]).unwrap();
    let entry_size = bincode::serialized_size(entry).unwrap();
    let count_size = vec_size - entry_size;

    (shred_data_size * num_shreds - count_size) / entry_size
}

pub fn verify_test_data_shred(
    shred: &Shred,
    index: u32,
    slot: Slot,
    parent: Slot,
    pk: &Pubkey,
    verify: bool,
    is_last_in_slot: bool,
    is_last_data: bool,
) {
    shred.sanitize().unwrap();
    assert_eq!(shred.payload.len(), SHRED_PAYLOAD_SIZE);
    assert!(shred.is_data());
    assert_eq!(shred.index(), index);
    assert_eq!(shred.slot(), slot);
    assert_eq!(shred.parent().unwrap(), parent);
    assert_eq!(verify, shred.verify(pk));
    if is_last_in_slot {
        assert!(shred.last_in_slot());
    } else {
        assert!(!shred.last_in_slot());
    }
    if is_last_data {
        assert!(shred.data_complete());
    } else {
        assert!(!shred.data_complete());
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bincode::serialized_size,
        matches::assert_matches,
        rand::Rng,
        rand_chacha::{rand_core::SeedableRng, ChaChaRng},
        solana_sdk::shred_version,
    };

    fn bs58_decode<T: AsRef<[u8]>>(data: T) -> Vec<u8> {
        bs58::decode(data).into_vec().unwrap()
    }

    #[test]
    fn test_shred_constants() {
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_type: ShredType::Code,
            slot: Slot::MAX,
            index: u32::MAX,
            version: u16::MAX,
            fec_set_index: u32::MAX,
        };
        assert_eq!(
            SIZE_OF_COMMON_SHRED_HEADER,
            serialized_size(&common_header).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_CODING_SHRED_HEADER,
            serialized_size(&CodingShredHeader::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_DATA_SHRED_HEADER,
            serialized_size(&DataShredHeader::default()).unwrap() as usize
        );
        let data_shred_header_with_size = DataShredHeader {
            size: 1000,
            ..DataShredHeader::default()
        };
        assert_eq!(
            SIZE_OF_DATA_SHRED_HEADER,
            serialized_size(&data_shred_header_with_size).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SIGNATURE,
            bincode::serialized_size(&Signature::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SHRED_TYPE,
            bincode::serialized_size(&ShredType::Code).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SHRED_SLOT,
            bincode::serialized_size(&Slot::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SHRED_INDEX,
            bincode::serialized_size(&common_header.index).unwrap() as usize
        );
    }

    #[test]
    fn test_version_from_hash() {
        let hash = [
            0xa5u8, 0xa5, 0x5a, 0x5a, 0xa5, 0xa5, 0x5a, 0x5a, 0xa5, 0xa5, 0x5a, 0x5a, 0xa5, 0xa5,
            0x5a, 0x5a, 0xa5, 0xa5, 0x5a, 0x5a, 0xa5, 0xa5, 0x5a, 0x5a, 0xa5, 0xa5, 0x5a, 0x5a,
            0xa5, 0xa5, 0x5a, 0x5a,
        ];
        let version = shred_version::version_from_hash(&Hash::new(&hash));
        assert_eq!(version, 1);
        let hash = [
            0xa5u8, 0xa5, 0x5a, 0x5a, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let version = shred_version::version_from_hash(&Hash::new(&hash));
        assert_eq!(version, 0xffff);
        let hash = [
            0xa5u8, 0xa5, 0x5a, 0x5a, 0xa5, 0xa5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let version = shred_version::version_from_hash(&Hash::new(&hash));
        assert_eq!(version, 0x5a5b);
    }

    #[test]
    fn test_invalid_parent_offset() {
        let shred = Shred::new_from_data(10, 0, 1000, &[1, 2, 3], false, false, 0, 1, 0);
        let mut packet = Packet::default();
        shred.copy_to_packet(&mut packet);
        let shred_res = Shred::new_from_serialized_shred(packet.data.to_vec());
        assert_matches!(
            shred.parent(),
            Err(Error::InvalidParentOffset {
                slot: 10,
                parent_offset: 1000
            })
        );
        assert_matches!(
            shred_res,
            Err(Error::InvalidParentOffset {
                slot: 10,
                parent_offset: 1000
            })
        );
    }

    #[test]
    fn test_shred_offsets() {
        solana_logger::setup();
        let mut packet = Packet::default();
        let shred = Shred::new_from_data(1, 3, 0, &[], true, true, 0, 0, 0);
        shred.copy_to_packet(&mut packet);
        let mut stats = ShredFetchStats::default();
        let ret = get_shred_slot_index_type(&packet, &mut stats);
        assert_eq!(Some((1, 3, ShredType::Data)), ret);
        assert_eq!(stats, ShredFetchStats::default());

        packet.meta.size = OFFSET_OF_SHRED_TYPE;
        assert_eq!(None, get_shred_slot_index_type(&packet, &mut stats));
        assert_eq!(stats.index_overrun, 1);

        packet.meta.size = OFFSET_OF_SHRED_INDEX;
        assert_eq!(None, get_shred_slot_index_type(&packet, &mut stats));
        assert_eq!(stats.index_overrun, 2);

        packet.meta.size = OFFSET_OF_SHRED_INDEX + 1;
        assert_eq!(None, get_shred_slot_index_type(&packet, &mut stats));
        assert_eq!(stats.index_overrun, 3);

        packet.meta.size = OFFSET_OF_SHRED_INDEX + SIZE_OF_SHRED_INDEX - 1;
        assert_eq!(None, get_shred_slot_index_type(&packet, &mut stats));
        assert_eq!(stats.index_overrun, 4);

        packet.meta.size = OFFSET_OF_SHRED_INDEX + SIZE_OF_SHRED_INDEX;
        assert_eq!(
            Some((1, 3, ShredType::Data)),
            get_shred_slot_index_type(&packet, &mut stats)
        );
        assert_eq!(stats.index_overrun, 4);

        let shred = Shred::new_from_parity_shard(
            8,   // slot
            2,   // index
            &[], // parity_shard
            10,  // fec_set_index
            30,  // num_data
            4,   // num_code
            1,   // position
            200, // version
        );
        shred.copy_to_packet(&mut packet);
        assert_eq!(
            Some((8, 2, ShredType::Code)),
            get_shred_slot_index_type(&packet, &mut stats)
        );

        let shred = Shred::new_from_data(1, std::u32::MAX - 10, 0, &[], true, true, 0, 0, 0);
        shred.copy_to_packet(&mut packet);
        assert_eq!(None, get_shred_slot_index_type(&packet, &mut stats));
        assert_eq!(1, stats.index_out_of_bounds);

        let shred = Shred::new_from_parity_shard(
            8,   // slot
            2,   // index
            &[], // parity_shard
            10,  // fec_set_index
            30,  // num_data_shreds
            4,   // num_coding_shreds
            3,   // position
            200, // version
        );
        shred.copy_to_packet(&mut packet);
        packet.data[OFFSET_OF_SHRED_TYPE] = u8::MAX;

        assert_eq!(None, get_shred_slot_index_type(&packet, &mut stats));
        assert_eq!(1, stats.bad_shred_type);
    }

    // Asserts that ShredType is backward compatible with u8.
    #[test]
    fn test_shred_type_compat() {
        assert_eq!(std::mem::size_of::<ShredType>(), std::mem::size_of::<u8>());
        assert_matches!(ShredType::try_from(0u8), Err(_));
        assert_matches!(ShredType::try_from(1u8), Err(_));
        assert_matches!(bincode::deserialize::<ShredType>(&[0u8]), Err(_));
        assert_matches!(bincode::deserialize::<ShredType>(&[1u8]), Err(_));
        // data shred
        assert_eq!(ShredType::Data as u8, 0b1010_0101);
        assert_eq!(ShredType::try_from(0b1010_0101), Ok(ShredType::Data));
        let buf = bincode::serialize(&ShredType::Data).unwrap();
        assert_eq!(buf, vec![0b1010_0101]);
        assert_matches!(
            bincode::deserialize::<ShredType>(&[0b1010_0101]),
            Ok(ShredType::Data)
        );
        // coding shred
        assert_eq!(ShredType::Code as u8, 0b0101_1010);
        assert_eq!(ShredType::try_from(0b0101_1010), Ok(ShredType::Code));
        let buf = bincode::serialize(&ShredType::Code).unwrap();
        assert_eq!(buf, vec![0b0101_1010]);
        assert_matches!(
            bincode::deserialize::<ShredType>(&[0b0101_1010]),
            Ok(ShredType::Code)
        );
    }

    #[test]
    fn test_sanitize_data_shred() {
        let data = [0xa5u8; SIZE_OF_DATA_SHRED_PAYLOAD];
        let mut shred = Shred::new_from_data(
            420, // slot
            19,  // index
            5,   // parent_offset
            &data, true,  // is_last_data
            false, // is_last_in_slot
            3,     // reference_tick
            1,     // version
            16,    // fec_set_index
        );
        assert_matches!(shred.sanitize(), Ok(()));
        // Corrupt shred by making it too large
        {
            let mut shred = shred.clone();
            shred.payload.push(10u8);
            assert_matches!(shred.sanitize(), Err(Error::InvalidPayloadSize(1229)));
        }
        {
            let mut shred = shred.clone();
            shred.data_header.size += 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 1140,
                    payload: 1228,
                })
            );
        }
        {
            let mut shred = shred.clone();
            shred.data_header.size = 0;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 0,
                    payload: 1228,
                })
            );
        }
        {
            let mut shred = shred.clone();
            shred.common_header.index = MAX_DATA_SHREDS_PER_SLOT as u32;
            assert_matches!(shred.sanitize(), Err(Error::InvalidDataShredIndex(32768)));
        }
        {
            shred.data_header.size = shred.payload().len() as u16 + 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 1229,
                    payload: 1228,
                })
            );
        }
    }

    #[test]
    fn test_sanitize_coding_shred() {
        let mut shred = Shred::new_from_parity_shard(
            1,   // slot
            12,  // index
            &[], // parity_shard
            11,  // fec_set_index
            11,  // num_data_shreds
            11,  // num_coding_shreds
            8,   // position
            0,   // version
        );
        assert_matches!(shred.sanitize(), Ok(()));
        // index < position is invalid.
        {
            let mut shred = shred.clone();
            let index = shred.index() - shred.fec_set_index() - 1;
            shred.set_index(index as u32);
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );
        }
        {
            let mut shred = shred.clone();
            shred.coding_header.num_coding_shreds = 0;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );
        }
        // pos >= num_coding is invalid.
        {
            let mut shred = shred.clone();
            let num_coding_shreds = shred.index() - shred.fec_set_index();
            shred.coding_header.num_coding_shreds = num_coding_shreds as u16;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );
        }
        // set_index with num_coding that would imply the last
        // shred has index > u32::MAX should fail.
        {
            let mut shred = shred.clone();
            shred.common_header.fec_set_index = std::u32::MAX - 1;
            shred.coding_header.num_data_shreds = 2;
            shred.coding_header.num_coding_shreds = 4;
            shred.coding_header.position = 1;
            shred.common_header.index = std::u32::MAX - 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );

            shred.coding_header.num_coding_shreds = 2000;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );

            // Decreasing the number of num_coding_shreds will put it within
            // the allowed limit.
            shred.coding_header.num_coding_shreds = 2;
            assert_matches!(shred.sanitize(), Ok(()));
        }
        {
            shred.coding_header.num_coding_shreds = u16::MAX;
            assert_matches!(shred.sanitize(), Err(Error::InvalidNumCodingShreds(65535)));
        }
    }

    #[test]
    fn test_serde_compat_shred_data() {
        const SEED: &str = "6qG9NGWEtoTugS4Zgs46u8zTccEJuRHtrNMiUayLHCxt";
        const PAYLOAD: &str = "Cuk5B7qosCx42HcZjwcUKPpeqE43sDhx1RFut5rEAk54dV\
        JFPrGEBYuPJSwaaNNbGyas9AuLS66NcFxNAcBzmBcb3gSmNrfQzmTUev5jSEeQRyqE5WG\
        rGwC67mS5QXmokCtVEdXu9B1SLQFBMB62CQGVEjqV1r6jz4xd5Zg1AyyVCjGpRe8ooqMB\
        1doeATLCEBhjnPPYLLyZGqPfrqfz5huq8BCoVC8DCMWFJhxtPLA5XHPPpxieAhDBmS";
        let mut rng = {
            let seed = <[u8; 32]>::try_from(bs58_decode(SEED)).unwrap();
            ChaChaRng::from_seed(seed)
        };
        let mut data = [0u8; SIZE_OF_DATA_SHRED_PAYLOAD];
        rng.fill(&mut data[..]);
        let keypair = Keypair::generate(&mut rng);
        let mut shred = Shred::new_from_data(
            141939602, // slot
            28685,     // index
            36390,     // parent_offset
            &data,     // data
            false,     // is_last_data
            true,      // is_last_in_slot
            37,        // reference_tick
            45189,     // version
            28657,     // fec_set_index
        );
        shred.sign(&keypair);
        assert!(shred.verify(&keypair.pubkey()));
        assert_matches!(shred.sanitize(), Ok(()));
        let mut payload = bs58_decode(PAYLOAD);
        payload.extend({
            let skip = payload.len() - SHRED_DATA_OFFSET;
            data.iter().skip(skip).copied()
        });
        let mut packet = Packet::default();
        packet.data[..payload.len()].copy_from_slice(&payload);
        packet.meta.size = payload.len();
        assert_eq!(shred.bytes_to_store(), payload);
        assert_eq!(shred, Shred::new_from_serialized_shred(payload).unwrap());
        assert_eq!(
            shred.reference_tick(),
            Shred::reference_tick_from_data(&packet.data)
        );
        assert_eq!(Shred::get_slot_from_packet(&packet), Some(shred.slot()));
        assert_eq!(
            get_shred_slot_index_type(&packet, &mut ShredFetchStats::default()),
            Some((shred.slot(), shred.index(), shred.shred_type()))
        );
    }

    #[test]
    fn test_serde_compat_shred_data_empty() {
        const SEED: &str = "E3M5hm8yAEB7iPhQxFypAkLqxNeZCTuGBDMa8Jdrghoo";
        const PAYLOAD: &str = "nRNFVBEsV9FEM5KfmsCXJsgELRSkCV55drTavdy5aZPnsp\
        B8WvsgY99ZuNHDnwkrqe6Lx7ARVmercwugR5HwDcLA9ivKMypk9PNucDPLs67TXWy6k9R\
        ozKmy";
        let mut rng = {
            let seed = <[u8; 32]>::try_from(bs58_decode(SEED)).unwrap();
            ChaChaRng::from_seed(seed)
        };
        let keypair = Keypair::generate(&mut rng);
        let mut shred = Shred::new_from_data(
            142076266, // slot
            21443,     // index
            51279,     // parent_offset
            &[],       // data
            true,      // is_last_data
            false,     // is_last_in_slot
            49,        // reference_tick
            59445,     // version
            21414,     // fec_set_index
        );
        shred.sign(&keypair);
        assert!(shred.verify(&keypair.pubkey()));
        assert_matches!(shred.sanitize(), Ok(()));
        let payload = bs58_decode(PAYLOAD);
        let mut packet = Packet::default();
        packet.data[..payload.len()].copy_from_slice(&payload);
        packet.meta.size = payload.len();
        assert_eq!(shred.bytes_to_store(), payload);
        assert_eq!(shred, Shred::new_from_serialized_shred(payload).unwrap());
        assert_eq!(
            shred.reference_tick(),
            Shred::reference_tick_from_data(&packet.data)
        );
        assert_eq!(Shred::get_slot_from_packet(&packet), Some(shred.slot()));
        assert_eq!(
            get_shred_slot_index_type(&packet, &mut ShredFetchStats::default()),
            Some((shred.slot(), shred.index(), shred.shred_type()))
        );
    }

    #[test]
    fn test_serde_compat_shred_code() {
        const SEED: &str = "4jfjh3UZVyaEgvyG9oQmNyFY9yHDmbeH9eUhnBKkrcrN";
        const PAYLOAD: &str = "3xGsXwzkPpLFuKwbbfKMUxt1B6VqQPzbvvAkxRNCX9kNEP\
        sa2VifwGBtFuNm3CWXdmQizDz5vJjDHu6ZqqaBCSfrHurag87qAXwTtjNPhZzKEew5pLc\
        aY6cooiAch2vpfixNYSDjnirozje5cmUtGuYs1asXwsAKSN3QdWHz3XGParWkZeUMAzRV\
        1UPEDZ7vETKbxeNixKbzZzo47Lakh3C35hS74ocfj23CWoW1JpkETkXjUpXcfcv6cS";
        let mut rng = {
            let seed = <[u8; 32]>::try_from(bs58_decode(SEED)).unwrap();
            ChaChaRng::from_seed(seed)
        };
        let mut parity_shard = vec![0u8; ENCODED_PAYLOAD_SIZE];
        rng.fill(&mut parity_shard[..]);
        let keypair = Keypair::generate(&mut rng);
        let mut shred = Shred::new_from_parity_shard(
            141945197, // slot
            23418,     // index
            &parity_shard,
            21259, // fec_set_index
            32,    // num_data_shreds
            58,    // num_coding_shreds
            43,    // position
            47298, // version
        );
        shred.sign(&keypair);
        assert!(shred.verify(&keypair.pubkey()));
        assert_matches!(shred.sanitize(), Ok(()));
        let mut payload = bs58_decode(PAYLOAD);
        payload.extend({
            let skip = payload.len() - SIZE_OF_CODING_SHRED_HEADERS;
            parity_shard.iter().skip(skip).copied()
        });
        let mut packet = Packet::default();
        packet.data[..payload.len()].copy_from_slice(&payload);
        packet.meta.size = payload.len();
        assert_eq!(shred.bytes_to_store(), payload);
        assert_eq!(shred, Shred::new_from_serialized_shred(payload).unwrap());
        assert_eq!(Shred::get_slot_from_packet(&packet), Some(shred.slot()));
        assert_eq!(
            get_shred_slot_index_type(&packet, &mut ShredFetchStats::default()),
            Some((shred.slot(), shred.index(), shred.shred_type()))
        );
    }

    #[test]
    fn test_shred_flags() {
        fn make_shred(is_last_data: bool, is_last_in_slot: bool, reference_tick: u8) -> Shred {
            Shred::new_from_data(
                0,   // slot
                0,   // index
                0,   // parent_offset
                &[], // data
                is_last_data,
                is_last_in_slot,
                reference_tick,
                0, // version
                0, // fec_set_index
            )
        }
        fn check_shred_flags(
            shred: &Shred,
            is_last_data: bool,
            is_last_in_slot: bool,
            reference_tick: u8,
        ) {
            assert_eq!(shred.data_complete(), is_last_data);
            assert_eq!(shred.last_in_slot(), is_last_in_slot);
            assert_eq!(shred.reference_tick(), reference_tick.min(63u8));
            assert_eq!(
                Shred::reference_tick_from_data(&shred.payload),
                reference_tick.min(63u8),
            );
        }
        for is_last_data in [false, true] {
            for is_last_in_slot in [false, true] {
                for reference_tick in [0, 37, 63, 64, 80, 128, 255] {
                    let mut shred = make_shred(is_last_data, is_last_in_slot, reference_tick);
                    check_shred_flags(&shred, is_last_data, is_last_in_slot, reference_tick);
                    shred.set_last_in_slot();
                    check_shred_flags(&shred, is_last_data, true, reference_tick);
                    shred.unset_data_complete();
                    check_shred_flags(&shred, false, true, reference_tick);
                }
            }
        }
    }
}
