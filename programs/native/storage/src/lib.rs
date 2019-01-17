//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.

use log::*;
extern crate solana_sdk;

use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::storage_program::*;

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

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    if keyed_accounts.len() != 2 {
        // keyed_accounts[1] should be the main storage key
        // to access its userdata
        Err(ProgramError::InvalidArgument)?;
    }

    // accounts_keys[0] must be signed
    if keyed_accounts[0].signer_key().is_none() {
        info!("account[0] is unsigned");
        Err(ProgramError::GenericError)?;
    }

    if *keyed_accounts[1].unsigned_key() != system_id() {
        info!(
            "invalid account id owner: {:?} system_id: {:?}",
            keyed_accounts[1].unsigned_key(),
            system_id()
        );
        Err(ProgramError::InvalidArgument)?;
    }

    if let Ok(syscall) = bincode::deserialize(data) {
        let mut storage_account_state = if let Ok(storage_account_state) =
            bincode::deserialize(&keyed_accounts[1].account.userdata)
        {
            storage_account_state
        } else {
            StorageProgramState::default()
        };

        debug!(
            "deserialized state height: {}",
            storage_account_state.entry_height
        );
        match syscall {
            StorageProgram::SubmitMiningProof {
                sha_state,
                entry_height,
                signature,
            } => {
                let segment_index = get_segment_from_entry(entry_height);
                let current_segment_index =
                    get_segment_from_entry(storage_account_state.entry_height);
                if segment_index >= current_segment_index {
                    return Err(ProgramError::InvalidArgument);
                }

                debug!(
                    "Mining proof submitted with state {:?} entry_height: {}",
                    sha_state, entry_height
                );

                let proof_info = ProofInfo {
                    id: *keyed_accounts[0].signer_key().unwrap(),
                    sha_state,
                    signature,
                };
                storage_account_state.proofs[segment_index].push(proof_info);
            }
            StorageProgram::AdvertiseStorageLastId { id, entry_height } => {
                let original_segments = storage_account_state.entry_height / ENTRIES_PER_SEGMENT;
                let segments = entry_height / ENTRIES_PER_SEGMENT;
                debug!(
                    "advertise new last id segments: {} orig: {}",
                    segments, original_segments
                );
                if segments <= original_segments {
                    return Err(ProgramError::InvalidArgument);
                }

                storage_account_state.entry_height = entry_height;
                storage_account_state.id = id;

                // move the proofs to previous_proofs
                storage_account_state.previous_proofs = storage_account_state.proofs.clone();
                storage_account_state.proofs.clear();
                storage_account_state
                    .proofs
                    .resize(segments as usize, Vec::new());

                // move lockout_validations to reward_validations
                storage_account_state.reward_validations =
                    storage_account_state.lockout_validations.clone();
                storage_account_state.lockout_validations.clear();
                storage_account_state
                    .lockout_validations
                    .resize(segments as usize, Vec::new());
            }
            StorageProgram::ProofValidation {
                entry_height,
                proof_mask,
            } => {
                if entry_height >= storage_account_state.entry_height {
                    return Err(ProgramError::InvalidArgument);
                }

                let segment_index = get_segment_from_entry(entry_height);
                if storage_account_state.previous_proofs[segment_index].len() != proof_mask.len() {
                    return Err(ProgramError::InvalidArgument);
                }

                // TODO: Check that each proof mask matches the signature
                /*for (i, entry) in proof_mask.iter().enumerate() {
                    if storage_account_state.previous_proofs[segment_index][i] != signature.as_ref[0] {
                        return Err(ProgramError::InvalidArgument);
                    }
                }*/

                let info = ValidationInfo {
                    id: *keyed_accounts[0].signer_key().unwrap(),
                    proof_mask,
                };
                storage_account_state.lockout_validations[segment_index].push(info);
            }
            StorageProgram::ClaimStorageReward { entry_height } => {
                let claims_index = get_segment_from_entry(entry_height);
                let account_key = keyed_accounts[0].signer_key().unwrap();
                let mut num_validations = 0;
                let mut total_validations = 0;
                for validation in &storage_account_state.reward_validations[claims_index] {
                    if *account_key == validation.id {
                        num_validations += count_valid_proofs(&validation.proof_mask);
                    } else {
                        total_validations += count_valid_proofs(&validation.proof_mask);
                    }
                }
                total_validations += num_validations;
                if total_validations > 0 {
                    keyed_accounts[0].account.tokens +=
                        (TOTAL_VALIDATOR_REWARDS * num_validations) / total_validations;
                }
            }
        }

        if bincode::serialize_into(
            &mut keyed_accounts[1].account.userdata[..],
            &storage_account_state,
        )
        .is_err()
        {
            return Err(ProgramError::UserdataTooSmall);
        }

        Ok(())
    } else {
        info!("Invalid instruction userdata: {:?}", data);
        Err(ProgramError::InvalidUserdata)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::account::{create_keyed_accounts, Account};
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::storage_program;
    use solana_sdk::storage_program::ProofStatus;
    use solana_sdk::storage_program::StorageTransaction;
    use solana_sdk::transaction::{Instruction, Transaction};

    fn test_transaction(
        tx: &Transaction,
        program_accounts: &mut [Account],
    ) -> Result<(), ProgramError> {
        assert_eq!(tx.instructions.len(), 1);
        let Instruction {
            ref accounts,
            ref userdata,
            ..
        } = tx.instructions[0];

        info!("accounts: {:?}", accounts);

        let mut keyed_accounts: Vec<_> = accounts
            .iter()
            .map(|&index| {
                let index = index as usize;
                let key = &tx.account_keys[index];
                (key, index < tx.signatures.len())
            })
            .zip(program_accounts.iter_mut())
            .map(|((key, is_signer), account)| KeyedAccount::new(key, is_signer, account))
            .collect();

        let ret = entrypoint(&id(), &mut keyed_accounts, &userdata, 42);
        info!("ret: {:?}", ret);
        ret
    }

    #[test]
    fn test_storage_tx() {
        let keypair = Keypair::new();
        let mut accounts = [(keypair.pubkey(), Default::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        assert!(entrypoint(&id(), &mut keyed_accounts, &[], 42).is_err());
    }

    #[test]
    fn test_serialize_overflow() {
        let keypair = Keypair::new();
        let mut keyed_accounts = Vec::new();
        let mut user_account = Account::default();
        let mut system_account = Account::default();
        let pubkey = keypair.pubkey();
        let system_key = storage_program::system_id();
        keyed_accounts.push(KeyedAccount::new(&pubkey, true, &mut user_account));
        keyed_accounts.push(KeyedAccount::new(&system_key, false, &mut system_account));

        let tx = Transaction::storage_new_advertise_last_id(
            &keypair,
            Hash::default(),
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        assert_eq!(
            entrypoint(&id(), &mut keyed_accounts, &tx.instructions[0].userdata, 42),
            Err(ProgramError::UserdataTooSmall)
        );
    }

    #[test]
    fn test_invalid_accounts_len() {
        let keypair = Keypair::new();
        let mut accounts = [Default::default()];

        let tx = Transaction::storage_new_mining_proof(
            &keypair,
            Hash::default(),
            Hash::default(),
            0,
            Signature::default(),
        );
        assert!(test_transaction(&tx, &mut accounts).is_err());

        let mut accounts = [Default::default(), Default::default(), Default::default()];

        assert!(test_transaction(&tx, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_invalid_entry_height() {
        solana_logger::setup();
        let keypair = Keypair::new();
        let mut accounts = [Account::default(), Account::default()];
        accounts[1].userdata.resize(16 * 1024, 0);

        let tx = Transaction::storage_new_mining_proof(
            &keypair,
            Hash::default(),
            Hash::default(),
            0,
            Signature::default(),
        );

        // Haven't seen a transaction to roll over the epoch, so this should fail
        assert!(test_transaction(&tx, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_ok() {
        solana_logger::setup();
        let keypair = Keypair::new();
        let mut accounts = [Account::default(), Account::default()];
        accounts[1].userdata.resize(16 * 1024, 0);

        let tx = Transaction::storage_new_advertise_last_id(
            &keypair,
            Hash::default(),
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        assert!(test_transaction(&tx, &mut accounts).is_ok());

        let tx = Transaction::storage_new_mining_proof(
            &keypair,
            Hash::default(),
            Hash::default(),
            0,
            Signature::default(),
        );

        assert!(test_transaction(&tx, &mut accounts).is_ok());
    }

    #[test]
    fn test_validate_mining() {
        solana_logger::setup();
        let keypair = Keypair::new();
        let mut accounts = [Account::default(), Account::default()];
        accounts[1].userdata.resize(16 * 1024, 0);

        let entry_height = 0;

        let tx = Transaction::storage_new_advertise_last_id(
            &keypair,
            Hash::default(),
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        assert!(test_transaction(&tx, &mut accounts).is_ok());

        let tx = Transaction::storage_new_mining_proof(
            &keypair,
            Hash::default(),
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        assert!(test_transaction(&tx, &mut accounts).is_ok());

        let tx = Transaction::storage_new_advertise_last_id(
            &keypair,
            Hash::default(),
            Hash::default(),
            ENTRIES_PER_SEGMENT * 2,
        );
        assert!(test_transaction(&tx, &mut accounts).is_ok());

        let tx = Transaction::storage_new_proof_validation(
            &keypair,
            Hash::default(),
            entry_height,
            vec![ProofStatus::Valid],
        );
        assert!(test_transaction(&tx, &mut accounts).is_ok());

        let tx = Transaction::storage_new_advertise_last_id(
            &keypair,
            Hash::default(),
            Hash::default(),
            ENTRIES_PER_SEGMENT * 3,
        );
        assert!(test_transaction(&tx, &mut accounts).is_ok());

        let tx = Transaction::storage_new_reward_claim(&keypair, Hash::default(), entry_height);
        assert!(test_transaction(&tx, &mut accounts).is_ok());

        assert!(accounts[0].tokens == TOTAL_VALIDATOR_REWARDS);
    }
}
