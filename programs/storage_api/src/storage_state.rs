use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ProofStatus {
    Valid,
    NotValid,
    Skipped,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ProofInfo {
    pub id: Pubkey,
    pub signature: Signature,
    pub sha_state: Hash,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ValidationInfo {
    pub id: Pubkey,
    pub proof_mask: Vec<ProofStatus>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct StorageState {
    pub entry_height: u64,
    pub hash: Hash,

    pub proofs: Vec<Vec<ProofInfo>>,
    pub previous_proofs: Vec<Vec<ProofInfo>>,

    pub lockout_validations: Vec<Vec<ValidationInfo>>,
    pub reward_validations: Vec<Vec<ValidationInfo>>,
}
