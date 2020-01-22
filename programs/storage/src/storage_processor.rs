//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.
use crate::{storage_contract::StorageAccount, storage_instruction::StorageInstruction};
use solana_sdk::{
    account::KeyedAccount,
    instruction::InstructionError,
    instruction_processor_utils::limited_deserialize,
    pubkey::Pubkey,
    sysvar::{clock::Clock, rewards::Rewards, Sysvar},
};

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let (me, rest) = keyed_accounts.split_at_mut(1);
    let me_unsigned = me[0].signer_key().is_none();
    let mut me_account = me[0].try_account_ref_mut()?;
    let mut storage_account = StorageAccount::new(*me[0].unsigned_key(), &mut me_account);

    match limited_deserialize(data)? {
        StorageInstruction::InitializeStorage {
            owner,
            account_type,
        } => {
            if !rest.is_empty() {
                return Err(InstructionError::InvalidArgument);
            }
            storage_account.initialize_storage(owner, account_type)
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
            let clock = Clock::from_keyed_account(&rest[0])?;
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
            let clock = Clock::from_keyed_account(&rest[0])?;
            storage_account.advertise_storage_recent_blockhash(hash, segment, clock)
        }
        StorageInstruction::ClaimStorageReward => {
            if rest.len() != 4 {
                return Err(InstructionError::InvalidArgument);
            }
            let (clock, rest) = rest.split_at_mut(1);
            let (rewards, rest) = rest.split_at_mut(1);
            let (rewards_pools, owner) = rest.split_at_mut(1);

            let rewards = Rewards::from_keyed_account(&rewards[0])?;
            let clock = Clock::from_keyed_account(&clock[0])?;
            let mut owner_account = owner[0].try_account_ref_mut()?;
            let mut owner = StorageAccount::new(*owner[0].unsigned_key(), &mut owner_account);

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
            let clock = Clock::from_keyed_account(&clock[0])?;
            let mut rest = rest
                .iter()
                .map(|keyed_account| Ok((keyed_account, keyed_account.try_account_ref_mut()?)))
                .collect::<Result<Vec<_>, InstructionError>>()?;
            let mut rest = rest
                .iter_mut()
                .map(|(keyed_account, account_ref)| {
                    StorageAccount::new(*keyed_account.unsigned_key(), account_ref)
                })
                .collect::<Vec<_>>();
            storage_account.proof_validation(&me_id, clock, segment, proofs, &mut rest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        id,
        storage_contract::STORAGE_ACCOUNT_SPACE,
        storage_instruction::{self, StorageAccountType},
    };
    use log::*;

    use assert_matches::assert_matches;
    use solana_sdk::{
        account::{create_keyed_accounts, Account, KeyedAccount},
        clock::DEFAULT_SLOTS_PER_SEGMENT,
        hash::Hash,
        instruction::{Instruction, InstructionError},
        signature::Signature,
        sysvar::{
            clock::{self, Clock},
            Sysvar,
        },
    };
    use std::cell::RefCell;

    fn test_instruction(
        ix: &Instruction,
        program_accounts: &mut [Account],
    ) -> Result<(), InstructionError> {
        let program_accounts: Vec<_> = program_accounts
            .iter()
            .map(|account| RefCell::new(account.clone()))
            .collect();
        let mut keyed_accounts: Vec<_> = ix
            .accounts
            .iter()
            .zip(program_accounts.iter())
            .map(|(account_meta, account)| {
                KeyedAccount::new(&account_meta.pubkey, account_meta.is_signer, account)
            })
            .collect();

        let ret = process_instruction(&id(), &mut keyed_accounts, &ix.data);
        info!("ret: {:?}", ret);
        ret
    }

    #[test]
    fn test_proof_bounds() {
        let account_owner = Pubkey::new_rand();
        let pubkey = Pubkey::new_rand();
        let mut account = Account {
            data: vec![0; STORAGE_ACCOUNT_SPACE as usize],
            ..Account::default()
        };
        {
            let mut storage_account = StorageAccount::new(pubkey, &mut account);
            storage_account
                .initialize_storage(account_owner, StorageAccountType::Archiver)
                .unwrap();
        }

        let ix = storage_instruction::mining_proof(
            &pubkey,
            Hash::default(),
            0,
            Signature::default(),
            Hash::default(),
        );
        // the proof is for segment 0, need to move the slot into segment 2
        let mut clock_account = Clock::default().create_account(1);
        Clock::to_account(
            &Clock {
                slot: DEFAULT_SLOTS_PER_SEGMENT * 2,
                segment: 2,
                ..Clock::default()
            },
            &mut clock_account,
        );

        assert_eq!(test_instruction(&ix, &mut [account, clock_account]), Ok(()));
    }

    #[test]
    fn test_storage_tx() {
        let pubkey = Pubkey::new_rand();
        let mut accounts = [(&pubkey, &RefCell::new(Account::default()))];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        assert!(process_instruction(&id(), &mut keyed_accounts, &[]).is_err());
    }

    #[test]
    fn test_serialize_overflow() {
        let pubkey = Pubkey::new_rand();
        let clock_id = clock::id();
        let mut keyed_accounts = Vec::new();
        let mut user_account = RefCell::new(Account::default());
        let mut clock_account = RefCell::new(Clock::default().create_account(1));
        keyed_accounts.push(KeyedAccount::new(&pubkey, true, &mut user_account));
        keyed_accounts.push(KeyedAccount::new(&clock_id, false, &mut clock_account));

        let ix = storage_instruction::advertise_recent_blockhash(&pubkey, Hash::default(), 1);

        assert_eq!(
            process_instruction(&id(), &mut keyed_accounts, &ix.data),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_invalid_accounts_len() {
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default()];

        let ix = storage_instruction::mining_proof(
            &pubkey,
            Hash::default(),
            0,
            Signature::default(),
            Hash::default(),
        );
        // move tick height into segment 1
        let mut clock_account = Clock::default().create_account(1);
        Clock::to_account(
            &Clock {
                slot: 16,
                segment: 1,
                ..Clock::default()
            },
            &mut clock_account,
        );

        assert!(test_instruction(&ix, &mut accounts).is_err());

        let mut accounts = [Account::default(), clock_account, Account::default()];

        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_invalid_slot() {
        solana_logger::setup();
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default(), Account::default()];
        accounts[0].data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);
        accounts[1].data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);

        let ix = storage_instruction::mining_proof(
            &pubkey,
            Hash::default(),
            0,
            Signature::default(),
            Hash::default(),
        );

        // submitting a proof for a slot in the past, so this should fail
        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_ok() {
        solana_logger::setup();
        let account_owner = Pubkey::new_rand();
        let pubkey = Pubkey::new_rand();
        let mut account = Account::default();
        account.data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);
        {
            let mut storage_account = StorageAccount::new(pubkey, &mut account);
            storage_account
                .initialize_storage(account_owner, StorageAccountType::Archiver)
                .unwrap();
        }

        let ix = storage_instruction::mining_proof(
            &pubkey,
            Hash::default(),
            0,
            Signature::default(),
            Hash::default(),
        );
        // move slot into segment 1
        let mut clock_account = Clock::default().create_account(1);
        Clock::to_account(
            &Clock {
                slot: DEFAULT_SLOTS_PER_SEGMENT,
                segment: 1,
                ..Clock::default()
            },
            &mut clock_account,
        );

        assert_matches!(test_instruction(&ix, &mut [account, clock_account]), Ok(_));
    }
}
