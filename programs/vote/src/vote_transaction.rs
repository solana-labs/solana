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
) -> Transaction {
    let votes = Vote::new(slots, bank_hash);
    let vote_ix = vote_instruction::vote(
        &vote_keypair.pubkey(),
        &authorized_voter_keypair.pubkey(),
        votes,
    );

    let mut vote_tx = Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));

    vote_tx.partial_sign(&[node_keypair], blockhash);
    vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
    vote_tx
}
