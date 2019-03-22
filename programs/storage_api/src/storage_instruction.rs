use crate::storage_state::ProofStatus;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageInstruction {
    SubmitMiningProof {
        sha_state: Hash,
        entry_height: u64,
        signature: Signature,
    },
    AdvertiseStorageRecentBlockhash {
        hash: Hash,
        entry_height: u64,
    },
    ClaimStorageReward {
        entry_height: u64,
    },
    ProofValidation {
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    },
}
