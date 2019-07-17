//! Spv proof Verification Program
use crate::id;
use crate::spv_state::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::{AccountMeta, Instruction};


#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum SpvInstruction {
    // Client Places request for a matching proof
    ClientRequest(ClientRequestInfo);
    // used to cancel a pending proof request
    CancelRequest(BitcoinTxHash);
    // used to submit a proof matching a posted BitcoinTxHash or for own benefit
    SubmitProof(ProofSubmitInfo);
}


pub fn client_request(
    owner         : &Pubkey,
    txHash        : BitcoinTxHash,
    fee           : u64,
    confirmations : u8,
    difficulty    : u64,
) -> Instruction {
    let account_meta = vec![AccountMeta::new(*owner, true)];
    Instruction::new(
        id(),
        &SpvInstruction::ClientRequest(ClientRequestInfo{
            txHash,
            confirmations,
            fee,
            difficulty,
        }),
        account_meta,
    )
}

pub fn cancel_request(
    owner   : &Pubkey,
    txHash  : BitcoinTxHash,
) -> Instruction {
    let account_meta = vec![AccountMeta::new(*owner, true)];
    Instruction::new(
        id(),
        &SpvInstruction::CancelRequest(BitcoinTxHash),
        account_meta,
    )
}

pub fn submit_proof(
    submitter : &Pubkey,
    proof     : MerkleProof,
    headers   : HeaderChain,
    txhash    : BitcoinTxHash,
) -> Instruction {
    let account_meta = vec![AccountMeta::new(*submitter, true)];
    Instruction::new(
        id(),
        &SpvInstruction::CancelRequest(ProofSubmitInfo{
            proof,
            headers,
            txhash,
        }),
        acccount_meta,
    )
}
