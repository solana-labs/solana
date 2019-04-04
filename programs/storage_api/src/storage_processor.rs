//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.

use crate::storage_contract::{CheckedProof, Proof, ProofStatus, StorageContract};
use crate::storage_instruction::StorageInstruction;
use crate::{get_segment_from_entry, ENTRIES_PER_SEGMENT};
use log::*;
use serde::Serialize;
use solana_sdk::account::{Account, KeyedAccount};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction::InstructionError::InvalidArgument;
use solana_sdk::pubkey::Pubkey;
use std::cmp;

pub const TOTAL_VALIDATOR_REWARDS: u64 = 1;
pub const TOTAL_REPLICATOR_REWARDS: u64 = 1;

fn count_valid_proofs(proofs: &[CheckedProof]) -> u64 {
    let mut num = 0;
    for proof in proofs {
        if let ProofStatus::Valid = proof.status {
            num += 1;
        }
    }
    num
}

/// Serialize account data
fn store_contract<T>(account: &mut Account, contract: &T) -> Result<(), InstructionError>
where
    T: Serialize,
{
    if bincode::serialize_into(&mut account.data[..], contract).is_err() {
        return Err(InstructionError::AccountDataTooSmall);
    }
    Ok(())
}

/// Deserialize account data
fn read_contract(account: &Account) -> Result<StorageContract, InstructionError> {
    if let Ok(storage_contract) = bincode::deserialize(&account.data) {
        Ok(storage_contract)
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

/// Deserialize account data but handle uninitialized accounts
fn read_contract_with_default(
    op: &StorageInstruction,
    account: &Account,
) -> Result<StorageContract, InstructionError> {
    let mut storage_contract = read_contract(&account);
    if let Ok(StorageContract::Default) = storage_contract {
        match op {
            StorageInstruction::SubmitMiningProof { .. } => {
                storage_contract = Ok(StorageContract::ReplicatorStorage {
                    proofs: vec![],
                    reward_validations: vec![],
                })
            }
            StorageInstruction::AdvertiseStorageRecentBlockhash { .. }
            | StorageInstruction::ProofValidation { .. } => {
                storage_contract = Ok(StorageContract::ValidatorStorage {
                    entry_height: 0,
                    hash: Hash::default(),
                    lockout_validations: vec![],
                    reward_validations: vec![],
                })
            }
            StorageInstruction::ClaimStorageReward { .. } => Err(InvalidArgument)?,
        }
    }
    storage_contract
}

/// Store the result of a proof validation into the replicator account
fn store_validation_result(
    account: &mut Account,
    segment_index: usize,
    status: ProofStatus,
) -> Result<(), InstructionError> {
    let mut storage_contract = read_contract(account)?;
    match &mut storage_contract {
        StorageContract::ReplicatorStorage {
            proofs,
            reward_validations,
            ..
        } => {
            if segment_index > reward_validations.len() || reward_validations.is_empty() {
                reward_validations.resize(cmp::max(1, segment_index), vec![]);
            }
            let result = proofs[segment_index].clone();
            reward_validations[segment_index].push(CheckedProof {
                proof: result,
                status,
            });
        }
        _ => return Err(InstructionError::InvalidAccountData),
    }
    store_contract(account, &storage_contract)?;
    Ok(())
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    // accounts_keys[0] must be signed
    if keyed_accounts[0].signer_key().is_none() {
        info!("account[0] is unsigned");
        Err(InstructionError::GenericError)?;
    }

    if let Ok(syscall) = bincode::deserialize(data) {
        let mut storage_contract =
            read_contract_with_default(&syscall, &keyed_accounts[0].account)?;
        match syscall {
            StorageInstruction::SubmitMiningProof {
                sha_state,
                entry_height,
                signature,
            } => {
                if let StorageContract::ReplicatorStorage { proofs, .. } = &mut storage_contract {
                    if keyed_accounts.len() != 1 {
                        // keyed_accounts[1] should be the main storage key
                        // to access its data
                        Err(InstructionError::InvalidArgument)?;
                    }

                    let segment_index = get_segment_from_entry(entry_height);
                    if segment_index > proofs.len() || proofs.is_empty() {
                        proofs.resize(cmp::max(1, segment_index), Proof::default());
                    }

                    if segment_index > proofs.len() {
                        // only possible if usize max < u64 max
                        return Err(InstructionError::InvalidArgument);
                    }

                    debug!(
                        "Mining proof submitted with contract {:?} entry_height: {}",
                        sha_state, entry_height
                    );

                    let proof_info = Proof {
                        id: *keyed_accounts[0].signer_key().unwrap(),
                        sha_state,
                        signature,
                    };
                    proofs[segment_index] = proof_info;
                } else {
                    Err(InstructionError::InvalidArgument)?;
                }
            }
            StorageInstruction::AdvertiseStorageRecentBlockhash { hash, entry_height } => {
                if let StorageContract::ValidatorStorage {
                    entry_height: state_entry_height,
                    hash: state_hash,
                    reward_validations,
                    lockout_validations,
                } = &mut storage_contract
                {
                    if keyed_accounts.len() != 1 {
                        // keyed_accounts[1] should be the main storage key
                        // to access its data
                        Err(InstructionError::InvalidArgument)?;
                    }

                    let original_segments = *state_entry_height / ENTRIES_PER_SEGMENT;
                    let segments = entry_height / ENTRIES_PER_SEGMENT;
                    debug!(
                        "advertise new last id segments: {} orig: {}",
                        segments, original_segments
                    );
                    if segments <= original_segments {
                        return Err(InstructionError::InvalidArgument);
                    }

                    *state_entry_height = entry_height;
                    *state_hash = hash;

                    // move lockout_validations to reward_validations
                    *reward_validations = lockout_validations.clone();
                    lockout_validations.clear();
                    lockout_validations.resize(segments as usize, Vec::new());
                } else {
                    return Err(InstructionError::InvalidArgument);
                }
            }
            StorageInstruction::ClaimStorageReward { entry_height } => {
                if keyed_accounts.len() != 1 {
                    // keyed_accounts[1] should be the main storage key
                    // to access its data
                    Err(InstructionError::InvalidArgument)?;
                }

                if let StorageContract::ValidatorStorage {
                    reward_validations, ..
                } = &mut storage_contract
                {
                    let claims_index = get_segment_from_entry(entry_height);
                    let _num_validations = count_valid_proofs(&reward_validations[claims_index]);
                    // TODO can't just create lamports out of thin air
                    // keyed_accounts[0].account.lamports += TOTAL_VALIDATOR_REWARDS * num_validations;
                    reward_validations.clear();
                } else if let StorageContract::ReplicatorStorage {
                    reward_validations, ..
                } = &mut storage_contract
                {
                    // if current tick height is a full segment away? then allow reward collection
                    // storage needs to move to tick heights too, until then this makes little sense
                    let current_index = get_segment_from_entry(tick_height);
                    let claims_index = get_segment_from_entry(entry_height);
                    if current_index <= claims_index || claims_index >= reward_validations.len() {
                        debug!(
                            "current {:?}, claim {:?}, rewards {:?}",
                            current_index,
                            claims_index,
                            reward_validations.len()
                        );
                        return Err(InstructionError::InvalidArgument);
                    }
                    let _num_validations = count_valid_proofs(&reward_validations[claims_index]);
                    // TODO can't just create lamports out of thin air
                    // keyed_accounts[0].account.lamports += num_validations
                    //     * TOTAL_REPLICATOR_REWARDS
                    //     * (num_validations / reward_validations[claims_index].len() as u64);
                    reward_validations.clear();
                } else {
                    return Err(InstructionError::InvalidArgument);
                }
            }
            StorageInstruction::ProofValidation {
                entry_height: proof_entry_height,
                proofs,
            } => {
                if let StorageContract::ValidatorStorage {
                    entry_height: current_entry_height,
                    lockout_validations,
                    ..
                } = &mut storage_contract
                {
                    if keyed_accounts.len() == 1 {
                        // have to have at least 1 replicator to do any verification
                        Err(InstructionError::InvalidArgument)?;
                    }

                    if proof_entry_height >= *current_entry_height {
                        return Err(InstructionError::InvalidArgument);
                    }

                    let segment_index = get_segment_from_entry(proof_entry_height);
                    let mut previous_proofs = keyed_accounts[1..]
                        .iter_mut()
                        .filter_map(|account| {
                            read_contract(&account.account)
                                .ok()
                                .map(move |contract| match contract {
                                    StorageContract::ReplicatorStorage { proofs, .. } => {
                                        Some((&mut account.account, proofs[segment_index].clone()))
                                    }
                                    _ => None,
                                })
                        })
                        .flatten()
                        .collect::<Vec<_>>();

                    if previous_proofs.len() != proofs.len() {
                        // don't have all the accounts to validate the proofs against
                        return Err(InstructionError::InvalidArgument);
                    }

                    let mut valid_proofs: Vec<_> = proofs
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, entry)| {
                            if previous_proofs[i].1.signature != entry.proof.signature
                                || entry.status != ProofStatus::Valid
                            {
                                let _ = store_validation_result(
                                    &mut previous_proofs[i].0,
                                    segment_index,
                                    entry.status,
                                );
                                None
                            } else if store_validation_result(
                                &mut previous_proofs[i].0,
                                segment_index,
                                entry.status.clone(),
                            )
                            .is_ok()
                            {
                                Some(entry)
                            } else {
                                None
                            }
                        })
                        .collect();

                    // allow validators to store successful validations
                    lockout_validations[segment_index].append(&mut valid_proofs);
                } else {
                    return Err(InstructionError::InvalidArgument);
                }
            }
        }
        store_contract(&mut keyed_accounts[0].account, &storage_contract)?;
        Ok(())
    } else {
        info!("Invalid instruction data: {:?}", data);
        Err(InstructionError::InvalidInstructionData)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::storage_instruction;
    use crate::ENTRIES_PER_SEGMENT;
    use bincode::deserialize;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::account::{create_keyed_accounts, Account};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::{hash, Hash};
    use solana_sdk::instruction::Instruction;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::sync_client::SyncClient;
    use solana_sdk::system_instruction;

    fn test_instruction(
        ix: &Instruction,
        program_accounts: &mut [Account],
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts: Vec<_> = ix
            .accounts
            .iter()
            .zip(program_accounts.iter_mut())
            .map(|(account_meta, account)| {
                KeyedAccount::new(&account_meta.pubkey, account_meta.is_signer, account)
            })
            .collect();

        let ret = process_instruction(&id(), &mut keyed_accounts, &ix.data, 42);
        info!("ret: {:?}", ret);
        ret
    }

    #[test]
    fn test_storage_tx() {
        let pubkey = Pubkey::new_rand();
        let mut accounts = [(pubkey, Account::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        assert!(process_instruction(&id(), &mut keyed_accounts, &[], 42).is_err());
    }

    #[test]
    fn test_serialize_overflow() {
        let pubkey = Pubkey::new_rand();
        let mut keyed_accounts = Vec::new();
        let mut user_account = Account::default();
        keyed_accounts.push(KeyedAccount::new(&pubkey, true, &mut user_account));

        let ix = storage_instruction::advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        assert_eq!(
            process_instruction(&id(), &mut keyed_accounts, &ix.data, 42),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_invalid_accounts_len() {
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default()];

        let ix =
            storage_instruction::mining_proof(&pubkey, Hash::default(), 0, Signature::default());
        assert!(test_instruction(&ix, &mut accounts).is_err());

        let mut accounts = [Account::default(), Account::default(), Account::default()];

        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_invalid_entry_height() {
        solana_logger::setup();
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default(), Account::default()];
        accounts[1].data.resize(16 * 1024, 0);

        let ix =
            storage_instruction::mining_proof(&pubkey, Hash::default(), 0, Signature::default());

        // Haven't seen a transaction to roll over the epoch, so this should fail
        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_ok() {
        solana_logger::setup();
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default(), Account::default()];
        accounts[0].data.resize(16 * 1024, 0);

        let ix =
            storage_instruction::mining_proof(&pubkey, Hash::default(), 0, Signature::default());

        test_instruction(&ix, &mut accounts).unwrap();
    }

    #[test]
    fn test_account_data() {
        solana_logger::setup();
        let mut account = Account::default();
        account.data.resize(4 * 1024, 0);
        let pubkey = &Pubkey::default();
        let mut keyed_account = KeyedAccount::new(&pubkey, false, &mut account);
        // pretend it's a validator op code
        let mut contract = read_contract(&keyed_account.account).unwrap();
        if let StorageContract::ValidatorStorage { .. } = contract {
            assert!(true)
        }
        if let StorageContract::ReplicatorStorage { .. } = &mut contract {
            panic!("this shouldn't work");
        }

        contract = StorageContract::ValidatorStorage {
            entry_height: 0,
            hash: Hash::default(),
            lockout_validations: vec![],
            reward_validations: vec![],
        };
        store_contract(&mut keyed_account.account, &contract).unwrap();
        if let StorageContract::ReplicatorStorage { .. } = contract {
            panic!("this shouldn't work");
        }
        contract = StorageContract::ReplicatorStorage {
            proofs: vec![],
            reward_validations: vec![],
        };
        store_contract(&mut keyed_account.account, &contract).unwrap();
        if let StorageContract::ValidatorStorage { .. } = contract {
            panic!("this shouldn't work");
        }
    }

    #[test]
    fn test_validate_mining() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let mint_pubkey = mint_keypair.pubkey();
        let replicator_keypair = Keypair::new();
        let replicator = replicator_keypair.pubkey();
        let validator_keypair = Keypair::new();
        let validator = validator_keypair.pubkey();

        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        let entry_height = 0;
        let bank_client = BankClient::new(&bank);

        let ix = system_instruction::create_account(&mint_pubkey, &validator, 10, 4 * 1042, &id());
        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix = system_instruction::create_account(&mint_pubkey, &replicator, 10, 4 * 1042, &id());
        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::mining_proof(
            &replicator,
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        bank_client
            .send_instruction(&replicator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator,
            Hash::default(),
            ENTRIES_PER_SEGMENT * 2,
        );
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::proof_validation(
            &validator,
            entry_height,
            vec![CheckedProof {
                proof: Proof {
                    id: replicator,
                    signature: Signature::default(),
                    sha_state: Hash::default(),
                },
                status: ProofStatus::Valid,
            }],
        );
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator,
            Hash::default(),
            ENTRIES_PER_SEGMENT * 3,
        );
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::reward_claim(&validator, entry_height);
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        // TODO enable when rewards are working
        // assert_eq!(bank.get_balance(&validator), TOTAL_VALIDATOR_REWARDS);

        // tick the bank into the next storage epoch so that rewards can be claimed
        for _ in 0..=ENTRIES_PER_SEGMENT {
            bank.register_tick(&bank.last_blockhash());
        }

        let ix = storage_instruction::reward_claim(&replicator, entry_height);
        bank_client
            .send_instruction(&replicator_keypair, ix)
            .unwrap();

        // TODO enable when rewards are working
        // assert_eq!(bank.get_balance(&replicator), TOTAL_REPLICATOR_REWARDS);
    }

    fn get_storage_entry_height<C: SyncClient>(client: &C, account: &Pubkey) -> u64 {
        match client.get_account_data(&account).unwrap() {
            Some(storage_system_account_data) => {
                let contract = deserialize(&storage_system_account_data);
                if let Ok(contract) = contract {
                    match contract {
                        StorageContract::ValidatorStorage { entry_height, .. } => {
                            return entry_height;
                        }
                        _ => info!("error in reading entry_height"),
                    }
                }
            }
            None => {
                info!("error in reading entry_height");
            }
        }
        0
    }

    fn get_storage_blockhash<C: SyncClient>(client: &C, account: &Pubkey) -> Hash {
        if let Some(storage_system_account_data) = client.get_account_data(&account).unwrap() {
            let contract = deserialize(&storage_system_account_data);
            if let Ok(contract) = contract {
                match contract {
                    StorageContract::ValidatorStorage { hash, .. } => {
                        return hash;
                    }
                    _ => (),
                }
            }
        }
        Hash::default()
    }

    #[test]
    fn test_bank_storage() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let mint_pubkey = mint_keypair.pubkey();
        let replicator_keypair = Keypair::new();
        let replicator_pubkey = replicator_keypair.pubkey();
        let validator_keypair = Keypair::new();
        let validator_pubkey = validator_keypair.pubkey();

        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        let bank_client = BankClient::new(&bank);

        let x = 42;
        let blockhash = genesis_block.hash();
        let x2 = x * 2;
        let storage_blockhash = hash(&[x2]);

        bank.register_tick(&blockhash);

        bank_client
            .transfer(10, &mint_keypair, &replicator_pubkey)
            .unwrap();

        let ix = system_instruction::create_account(
            &mint_pubkey,
            &replicator_pubkey,
            1,
            4 * 1024,
            &id(),
        );

        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix =
            system_instruction::create_account(&mint_pubkey, &validator_pubkey, 1, 4 * 1024, &id());

        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator_pubkey,
            storage_blockhash,
            ENTRIES_PER_SEGMENT,
        );

        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let entry_height = 0;
        let ix = storage_instruction::mining_proof(
            &replicator_pubkey,
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        let _result = bank_client
            .send_instruction(&replicator_keypair, ix)
            .unwrap();

        assert_eq!(
            get_storage_entry_height(&bank_client, &validator_pubkey),
            ENTRIES_PER_SEGMENT
        );
        assert_eq!(
            get_storage_blockhash(&bank_client, &validator_pubkey),
            storage_blockhash
        );
    }

    /// check that uninitialized accounts are handled
    #[test]
    fn test_read_contract_with_default() {
        let mut account = Account::default();
        // no space allocated
        assert!(read_contract(&account).is_err());
        account.data.resize(4 * 1024, 0);
        let instruction = StorageInstruction::AdvertiseStorageRecentBlockhash {
            hash: Hash::default(),
            entry_height: 0,
        };
        read_contract_with_default(&instruction, &account).unwrap();
        let instruction = StorageInstruction::SubmitMiningProof {
            sha_state: Hash::default(),
            entry_height: 0,
            signature: Signature::default(),
        };
        read_contract_with_default(&instruction, &account).unwrap();
        let instruction = StorageInstruction::ProofValidation {
            entry_height: 0,
            proofs: vec![],
        };
        read_contract_with_default(&instruction, &account).unwrap();
        // Can't claim rewards on an uninitialized account
        let instruction = StorageInstruction::ClaimStorageReward { entry_height: 0 };
        assert!(read_contract_with_default(&instruction, &account).is_err());
    }
}
