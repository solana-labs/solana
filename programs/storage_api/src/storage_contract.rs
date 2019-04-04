use crate::{get_segment_from_entry, ENTRIES_PER_SEGMENT};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::State;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::cmp;

pub const TOTAL_VALIDATOR_REWARDS: u64 = 1;
pub const TOTAL_REPLICATOR_REWARDS: u64 = 1;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ProofStatus {
    Skipped,
    Valid,
    NotValid,
}

impl Default for ProofStatus {
    fn default() -> Self {
        ProofStatus::Skipped
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct Proof {
    pub id: Pubkey,
    pub signature: Signature,
    pub sha_state: Hash,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct CheckedProof {
    pub proof: Proof,
    pub status: ProofStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StorageContract {
    //don't move this
    Default,

    ValidatorStorage {
        entry_height: u64,
        hash: Hash,
        lockout_validations: Vec<Vec<CheckedProof>>,
        reward_validations: Vec<Vec<CheckedProof>>,
    },
    ReplicatorStorage {
        proofs: Vec<Proof>,
        reward_validations: Vec<Vec<CheckedProof>>,
    },
}

pub struct StorageAccount<'a> {
    account: &'a mut Account,
}

impl<'a> StorageAccount<'a> {
    pub fn new(account: &'a mut Account) -> Self {
        Self { account }
    }

    pub fn submit_mining_proof(
        &mut self,
        id: Pubkey,
        sha_state: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ReplicatorStorage {
                proofs: vec![],
                reward_validations: vec![],
            };
        };

        if let StorageContract::ReplicatorStorage { proofs, .. } = &mut storage_contract {
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
                id,
                sha_state,
                signature,
            };
            proofs[segment_index] = proof_info;
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn advertise_storage_recent_blockhash(
        &mut self,
        hash: Hash,
        entry_height: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ValidatorStorage {
                entry_height: 0,
                hash: Hash::default(),
                lockout_validations: vec![],
                reward_validations: vec![],
            };
        };

        if let StorageContract::ValidatorStorage {
            entry_height: state_entry_height,
            hash: state_hash,
            reward_validations,
            lockout_validations,
        } = &mut storage_contract
        {
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
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn proof_validation(
        &mut self,
        entry_height: u64,
        proofs: Vec<CheckedProof>,
        replicator_accounts: &mut [StorageAccount],
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ValidatorStorage {
                entry_height: 0,
                hash: Hash::default(),
                lockout_validations: vec![],
                reward_validations: vec![],
            };
        };

        if let StorageContract::ValidatorStorage {
            entry_height: current_entry_height,
            lockout_validations,
            ..
        } = &mut storage_contract
        {
            if entry_height >= *current_entry_height {
                return Err(InstructionError::InvalidArgument);
            }

            let segment_index = get_segment_from_entry(entry_height);
            let mut previous_proofs = replicator_accounts
                .iter_mut()
                .filter_map(|account| {
                    account
                        .account
                        .state()
                        .ok()
                        .map(move |contract| match contract {
                            StorageContract::ReplicatorStorage { proofs, .. } => {
                                Some((account, proofs[segment_index].clone()))
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
                    let (account, proof) = &mut previous_proofs[i];
                    if process_validation(account, segment_index, &proof, &entry).is_ok() {
                        Some(entry)
                    } else {
                        None
                    }
                })
                .collect();

            // allow validators to store successful validations
            lockout_validations[segment_index].append(&mut valid_proofs);
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn claim_storage_reward(
        &mut self,
        entry_height: u64,
        tick_height: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            Err(InstructionError::InvalidArgument)?
        };

        if let StorageContract::ValidatorStorage {
            reward_validations, ..
        } = &mut storage_contract
        {
            let claims_index = get_segment_from_entry(entry_height);
            let _num_validations = count_valid_proofs(&reward_validations[claims_index]);
            // TODO can't just create lamports out of thin air
            // self.account.lamports += TOTAL_VALIDATOR_REWARDS * num_validations;
            reward_validations.clear();
            self.account.set_state(storage_contract)
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
            // self.account.lamports += num_validations
            //     * TOTAL_REPLICATOR_REWARDS
            //     * (num_validations / reward_validations[claims_index].len() as u64);
            reward_validations.clear();
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }
}

/// Store the result of a proof validation into the replicator account
fn store_validation_result(
    storage_account: &mut StorageAccount,
    segment_index: usize,
    status: ProofStatus,
) -> Result<(), InstructionError> {
    let mut storage_contract = storage_account.account.state()?;
    match &mut storage_contract {
        StorageContract::ReplicatorStorage {
            proofs,
            reward_validations,
            ..
        } => {
            if segment_index >= proofs.len() {
                return Err(InstructionError::InvalidAccountData);
            }
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
    storage_account.account.set_state(&storage_contract)
}

fn count_valid_proofs(proofs: &[CheckedProof]) -> u64 {
    let mut num = 0;
    for proof in proofs {
        if let ProofStatus::Valid = proof.status {
            num += 1;
        }
    }
    num
}

fn process_validation(
    account: &mut StorageAccount,
    segment_index: usize,
    proof: &Proof,
    checked_proof: &CheckedProof,
) -> Result<(), InstructionError> {
    store_validation_result(account, segment_index, checked_proof.status.clone())?;
    if proof.signature != checked_proof.proof.signature
        || checked_proof.status != ProofStatus::Valid
    {
        return Err(InstructionError::GenericError);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;

    #[test]
    fn test_account_data() {
        solana_logger::setup();
        let mut account = Account::default();
        account.data.resize(4 * 1024, 0);
        let storage_account = StorageAccount::new(&mut account);
        // pretend it's a validator op code
        let mut contract = storage_account.account.state().unwrap();
        if let StorageContract::ValidatorStorage { .. } = contract {
            assert!(true)
        }
        if let StorageContract::ReplicatorStorage { .. } = &mut contract {
            panic!("Contract should not decode into two types");
        }

        contract = StorageContract::ValidatorStorage {
            entry_height: 0,
            hash: Hash::default(),
            lockout_validations: vec![],
            reward_validations: vec![],
        };
        storage_account.account.set_state(&contract).unwrap();
        if let StorageContract::ReplicatorStorage { .. } = contract {
            panic!("Wrong contract type");
        }
        contract = StorageContract::ReplicatorStorage {
            proofs: vec![],
            reward_validations: vec![],
        };
        storage_account.account.set_state(&contract).unwrap();
        if let StorageContract::ValidatorStorage { .. } = contract {
            panic!("Wrong contract type");
        }
    }

    #[test]
    fn test_process_validation() {
        let mut account = StorageAccount {
            account: &mut Account {
                lamports: 0,
                data: vec![],
                owner: id(),
                executable: false,
            },
        };
        let segment_index = 0_usize;
        let proof = Proof {
            id: Pubkey::default(),
            signature: Signature::default(),
            sha_state: Hash::default(),
        };
        let mut checked_proof = CheckedProof {
            proof: proof.clone(),
            status: ProofStatus::Valid,
        };

        // account has no space
        process_validation(&mut account, segment_index, &proof, &checked_proof).unwrap_err();

        account.account.data.resize(4 * 1024, 0);
        let storage_contract = &mut account.account.state().unwrap();
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ReplicatorStorage {
                proofs: vec![proof.clone()],
                reward_validations: vec![],
            };
        };
        account.account.set_state(storage_contract).unwrap();

        // proof is valid
        process_validation(&mut account, segment_index, &proof, &checked_proof).unwrap();

        checked_proof.status = ProofStatus::NotValid;

        // proof failed verification
        process_validation(&mut account, segment_index, &proof, &checked_proof).unwrap_err();
    }
}
