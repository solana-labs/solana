//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.
use crate::storage_contract::StorageAccount;
use crate::storage_instruction::StorageInstruction;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::sysvar;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let (me, rest) = keyed_accounts.split_at_mut(1);
    let me_unsigned = me[0].signer_key().is_none();
    let mut storage_account = StorageAccount::new(*me[0].unsigned_key(), &mut me[0].account);

    match bincode::deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        StorageInstruction::InitializeReplicatorStorage { owner } => {
            if !rest.is_empty() {
                return Err(InstructionError::InvalidArgument);
            }
            storage_account.initialize_replicator_storage(owner)
        }
        StorageInstruction::InitializeValidatorStorage { owner } => {
            if !rest.is_empty() {
                return Err(InstructionError::InvalidArgument);
            }
            storage_account.initialize_validator_storage(owner)
        }
        StorageInstruction::SubmitMiningProof {
            sha_state,
            segment_index,
            signature,
            blockhash,
        } => {
            if me_unsigned || rest.len() != 1 {
                // This instruction must be signed by `me`
                return Err(InstructionError::InvalidArgument);
            }
            let clock = sysvar::clock::from_keyed_account(&rest[0])?;
            storage_account.submit_mining_proof(
                sha_state,
                segment_index,
                signature,
                blockhash,
                clock,
            )
        }
        StorageInstruction::AdvertiseStorageRecentBlockhash { hash, segment } => {
            if me_unsigned || rest.len() != 1 {
                // This instruction must be signed by `me`
                return Err(InstructionError::InvalidArgument);
            }
            let clock = sysvar::clock::from_keyed_account(&rest[0])?;
            storage_account.advertise_storage_recent_blockhash(hash, segment, clock)
        }
        StorageInstruction::ClaimStorageReward => {
            if rest.len() != 4 {
                return Err(InstructionError::InvalidArgument);
            }
            let (clock, rest) = rest.split_at_mut(1);
            let (rewards, rest) = rest.split_at_mut(1);
            let (rewards_pools, owner) = rest.split_at_mut(1);

            let rewards = sysvar::rewards::from_keyed_account(&rewards[0])?;
            let clock = sysvar::clock::from_keyed_account(&clock[0])?;
            let mut owner = StorageAccount::new(*owner[0].unsigned_key(), &mut owner[0].account);

            storage_account.claim_storage_reward(&mut rewards_pools[0], clock, rewards, &mut owner)
        }
        StorageInstruction::ProofValidation { segment, proofs } => {
            if rest.is_empty() {
                return Err(InstructionError::InvalidArgument);
            }

            let (clock, rest) = rest.split_at_mut(1);
            if me_unsigned || rest.is_empty() {
                // This instruction must be signed by `me` and `rest` cannot be empty
                return Err(InstructionError::InvalidArgument);
            }
            let me_id = storage_account.id;
            let clock = sysvar::clock::from_keyed_account(&clock[0])?;
            let mut rest: Vec<_> = rest
                .iter_mut()
                .map(|keyed_account| {
                    StorageAccount::new(*keyed_account.unsigned_key(), &mut keyed_account.account)
                })
                .collect();
            storage_account.proof_validation(&me_id, clock, segment, proofs, &mut rest)
        }
    }
}
