use {
    crate::{
        vote_instruction::{self, VoteInstruction},
        vote_state::{Vote, VoteTransaction},
    },
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        instruction::CompiledInstruction,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::{SanitizedTransaction, Transaction},
    },
};

pub type ParsedVote = (Pubkey, Box<dyn VoteTransaction>, Option<Hash>);

fn parse_vote(vote_ix: &CompiledInstruction, vote_key: &Pubkey) -> Option<ParsedVote> {
    let vote_instruction = limited_deserialize(&vote_ix.data).ok();
    vote_instruction.and_then(|vote_instruction| {
        let result: Option<ParsedVote> = match vote_instruction {
            VoteInstruction::Vote(vote) => Some((*vote_key, Box::new(vote), None)),
            VoteInstruction::VoteSwitch(vote, hash) => {
                Some((*vote_key, Box::new(vote), Some(hash)))
            }
            VoteInstruction::UpdateVoteState(vote_state_update) => {
                Some((*vote_key, Box::new(vote_state_update), None))
            }
            VoteInstruction::UpdateVoteStateSwitch(vote_state_update, hash) => {
                Some((*vote_key, Box::new(vote_state_update), Some(hash)))
            }
            VoteInstruction::Authorize(_, _)
            | VoteInstruction::AuthorizeChecked(_)
            | VoteInstruction::InitializeAccount(_)
            | VoteInstruction::UpdateCommission(_)
            | VoteInstruction::UpdateValidatorIdentity
            | VoteInstruction::Withdraw(_) => None,
        };
        result
    })
}

pub fn parse_sanitized_vote_transaction(tx: &SanitizedTransaction) -> Option<ParsedVote> {
    // Check first instruction for a vote
    let message = tx.message();
    message
        .program_instructions_iter()
        .next()
        .and_then(|(program_id, first_ix)| {
            if !crate::check_id(program_id) {
                return None;
            }

            first_ix.accounts.first().and_then(|first_account| {
                message
                    .get_account_key(*first_account as usize)
                    .and_then(|key| parse_vote(first_ix, key))
            })
        })
}

pub fn parse_vote_transaction(tx: &Transaction) -> Option<ParsedVote> {
    // Check first instruction for a vote
    let message = tx.message();
    message.instructions.get(0).and_then(|first_instruction| {
        let prog_id_idx = first_instruction.program_id_index as usize;
        match message.account_keys.get(prog_id_idx) {
            Some(program_id) => {
                if !crate::check_id(program_id) {
                    return None;
                }
            }
            _ => {
                return None;
            }
        };
        first_instruction
            .accounts
            .first()
            .and_then(|first_account| {
                message
                    .account_keys
                    .get(*first_account as usize)
                    .and_then(|key| parse_vote(first_instruction, key))
            })
    })
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
    use {super::*, solana_sdk::hash::hash};

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
        assert_eq!(
            *vote.as_any().downcast_ref::<Vote>().unwrap(),
            Vote::new(vec![42], bank_hash)
        );
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
