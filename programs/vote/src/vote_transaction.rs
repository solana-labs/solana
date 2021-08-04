use solana_sdk::{
    clock::Slot,
    hash::Hash,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use crate::{
    vote_instruction::{self, VoteInstruction},
    vote_state::Vote,
};

pub fn parse_vote_transaction(tx: &Transaction) -> Option<(Pubkey, Vote, Option<Hash>)> {
    // Check first instruction for a vote
    let account_keys = &tx.message.account_keys;
    let instruction = tx.message.instructions.first()?;
    let id = account_keys.get(instruction.program_id_index as usize)?;
    if !crate::check_id(id) {
        return None;
    }
    let account = *instruction.accounts.first()?;
    let key = *account_keys.get(account as usize)?;
    match limited_deserialize(&instruction.data).ok()? {
        VoteInstruction::Vote(vote) => Some((key, vote, None)),
        VoteInstruction::VoteSwitch(vote, hash) => Some((key, vote, Some(hash))),
        _ => None,
    }
}

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
        vote_instruction::vote_switch(
            &vote_keypair.pubkey(),
            &authorized_voter_keypair.pubkey(),
            votes,
            switch_proof_hash,
        )
    } else {
        vote_instruction::vote(
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

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::hash::hash;

    fn run_test_parse_vote_transaction(input_hash: Option<Hash>) {
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let auth_voter_keypair = Keypair::new();
        let bank_hash = Hash::default();
        let vote_tx = new_vote_transaction(
            vec![42],
            bank_hash,
            Hash::default(),
            &node_keypair,
            &vote_keypair,
            &auth_voter_keypair,
            input_hash,
        );
        let (key, vote, hash) = parse_vote_transaction(&vote_tx).unwrap();
        assert_eq!(hash, input_hash);
        assert_eq!(vote, Vote::new(vec![42], bank_hash));
        assert_eq!(key, vote_keypair.pubkey());

        // Test bad program id fails
        let mut vote_ix = vote_instruction::vote(
            &vote_keypair.pubkey(),
            &auth_voter_keypair.pubkey(),
            Vote::new(vec![1, 2], Hash::default()),
        );
        vote_ix.program_id = Pubkey::default();
        let vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));
        assert!(parse_vote_transaction(&vote_tx).is_none());
    }

    #[test]
    fn test_parse_vote_transaction() {
        run_test_parse_vote_transaction(None);
        run_test_parse_vote_transaction(Some(hash(&[42u8])));
    }
}
