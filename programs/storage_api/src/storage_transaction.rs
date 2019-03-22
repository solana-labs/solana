use crate::id;
use crate::storage_instruction::StorageInstruction;
use crate::storage_state::ProofStatus;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::transaction::Transaction;

pub struct StorageTransaction {}

impl StorageTransaction {
    pub fn new_mining_proof(
        from_keypair: &Keypair,
        sha_state: Hash,
        recent_blockhash: Hash,
        entry_height: u64,
        signature: Signature,
    ) -> Transaction {
        let program = StorageInstruction::SubmitMiningProof {
            sha_state,
            entry_height,
            signature,
        };
        Transaction::new_signed(from_keypair, &[], &id(), &program, recent_blockhash, 0)
    }

    pub fn new_advertise_recent_blockhash(
        from_keypair: &Keypair,
        storage_hash: Hash,
        recent_blockhash: Hash,
        entry_height: u64,
    ) -> Transaction {
        let program = StorageInstruction::AdvertiseStorageRecentBlockhash {
            hash: storage_hash,
            entry_height,
        };
        Transaction::new_signed(from_keypair, &[], &id(), &program, recent_blockhash, 0)
    }

    pub fn new_proof_validation(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    ) -> Transaction {
        let program = StorageInstruction::ProofValidation {
            entry_height,
            proof_mask,
        };
        Transaction::new_signed(from_keypair, &[], &id(), &program, recent_blockhash, 0)
    }

    pub fn new_reward_claim(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        entry_height: u64,
    ) -> Transaction {
        let program = StorageInstruction::ClaimStorageReward { entry_height };
        Transaction::new_signed(from_keypair, &[], &id(), &program, recent_blockhash, 0)
    }
}
