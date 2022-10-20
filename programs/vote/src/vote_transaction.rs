use {
    solana_program::vote::{
        self,
        state::{Vote, VoteStateUpdate},
    },
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
};

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
    let vote_ix = if let Some(switch_proof_hash) = switch_proof_hash {
        vote::instruction::vote_switch(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            votes,
            switch_proof_hash,
        )
    } else {
        vote::instruction::vote(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            votes,
        )
    };

    let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));

    vote_tx.partial_sign(&[node_keypair], blockhash);
    vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
    vote_tx
}

pub fn new_vote_state_update_transaction(
    vote_state_update: VoteStateUpdate,
    blockhash: Hash,
    node_keypair: &Keypair,
    vote_keypair: &Keypair,
    authorized_voter_keypair: &Keypair,
    switch_proof_hash: Option<Hash>,
) -> Transaction {
    let vote_ix = if let Some(switch_proof_hash) = switch_proof_hash {
        vote::instruction::update_vote_state_switch(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            vote_state_update,
            switch_proof_hash,
        )
    } else {
        vote::instruction::update_vote_state(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            vote_state_update,
        )
    };

    let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));

    vote_tx.partial_sign(&[node_keypair], blockhash);
    vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
    vote_tx
}

pub fn new_compact_vote_state_update_transaction(
    vote_state_update: VoteStateUpdate,
    blockhash: Hash,
    node_keypair: &Keypair,
    vote_keypair: &Keypair,
    authorized_voter_keypair: &Keypair,
    switch_proof_hash: Option<Hash>,
) -> Transaction {
    let vote_ix = if let Some(switch_proof_hash) = switch_proof_hash {
        vote::instruction::compact_update_vote_state_switch(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            vote_state_update,
            switch_proof_hash,
        )
    } else {
        vote::instruction::compact_update_vote_state(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            vote_state_update,
        )
    };

    let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));

    vote_tx.partial_sign(&[node_keypair], blockhash);
    vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
    vote_tx
}
