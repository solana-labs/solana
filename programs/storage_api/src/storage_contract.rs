use crate::get_segment_from_slot;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::State;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::cmp;
use std::collections::HashMap;

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

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
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
        slot: u64,
        hash: Hash,
        lockout_validations: HashMap<usize, HashMap<Hash, CheckedProof>>,
        reward_validations: HashMap<usize, HashMap<Hash, CheckedProof>>,
    },
    ReplicatorStorage {
        /// Proofs per segment, in a HashMap based on the sha_state
        proofs: Vec<HashMap<Hash, Proof>>,
        /// Rewards per segment, in a HashMap based on the sha_state
        /// Multiple validators can validate the same set of proofs so it needs a Vec
        reward_validations: Vec<HashMap<Hash, Vec<CheckedProof>>>,
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
        slot: u64,
        signature: Signature,
        current_slot: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ReplicatorStorage {
                proofs: vec![],
                reward_validations: vec![],
            };
        };

        if let StorageContract::ReplicatorStorage { proofs, .. } = &mut storage_contract {
            let segment_index = get_segment_from_slot(slot);
            let current_segment = get_segment_from_slot(current_slot);

            if segment_index >= current_segment {
                // attempt to submit proof for unconfirmed segment
                return Err(InstructionError::InvalidArgument);
            }

            if segment_index >= proofs.len() || proofs.is_empty() {
                proofs.resize(cmp::max(1, segment_index + 1), HashMap::new());
            }

            if segment_index >= proofs.len() {
                // only possible if usize max < u64 max
                return Err(InstructionError::InvalidArgument);
            }

            debug!(
                "Mining proof submitted with contract {:?} slot: {}",
                sha_state, slot
            );

            proofs[segment_index].insert(
                sha_state,
                Proof {
                    id,
                    sha_state,
                    signature,
                },
            );
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn advertise_storage_recent_blockhash(
        &mut self,
        hash: Hash,
        slot: u64,
        current_slot: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ValidatorStorage {
                slot: 0,
                hash: Hash::default(),
                lockout_validations: HashMap::new(),
                reward_validations: HashMap::new(),
            };
        };

        if let StorageContract::ValidatorStorage {
            slot: state_slot,
            hash: state_hash,
            reward_validations,
            lockout_validations,
        } = &mut storage_contract
        {
            let current_segment = get_segment_from_slot(current_slot);
            let original_segment = get_segment_from_slot(*state_slot);
            let segment = get_segment_from_slot(slot);
            debug!(
                "advertise new segment: {} orig: {}",
                segment, current_segment
            );
            if segment < original_segment || segment >= current_segment {
                return Err(InstructionError::InvalidArgument);
            }

            *state_slot = slot;
            *state_hash = hash;

            // move storage epoch updated, move the lockout_validations to reward_validations
            reward_validations.extend(lockout_validations.drain());
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn proof_validation(
        &mut self,
        slot: u64,
        proofs: Vec<CheckedProof>,
        replicator_accounts: &mut [StorageAccount],
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            *storage_contract = StorageContract::ValidatorStorage {
                slot: 0,
                hash: Hash::default(),
                lockout_validations: HashMap::new(),
                reward_validations: HashMap::new(),
            };
        };

        if let StorageContract::ValidatorStorage {
            slot: state_slot,
            lockout_validations,
            ..
        } = &mut storage_contract
        {
            let segment_index = get_segment_from_slot(slot);
            let state_segment = get_segment_from_slot(*state_slot);

            if segment_index > state_segment {
                return Err(InstructionError::InvalidArgument);
            }

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

            let valid_proofs: Vec<_> = proofs
                .into_iter()
                .enumerate()
                .filter_map(|(i, entry)| {
                    let (account, proofs) = &mut previous_proofs[i];
                    proofs.get(&entry.proof.sha_state).map(|proof| {
                        if process_validation(account, segment_index, &proof, &entry).is_ok() {
                            Some(entry)
                        } else {
                            None
                        }
                    })
                })
                .flatten()
                .collect();

            // allow validators to store successful validations
            valid_proofs.into_iter().for_each(|proof| {
                lockout_validations
                    .entry(segment_index)
                    .or_default()
                    .insert(proof.proof.sha_state, proof);
            });
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn claim_storage_reward(
        &mut self,
        slot: u64,
        current_slot: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::Default = storage_contract {
            Err(InstructionError::InvalidArgument)?
        };

        if let StorageContract::ValidatorStorage {
            reward_validations,
            slot: state_slot,
            ..
        } = &mut storage_contract
        {
            let state_segment = get_segment_from_slot(*state_slot);
            let claim_segment = get_segment_from_slot(slot);
            if state_segment <= claim_segment || claim_segment >= reward_validations.len() {
                debug!(
                    "current {:?}, claim {:?}, rewards {:?}",
                    state_segment,
                    claim_segment,
                    reward_validations.len()
                );
                return Err(InstructionError::InvalidArgument);
            }
            let _num_validations = count_valid_proofs(
                &reward_validations
                    .get_mut(&claim_segment)
                    .map(|proofs| proofs.drain().map(|(_, proof)| proof).collect::<Vec<_>>())
                    .unwrap_or_default(),
            );
            // TODO can't just create lamports out of thin air
            // self.account.lamports += TOTAL_VALIDATOR_REWARDS * num_validations;
            self.account.set_state(storage_contract)
        } else if let StorageContract::ReplicatorStorage {
            reward_validations, ..
        } = &mut storage_contract
        {
            // if current tick height is a full segment away, allow reward collection
            let current_index = get_segment_from_slot(current_slot);
            let claims_index = get_segment_from_slot(slot);
            // Todo this might might always be true
            if current_index <= claims_index || claims_index >= reward_validations.len() {
                debug!(
                    "current {:?}, claim {:?}, rewards {:?}",
                    current_index,
                    claims_index,
                    reward_validations.len()
                );
                return Err(InstructionError::InvalidArgument);
            }
            let _num_validations = count_valid_proofs(
                &reward_validations[claims_index]
                    .drain()
                    .map(|(_, proof)| proof.into_iter().collect::<Vec<_>>())
                    .flatten()
                    .collect::<Vec<_>>(),
            );
            // TODO can't just create lamports out of thin air
            // self.account.lamports += num_validations
            //     * TOTAL_REPLICATOR_REWARDS
            //     * (num_validations / reward_validations[claims_index].len() as u64);
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
    checked_proof: CheckedProof,
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

            if let Some(proof) = proofs[segment_index].get(&checked_proof.proof.sha_state) {
                if segment_index > reward_validations.len() || reward_validations.is_empty() {
                    reward_validations.resize(cmp::max(1, segment_index), HashMap::new());
                }

                let checked_proof = CheckedProof {
                    proof: proof.clone(),
                    status: checked_proof.status,
                };

                reward_validations[segment_index]
                    .entry(proof.sha_state)
                    .or_default()
                    .push(checked_proof);
            } else {
                return Err(InstructionError::InvalidAccountData);
            }
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
    store_validation_result(account, segment_index, checked_proof.clone())?;
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
            slot: 0,
            hash: Hash::default(),
            lockout_validations: HashMap::new(),
            reward_validations: HashMap::new(),
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
            let mut proof_map = HashMap::new();
            proof_map.insert(proof.sha_state, proof.clone());
            *storage_contract = StorageContract::ReplicatorStorage {
                proofs: vec![proof_map],
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
