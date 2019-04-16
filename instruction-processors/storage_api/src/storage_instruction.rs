use crate::id;
use crate::storage_contract::CheckedProof;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

// TODO maybe split this into StorageReplicator and StorageValidator
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
        proofs: Vec<CheckedProof>,
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
    proofs: Vec<CheckedProof>,
) -> Instruction {
    let mut account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    proofs.iter().for_each(|checked_proof| {
        account_metas.push(AccountMeta::new(checked_proof.proof.id, false))
    });
    let storage_instruction = StorageInstruction::ProofValidation {
        entry_height,
        proofs,
    };
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn reward_claim(from_pubkey: &Pubkey, entry_height: u64) -> Instruction {
    let storage_instruction = StorageInstruction::ClaimStorageReward { entry_height };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}
