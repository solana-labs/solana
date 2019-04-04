use crate::{get_segment_from_entry, ENTRIES_PER_SEGMENT};
use bincode::{deserialize, serialize_into, ErrorKind};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::InstructionError;
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

pub trait StorageAccount {
    fn submit_mining_proof(
        &mut self,
        sha_state: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Result<(), InstructionError>;
    fn advertise_storage_recent_blockhash(
        &mut self,
        hash: Hash,
        entry_height: u64,
    ) -> Result<(), InstructionError>;
    fn proof_validation(
        &mut self,
        entry_height: u64,
        proofs: Vec<CheckedProof>,
        replicator_accounts: &mut [KeyedAccount],
    ) -> Result<(), InstructionError>;
    fn claim_storage_reward(
        &mut self,
        entry_height: u64,
        tick_height: u64,
    ) -> Result<(), InstructionError>;
}

pub trait State<T> {
    fn state(&self) -> Result<T, InstructionError>;
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError>;
}

impl<'a, T> State<T> for KeyedAccount<'a>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn state(&self) -> Result<T, InstructionError> {
        deserialize(&self.account.data).map_err(|_| InstructionError::InvalidAccountData)
    }
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError> {
        serialize_into(&mut self.account.data[..], state).map_err(|err| match *err {
            ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
            _ => InstructionError::GenericError,
        })
    }
}

impl<'a> StorageAccount for KeyedAccount<'a> {
    fn submit_mining_proof(
        &mut self,
        sha_state: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.state()?;
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
                id: *self.signer_key().unwrap(),
                sha_state,
                signature,
            };
            proofs[segment_index] = proof_info;
            self.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    fn advertise_storage_recent_blockhash(
        &mut self,
        hash: Hash,
        entry_height: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.state()?;
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
            self.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    fn proof_validation(
        &mut self,
        entry_height: u64,
        proofs: Vec<CheckedProof>,
        replicator_accounts: &mut [KeyedAccount],
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.state()?;
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
                    account.state().ok().map(move |contract| match contract {
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
            self.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    fn claim_storage_reward(
        &mut self,
        entry_height: u64,
        tick_height: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.state()?;
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
            self.set_state(storage_contract)
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
            self.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }
}

/// Store the result of a proof validation into the replicator account
fn store_validation_result(
    keyed_account: &mut KeyedAccount,
    segment_index: usize,
    status: ProofStatus,
) -> Result<(), InstructionError> {
    let mut storage_contract = keyed_account.state()?;
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
    keyed_account.set_state(&storage_contract)
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
