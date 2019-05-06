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
        slot: u64,
        signature: Signature,
    },
    AdvertiseStorageRecentBlockhash {
        hash: Hash,
        slot: u64,
    },
    ClaimStorageReward {
        slot: u64,
    },
    ProofValidation {
        slot: u64,
        proofs: Vec<CheckedProof>,
    },
}

pub fn mining_proof(
    from_pubkey: &Pubkey,
    sha_state: Hash,
    slot: u64,
    signature: Signature,
) -> Instruction {
    let storage_instruction = StorageInstruction::SubmitMiningProof {
        sha_state,
        slot,
        signature,
    };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn advertise_recent_blockhash(
    from_pubkey: &Pubkey,
    storage_hash: Hash,
    slot: u64,
) -> Instruction {
    let storage_instruction = StorageInstruction::AdvertiseStorageRecentBlockhash {
        hash: storage_hash,
        slot,
    };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn proof_validation(from_pubkey: &Pubkey, slot: u64, proofs: Vec<CheckedProof>) -> Instruction {
    let mut account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    proofs.iter().for_each(|checked_proof| {
        account_metas.push(AccountMeta::new(checked_proof.proof.id, false))
    });
    let storage_instruction = StorageInstruction::ProofValidation { slot, proofs };
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn reward_claim(from_pubkey: &Pubkey, slot: u64) -> Instruction {
    let storage_instruction = StorageInstruction::ClaimStorageReward { slot };
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}
