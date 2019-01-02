use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil, Signature};
use crate::transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageProgram {
    SubmitMiningProof {
        sha_state: Hash,
        entry_height: u64,
        signature: Signature,
    },
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
    fn storage_new_mining_proof(
        from_keypair: &Keypair,
        sha_state: Hash,
        last_id: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Self;
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
            &[from_keypair.pubkey()],
            id(),
            &program,
            last_id,
            0,
        )
    }
}
