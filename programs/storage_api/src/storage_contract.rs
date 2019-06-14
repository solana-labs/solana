use crate::get_segment_from_slot;
use log::*;
use num_derive::FromPrimitive;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::Account;
use solana_sdk::account::KeyedAccount;
use solana_sdk::account_utils::State;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashMap;

pub const VALIDATOR_REWARD: u64 = 25;
pub const REPLICATOR_REWARD: u64 = 25;
// Todo Tune this for actual use cases when replicators are feature complete
pub const STORAGE_ACCOUNT_SPACE: u64 = 1024 * 8;
pub const MAX_PROOFS_PER_SEGMENT: usize = 80;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive)]
pub enum StorageError {
    InvalidSegment,
    InvalidBlockhash,
    InvalidProofMask,
    DuplicateProof,
    RewardPoolDepleted,
    InvalidOwner,
    ProofLimitReached,
}

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
    /// The encryption key the replicator used (also used to generate offsets)
    pub signature: Signature,
    /// A "recent" blockhash used to generate the seed
    pub blockhash: Hash,
    /// The resulting sampled state
    pub sha_state: Hash,
    /// The start index of the segment proof is for
    pub segment_index: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StorageContract {
    Uninitialized, // Must be first (aka, 0)

    ValidatorStorage {
        owner: Pubkey,
        // Most recently advertised slot
        slot: u64,
        // Most recently advertised blockhash
        hash: Hash,
        // Lockouts and Rewards are per segment per replicator. It needs to remain this way until
        // the challenge stage is added. Once challenges are in rewards can just be a number
        lockout_validations: HashMap<usize, HashMap<Pubkey, Vec<ProofStatus>>>,
        // lamports that are ready to be claimed
        pending_lamports: u64,
    },
    ReplicatorStorage {
        owner: Pubkey,
        // TODO what to do about duplicate proofs across segments? - Check the blockhashes
        // Map of Proofs per segment, in a Vec
        proofs: HashMap<usize, Vec<Proof>>,
        // Map of Rewards per segment, in a HashMap based on the validator account that verified
        // the proof. This can be used for challenge stage when its added
        reward_validations: HashMap<usize, HashMap<Pubkey, Vec<ProofStatus>>>,
    },

    MiningPool,
}

// utility function, used by Bank, tests, genesis
pub fn create_validator_storage_account(owner: Pubkey, lamports: u64) -> Account {
    let mut storage_account = Account::new(lamports, STORAGE_ACCOUNT_SPACE as usize, &crate::id());

    storage_account
        .set_state(&StorageContract::ValidatorStorage {
            owner,
            slot: 0,
            hash: Hash::default(),
            lockout_validations: HashMap::new(),
            pending_lamports: 0,
        })
        .expect("set_state");

    storage_account
}

pub struct StorageAccount<'a> {
    pub(crate) id: Pubkey,
    account: &'a mut Account,
}

impl<'a> StorageAccount<'a> {
    pub fn new(id: Pubkey, account: &'a mut Account) -> Self {
        Self { id, account }
    }

    pub fn initialize_mining_pool(&mut self) -> Result<(), InstructionError> {
        let storage_contract = &mut self.account.state()?;
        if let StorageContract::Uninitialized = storage_contract {
            *storage_contract = StorageContract::MiningPool;
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::AccountAlreadyInitialized)?
        }
    }

    pub fn initialize_replicator_storage(&mut self, owner: Pubkey) -> Result<(), InstructionError> {
        let storage_contract = &mut self.account.state()?;
        if let StorageContract::Uninitialized = storage_contract {
            *storage_contract = StorageContract::ReplicatorStorage {
                owner,
                proofs: HashMap::new(),
                reward_validations: HashMap::new(),
            };
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::AccountAlreadyInitialized)?
        }
    }

    pub fn initialize_validator_storage(&mut self, owner: Pubkey) -> Result<(), InstructionError> {
        let storage_contract = &mut self.account.state()?;
        if let StorageContract::Uninitialized = storage_contract {
            *storage_contract = StorageContract::ValidatorStorage {
                owner,
                slot: 0,
                hash: Hash::default(),
                lockout_validations: HashMap::new(),
                pending_lamports: 0,
            };
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::AccountAlreadyInitialized)?
        }
    }

    pub fn submit_mining_proof(
        &mut self,
        sha_state: Hash,
        segment_index: usize,
        signature: Signature,
        blockhash: Hash,
        current_slot: u64,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::ReplicatorStorage {
            proofs,
            reward_validations,
            ..
        } = &mut storage_contract
        {
            let current_segment = get_segment_from_slot(current_slot);

            // clean up the account
            // TODO check for time correctness - storage seems to run at a delay of about 3
            proofs.retain(|segment, _| *segment >= current_segment.saturating_sub(5));
            reward_validations.retain(|segment, _| *segment >= current_segment.saturating_sub(10));

            if segment_index >= current_segment {
                // attempt to submit proof for unconfirmed segment
                return Err(InstructionError::CustomError(
                    StorageError::InvalidSegment as u32,
                ));
            }

            debug!(
                "Mining proof submitted with contract {:?} segment_index: {}",
                sha_state, segment_index
            );

            // TODO check that this blockhash is valid and recent
            //            if !is_valid(&blockhash) {
            //                // proof isn't using a recent blockhash
            //                return Err(InstructionError::CustomError(InvalidBlockhash as u32));
            //            }

            let proof = Proof {
                sha_state,
                signature,
                blockhash,
                segment_index,
            };
            // store the proofs in the "current" segment's entry in the hash map.
            let segment_proofs = proofs.entry(current_segment).or_default();
            if segment_proofs.contains(&proof) {
                // do not accept duplicate proofs
                return Err(InstructionError::CustomError(
                    StorageError::DuplicateProof as u32,
                ));
            }
            if segment_proofs.len() >= MAX_PROOFS_PER_SEGMENT {
                // do not accept more than MAX_PROOFS_PER_SEGMENT
                return Err(InstructionError::CustomError(
                    StorageError::ProofLimitReached as u32,
                ));
            }
            segment_proofs.push(proof);
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
        if let StorageContract::ValidatorStorage {
            slot: state_slot,
            hash: state_hash,
            lockout_validations,
            pending_lamports,
            ..
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
                return Err(InstructionError::CustomError(
                    StorageError::InvalidSegment as u32,
                ));
            }

            *state_slot = slot;
            *state_hash = hash;

            // storage epoch updated, move the lockout_validations to pending_lamports
            let num_validations = count_valid_proofs(
                &lockout_validations
                    .drain()
                    .flat_map(|(_segment, mut proofs)| {
                        proofs
                            .drain()
                            .flat_map(|(_, proof)| proof)
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>(),
            );
            *pending_lamports += VALIDATOR_REWARD * num_validations;
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn proof_validation(
        &mut self,
        me: &Pubkey,
        segment: u64,
        proofs_per_account: Vec<Vec<ProofStatus>>,
        replicator_accounts: &mut [StorageAccount],
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;
        if let StorageContract::ValidatorStorage {
            slot: state_slot,
            lockout_validations,
            ..
        } = &mut storage_contract
        {
            let segment_index = segment as usize;
            let state_segment = get_segment_from_slot(*state_slot);

            if segment_index > state_segment {
                return Err(InstructionError::CustomError(
                    StorageError::InvalidSegment as u32,
                ));
            }

            let accounts = replicator_accounts
                .iter_mut()
                .enumerate()
                .filter_map(|(i, account)| {
                    account.account.state().ok().map(|contract| match contract {
                        StorageContract::ReplicatorStorage {
                            proofs: account_proofs,
                            ..
                        } => {
                            //TODO do this better
                            if let Some(segment_proofs) =
                                account_proofs.get(&segment_index).cloned()
                            {
                                if proofs_per_account
                                    .get(i)
                                    .filter(|proofs| proofs.len() == segment_proofs.len())
                                    .is_some()
                                {
                                    Some(account)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                })
                .flatten()
                .collect::<Vec<_>>();

            if accounts.len() != proofs_per_account.len() {
                // don't have all the accounts to validate the proofs_per_account against
                return Err(InstructionError::CustomError(
                    StorageError::InvalidProofMask as u32,
                ));
            }

            let stored_proofs: Vec<_> = proofs_per_account
                .into_iter()
                .zip(accounts.into_iter())
                .filter_map(|(checked_proofs, account)| {
                    if store_validation_result(me, account, segment_index, &checked_proofs).is_ok()
                    {
                        Some((account.id, checked_proofs))
                    } else {
                        None
                    }
                })
                .collect();

            // allow validators to store successful validations
            stored_proofs
                .into_iter()
                .for_each(|(replicator_account_id, proof_mask)| {
                    lockout_validations
                        .entry(segment_index)
                        .or_default()
                        .insert(replicator_account_id, proof_mask);
                });

            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }

    pub fn claim_storage_reward(
        &mut self,
        mining_pool: &mut KeyedAccount,
        owner: &mut StorageAccount,
    ) -> Result<(), InstructionError> {
        let mut storage_contract = &mut self.account.state()?;

        if let StorageContract::ValidatorStorage {
            owner: account_owner,
            pending_lamports,
            ..
        } = &mut storage_contract
        {
            if owner.id != *account_owner {
                Err(InstructionError::CustomError(
                    StorageError::InvalidOwner as u32,
                ))?
            }

            let pending = *pending_lamports;
            if mining_pool.account.lamports < pending {
                Err(InstructionError::CustomError(
                    StorageError::RewardPoolDepleted as u32,
                ))?
            }
            mining_pool.account.lamports -= pending;
            owner.account.lamports += pending;
            //clear pending_lamports
            *pending_lamports = 0;
            self.account.set_state(storage_contract)
        } else if let StorageContract::ReplicatorStorage {
            owner: account_owner,
            reward_validations,
            ..
        } = &mut storage_contract
        {
            if owner.id != *account_owner {
                Err(InstructionError::CustomError(
                    StorageError::InvalidOwner as u32,
                ))?
            }

            let checked_proofs = reward_validations
                .drain()
                .flat_map(|(_, mut proofs)| {
                    proofs
                        .drain()
                        .flat_map(|(_, proofs)| proofs)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let total_proofs = checked_proofs.len() as u64;
            let num_validations = count_valid_proofs(&checked_proofs);
            let reward = num_validations * REPLICATOR_REWARD * (num_validations / total_proofs);
            mining_pool.account.lamports -= reward;
            owner.account.lamports += reward;
            self.account.set_state(storage_contract)
        } else {
            Err(InstructionError::InvalidArgument)?
        }
    }
}

/// Store the result of a proof validation into the replicator account
fn store_validation_result(
    me: &Pubkey,
    storage_account: &mut StorageAccount,
    segment: usize,
    proof_mask: &[ProofStatus],
) -> Result<(), InstructionError> {
    let mut storage_contract = storage_account.account.state()?;
    match &mut storage_contract {
        StorageContract::ReplicatorStorage {
            proofs,
            reward_validations,
            ..
        } => {
            if !proofs.contains_key(&segment) {
                return Err(InstructionError::InvalidAccountData);
            }

            if proofs.get(&segment).unwrap().len() != proof_mask.len() {
                return Err(InstructionError::InvalidAccountData);
            }

            reward_validations
                .entry(segment)
                .or_default()
                .insert(*me, proof_mask.to_vec());
        }
        _ => return Err(InstructionError::InvalidAccountData),
    }
    storage_account.account.set_state(&storage_contract)
}

fn count_valid_proofs(proofs: &[ProofStatus]) -> u64 {
    let mut num = 0;
    for proof in proofs {
        if let ProofStatus::Valid = proof {
            num += 1;
        }
    }
    num
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;

    #[test]
    fn test_account_data() {
        solana_logger::setup();
        let mut account = Account::default();
        account.data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);
        let storage_account = StorageAccount::new(Pubkey::default(), &mut account);
        // pretend it's a validator op code
        let mut contract = storage_account.account.state().unwrap();
        if let StorageContract::ValidatorStorage { .. } = contract {
            assert!(true)
        }
        if let StorageContract::ReplicatorStorage { .. } = &mut contract {
            panic!("Contract should not decode into two types");
        }

        contract = StorageContract::ValidatorStorage {
            owner: Pubkey::default(),
            slot: 0,
            hash: Hash::default(),
            lockout_validations: HashMap::new(),
            pending_lamports: 0,
        };
        storage_account.account.set_state(&contract).unwrap();
        if let StorageContract::ReplicatorStorage { .. } = contract {
            panic!("Wrong contract type");
        }
        contract = StorageContract::ReplicatorStorage {
            owner: Pubkey::default(),
            proofs: HashMap::new(),
            reward_validations: HashMap::new(),
        };
        storage_account.account.set_state(&contract).unwrap();
        if let StorageContract::ValidatorStorage { .. } = contract {
            panic!("Wrong contract type");
        }
    }

    #[test]
    fn test_process_validation() {
        let mut account = StorageAccount {
            id: Pubkey::default(),
            account: &mut Account {
                lamports: 0,
                data: vec![],
                owner: id(),
                executable: false,
            },
        };
        let segment_index = 0_usize;
        let proof = Proof {
            segment_index,
            ..Proof::default()
        };

        // account has no space
        store_validation_result(
            &Pubkey::default(),
            &mut account,
            segment_index,
            &vec![ProofStatus::default(); 1],
        )
        .unwrap_err();

        account
            .account
            .data
            .resize(STORAGE_ACCOUNT_SPACE as usize, 0);
        let storage_contract = &mut account.account.state().unwrap();
        if let StorageContract::Uninitialized = storage_contract {
            let mut proofs = HashMap::new();
            proofs.insert(0, vec![proof.clone()]);
            *storage_contract = StorageContract::ReplicatorStorage {
                owner: Pubkey::default(),
                proofs,
                reward_validations: HashMap::new(),
            };
        };
        account.account.set_state(storage_contract).unwrap();

        // proof is valid
        store_validation_result(
            &Pubkey::default(),
            &mut account,
            segment_index,
            &vec![ProofStatus::Valid],
        )
        .unwrap();

        // proof failed verification but we should still be able to store it
        store_validation_result(
            &Pubkey::default(),
            &mut account,
            segment_index,
            &vec![ProofStatus::NotValid],
        )
        .unwrap();
    }
}
