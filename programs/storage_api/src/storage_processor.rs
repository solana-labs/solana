//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.
use crate::storage_contract::StorageAccount;
use crate::storage_instruction::StorageInstruction;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::syscall::tick_height::TickHeight;
use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let (me, rest) = keyed_accounts.split_at_mut(1);
    let me_unsigned = me[0].signer_key().is_none();
    let mut storage_account = StorageAccount::new(&mut me[0].account);

    match bincode::deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        StorageInstruction::InitializeMiningPool => {
            if !rest.is_empty() {
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.initialize_mining_pool()
        }
        StorageInstruction::InitializeReplicatorStorage { owner } => {
            if !rest.is_empty() {
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.initialize_replicator_storage(owner)
        }
        StorageInstruction::InitializeValidatorStorage { owner } => {
            if !rest.is_empty() {
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.initialize_validator_storage(owner)
        }
        StorageInstruction::SubmitMiningProof {
            sha_state,
            slot,
            signature,
        } => {
            if me_unsigned || rest.len() != 1 {
                // This instruction must be signed by `me`
                Err(InstructionError::InvalidArgument)?;
            }
            let tick_height = TickHeight::from(&rest[0].account).unwrap();
            storage_account.submit_mining_proof(
                sha_state,
                slot,
                signature,
                tick_height / DEFAULT_TICKS_PER_SLOT,
            )
        }
        StorageInstruction::AdvertiseStorageRecentBlockhash { hash, slot } => {
            if me_unsigned || rest.len() != 1 {
                // This instruction must be signed by `me`
                Err(InstructionError::InvalidArgument)?;
            }
            let tick_height = TickHeight::from(&rest[0].account).unwrap();
            storage_account.advertise_storage_recent_blockhash(
                hash,
                slot,
                tick_height / DEFAULT_TICKS_PER_SLOT,
            )
        }
        StorageInstruction::ClaimStorageReward { slot } => {
            if rest.len() != 2 {
                Err(InstructionError::InvalidArgument)?;
            }
            let tick_height = TickHeight::from(&rest[1].account).unwrap();
            storage_account.claim_storage_reward(
                &mut rest[0],
                slot,
                tick_height / DEFAULT_TICKS_PER_SLOT,
            )
        }
        StorageInstruction::ProofValidation { segment, proofs } => {
            if me_unsigned || rest.is_empty() {
                // This instruction must be signed by `me` and `rest` cannot be empty
                Err(InstructionError::InvalidArgument)?;
            }
            let mut rest: Vec<_> = rest
                .iter_mut()
                .map(|keyed_account| StorageAccount::new(&mut keyed_account.account))
                .collect();
            storage_account.proof_validation(segment, proofs, &mut rest)
        }
    }
}
