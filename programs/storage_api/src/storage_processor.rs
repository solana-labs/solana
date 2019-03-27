//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.

use crate::storage_contract::{ProofInfo, ProofStatus, StorageContract, ValidationInfo};
use crate::storage_instruction::StorageInstruction;
use crate::{get_segment_from_entry, ENTRIES_PER_SEGMENT};
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub const TOTAL_VALIDATOR_REWARDS: u64 = 1000;
pub const TOTAL_REPLICATOR_REWARDS: u64 = 1000;

fn count_valid_proofs(proofs: &[ProofStatus]) -> u64 {
    let mut num = 0;
    for proof in proofs {
        if let ProofStatus::Valid = proof {
            num += 1;
        }
    }
    num
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    if keyed_accounts.len() != 1 {
        // keyed_accounts[1] should be the main storage key
        // to access its data
        Err(InstructionError::InvalidArgument)?;
    }

    // accounts_keys[0] must be signed
    if keyed_accounts[0].signer_key().is_none() {
        info!("account[0] is unsigned");
        Err(InstructionError::GenericError)?;
    }

    if let Ok(syscall) = bincode::deserialize(data) {
        let mut storage_contract =
            if let Ok(storage_contract) = bincode::deserialize(&keyed_accounts[0].account.data) {
                storage_contract
            } else {
                StorageContract::default()
            };

        debug!(
            "deserialized contract height: {}",
            storage_contract.entry_height
        );
        match syscall {
            StorageInstruction::SubmitMiningProof {
                sha_state,
                entry_height,
                signature,
            } => {
                let segment_index = get_segment_from_entry(entry_height);
                let current_segment_index = get_segment_from_entry(storage_contract.entry_height);
                if segment_index >= current_segment_index {
                    return Err(InstructionError::InvalidArgument);
                }

                debug!(
                    "Mining proof submitted with contract {:?} entry_height: {}",
                    sha_state, entry_height
                );

                let proof_info = ProofInfo {
                    id: *keyed_accounts[0].signer_key().unwrap(),
                    sha_state,
                    signature,
                };
                storage_contract.proofs[segment_index].push(proof_info);
            }
            StorageInstruction::AdvertiseStorageRecentBlockhash { hash, entry_height } => {
                let original_segments = storage_contract.entry_height / ENTRIES_PER_SEGMENT;
                let segments = entry_height / ENTRIES_PER_SEGMENT;
                debug!(
                    "advertise new last id segments: {} orig: {}",
                    segments, original_segments
                );
                if segments <= original_segments {
                    return Err(InstructionError::InvalidArgument);
                }

                storage_contract.entry_height = entry_height;
                storage_contract.hash = hash;

                // move the proofs to previous_proofs
                storage_contract.previous_proofs = storage_contract.proofs.clone();
                storage_contract.proofs.clear();
                storage_contract
                    .proofs
                    .resize(segments as usize, Vec::new());

                // move lockout_validations to reward_validations
                storage_contract.reward_validations = storage_contract.lockout_validations.clone();
                storage_contract.lockout_validations.clear();
                storage_contract
                    .lockout_validations
                    .resize(segments as usize, Vec::new());
            }
            StorageInstruction::ProofValidation {
                entry_height,
                proof_mask,
            } => {
                if entry_height >= storage_contract.entry_height {
                    return Err(InstructionError::InvalidArgument);
                }

                let segment_index = get_segment_from_entry(entry_height);
                if storage_contract.previous_proofs[segment_index].len() != proof_mask.len() {
                    return Err(InstructionError::InvalidArgument);
                }

                // TODO: Check that each proof mask matches the signature
                /*for (i, entry) in proof_mask.iter().enumerate() {
                    if storage_contract.previous_proofs[segment_index][i] != signature.as_ref[0] {
                        return Err(InstructionError::InvalidArgument);
                    }
                }*/

                let info = ValidationInfo {
                    id: *keyed_accounts[0].signer_key().unwrap(),
                    proof_mask,
                };
                storage_contract.lockout_validations[segment_index].push(info);
            }
            StorageInstruction::ClaimStorageReward { entry_height } => {
                let claims_index = get_segment_from_entry(entry_height);
                let account_key = keyed_accounts[0].signer_key().unwrap();
                let mut num_validations = 0;
                let mut total_validations = 0;
                for validation in &storage_contract.reward_validations[claims_index] {
                    if *account_key == validation.id {
                        num_validations += count_valid_proofs(&validation.proof_mask);
                    } else {
                        total_validations += count_valid_proofs(&validation.proof_mask);
                    }
                }
                total_validations += num_validations;
                if total_validations > 0 {
                    keyed_accounts[0].account.lamports +=
                        (TOTAL_VALIDATOR_REWARDS * num_validations) / total_validations;
                }
            }
        }

        if bincode::serialize_into(&mut keyed_accounts[0].account.data[..], &storage_contract)
            .is_err()
        {
            return Err(InstructionError::AccountDataTooSmall);
        }

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
    use crate::storage_contract::ProofStatus;
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
    use solana_sdk::system_instruction::SystemInstruction;

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
        let pubkey = Keypair::new().pubkey();
        let mut accounts = [(pubkey, Account::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        assert!(process_instruction(&id(), &mut keyed_accounts, &[], 42).is_err());
    }

    #[test]
    fn test_serialize_overflow() {
        let pubkey = Keypair::new().pubkey();
        let mut keyed_accounts = Vec::new();
        let mut user_account = Account::default();
        keyed_accounts.push(KeyedAccount::new(&pubkey, true, &mut user_account));

        let ix = StorageInstruction::new_advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        assert_eq!(
            process_instruction(&id(), &mut keyed_accounts, &ix.data, 42),
            Err(InstructionError::AccountDataTooSmall)
        );
    }

    #[test]
    fn test_invalid_accounts_len() {
        let pubkey = Keypair::new().pubkey();
        let mut accounts = [Account::default()];

        let ix =
            StorageInstruction::new_mining_proof(&pubkey, Hash::default(), 0, Signature::default());
        assert!(test_instruction(&ix, &mut accounts).is_err());

        let mut accounts = [Account::default(), Account::default(), Account::default()];

        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_invalid_entry_height() {
        solana_logger::setup();
        let pubkey = Keypair::new().pubkey();
        let mut accounts = [Account::default(), Account::default()];
        accounts[1].data.resize(16 * 1024, 0);

        let ix =
            StorageInstruction::new_mining_proof(&pubkey, Hash::default(), 0, Signature::default());

        // Haven't seen a transaction to roll over the epoch, so this should fail
        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_ok() {
        solana_logger::setup();
        let pubkey = Keypair::new().pubkey();
        let mut accounts = [Account::default(), Account::default()];
        accounts[0].data.resize(16 * 1024, 0);

        let ix = StorageInstruction::new_advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        test_instruction(&ix, &mut accounts).unwrap();

        let ix =
            StorageInstruction::new_mining_proof(&pubkey, Hash::default(), 0, Signature::default());

        test_instruction(&ix, &mut accounts).unwrap();
    }

    #[test]
    fn test_validate_mining() {
        solana_logger::setup();
        let pubkey = Keypair::new().pubkey();
        let mut accounts = [Account::default(), Account::default()];
        accounts[0].data.resize(16 * 1024, 0);

        let entry_height = 0;

        let ix = StorageInstruction::new_advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        test_instruction(&ix, &mut accounts).unwrap();

        let ix = StorageInstruction::new_mining_proof(
            &pubkey,
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        test_instruction(&ix, &mut accounts).unwrap();

        let ix = StorageInstruction::new_advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT * 2,
        );
        test_instruction(&ix, &mut accounts).unwrap();

        let ix = StorageInstruction::new_proof_validation(
            &pubkey,
            entry_height,
            vec![ProofStatus::Valid],
        );
        test_instruction(&ix, &mut accounts).unwrap();

        let ix = StorageInstruction::new_advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT * 3,
        );
        test_instruction(&ix, &mut accounts).unwrap();

        let ix = StorageInstruction::new_reward_claim(&pubkey, entry_height);
        test_instruction(&ix, &mut accounts).unwrap();

        assert!(accounts[0].lamports == TOTAL_VALIDATOR_REWARDS);
    }

    fn get_storage_entry_height(bank: &Bank, account: &Pubkey) -> u64 {
        match bank.get_account(&account) {
            Some(storage_system_account) => {
                let contract = deserialize(&storage_system_account.data);
                if let Ok(contract) = contract {
                    let contract: StorageContract = contract;
                    return contract.entry_height;
                }
            }
            None => {
                info!("error in reading entry_height");
            }
        }
        0
    }

    fn get_storage_blockhash(bank: &Bank, account: &Pubkey) -> Hash {
        if let Some(storage_system_account) = bank.get_account(&account) {
            let contract = deserialize(&storage_system_account.data);
            if let Ok(contract) = contract {
                let contract: StorageContract = contract;
                return contract.hash;
            }
        }
        Hash::default()
    }

    #[test]
    fn test_bank_storage() {
        let (genesis_block, alice_keypair) = GenesisBlock::new(1000);
        let alice_pubkey = alice_keypair.pubkey();
        let bob_keypair = Keypair::new();
        let bob_pubkey = bob_keypair.pubkey();
        let jack_pubkey = Keypair::new().pubkey();
        let jill_pubkey = Keypair::new().pubkey();

        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        let bank_client = BankClient::new(&bank);

        let x = 42;
        let blockhash = genesis_block.hash();
        let x2 = x * 2;
        let storage_blockhash = hash(&[x2]);

        bank.register_tick(&blockhash);

        bank_client
            .transfer(10, &alice_keypair, &jill_pubkey)
            .unwrap();
        bank_client
            .transfer(10, &alice_keypair, &bob_pubkey)
            .unwrap();
        bank_client
            .transfer(10, &alice_keypair, &jack_pubkey)
            .unwrap();

        let ix =
            SystemInstruction::new_program_account(&alice_pubkey, &bob_pubkey, 1, 4 * 1024, &id());

        bank_client.process_instruction(&alice_keypair, ix).unwrap();

        let ix = StorageInstruction::new_advertise_recent_blockhash(
            &bob_pubkey,
            storage_blockhash,
            ENTRIES_PER_SEGMENT,
        );

        bank_client.process_instruction(&bob_keypair, ix).unwrap();

        let entry_height = 0;
        let ix = StorageInstruction::new_mining_proof(
            &bob_pubkey,
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        let _result = bank_client.process_instruction(&bob_keypair, ix).unwrap();

        assert_eq!(
            get_storage_entry_height(&bank, &bob_pubkey),
            ENTRIES_PER_SEGMENT
        );
        assert_eq!(get_storage_blockhash(&bank, &bob_pubkey), storage_blockhash);
    }
}
