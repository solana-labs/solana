use crate::header_store::*;
use crate::utils::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{error, fmt};

pub type BitcoinTxHash = [u8; 32];

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct BlockHeader {
    // Bitcoin network version
    pub version: u32,
    // Previous block's hash/digest
    pub parent: [u8; 32],
    // merkle Root of the block, proofEntry side should be None
    pub merkle_root: ProofEntry,
    // the blocktime associate with the block
    pub time: u32,
    // An encoded version of the target threshold this block’s header hash must be less than or equal to.
    pub nbits: [u8; 4],
    // block header's nonce
    pub nonce: [u8; 4],
    // Block hash
    pub blockhash: [u8; 32],
}

impl BlockHeader {
    pub fn new(header: &[u8; 80], blockhash: &[u8; 32]) -> Result<BlockHeader, SpvError> {
        let mut va: [u8; 4] = [0; 4];
        va.copy_from_slice(&header[0..4]);
        let version = u32::from_le_bytes(va);

        let mut ph: [u8; 32] = [0; 32];
        ph.copy_from_slice(&header[4..36]);
        let parent = ph;
        // extract merkle root in internal byte order
        let mut mrr: [u8; 32] = [0; 32];
        mrr.copy_from_slice(&header[36..68]);
        let merkle_root = ProofEntry {
            hash: mrr,
            side: EntrySide::Root,
        };
        // timestamp associate with the block
        let mut bt: [u8; 4] = [0; 4];
        bt.copy_from_slice(&header[68..72]);
        let time = u32::from_le_bytes(bt);

        // nbits field is an encoded version of the
        let mut nb: [u8; 4] = [0; 4];
        nb.copy_from_slice(&header[72..76]);
        let nbits = nb;

        let mut nn: [u8; 4] = [0; 4];
        nn.copy_from_slice(&header[76..80]);
        let nonce = nn;

        let bh = BlockHeader {
            version,
            parent,
            merkle_root,
            time,
            nbits,
            nonce,
            blockhash: *blockhash,
        };
        Ok(bh)
    }

    pub fn hexnew(header: &str, blockhash: &str) -> Result<BlockHeader, SpvError> {
        if header.len() != 160 || blockhash.len() != 64 {
            return Err(SpvError::InvalidBlockHeader);
        }

        match decode_hex(header) {
            Ok(header) => {
                let bhbytes = decode_hex(blockhash)?;
                const SIZE: usize = 80;
                let mut hh = [0; SIZE];
                hh.copy_from_slice(&header[..header.len()]);

                let mut bhb: [u8; 32] = [0; 32];
                bhb.copy_from_slice(&bhbytes[..bhbytes.len()]);

                Ok(BlockHeader::new(&hh, &bhb).unwrap())
            }
            Err(e) => Err(SpvError::InvalidBlockHeader),
        }
    }

    pub fn difficulty(mut self) -> u32 {
        // calculates difficulty from nbits
        let standin: u32 = 123_456_789;
        standin
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction {
    inputs: Vec<Input>,
    //input utxos
    outputs: Vec<Output>,
    //output utxos
    version: u32,
    //bitcoin network version
    locktime: u32,
}

// impl Transaction {
//     fn new(bytes: Vec<u8>) -> Self {
//         //reinsert later
//     }
//     fn hexnew(hex: String) -> Self {
//         //reinsert later
//     }
// }

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Input {
    r#type: InputType,
    // Type of the input
    position: u32,
    // position of the tx in its Block
    txhash: BitcoinTxHash,
    // hash of the transaction
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum InputType {
    LEGACY,
    COMPATIBILITY,
    WITNESS,
    NONE,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Output {
    r#type: OutputType,
    // type of the output
    value: u64,
    // amount of btc in sats
    payload: Vec<u8>, // data sent with the transaction
}

#[allow(non_camel_case_types)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum OutputType {
    WPKH,
    WSH,
    OP_RETURN,
    PKH,
    SH,
    NONSTANDARD,
}

pub type HeaderChain = Vec<BlockHeader>;
// a vector of BlockHeaders used as part of a Proof
// index 0    : the block header of the block prior to the proof Block
// index 1    : the block header of the proof block
// index 2-n* : the block headers for the confirmation chain
// (where n is the confirmations value from the proof request)

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ProofEntry {
    // 32 byte merkle hashes
    pub hash: [u8; 32],
    // side of the merkle tree entry
    pub side: EntrySide,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum EntrySide {
    // Left side of the hash combination
    Left,
    // Right side of hash combination
    Right,
    // Root hash (neither side)
    Root,
}

pub type MerkleProof = Vec<ProofEntry>;
// a vector of ProofEntries used as part of a Proof
// index 0     : a ProofEntry representing the txid
// indices 0-n : ProofEntries linking the txhash and the merkle root
// index n     : a ProofEntry representing the merkel root for the block in question

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ClientRequestInfo {
    // bitcoin transaction hash
    pub txhash: BitcoinTxHash,
    // confirmation count
    pub confirmations: u8,
    // fee paid for tx verification
    pub fee: u64,
    // required minimum difficulty for submitted blocks
    pub difficulty: u64,
    // expiration slot height
    pub expiration: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ProofRequest {
    pub owner: Pubkey,
    // bitcoin transaction hash
    pub txhash: BitcoinTxHash,
    // confirmation count
    pub confirmations: u8,
    // fee paid for tx verification
    pub fee: u64,
    // minimum allowable difficulty
    pub difficulty: u64,
    // expiration slot height
    pub expiration: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Proof {
    // the pubkey who submitted the proof in question, entitled to fees from any corresponding proof requests
    pub submitter: Pubkey,
    // merkle branch connecting txhash to block header merkle root
    pub proof: MerkleProof,
    // chain of bitcoin headers provifing context for the proof
    pub headers: HeaderChain,
    // transaction associated with the Proof
    pub transaction: Transaction,
    // public key of the request this proof corresponds to
    pub request: Pubkey,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum AccountState {
    // Request Account
    Request(ClientRequestInfo),
    // Verified Proof
    Verification(Proof),
    // Account's userdata is Unallocated
    Unallocated,
    // Invalid
    Invalid,
}

impl Default for AccountState {
    fn default() -> Self {
        AccountState::Unallocated
    }
}

///Errors
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum SpvError {
    InvalidBlockHeader,
    // blockheader is malformed or out of order
    HeaderStoreError,
    // header store write/read result is invalid
    ParseError,
    // other errors with parsing inputs
}

impl error::Error for SpvError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        // temporary measure
        None
    }
}

impl From<HeaderStoreError> for SpvError {
    fn from(e: HeaderStoreError) -> Self {
        SpvError::HeaderStoreError
    }
}

impl From<DecodeHexError> for SpvError {
    fn from(e: DecodeHexError) -> Self {
        SpvError::ParseError
    }
}

// impl fmt::Debug for SpvError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result{
//         match self {
//             SpvError::InvalidBlockHeader  => "BlockHeader is malformed or does not apply ".fmt(f),
//             SpvError::HeaderStoreError => "Placeholder headerstore error debug text".fmt(f),
//             SpvError::ParseError => "Error parsing blockheaders debug".fmt(f),
//         }
//     }
// }

impl fmt::Display for SpvError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SpvError::InvalidBlockHeader => "BlockHeader is malformed or does not apply ".fmt(f),
            SpvError::HeaderStoreError => "Placeholder headerstore error text".fmt(f),
            SpvError::ParseError => "Error parsing blockheaders placceholder text".fmt(f),
        }
    }
}
