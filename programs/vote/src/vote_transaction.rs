use solana_sdk::{
    clock::Slot,
    hash::Hash,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use crate::{vote_instruction, vote_state::Vote};

pub fn new_vote_transaction(
    slots: Vec<Slot>,
    bank_hash: Hash,
    blockhash: Hash,
    node_keypair: &Keypair,
    vote_keypair: &Keypair,
    authorized_voter_keypair: &Keypair,
    switch_proof_hash: Option<Hash>,
) -> Transaction {
    let votes = Vote::new(slots, bank_hash);
    let mut instructions = vec![vote_instruction::vote(
        &vote_keypair.pubkey(),
        &authorized_voter_keypair.pubkey(),
        votes,
    )];
    if let Some(switch_proof_hash) = switch_proof_hash {
        instructions.push(vote_instruction::add_switch_proof(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            switch_proof_hash,
        ));
    }

    let mut vote_tx = Transaction::new_with_payer(&instructions, Some(&node_keypair.pubkey()));

    vote_tx.partial_sign(&[node_keypair], blockhash);
    vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
    vote_tx
}
