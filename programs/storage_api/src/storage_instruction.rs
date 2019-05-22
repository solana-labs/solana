use crate::id;
use crate::storage_contract::{CheckedProof, STORAGE_ACCOUNT_SPACE};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageInstruction {
    /// Initialize the account as a mining pool, validator or replicator
    ///
    /// Expects 1 Account:
    ///    0 - Account to be initialized
    InitializeMiningPool,
    InitializeValidatorStorage,
    InitializeReplicatorStorage,

    SubmitMiningProof {
        sha_state: Hash,
        slot: u64,
        signature: Signature,
    },
    AdvertiseStorageRecentBlockhash {
        hash: Hash,
        slot: u64,
    },
    /// Redeem storage reward credits
    ///
    /// Expects 1 Account:
    ///    0 - Storage account with credits to redeem
    ///    1 - MiningPool account to redeem credits from
    ClaimStorageReward {
        slot: u64,
    },
    ProofValidation {
        slot: u64,
        proofs: Vec<CheckedProof>,
    },
}

pub fn create_validator_storage_account(
    from_pubkey: &Pubkey,
    storage_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            storage_pubkey,
            lamports,
            STORAGE_ACCOUNT_SPACE,
            &id(),
        ),
        Instruction::new(
            id(),
            &StorageInstruction::InitializeValidatorStorage,
            vec![AccountMeta::new(*storage_pubkey, false)],
        ),
    ]
}

pub fn create_replicator_storage_account(
    from_pubkey: &Pubkey,
    storage_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            storage_pubkey,
            lamports,
            STORAGE_ACCOUNT_SPACE,
            &id(),
        ),
        Instruction::new(
            id(),
            &StorageInstruction::InitializeReplicatorStorage,
            vec![AccountMeta::new(*storage_pubkey, false)],
        ),
    ]
}

pub fn create_mining_pool_account(
    from_pubkey: &Pubkey,
    storage_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            storage_pubkey,
            lamports,
            STORAGE_ACCOUNT_SPACE,
            &id(),
        ),
        Instruction::new(
            id(),
            &StorageInstruction::InitializeMiningPool,
            vec![AccountMeta::new(*storage_pubkey, false)],
        ),
    ]
}

pub fn mining_proof(
    storage_pubkey: &Pubkey,
    sha_state: Hash,
    slot: u64,
    signature: Signature,
) -> Instruction {
    let storage_instruction = StorageInstruction::SubmitMiningProof {
        sha_state,
        slot,
        signature,
    };
    let account_metas = vec![AccountMeta::new(*storage_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn advertise_recent_blockhash(
    storage_pubkey: &Pubkey,
    storage_hash: Hash,
    slot: u64,
) -> Instruction {
    let storage_instruction = StorageInstruction::AdvertiseStorageRecentBlockhash {
        hash: storage_hash,
        slot,
    };
    let account_metas = vec![AccountMeta::new(*storage_pubkey, true)];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn proof_validation(
    storage_pubkey: &Pubkey,
    slot: u64,
    proofs: Vec<CheckedProof>,
) -> Instruction {
    let mut account_metas = vec![AccountMeta::new(*storage_pubkey, true)];
    proofs.iter().for_each(|checked_proof| {
        account_metas.push(AccountMeta::new(checked_proof.proof.id, false))
    });
    let storage_instruction = StorageInstruction::ProofValidation { slot, proofs };
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn claim_reward(
    storage_pubkey: &Pubkey,
    mining_pool_pubkey: &Pubkey,
    slot: u64,
) -> Instruction {
    let storage_instruction = StorageInstruction::ClaimStorageReward { slot };
    let account_metas = vec![
        AccountMeta::new(*storage_pubkey, false),
        AccountMeta::new(*mining_pool_pubkey, false),
    ];
    Instruction::new(id(), &storage_instruction, account_metas)
}
