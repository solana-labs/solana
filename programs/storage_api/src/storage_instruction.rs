use crate::id;
use crate::storage_contract::ProofStatus;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageInstruction {
    SubmitMiningProof {
        sha_state: Hash,
        entry_height: u64,
        signature: Signature,
    },
    AdvertiseStorageRecentBlockhash {
        hash: Hash,
        entry_height: u64,
    },
    ClaimStorageReward {
        entry_height: u64,
    },
    ProofValidation {
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    },
}

pub fn mining_proof(
    from_pubkey: &Pubkey,
    sha_state: Hash,
    entry_height: u64,
    signature: Signature,
) -> Instruction {
    let storage_instruction = StorageInstruction::SubmitMiningProof {
        sha_state,
        entry_height,
        signature,
    };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn advertise_recent_blockhash(
    from_pubkey: &Pubkey,
    storage_hash: Hash,
    entry_height: u64,
) -> Instruction {
    let storage_instruction = StorageInstruction::AdvertiseStorageRecentBlockhash {
        hash: storage_hash,
        entry_height,
    };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn proof_validation(
    from_pubkey: &Pubkey,
    entry_height: u64,
    proof_mask: Vec<ProofStatus>,
) -> Instruction {
    let storage_instruction = StorageInstruction::ProofValidation {
        entry_height,
        proof_mask,
    };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn reward_claim(from_pubkey: &Pubkey, entry_height: u64) -> Instruction {
    let storage_instruction = StorageInstruction::ClaimStorageReward { entry_height };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}
