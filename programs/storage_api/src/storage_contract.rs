use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

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
