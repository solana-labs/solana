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

pub(crate) use self::merkle::{MerkleRoot, SIZE_OF_MERKLE_ROOT};
#[cfg(test)]
pub(crate) use self::shred_code::MAX_CODE_SHREDS_PER_SLOT;
use {
    self::{shred_code::ShredCode, traits::Shred as _},
    crate::blockstore::{self, MAX_DATA_SHREDS_PER_SLOT},
    bitflags::bitflags,
    num_enum::{IntoPrimitive, TryFromPrimitive},
    rayon::ThreadPool,
    reed_solomon_erasure::Error::TooFewShardsPresent,
    serde::{Deserialize, Serialize},
    solana_entry::entry::{create_ticks, Entry},
    solana_perf::packet::Packet,
    solana_sdk::{
        clock::Slot,
        hash::{hashv, Hash},
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer, SIGNATURE_BYTES},
    },
    static_assertions::const_assert_eq,
    std::{fmt::Debug, time::Instant},
    thiserror::Error,
};
pub use {
    self::{
        shred_data::ShredData,
        stats::{ProcessShredsStats, ShredFetchStats},
    },
    crate::shredder::{ReedSolomonCache, Shredder},
};

mod common;
mod legacy;
mod merkle;
mod shred_code;
mod shred_data;
mod stats;
mod traits;

pub type Nonce = u32;
const_assert_eq!(SIZE_OF_NONCE, 4);
pub const SIZE_OF_NONCE: usize = std::mem::size_of::<Nonce>();

/// The following constants are computed by hand, and hardcoded.
/// `test_shred_constants` ensures that the values are correct.
/// Constants are used over lazy_static for performance reasons.
const SIZE_OF_COMMON_SHRED_HEADER: usize = 83;
const SIZE_OF_DATA_SHRED_HEADERS: usize = 88;
const SIZE_OF_CODING_SHRED_HEADERS: usize = 89;
const SIZE_OF_SIGNATURE: usize = SIGNATURE_BYTES;
const SIZE_OF_SHRED_VARIANT: usize = 1;
const SIZE_OF_SHRED_SLOT: usize = 8;

const OFFSET_OF_SHRED_VARIANT: usize = SIZE_OF_SIGNATURE;
const OFFSET_OF_SHRED_SLOT: usize = SIZE_OF_SIGNATURE + SIZE_OF_SHRED_VARIANT;
const OFFSET_OF_SHRED_INDEX: usize = OFFSET_OF_SHRED_SLOT + SIZE_OF_SHRED_SLOT;

// Shreds are uniformly split into erasure batches with a "target" number of
// data shreds per each batch as below. The actual number of data shreds in
// each erasure batch depends on the number of shreds obtained from serializing
// a &[Entry].
pub const DATA_SHREDS_PER_FEC_BLOCK: usize = 32;

// For legacy tests and benchmarks.
const_assert_eq!(LEGACY_SHRED_DATA_CAPACITY, 1051);
pub const LEGACY_SHRED_DATA_CAPACITY: usize = legacy::ShredData::CAPACITY;

// LAST_SHRED_IN_SLOT also implies DATA_COMPLETE_SHRED.
// So it cannot be LAST_SHRED_IN_SLOT if not also DATA_COMPLETE_SHRED.
bitflags! {
    #[derive(Default, Serialize, Deserialize)]
    pub struct ShredFlags:u8 {
        const SHRED_TICK_REFERENCE_MASK = 0b0011_1111;
        const DATA_COMPLETE_SHRED       = 0b0100_0000;
        const LAST_SHRED_IN_SLOT        = 0b1100_0000;
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    BincodeError(#[from] bincode::Error),
    #[error(transparent)]
    ErasureError(#[from] reed_solomon_erasure::Error),
    #[error("Invalid data size: {size}, payload: {payload}")]
    InvalidDataSize { size: u16, payload: usize },
    #[error("Invalid erasure shard index: {0:?}")]
    InvalidErasureShardIndex(/*headers:*/ Box<dyn Debug + Send>),
    #[error("Invalid merkle proof")]
    InvalidMerkleProof,
    #[error("Invalid num coding shreds: {0}")]
    InvalidNumCodingShreds(u16),
    #[error("Invalid parent_offset: {parent_offset}, slot: {slot}")]
    InvalidParentOffset { slot: Slot, parent_offset: u16 },
    #[error("Invalid parent slot: {parent_slot}, slot: {slot}")]
    InvalidParentSlot { slot: Slot, parent_slot: Slot },
    #[error("Invalid payload size: {0}")]
    InvalidPayloadSize(/*payload size:*/ usize),
    #[error("Invalid proof size: {0}")]
    InvalidProofSize(/*proof_size:*/ u8),
    #[error("Invalid recovered shred")]
    InvalidRecoveredShred,
    #[error("Invalid shard size: {0}")]
    InvalidShardSize(/*shard_size:*/ usize),
    #[error("Invalid shred flags: {0}")]
    InvalidShredFlags(u8),
    #[error("Invalid {0:?} shred index: {1}")]
    InvalidShredIndex(ShredType, /*shred index:*/ u32),
    #[error("Invalid shred type")]
    InvalidShredType,
    #[error("Invalid shred variant")]
    InvalidShredVariant,
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("Unknown proof size")]
    UnknownProofSize,
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(into = "u8", try_from = "u8")]
enum ShredVariant {
    LegacyCode, // 0b0101_1010
    LegacyData, // 0b1010_0101
    // proof_size is the number of proof entries in the merkle tree branch.
    MerkleCode(/*proof_size:*/ u8), // 0b0100_????
    MerkleData(/*proof_size:*/ u8), // 0b1000_????
}

/// A common header that is present in data and code shred headers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct ShredCommonHeader {
    signature: Signature,
    shred_variant: ShredVariant,
    slot: Slot,
    index: u32,
    version: u16,
    fec_set_index: u32,
}

/// The data shred header has parent offset and flags
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct DataShredHeader {
    parent_offset: u16,
    flags: ShredFlags,
    size: u16, // common shred header + data shred header + data
}

/// The coding shred header has FEC information
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct CodingShredHeader {
    num_data_shreds: u16,
    num_coding_shreds: u16,
    position: u16, // [0..num_coding_shreds)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shred {
    ShredCode(ShredCode),
    ShredData(ShredData),
}

pub(crate) enum SignedData<'a> {
    Chunk(&'a [u8]), // Chunk of payload past signature.
    MerkleRoot(MerkleRoot),
}

impl<'a> AsRef<[u8]> for SignedData<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Chunk(chunk) => chunk,
            Self::MerkleRoot(root) => root,
        }
    }
}

/// Tuple which uniquely identifies a shred should it exists.
#[derive(Clone, Copy, Eq, Debug, Hash, PartialEq)]
pub struct ShredId(Slot, /*shred index:*/ u32, ShredType);

impl ShredId {
    pub(crate) fn new(slot: Slot, index: u32, shred_type: ShredType) -> ShredId {
        ShredId(slot, index, shred_type)
    }

    pub fn slot(&self) -> Slot {
        self.0
    }

    pub(crate) fn unwrap(&self) -> (Slot, /*shred index:*/ u32, ShredType) {
        (self.0, self.1, self.2)
    }

    pub fn seed(&self, leader: &Pubkey) -> [u8; 32] {
        let ShredId(slot, index, shred_type) = self;
        hashv(&[
            &slot.to_le_bytes(),
            &u8::from(*shred_type).to_le_bytes(),
            &index.to_le_bytes(),
            AsRef::<[u8]>::as_ref(leader),
        ])
        .to_bytes()
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

macro_rules! dispatch {
    ($vis:vis fn $name:ident(&self $(, $arg:ident : $ty:ty)?) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&self $(, $arg:$ty)?) $(-> $out)? {
            match self {
                Self::ShredCode(shred) => shred.$name($($arg, )?),
                Self::ShredData(shred) => shred.$name($($arg, )?),
            }
        }
    };
    ($vis:vis fn $name:ident(self $(, $arg:ident : $ty:ty)?) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(self $(, $arg:$ty)?) $(-> $out)? {
            match self {
                Self::ShredCode(shred) => shred.$name($($arg, )?),
                Self::ShredData(shred) => shred.$name($($arg, )?),
            }
        }
    };
    ($vis:vis fn $name:ident(&mut self $(, $arg:ident : $ty:ty)?) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&mut self $(, $arg:$ty)?) $(-> $out)? {
            match self {
                Self::ShredCode(shred) => shred.$name($($arg, )?),
                Self::ShredData(shred) => shred.$name($($arg, )?),
            }
        }
    }
}

use dispatch;

impl Shred {
    dispatch!(fn common_header(&self) -> &ShredCommonHeader);
    dispatch!(fn set_signature(&mut self, signature: Signature));
    dispatch!(fn signed_data(&self) -> Result<SignedData, Error>);

    // Returns the portion of the shred's payload which is erasure coded.
    dispatch!(pub(crate) fn erasure_shard(self) -> Result<Vec<u8>, Error>);
    // Like Shred::erasure_shard but returning a slice.
    dispatch!(pub(crate) fn erasure_shard_as_slice(&self) -> Result<&[u8], Error>);
    // Returns the shard index within the erasure coding set.
    dispatch!(pub(crate) fn erasure_shard_index(&self) -> Result<usize, Error>);

    dispatch!(pub fn into_payload(self) -> Vec<u8>);
    dispatch!(pub fn payload(&self) -> &Vec<u8>);
    dispatch!(pub fn sanitize(&self) -> Result<(), Error>);

    // Only for tests.
    dispatch!(pub fn set_index(&mut self, index: u32));
    dispatch!(pub fn set_slot(&mut self, slot: Slot));

    pub fn copy_to_packet(&self, packet: &mut Packet) {
        let payload = self.payload();
        let size = payload.len();
        packet.buffer_mut()[..size].copy_from_slice(&payload[..]);
        packet.meta_mut().size = size;
    }

    // TODO: Should this sanitize output?
    pub fn new_from_data(
        slot: Slot,
        index: u32,
        parent_offset: u16,
        data: &[u8],
        flags: ShredFlags,
        reference_tick: u8,
        version: u16,
        fec_set_index: u32,
    ) -> Self {
        Self::from(ShredData::new_from_data(
            slot,
            index,
            parent_offset,
            data,
            flags,
            reference_tick,
            version,
            fec_set_index,
        ))
    }

    pub fn new_from_serialized_shred(shred: Vec<u8>) -> Result<Self, Error> {
        Ok(match layout::get_shred_variant(&shred)? {
            ShredVariant::LegacyCode => {
                let shred = legacy::ShredCode::from_payload(shred)?;
                Self::from(ShredCode::from(shred))
            }
            ShredVariant::LegacyData => {
                let shred = legacy::ShredData::from_payload(shred)?;
                Self::from(ShredData::from(shred))
            }
            ShredVariant::MerkleCode(_) => {
                let shred = merkle::ShredCode::from_payload(shred)?;
                Self::from(ShredCode::from(shred))
            }
            ShredVariant::MerkleData(_) => {
                let shred = merkle::ShredData::from_payload(shred)?;
                Self::from(ShredData::from(shred))
            }
        })
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
        Self::from(ShredCode::new_from_parity_shard(
            slot,
            index,
            parity_shard,
            fec_set_index,
            num_data_shreds,
            num_coding_shreds,
            position,
            version,
        ))
    }

    /// Unique identifier for each shred.
    pub fn id(&self) -> ShredId {
        ShredId(self.slot(), self.index(), self.shred_type())
    }

    pub fn slot(&self) -> Slot {
        self.common_header().slot
    }

    pub fn parent(&self) -> Result<Slot, Error> {
        match self {
            Self::ShredCode(_) => Err(Error::InvalidShredType),
            Self::ShredData(shred) => shred.parent(),
        }
    }

    pub fn index(&self) -> u32 {
        self.common_header().index
    }

    pub(crate) fn data(&self) -> Result<&[u8], Error> {
        match self {
            Self::ShredCode(_) => Err(Error::InvalidShredType),
            Self::ShredData(shred) => shred.data(),
        }
    }

    // Possibly trimmed payload;
    // Should only be used when storing shreds to blockstore.
    pub(crate) fn bytes_to_store(&self) -> &[u8] {
        match self {
            Self::ShredCode(shred) => shred.payload(),
            Self::ShredData(shred) => shred.bytes_to_store(),
        }
    }

    pub fn fec_set_index(&self) -> u32 {
        self.common_header().fec_set_index
    }

    pub(crate) fn first_coding_index(&self) -> Option<u32> {
        match self {
            Self::ShredCode(shred) => shred.first_coding_index(),
            Self::ShredData(_) => None,
        }
    }

    pub fn version(&self) -> u16 {
        self.common_header().version
    }

    // Identifier for the erasure coding set that the shred belongs to.
    pub(crate) fn erasure_set(&self) -> ErasureSetId {
        ErasureSetId(self.slot(), self.fec_set_index())
    }

    pub fn signature(&self) -> &Signature {
        &self.common_header().signature
    }

    pub fn sign(&mut self, keypair: &Keypair) {
        let data = self.signed_data().unwrap();
        let signature = keypair.sign_message(data.as_ref());
        self.set_signature(signature);
    }

    #[inline]
    pub fn shred_type(&self) -> ShredType {
        ShredType::from(self.common_header().shred_variant)
    }

    pub fn is_data(&self) -> bool {
        self.shred_type() == ShredType::Data
    }
    pub fn is_code(&self) -> bool {
        self.shred_type() == ShredType::Code
    }

    pub fn last_in_slot(&self) -> bool {
        match self {
            Self::ShredCode(_) => false,
            Self::ShredData(shred) => shred.last_in_slot(),
        }
    }

    /// This is not a safe function. It only changes the meta information.
    /// Use this only for test code which doesn't care about actual shred
    pub fn set_last_in_slot(&mut self) {
        match self {
            Self::ShredCode(_) => (),
            Self::ShredData(shred) => shred.set_last_in_slot(),
        }
    }

    pub fn data_complete(&self) -> bool {
        match self {
            Self::ShredCode(_) => false,
            Self::ShredData(shred) => shred.data_complete(),
        }
    }

    pub(crate) fn reference_tick(&self) -> u8 {
        match self {
            Self::ShredCode(_) => ShredFlags::SHRED_TICK_REFERENCE_MASK.bits(),
            Self::ShredData(shred) => shred.reference_tick(),
        }
    }

    #[must_use]
    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        match self.signed_data() {
            Ok(data) => self.signature().verify(pubkey.as_ref(), data.as_ref()),
            Err(_) => false,
        }
    }

    // Returns true if the erasure coding of the two shreds mismatch.
    pub(crate) fn erasure_mismatch(&self, other: &Self) -> Result<bool, Error> {
        match (self, other) {
            (Self::ShredCode(shred), Self::ShredCode(other)) => Ok(shred.erasure_mismatch(other)),
            _ => Err(Error::InvalidShredType),
        }
    }

    pub(crate) fn num_data_shreds(&self) -> Result<u16, Error> {
        match self {
            Self::ShredCode(shred) => Ok(shred.num_data_shreds()),
            Self::ShredData(_) => Err(Error::InvalidShredType),
        }
    }

    pub(crate) fn num_coding_shreds(&self) -> Result<u16, Error> {
        match self {
            Self::ShredCode(shred) => Ok(shred.num_coding_shreds()),
            Self::ShredData(_) => Err(Error::InvalidShredType),
        }
    }
}

// Helper methods to extract pieces of the shred from the payload
// without deserializing the entire payload.
pub mod layout {
    use {super::*, std::ops::Range};
    #[cfg(test)]
    use {
        rand::{seq::SliceRandom, Rng},
        std::collections::HashMap,
    };

    fn get_shred_size(packet: &Packet) -> Option<usize> {
        let size = packet.data(..)?.len();
        if packet.meta().repair() {
            size.checked_sub(SIZE_OF_NONCE)
        } else {
            Some(size)
        }
    }

    pub fn get_shred(packet: &Packet) -> Option<&[u8]> {
        let size = get_shred_size(packet)?;
        packet.data(..size)
    }

    pub(crate) fn get_signature(shred: &[u8]) -> Option<Signature> {
        Some(Signature::new(shred.get(..SIZE_OF_SIGNATURE)?))
    }

    pub(crate) const fn get_signature_range() -> Range<usize> {
        0..SIZE_OF_SIGNATURE
    }

    pub(super) fn get_shred_variant(shred: &[u8]) -> Result<ShredVariant, Error> {
        let shred_variant = match shred.get(OFFSET_OF_SHRED_VARIANT) {
            None => return Err(Error::InvalidPayloadSize(shred.len())),
            Some(shred_variant) => *shred_variant,
        };
        ShredVariant::try_from(shred_variant).map_err(|_| Error::InvalidShredVariant)
    }

    #[inline]
    pub(super) fn get_shred_type(shred: &[u8]) -> Result<ShredType, Error> {
        let shred_variant = get_shred_variant(shred)?;
        Ok(ShredType::from(shred_variant))
    }

    #[inline]
    pub fn get_slot(shred: &[u8]) -> Option<Slot> {
        <[u8; 8]>::try_from(shred.get(OFFSET_OF_SHRED_SLOT..)?.get(..8)?)
            .map(Slot::from_le_bytes)
            .ok()
    }

    #[inline]
    pub(super) fn get_index(shred: &[u8]) -> Option<u32> {
        <[u8; 4]>::try_from(shred.get(OFFSET_OF_SHRED_INDEX..)?.get(..4)?)
            .map(u32::from_le_bytes)
            .ok()
    }

    pub fn get_version(shred: &[u8]) -> Option<u16> {
        <[u8; 2]>::try_from(shred.get(77..79)?)
            .map(u16::from_le_bytes)
            .ok()
    }

    // The caller should verify first that the shred is data and not code!
    pub(super) fn get_parent_offset(shred: &[u8]) -> Option<u16> {
        debug_assert_eq!(get_shred_type(shred).unwrap(), ShredType::Data);
        <[u8; 2]>::try_from(shred.get(83..85)?)
            .map(u16::from_le_bytes)
            .ok()
    }

    #[inline]
    pub fn get_shred_id(shred: &[u8]) -> Option<ShredId> {
        Some(ShredId(
            get_slot(shred)?,
            get_index(shred)?,
            get_shred_type(shred).ok()?,
        ))
    }

    pub(crate) fn get_signed_data(shred: &[u8]) -> Option<SignedData> {
        let data = match get_shred_variant(shred).ok()? {
            ShredVariant::LegacyCode | ShredVariant::LegacyData => {
                let chunk = shred.get(self::legacy::SIGNED_MESSAGE_OFFSETS)?;
                SignedData::Chunk(chunk)
            }
            ShredVariant::MerkleCode(proof_size) => {
                let merkle_root = self::merkle::ShredCode::get_merkle_root(shred, proof_size)?;
                SignedData::MerkleRoot(merkle_root)
            }
            ShredVariant::MerkleData(proof_size) => {
                let merkle_root = self::merkle::ShredData::get_merkle_root(shred, proof_size)?;
                SignedData::MerkleRoot(merkle_root)
            }
        };
        Some(data)
    }

    // Returns offsets within the shred payload which is signed.
    pub(crate) fn get_signed_data_offsets(shred: &[u8]) -> Option<Range<usize>> {
        let offsets = match get_shred_variant(shred).ok()? {
            ShredVariant::LegacyCode | ShredVariant::LegacyData => legacy::SIGNED_MESSAGE_OFFSETS,
            ShredVariant::MerkleCode(proof_size) => {
                merkle::ShredCode::get_signed_data_offsets(proof_size)?
            }
            ShredVariant::MerkleData(proof_size) => {
                merkle::ShredData::get_signed_data_offsets(proof_size)?
            }
        };
        (offsets.end <= shred.len()).then_some(offsets)
    }

    pub(crate) fn get_reference_tick(shred: &[u8]) -> Result<u8, Error> {
        if get_shred_type(shred)? != ShredType::Data {
            return Err(Error::InvalidShredType);
        }
        let flags = match shred.get(85) {
            None => return Err(Error::InvalidPayloadSize(shred.len())),
            Some(flags) => flags,
        };
        Ok(flags & ShredFlags::SHRED_TICK_REFERENCE_MASK.bits())
    }

    pub(crate) fn get_merkle_root(shred: &[u8]) -> Option<MerkleRoot> {
        match get_shred_variant(shred).ok()? {
            ShredVariant::LegacyCode | ShredVariant::LegacyData => None,
            ShredVariant::MerkleCode(proof_size) => {
                merkle::ShredCode::get_merkle_root(shred, proof_size)
            }
            ShredVariant::MerkleData(proof_size) => {
                merkle::ShredData::get_merkle_root(shred, proof_size)
            }
        }
    }

    // Minimally corrupts the packet so that the signature no longer verifies.
    #[cfg(test)]
    pub(crate) fn corrupt_packet<R: Rng>(
        rng: &mut R,
        packet: &mut Packet,
        keypairs: &HashMap<Slot, Keypair>,
    ) {
        fn modify_packet<R: Rng>(rng: &mut R, packet: &mut Packet, offsets: Range<usize>) {
            let buffer = packet.buffer_mut();
            let byte = buffer[offsets].choose_mut(rng).unwrap();
            *byte = rng.gen::<u8>().max(1u8).wrapping_add(*byte);
        }
        let shred = get_shred(packet).unwrap();
        let merkle_proof_size = match get_shred_variant(shred).unwrap() {
            ShredVariant::LegacyCode | ShredVariant::LegacyData => None,
            ShredVariant::MerkleCode(proof_size) | ShredVariant::MerkleData(proof_size) => {
                Some(proof_size)
            }
        };
        let coin_flip: bool = rng.gen();
        if coin_flip {
            // Corrupt one byte within the signature offsets.
            modify_packet(rng, packet, 0..SIGNATURE_BYTES);
        } else {
            // Corrupt one byte within the signed data offsets.
            let size = shred.len();
            let offsets = get_signed_data_offsets(shred).unwrap();
            modify_packet(rng, packet, offsets);
            if let Some(proof_size) = merkle_proof_size {
                // Also need to corrupt the merkle proof.
                // Proof entries are each 20 bytes at the end of shreds.
                let offset = usize::from(proof_size) * 20;
                modify_packet(rng, packet, size - offset..size);
            }
        }
        // Assert that the signature no longer verifies.
        let shred = get_shred(packet).unwrap();
        let slot = get_slot(shred).unwrap();
        let signature = get_signature(shred).unwrap();
        if coin_flip {
            let pubkey = keypairs[&slot].pubkey();
            let data = get_signed_data(shred).unwrap();
            assert!(!signature.verify(pubkey.as_ref(), data.as_ref()));
            let offsets = get_signed_data_offsets(shred).unwrap();
            assert!(!signature.verify(pubkey.as_ref(), &shred[offsets]));
        } else {
            // Slot may have been corrupted and no longer mapping to a keypair.
            let pubkey = keypairs.get(&slot).map(Keypair::pubkey).unwrap_or_default();
            if let Some(data) = get_signed_data(shred) {
                assert!(!signature.verify(pubkey.as_ref(), data.as_ref()));
            }
            let offsets = get_signed_data_offsets(shred).unwrap_or_default();
            assert!(!signature.verify(pubkey.as_ref(), &shred[offsets]));
        }
    }
}

impl From<ShredCode> for Shred {
    fn from(shred: ShredCode) -> Self {
        Self::ShredCode(shred)
    }
}

impl From<ShredData> for Shred {
    fn from(shred: ShredData) -> Self {
        Self::ShredData(shred)
    }
}

impl From<merkle::Shred> for Shred {
    fn from(shred: merkle::Shred) -> Self {
        match shred {
            merkle::Shred::ShredCode(shred) => Self::ShredCode(ShredCode::Merkle(shred)),
            merkle::Shred::ShredData(shred) => Self::ShredData(ShredData::Merkle(shred)),
        }
    }
}

impl TryFrom<Shred> for merkle::Shred {
    type Error = Error;

    fn try_from(shred: Shred) -> Result<Self, Self::Error> {
        match shred {
            Shred::ShredCode(ShredCode::Legacy(_)) => Err(Error::InvalidShredVariant),
            Shred::ShredCode(ShredCode::Merkle(shred)) => Ok(Self::ShredCode(shred)),
            Shred::ShredData(ShredData::Legacy(_)) => Err(Error::InvalidShredVariant),
            Shred::ShredData(ShredData::Merkle(shred)) => Ok(Self::ShredData(shred)),
        }
    }
}

impl From<ShredVariant> for ShredType {
    #[inline]
    fn from(shred_variant: ShredVariant) -> Self {
        match shred_variant {
            ShredVariant::LegacyCode => ShredType::Code,
            ShredVariant::LegacyData => ShredType::Data,
            ShredVariant::MerkleCode(_) => ShredType::Code,
            ShredVariant::MerkleData(_) => ShredType::Data,
        }
    }
}

impl From<ShredVariant> for u8 {
    fn from(shred_variant: ShredVariant) -> u8 {
        match shred_variant {
            ShredVariant::LegacyCode => u8::from(ShredType::Code),
            ShredVariant::LegacyData => u8::from(ShredType::Data),
            ShredVariant::MerkleCode(proof_size) => proof_size | 0x40,
            ShredVariant::MerkleData(proof_size) => proof_size | 0x80,
        }
    }
}

impl TryFrom<u8> for ShredVariant {
    type Error = Error;
    fn try_from(shred_variant: u8) -> Result<Self, Self::Error> {
        if shred_variant == u8::from(ShredType::Code) {
            Ok(ShredVariant::LegacyCode)
        } else if shred_variant == u8::from(ShredType::Data) {
            Ok(ShredVariant::LegacyData)
        } else {
            match shred_variant & 0xF0 {
                0x40 => Ok(ShredVariant::MerkleCode(shred_variant & 0x0F)),
                0x80 => Ok(ShredVariant::MerkleData(shred_variant & 0x0F)),
                _ => Err(Error::InvalidShredVariant),
            }
        }
    }
}

pub(crate) fn recover(
    shreds: Vec<Shred>,
    reed_solomon_cache: &ReedSolomonCache,
) -> Result<Vec<Shred>, Error> {
    match shreds
        .first()
        .ok_or(TooFewShardsPresent)?
        .common_header()
        .shred_variant
    {
        ShredVariant::LegacyData | ShredVariant::LegacyCode => {
            Shredder::try_recovery(shreds, reed_solomon_cache)
        }
        ShredVariant::MerkleCode(_) | ShredVariant::MerkleData(_) => {
            let shreds = shreds
                .into_iter()
                .map(merkle::Shred::try_from)
                .collect::<Result<_, _>>()?;
            Ok(merkle::recover(shreds, reed_solomon_cache)?
                .into_iter()
                .map(Shred::from)
                .collect())
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn make_merkle_shreds_from_entries(
    thread_pool: &ThreadPool,
    keypair: &Keypair,
    entries: &[Entry],
    slot: Slot,
    parent_slot: Slot,
    shred_version: u16,
    reference_tick: u8,
    is_last_in_slot: bool,
    next_shred_index: u32,
    next_code_index: u32,
    reed_solomon_cache: &ReedSolomonCache,
    stats: &mut ProcessShredsStats,
) -> Result<Vec<Shred>, Error> {
    let now = Instant::now();
    let entries = bincode::serialize(entries)?;
    stats.serialize_elapsed += now.elapsed().as_micros() as u64;
    let shreds = merkle::make_shreds_from_data(
        thread_pool,
        keypair,
        &entries[..],
        slot,
        parent_slot,
        shred_version,
        reference_tick,
        is_last_in_slot,
        next_shred_index,
        next_code_index,
        reed_solomon_cache,
        stats,
    )?;
    Ok(shreds.into_iter().flatten().map(Shred::from).collect())
}

// Accepts shreds in the slot range [root + 1, max_slot].
#[must_use]
pub fn should_discard_shred(
    packet: &Packet,
    root: Slot,
    max_slot: Slot,
    shred_version: u16,
    stats: &mut ShredFetchStats,
) -> bool {
    debug_assert!(root < max_slot);
    let shred = match layout::get_shred(packet) {
        None => {
            stats.index_overrun += 1;
            return true;
        }
        Some(shred) => shred,
    };
    match layout::get_version(shred) {
        None => {
            stats.index_overrun += 1;
            return true;
        }
        Some(version) => {
            if version != shred_version {
                stats.shred_version_mismatch += 1;
                return true;
            }
        }
    }
    let shred_type = match layout::get_shred_type(shred) {
        Ok(shred_type) => shred_type,
        Err(_) => {
            stats.bad_shred_type += 1;
            return true;
        }
    };
    let slot = match layout::get_slot(shred) {
        Some(slot) => {
            if slot > max_slot {
                stats.slot_out_of_range += 1;
                return true;
            }
            slot
        }
        None => {
            stats.slot_bad_deserialize += 1;
            return true;
        }
    };
    let index = match layout::get_index(shred) {
        Some(index) => index,
        None => {
            stats.index_bad_deserialize += 1;
            return true;
        }
    };
    match shred_type {
        ShredType::Code => {
            if index >= shred_code::MAX_CODE_SHREDS_PER_SLOT as u32 {
                stats.index_out_of_bounds += 1;
                return true;
            }
            if slot <= root {
                stats.slot_out_of_range += 1;
                return true;
            }
        }
        ShredType::Data => {
            if index >= MAX_DATA_SHREDS_PER_SLOT as u32 {
                stats.index_out_of_bounds += 1;
                return true;
            }
            let parent_offset = match layout::get_parent_offset(shred) {
                Some(parent_offset) => parent_offset,
                None => {
                    stats.bad_parent_offset += 1;
                    return true;
                }
            };
            let parent = match slot.checked_sub(Slot::from(parent_offset)) {
                Some(parent) => parent,
                None => {
                    stats.bad_parent_offset += 1;
                    return true;
                }
            };
            if !blockstore::verify_shred_slots(slot, parent, root) {
                stats.slot_out_of_range += 1;
                return true;
            }
        }
    }
    false
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
    let data_buffer_size = ShredData::capacity(/*merkle_proof_size:*/ None).unwrap();
    let shred_data_size = shred_data_size.unwrap_or(data_buffer_size) as u64;
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
        solana_sdk::{shred_version, signature::Signer},
    };

    const SIZE_OF_SHRED_INDEX: usize = 4;

    fn bs58_decode<T: AsRef<[u8]>>(data: T) -> Vec<u8> {
        bs58::decode(data).into_vec().unwrap()
    }

    #[test]
    fn test_shred_constants() {
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_variant: ShredVariant::LegacyCode,
            slot: Slot::MAX,
            index: u32::MAX,
            version: u16::MAX,
            fec_set_index: u32::MAX,
        };
        let data_shred_header = DataShredHeader {
            parent_offset: u16::MAX,
            flags: ShredFlags::all(),
            size: u16::MAX,
        };
        let coding_shred_header = CodingShredHeader {
            num_data_shreds: u16::MAX,
            num_coding_shreds: u16::MAX,
            position: u16::MAX,
        };
        assert_eq!(
            SIZE_OF_COMMON_SHRED_HEADER,
            serialized_size(&common_header).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_CODING_SHRED_HEADERS - SIZE_OF_COMMON_SHRED_HEADER,
            serialized_size(&coding_shred_header).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_DATA_SHRED_HEADERS - SIZE_OF_COMMON_SHRED_HEADER,
            serialized_size(&data_shred_header).unwrap() as usize
        );
        let data_shred_header_with_size = DataShredHeader {
            size: 1000,
            ..data_shred_header
        };
        assert_eq!(
            SIZE_OF_DATA_SHRED_HEADERS - SIZE_OF_COMMON_SHRED_HEADER,
            serialized_size(&data_shred_header_with_size).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SIGNATURE,
            bincode::serialized_size(&Signature::default()).unwrap() as usize
        );
        assert_eq!(
            SIZE_OF_SHRED_VARIANT,
            bincode::serialized_size(&ShredVariant::MerkleCode(15)).unwrap() as usize
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
        let shred = Shred::new_from_data(10, 0, 1000, &[1, 2, 3], ShredFlags::empty(), 0, 1, 0);
        let mut packet = Packet::default();
        shred.copy_to_packet(&mut packet);
        let shred_res = Shred::new_from_serialized_shred(packet.data(..).unwrap().to_vec());
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
    fn test_should_discard_shred() {
        solana_logger::setup();
        let mut packet = Packet::default();
        let root = 1;
        let shred_version = 798;
        let max_slot = 16;
        let shred = Shred::new_from_data(
            2,   // slot
            3,   // index
            1,   // parent_offset
            &[], // data
            ShredFlags::LAST_SHRED_IN_SLOT,
            0, // reference_tick
            shred_version,
            0, // fec_set_index
        );
        shred.copy_to_packet(&mut packet);
        let mut stats = ShredFetchStats::default();
        assert!(!should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(stats, ShredFetchStats::default());

        packet.meta_mut().size = OFFSET_OF_SHRED_VARIANT;
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(stats.index_overrun, 1);

        packet.meta_mut().size = OFFSET_OF_SHRED_INDEX;
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(stats.index_overrun, 2);

        packet.meta_mut().size = OFFSET_OF_SHRED_INDEX + 1;
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(stats.index_overrun, 3);

        packet.meta_mut().size = OFFSET_OF_SHRED_INDEX + SIZE_OF_SHRED_INDEX - 1;
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(stats.index_overrun, 4);

        packet.meta_mut().size = OFFSET_OF_SHRED_INDEX + SIZE_OF_SHRED_INDEX + 2;
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(stats.bad_parent_offset, 1);

        let shred = Shred::new_from_parity_shard(
            8,   // slot
            2,   // index
            &[], // parity_shard
            10,  // fec_set_index
            30,  // num_data
            4,   // num_code
            1,   // position
            shred_version,
        );
        shred.copy_to_packet(&mut packet);
        assert!(!should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));

        let shred = Shred::new_from_data(
            2,                  // slot
            std::u32::MAX - 10, // index
            1,                  // parent_offset
            &[],                // data
            ShredFlags::LAST_SHRED_IN_SLOT,
            0, // reference_tick
            shred_version,
            0, // fec_set_index
        );
        shred.copy_to_packet(&mut packet);
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(1, stats.index_out_of_bounds);

        let shred = Shred::new_from_parity_shard(
            8,   // slot
            2,   // index
            &[], // parity_shard
            10,  // fec_set_index
            30,  // num_data_shreds
            4,   // num_coding_shreds
            3,   // position
            shred_version,
        );
        shred.copy_to_packet(&mut packet);
        assert!(!should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        packet.buffer_mut()[OFFSET_OF_SHRED_VARIANT] = u8::MAX;

        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(1, stats.bad_shred_type);
        assert_eq!(stats.shred_version_mismatch, 0);

        packet.buffer_mut()[OFFSET_OF_SHRED_INDEX + SIZE_OF_SHRED_INDEX + 1] = u8::MAX;
        assert!(should_discard_shred(
            &packet,
            root,
            max_slot,
            shred_version,
            &mut stats
        ));
        assert_eq!(1, stats.bad_shred_type);
        assert_eq!(stats.shred_version_mismatch, 1);
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
        assert_eq!(u8::from(ShredType::Data), 0b1010_0101);
        assert_eq!(ShredType::try_from(0b1010_0101), Ok(ShredType::Data));
        let buf = bincode::serialize(&ShredType::Data).unwrap();
        assert_eq!(buf, vec![0b1010_0101]);
        assert_matches!(
            bincode::deserialize::<ShredType>(&[0b1010_0101]),
            Ok(ShredType::Data)
        );
        // coding shred
        assert_eq!(ShredType::Code as u8, 0b0101_1010);
        assert_eq!(u8::from(ShredType::Code), 0b0101_1010);
        assert_eq!(ShredType::try_from(0b0101_1010), Ok(ShredType::Code));
        let buf = bincode::serialize(&ShredType::Code).unwrap();
        assert_eq!(buf, vec![0b0101_1010]);
        assert_matches!(
            bincode::deserialize::<ShredType>(&[0b0101_1010]),
            Ok(ShredType::Code)
        );
    }

    #[test]
    fn test_shred_variant_compat() {
        assert_matches!(ShredVariant::try_from(0u8), Err(_));
        assert_matches!(ShredVariant::try_from(1u8), Err(_));
        assert_matches!(ShredVariant::try_from(0b0101_0000), Err(_));
        assert_matches!(ShredVariant::try_from(0b1010_0000), Err(_));
        assert_matches!(bincode::deserialize::<ShredVariant>(&[0b0101_0000]), Err(_));
        assert_matches!(bincode::deserialize::<ShredVariant>(&[0b1010_0000]), Err(_));
        // Legacy coding shred.
        assert_eq!(u8::from(ShredVariant::LegacyCode), 0b0101_1010);
        assert_eq!(ShredType::from(ShredVariant::LegacyCode), ShredType::Code);
        assert_matches!(
            ShredVariant::try_from(0b0101_1010),
            Ok(ShredVariant::LegacyCode)
        );
        let buf = bincode::serialize(&ShredVariant::LegacyCode).unwrap();
        assert_eq!(buf, vec![0b0101_1010]);
        assert_matches!(
            bincode::deserialize::<ShredVariant>(&[0b0101_1010]),
            Ok(ShredVariant::LegacyCode)
        );
        // Legacy data shred.
        assert_eq!(u8::from(ShredVariant::LegacyData), 0b1010_0101);
        assert_eq!(ShredType::from(ShredVariant::LegacyData), ShredType::Data);
        assert_matches!(
            ShredVariant::try_from(0b1010_0101),
            Ok(ShredVariant::LegacyData)
        );
        let buf = bincode::serialize(&ShredVariant::LegacyData).unwrap();
        assert_eq!(buf, vec![0b1010_0101]);
        assert_matches!(
            bincode::deserialize::<ShredVariant>(&[0b1010_0101]),
            Ok(ShredVariant::LegacyData)
        );
        // Merkle coding shred.
        assert_eq!(u8::from(ShredVariant::MerkleCode(5)), 0b0100_0101);
        assert_eq!(
            ShredType::from(ShredVariant::MerkleCode(5)),
            ShredType::Code
        );
        assert_matches!(
            ShredVariant::try_from(0b0100_0101),
            Ok(ShredVariant::MerkleCode(5))
        );
        let buf = bincode::serialize(&ShredVariant::MerkleCode(5)).unwrap();
        assert_eq!(buf, vec![0b0100_0101]);
        assert_matches!(
            bincode::deserialize::<ShredVariant>(&[0b0100_0101]),
            Ok(ShredVariant::MerkleCode(5))
        );
        for proof_size in 0..=15u8 {
            let byte = proof_size | 0b0100_0000;
            assert_eq!(u8::from(ShredVariant::MerkleCode(proof_size)), byte);
            assert_eq!(
                ShredType::from(ShredVariant::MerkleCode(proof_size)),
                ShredType::Code
            );
            assert_eq!(
                ShredVariant::try_from(byte).unwrap(),
                ShredVariant::MerkleCode(proof_size)
            );
            let buf = bincode::serialize(&ShredVariant::MerkleCode(proof_size)).unwrap();
            assert_eq!(buf, vec![byte]);
            assert_eq!(
                bincode::deserialize::<ShredVariant>(&[byte]).unwrap(),
                ShredVariant::MerkleCode(proof_size)
            );
        }
        // Merkle data shred.
        assert_eq!(u8::from(ShredVariant::MerkleData(10)), 0b1000_1010);
        assert_eq!(
            ShredType::from(ShredVariant::MerkleData(10)),
            ShredType::Data
        );
        assert_matches!(
            ShredVariant::try_from(0b1000_1010),
            Ok(ShredVariant::MerkleData(10))
        );
        let buf = bincode::serialize(&ShredVariant::MerkleData(10)).unwrap();
        assert_eq!(buf, vec![0b1000_1010]);
        assert_matches!(
            bincode::deserialize::<ShredVariant>(&[0b1000_1010]),
            Ok(ShredVariant::MerkleData(10))
        );
        for proof_size in 0..=15u8 {
            let byte = proof_size | 0b1000_0000;
            assert_eq!(u8::from(ShredVariant::MerkleData(proof_size)), byte);
            assert_eq!(
                ShredType::from(ShredVariant::MerkleData(proof_size)),
                ShredType::Data
            );
            assert_eq!(
                ShredVariant::try_from(byte).unwrap(),
                ShredVariant::MerkleData(proof_size)
            );
            let buf = bincode::serialize(&ShredVariant::MerkleData(proof_size)).unwrap();
            assert_eq!(buf, vec![byte]);
            assert_eq!(
                bincode::deserialize::<ShredVariant>(&[byte]).unwrap(),
                ShredVariant::MerkleData(proof_size)
            );
        }
    }

    #[test]
    fn test_shred_seed() {
        let mut rng = ChaChaRng::from_seed([147u8; 32]);
        let leader = Pubkey::new_from_array(rng.gen());
        let key = ShredId(
            141939602, // slot
            28685,     // index
            ShredType::Data,
        );
        assert_eq!(
            bs58::encode(key.seed(&leader)).into_string(),
            "Gp4kUM4ZpWGQN5XSCyM9YHYWEBCAZLa94ZQuSgDE4r56"
        );
        let leader = Pubkey::new_from_array(rng.gen());
        let key = ShredId(
            141945197, // slot
            23418,     // index
            ShredType::Code,
        );
        assert_eq!(
            bs58::encode(key.seed(&leader)).into_string(),
            "G1gmFe1QUM8nhDApk6BqvPgw3TQV2Qc5bpKppa96qbVb"
        );
    }

    fn verify_shred_layout(shred: &Shred, packet: &Packet) {
        let data = layout::get_shred(packet).unwrap();
        assert_eq!(data, packet.data(..).unwrap());
        assert_eq!(layout::get_slot(data), Some(shred.slot()));
        assert_eq!(layout::get_index(data), Some(shred.index()));
        assert_eq!(layout::get_version(data), Some(shred.version()));
        assert_eq!(layout::get_shred_id(data), Some(shred.id()));
        assert_eq!(layout::get_signature(data), Some(*shred.signature()));
        assert_eq!(layout::get_shred_type(data).unwrap(), shred.shred_type());
        match shred.shred_type() {
            ShredType::Code => {
                assert_matches!(
                    layout::get_reference_tick(data),
                    Err(Error::InvalidShredType)
                );
            }
            ShredType::Data => {
                assert_eq!(
                    layout::get_reference_tick(data).unwrap(),
                    shred.reference_tick()
                );
                let parent_offset = layout::get_parent_offset(data).unwrap();
                let slot = layout::get_slot(data).unwrap();
                let parent = slot.checked_sub(Slot::from(parent_offset)).unwrap();
                assert_eq!(parent, shred.parent().unwrap());
            }
        }
    }

    #[test]
    fn test_serde_compat_shred_data() {
        const SEED: &str = "6qG9NGWEtoTugS4Zgs46u8zTccEJuRHtrNMiUayLHCxt";
        const PAYLOAD: &str = "hNX8YgJCQwSFGJkZ6qZLiepwPjpctC9UCsMD1SNNQurBXv\
        rm7KKfLmPRMM9CpWHt6MsJuEWpDXLGwH9qdziJzGKhBMfYH63avcchjdaUiMqzVip7cUD\
        kqZ9zZJMrHCCUDnxxKMupsJWKroUSjKeo7hrug2KfHah85VckXpRna4R9QpH7tf2WVBTD\
        M4m3EerctsEQs8eZaTRxzTVkhtJYdNf74KZbH58dc3Yn2qUxF1mexWoPS6L5oZBatx";
        let mut rng = {
            let seed = <[u8; 32]>::try_from(bs58_decode(SEED)).unwrap();
            ChaChaRng::from_seed(seed)
        };
        let mut data = [0u8; legacy::ShredData::CAPACITY];
        rng.fill(&mut data[..]);
        let keypair = Keypair::generate(&mut rng);
        let mut shred = Shred::new_from_data(
            141939602, // slot
            28685,     // index
            36390,     // parent_offset
            &data,     // data
            ShredFlags::LAST_SHRED_IN_SLOT,
            37,    // reference_tick
            45189, // version
            28657, // fec_set_index
        );
        shred.sign(&keypair);
        assert!(shred.verify(&keypair.pubkey()));
        assert_matches!(shred.sanitize(), Ok(()));
        let mut payload = bs58_decode(PAYLOAD);
        payload.extend({
            let skip = payload.len() - SIZE_OF_DATA_SHRED_HEADERS;
            data.iter().skip(skip).copied()
        });
        let mut packet = Packet::default();
        packet.buffer_mut()[..payload.len()].copy_from_slice(&payload);
        packet.meta_mut().size = payload.len();
        assert_eq!(shred.bytes_to_store(), payload);
        assert_eq!(shred, Shred::new_from_serialized_shred(payload).unwrap());
        verify_shred_layout(&shred, &packet);
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
            ShredFlags::DATA_COMPLETE_SHRED,
            49,    // reference_tick
            59445, // version
            21414, // fec_set_index
        );
        shred.sign(&keypair);
        assert!(shred.verify(&keypair.pubkey()));
        assert_matches!(shred.sanitize(), Ok(()));
        let payload = bs58_decode(PAYLOAD);
        let mut packet = Packet::default();
        packet.buffer_mut()[..payload.len()].copy_from_slice(&payload);
        packet.meta_mut().size = payload.len();
        assert_eq!(shred.bytes_to_store(), payload);
        assert_eq!(shred, Shred::new_from_serialized_shred(payload).unwrap());
        verify_shred_layout(&shred, &packet);
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
        let mut parity_shard = vec![0u8; legacy::SIZE_OF_ERASURE_ENCODED_SLICE];
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
        packet.buffer_mut()[..payload.len()].copy_from_slice(&payload);
        packet.meta_mut().size = payload.len();
        assert_eq!(shred.bytes_to_store(), payload);
        assert_eq!(shred, Shred::new_from_serialized_shred(payload).unwrap());
        verify_shred_layout(&shred, &packet);
    }

    #[test]
    fn test_shred_flags() {
        fn make_shred(is_last_data: bool, is_last_in_slot: bool, reference_tick: u8) -> Shred {
            let flags = if is_last_in_slot {
                assert!(is_last_data);
                ShredFlags::LAST_SHRED_IN_SLOT
            } else if is_last_data {
                ShredFlags::DATA_COMPLETE_SHRED
            } else {
                ShredFlags::empty()
            };
            Shred::new_from_data(
                0,   // slot
                0,   // index
                0,   // parent_offset
                &[], // data
                flags,
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
                layout::get_reference_tick(shred.payload()).unwrap(),
                reference_tick.min(63u8),
            );
        }
        for is_last_data in [false, true] {
            for is_last_in_slot in [false, true] {
                // LAST_SHRED_IN_SLOT also implies DATA_COMPLETE_SHRED. So it
                // cannot be LAST_SHRED_IN_SLOT if not DATA_COMPLETE_SHRED.
                let is_last_in_slot = is_last_in_slot && is_last_data;
                for reference_tick in [0, 37, 63, 64, 80, 128, 255] {
                    let mut shred = make_shred(is_last_data, is_last_in_slot, reference_tick);
                    check_shred_flags(&shred, is_last_data, is_last_in_slot, reference_tick);
                    shred.set_last_in_slot();
                    check_shred_flags(&shred, true, true, reference_tick);
                }
            }
        }
    }

    #[test]
    fn test_shred_flags_serde() {
        let flags: ShredFlags = bincode::deserialize(&[0b0111_0001]).unwrap();
        assert!(flags.contains(ShredFlags::DATA_COMPLETE_SHRED));
        assert!(!flags.contains(ShredFlags::LAST_SHRED_IN_SLOT));
        assert_eq!((flags & ShredFlags::SHRED_TICK_REFERENCE_MASK).bits(), 49u8);
        assert_eq!(bincode::serialize(&flags).unwrap(), [0b0111_0001]);

        let flags: ShredFlags = bincode::deserialize(&[0b1110_0101]).unwrap();
        assert!(flags.contains(ShredFlags::DATA_COMPLETE_SHRED));
        assert!(flags.contains(ShredFlags::LAST_SHRED_IN_SLOT));
        assert_eq!((flags & ShredFlags::SHRED_TICK_REFERENCE_MASK).bits(), 37u8);
        assert_eq!(bincode::serialize(&flags).unwrap(), [0b1110_0101]);

        let flags: ShredFlags = bincode::deserialize(&[0b1011_1101]).unwrap();
        assert!(!flags.contains(ShredFlags::DATA_COMPLETE_SHRED));
        assert!(!flags.contains(ShredFlags::LAST_SHRED_IN_SLOT));
        assert_eq!((flags & ShredFlags::SHRED_TICK_REFERENCE_MASK).bits(), 61u8);
        assert_eq!(bincode::serialize(&flags).unwrap(), [0b1011_1101]);
    }
}
