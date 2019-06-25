use crate::storage_contract::{ProofStatus, STORAGE_ACCOUNT_SPACE};
use crate::{id, rewards_pools};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::syscall::{current, rewards};
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageInstruction {
    /// Initialize the account as a validator or replicator
    ///
    /// Expects 1 Account:
    ///    0 - Account to be initialized
    InitializeValidatorStorage {
        owner: Pubkey,
    },
    InitializeReplicatorStorage {
        owner: Pubkey,
    },

    SubmitMiningProof {
        sha_state: Hash,
        segment_index: usize,
        signature: Signature,
        blockhash: Hash,
    },
    AdvertiseStorageRecentBlockhash {
        hash: Hash,
        slot: u64,
    },
    /// Redeem storage reward credits
    ///
    /// Expects 1 Account:
    ///    0 - Storage account with credits to redeem
    ///    1 - Current Syscall to figure out the current epoch
    ///    2 - Replicator account to credit - this account *must* be the owner
    ///    3 - MiningPool account to redeem credits from
    ///    4 - Rewards Syscall to figure out point values
    ClaimStorageReward,
    ProofValidation {
        /// The segment during which this proof was generated
        segment: u64,
        /// A Vec of proof masks per keyed replicator account loaded by the instruction
        proofs: Vec<Vec<ProofStatus>>,
    },
}

fn get_ratios() -> (u64, u64) {
    // max number bytes available for account metas and proofs
    // The maximum transaction size is == `PACKET_DATA_SIZE` (1232 bytes)
    // There are approx. 900 bytes left over after the storage instruction is wrapped into
    // a signed transaction.
    static MAX_BYTES: u64 = 900;
    let account_meta_size: u64 =
        bincode::serialized_size(&AccountMeta::new(Pubkey::new_rand(), false)).unwrap_or(0);
    let proof_size: u64 = bincode::serialized_size(&ProofStatus::default()).unwrap_or(0);

    // the ratio between account meta size and a single proof status
    let ratio = (account_meta_size + proof_size - 1) / proof_size;
    let bytes = (MAX_BYTES + ratio - 1) / ratio;
    (ratio, bytes)
}

/// Returns how many accounts and their proofs will fit in a single proof validation tx
///
/// # Arguments
///
/// * `proof_mask_max` - The largest proof mask across all accounts intended for submission
///
pub fn validation_account_limit(proof_mask_max: usize) -> u64 {
    let (ratio, bytes) = get_ratios();
    // account_meta_count * (ratio + proof_mask_max) = bytes
    bytes / (ratio + proof_mask_max as u64)
}

pub fn proof_mask_limit() -> u64 {
    let (ratio, bytes) = get_ratios();
    bytes - ratio
}

pub fn create_validator_storage_account(
    from_pubkey: &Pubkey,
    storage_owner: &Pubkey,
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
            &StorageInstruction::InitializeValidatorStorage {
                owner: *storage_owner,
            },
            vec![AccountMeta::new(*storage_pubkey, false)],
        ),
    ]
}

pub fn create_replicator_storage_account(
    from_pubkey: &Pubkey,
    storage_owner: &Pubkey,
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
            &StorageInstruction::InitializeReplicatorStorage {
                owner: *storage_owner,
            },
            vec![AccountMeta::new(*storage_pubkey, false)],
        ),
    ]
}

pub fn mining_proof(
    storage_pubkey: &Pubkey,
    sha_state: Hash,
    segment_index: usize,
    signature: Signature,
    blockhash: Hash,
) -> Instruction {
    let storage_instruction = StorageInstruction::SubmitMiningProof {
        sha_state,
        segment_index,
        signature,
        blockhash,
    };
    let account_metas = vec![
        AccountMeta::new(*storage_pubkey, true),
        AccountMeta::new(current::id(), false),
    ];
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
    let account_metas = vec![
        AccountMeta::new(*storage_pubkey, true),
        AccountMeta::new(current::id(), false),
    ];
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn proof_validation(
    storage_pubkey: &Pubkey,
    segment: u64,
    checked_proofs: Vec<(Pubkey, Vec<ProofStatus>)>,
) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*storage_pubkey, true),
        AccountMeta::new(current::id(), false),
    ];
    let mut proofs = vec![];
    checked_proofs.into_iter().for_each(|(id, p)| {
        proofs.push(p);
        account_metas.push(AccountMeta::new(id, false))
    });
    let storage_instruction = StorageInstruction::ProofValidation { segment, proofs };
    Instruction::new(id(), &storage_instruction, account_metas)
}

pub fn claim_reward(owner_pubkey: &Pubkey, storage_pubkey: &Pubkey) -> Instruction {
    let storage_instruction = StorageInstruction::ClaimStorageReward;
    let account_metas = vec![
        AccountMeta::new(*storage_pubkey, false),
        AccountMeta::new(current::id(), false),
        AccountMeta::new(rewards::id(), false),
        AccountMeta::new(rewards_pools::random_id(), false),
        AccountMeta::new(*owner_pubkey, false),
    ];
    Instruction::new(id(), &storage_instruction, account_metas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_size() {
        // check that if there's 50 proof per account, only 1 account can fit in a single tx
        assert_eq!(validation_account_limit(50), 1);
    }
}
