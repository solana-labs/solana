use crate::storage_contract::ProofStatus;
use crate::storage_instruction::StorageInstruction;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
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
        let from_pubkey = from_keypair.pubkey();
        let storage_instruction =
            StorageInstruction::new_mining_proof(&from_pubkey, sha_state, entry_height, signature);
        let instructions = vec![storage_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, 0)
    }

    pub fn new_advertise_recent_blockhash(
        from_keypair: &Keypair,
        storage_hash: Hash,
        recent_blockhash: Hash,
        entry_height: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let storage_instruction = StorageInstruction::new_advertise_recent_blockhash(
            &from_pubkey,
            storage_hash,
            entry_height,
        );
        let instructions = vec![storage_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, 0)
    }

    pub fn new_proof_validation(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        entry_height: u64,
        proof_mask: Vec<ProofStatus>,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let storage_instruction =
            StorageInstruction::new_proof_validation(&from_pubkey, entry_height, proof_mask);
        let instructions = vec![storage_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, 0)
    }

    pub fn new_reward_claim(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        entry_height: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let storage_instruction = StorageInstruction::new_reward_claim(&from_pubkey, entry_height);
        let instructions = vec![storage_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, 0)
    }
}
