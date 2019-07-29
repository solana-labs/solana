use crate::id;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::{AccountMeta, Instruction};


pub type BitcoinTxHash = [u8;32];

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct BlockHeader {
    // Block hash
    pub digest      : BitcoinTxHash,
    // Previous block's hash/digest
    pub parent      : BitcoinTxHash,
    // merkle Root of the block, proofEntry side should be None
    pub merkle_root : ProofEntry,
    // Bitcoin network version
    pub version     : u32,
    // the blocktime associate with the block
    pub time        : u32,
    // An encoded version of the target threshold this block’s header hash must be less than or equal to.
    pub difficulty  : u32,
}


pub type HeaderChain = Vec<BlockHeader>;
// a vector of BlockHeaders used as part of a Proof
// index 0    : the block header of the block prior to the proof Block
// index 1    : the block header of the proof block
// index 2-n* : the block headers for the confirmation chain
// (where n is the confirmations value from the proof request)

pub struct ProofEntry    = {
    // 32 byte merkle hashes
    pub hash: [u8;32],
    // side of the merkle tree entry
    pub side: EntrySide,
}

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
    pub txHash:        BitcoinTxHash,
    // confirmation count
    pub confirmations: u8,
    // fee paid for tx verification
    pub fee:           u64,
    // required minimum difficulty for submitted blocks
    pub difficulty:    Option<u32>,
    // expiration slot height
    pub expiration:    u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ProofRequest {
    pub owner:         Pubkey,
    // bitcoin transaction hash
    pub txHash:        BitcoinTxHash,
    // confirmation count
    pub confirmations: u8,
    // fee paid for tx verification
    pub fee:           u64,
    // minimum allowable difficulty
    pub difficulty:    u64,
    // expiration slot height
    pub expiration:    u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ProofInfo {
    // the pubkey who submitted the proof in question, entitled to fees from any corresponding proof requests
    pub submitter:  Pubkey,
    // merkle branch connecting txhash to block header merkle root
    pub proof:      MerkleProof,
    // chain of bitcoin headers provifing context for the proof
    pub headers:    HeaderChain,
    // computed validity of the proof in question
    pub validity:   Option<bool>,
    // txhash associated with the Proof
    pub txhash:     BitcoinTxHash,
}

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
