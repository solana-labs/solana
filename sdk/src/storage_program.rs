use hash::Hash;
use pubkey::Pubkey;
use signature::{Keypair, KeypairUtil};
use transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageProgram {
    SubmitMiningProof { sha_state: Hash },
}

pub const STORAGE_PROGRAM_ID: [u8; 32] = [
    130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STORAGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STORAGE_PROGRAM_ID)
}

pub trait StorageTransaction {
    fn storage_new_mining_proof(from_keypair: &Keypair, sha_state: Hash, last_id: Hash) -> Self;
}

impl StorageTransaction for Transaction {
    fn storage_new_mining_proof(from_keypair: &Keypair, sha_state: Hash, last_id: Hash) -> Self {
        let program = StorageProgram::SubmitMiningProof { sha_state };
        Transaction::new(
            from_keypair,
            &[from_keypair.pubkey()],
            id(),
            &program,
            last_id,
            0,
        )
    }
}
