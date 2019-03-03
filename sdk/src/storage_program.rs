use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, Signature};
use crate::transaction::Transaction;

pub const ENTRIES_PER_SEGMENT: u64 = 16;

pub fn get_segment_from_entry(entry_height: u64) -> usize {
    (entry_height / ENTRIES_PER_SEGMENT) as usize
}

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
pub struct StorageProgramState {
    pub entry_height: u64,
    pub hash: Hash,

    pub proofs: Vec<Vec<ProofInfo>>,
    pub previous_proofs: Vec<Vec<ProofInfo>>,

    pub lockout_validations: Vec<Vec<ValidationInfo>>,
    pub reward_validations: Vec<Vec<ValidationInfo>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageProgram {
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

const STORAGE_PROGRAM_ID: [u8; 32] = [
    130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STORAGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STORAGE_PROGRAM_ID)
}

pub struct StorageTransaction {}

impl StorageTransaction {
    pub fn new_mining_proof(
        from_keypair: &Keypair,
        sha_state: Hash,
        recent_blockhash: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Transaction {
        let program = StorageProgram::SubmitMiningProof {
            sha_state,
            entry_height,
            signature,
        };
        Transaction::new(from_keypair, &[], id(), &program, recent_blockhash, 0)
    }

    pub fn new_advertise_recent_blockhash(
        from_keypair: &Keypair,
        storage_hash: Hash,
        recent_blockhash: Hash,
        entry_height: u64,
    ) -> Transaction {
        let program = StorageProgram::AdvertiseStorageRecentBlockhash {
            hash: storage_hash,
            entry_height,
        };
        Transaction::new(from_keypair, &[], id(), &program, recent_blockhash, 0)
    }

    pub fn new_proof_validation(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    ) -> Transaction {
        let program = StorageProgram::ProofValidation {
            entry_height,
            proof_mask,
        };
        Transaction::new(from_keypair, &[], id(), &program, recent_blockhash, 0)
    }

    pub fn new_reward_claim(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        entry_height: u64,
    ) -> Transaction {
        let program = StorageProgram::ClaimStorageReward { entry_height };
        Transaction::new(from_keypair, &[], id(), &program, recent_blockhash, 0)
    }
}
