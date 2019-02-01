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
    pub id: Hash,

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
    AdvertiseStorageLastId {
        id: Hash,
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

pub const STORAGE_PROGRAM_ID: [u8; 32] = [
    130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub const STORAGE_SYSTEM_ACCOUNT_ID: [u8; 32] = [
    133, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STORAGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STORAGE_PROGRAM_ID)
}

pub fn system_id() -> Pubkey {
    Pubkey::new(&STORAGE_SYSTEM_ACCOUNT_ID)
}

pub trait StorageTransaction {
    fn storage_new_mining_proof(
        from_keypair: &Keypair,
        sha_state: Hash,
        last_id: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Self;

    fn storage_new_advertise_last_id(
        from_keypair: &Keypair,
        storage_last_id: Hash,
        last_id: Hash,
        entry_height: u64,
    ) -> Self;

    fn storage_new_proof_validation(
        from_keypair: &Keypair,
        last_id: Hash,
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    ) -> Self;

    fn storage_new_reward_claim(from_keypair: &Keypair, last_id: Hash, entry_height: u64) -> Self;
}

impl StorageTransaction for Transaction {
    fn storage_new_mining_proof(
        from_keypair: &Keypair,
        sha_state: Hash,
        last_id: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Self {
        let program = StorageProgram::SubmitMiningProof {
            sha_state,
            entry_height,
            signature,
        };
        Transaction::new(
            from_keypair,
            &[Pubkey::new(&STORAGE_SYSTEM_ACCOUNT_ID)],
            id(),
            &program,
            last_id,
            0,
        )
    }

    fn storage_new_advertise_last_id(
        from_keypair: &Keypair,
        storage_id: Hash,
        last_id: Hash,
        entry_height: u64,
    ) -> Self {
        let program = StorageProgram::AdvertiseStorageLastId {
            id: storage_id,
            entry_height,
        };
        Transaction::new(
            from_keypair,
            &[Pubkey::new(&STORAGE_SYSTEM_ACCOUNT_ID)],
            id(),
            &program,
            last_id,
            0,
        )
    }

    fn storage_new_proof_validation(
        from_keypair: &Keypair,
        last_id: Hash,
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    ) -> Self {
        let program = StorageProgram::ProofValidation {
            entry_height,
            proof_mask,
        };
        Transaction::new(
            from_keypair,
            &[Pubkey::new(&STORAGE_SYSTEM_ACCOUNT_ID)],
            id(),
            &program,
            last_id,
            0,
        )
    }

    fn storage_new_reward_claim(from_keypair: &Keypair, last_id: Hash, entry_height: u64) -> Self {
        let program = StorageProgram::ClaimStorageReward { entry_height };
        Transaction::new(
            from_keypair,
            &[Pubkey::new(&STORAGE_SYSTEM_ACCOUNT_ID)],
            id(),
            &program,
            last_id,
            0,
        )
    }
}
