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

        match hex::decode(header) {
            Ok(header) => {
                let bhbytes = hex::decode(blockhash)?;
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
pub struct BitcoinTransaction {
    pub inputs: Vec<Input>,

    pub inputs_num: u64,
    //input utxos
    pub outputs: Vec<Output>,

    pub outputs_num: u64,
    //output utxos
    pub version: u32,
    //bitcoin network version
    pub lock_time: u32,

    pub bytes_len: usize,
}

impl BitcoinTransaction {
    pub fn new(txbytes: Vec<u8>) -> Self {
        let mut ver: [u8; 4] = [0; 4];
        ver.copy_from_slice(&txbytes[..4]);
        let version = u32::from_le_bytes(ver);

        let inputs_num: u64 = decode_variable_int(&txbytes[4..13]).unwrap();
        let vinlen: usize = measure_variable_int(&txbytes[4..13]).unwrap();
        let mut inputstart: usize = 4 + vinlen;
        let mut inputs = Vec::new();

        if inputs_num > 0 {
            for i in 0..inputs_num {
                let mut input = Input::new(txbytes[inputstart..].to_vec());
                inputstart += input.bytes_len;
                inputs.push(input);
            }
            inputs.to_vec();
        }

        let outputs_num: u64 = decode_variable_int(&txbytes[inputstart..9 + inputstart]).unwrap();
        let voutlen: usize = measure_variable_int(&txbytes[inputstart..9 + inputstart]).unwrap();

        let mut outputstart: usize = inputstart + voutlen;
        let mut outputs = Vec::new();
        for i in 0..outputs_num {
            let mut output = Output::new(txbytes[outputstart..].to_vec());
            outputstart += output.bytes_len;
            outputs.push(output);
        }

        let mut lt: [u8; 4] = [0; 4];
        lt.copy_from_slice(&txbytes[outputstart..4 + outputstart]);
        let lock_time = u32::from_le_bytes(lt);

        assert_eq!(inputs.len(), inputs_num as usize);
        assert_eq!(outputs.len(), outputs_num as usize);

        BitcoinTransaction {
            inputs,
            inputs_num,
            outputs,
            outputs_num,
            version,
            lock_time,
            bytes_len: 4 + outputstart,
        }
    }
    pub fn hexnew(hex: String) -> Result<BitcoinTransaction, SpvError> {
        match hex::decode(&hex) {
            Ok(txbytes) => Ok(BitcoinTransaction::new(txbytes)),
            Err(e) => Err(SpvError::ParseError),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Input {
    pub input_type: InputType,
    // Type of the input
    pub position: u32,
    // position of the tx in its Block
    pub txhash: BitcoinTxHash,
    // hash of the transaction
    pub script_length: u64,
    // length of the spend script
    pub script: Vec<u8>,
    // script bytes
    pub sequence: [u8; 4],
    // length of the input in bytes
    pub bytes_len: usize,
}

impl Input {
    fn new(ibytes: Vec<u8>) -> Self {
        let mut txhash: [u8; 32] = [0; 32];
        txhash.copy_from_slice(&ibytes[..32]);

        let mut tx_out_index: [u8; 4] = [0; 4];
        tx_out_index.copy_from_slice(&ibytes[32..36]);
        let position = u32::from_le_bytes(tx_out_index);

        let script_length: u64 = decode_variable_int(&ibytes[36..45]).unwrap();
        let script_length_len: usize = measure_variable_int(&ibytes[36..45]).unwrap();
        let script_start = 36 + script_length_len; //checkc for correctness
        let script_end = script_start + script_length as usize;
        let input_end = script_end + 4;

        let script: Vec<u8> = ibytes[script_start..script_length as usize].to_vec();

        let mut sequence: [u8; 4] = [0; 4];
        sequence.copy_from_slice(&ibytes[script_end..input_end]);

        let input_type: InputType = InputType::NONE; // testing measure

        Self {
            input_type,
            position,
            txhash,
            script_length,
            script,
            sequence,
            bytes_len: input_end,
        }
    }

    fn default() -> Self {
        let txh: [u8; 32] = [0; 32];
        let seq: [u8; 4] = [0; 4];

        Self {
            input_type: InputType::NONE,
            position: 55,
            txhash: txh,
            script_length: 45,
            script: txh.to_vec(),
            sequence: seq,
            bytes_len: 123,
        }
    }
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
    pub output_type: OutputType,
    // type of the output
    pub value: u64,
    // amount of btc in sats
    pub script: Vec<u8>,

    pub script_length: u64,

    pub bytes_len: usize,
    // payload: Option<Vec<u8>>,
    // // data sent with the transaction (Op return)
}

impl Output {
    fn new(obytes: Vec<u8>) -> Self {
        let mut val: [u8; 8] = [0; 8];
        val.copy_from_slice(&obytes[..8]);
        let value: u64 = u64::from_le_bytes(val);

        let script_start: usize = 8 + measure_variable_int(&obytes[8..17]).unwrap();
        let script_length = decode_variable_int(&obytes[8..script_start]).unwrap();
        let script_end: usize = script_start + script_length as usize;

        let script = obytes[script_start..script_end].to_vec();

        let output_type = OutputType::WPKH; // temporary hardcode

        Self {
            output_type,
            value,
            script,
            script_length,
            bytes_len: script_end,
        }
    }

    fn default() -> Self {
        let transaction_hash: [u8; 32] = [0; 32];

        Self {
            output_type: OutputType::WPKH,
            value: 55,
            script: transaction_hash.to_vec(),
            script_length: 45,
            bytes_len: 123,
        }
    }
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
    // https://github.com/bitcoin/bitcoin/blob/master/doc/descriptors.md
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
    pub transaction: BitcoinTransaction,
    // public key of the request this proof corresponds to
    pub request: Pubkey,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum AccountState {
    // Request Account
    Request(ClientRequestInfo),
    // Verified Proof
    Verification(Proof),
    // Account holds a HeaderStore structure
    Headers(HeaderAccountInfo),
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
    InvalidAccount,
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

impl From<hex::FromHexError> for SpvError {
    fn from(e: hex::FromHexError) -> Self {
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
            SpvError::InvalidAccount => "Provided account is not usable or does not exist".fmt(f),
        }
    }
}
