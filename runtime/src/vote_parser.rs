use {
    solana_sdk::{
        hash::Hash,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, Transaction},
    },
    solana_vote_program::{vote_instruction::VoteInstruction, vote_state::Vote},
};

pub type ParsedVote = (Pubkey, Vote, Option<Hash>);

// Used for filtering out votes from the transaction log collector
pub(crate) fn is_simple_vote_transaction(transaction: &SanitizedTransaction) -> bool {
    if transaction.message().instructions().len() == 1 {
        let (program_pubkey, instruction) = transaction
            .message()
            .program_instructions_iter()
            .next()
            .unwrap();
        if program_pubkey == &solana_vote_program::id() {
            if let Ok(vote_instruction) = limited_deserialize::<VoteInstruction>(&instruction.data)
            {
                return matches!(
                    vote_instruction,
                    VoteInstruction::Vote(_) | VoteInstruction::VoteSwitch(_, _)
                );
            }
        }
    }
    false
}

// Used for locally forwarding processed vote transactions to consensus
pub fn parse_sanitized_vote_transaction(tx: &SanitizedTransaction) -> Option<ParsedVote> {
    // Check first instruction for a vote
    let message = tx.message();
    let (program_id, first_instruction) = message.program_instructions_iter().next()?;
    if !solana_vote_program::check_id(program_id) {
        return None;
    }
    let first_account = usize::from(*first_instruction.accounts.first()?);
    let key = message.get_account_key(first_account)?;
    let (vote, switch_proof_hash) = parse_vote_instruction_data(&first_instruction.data)?;
    Some((*key, vote, switch_proof_hash))
}

// Used for parsing gossip vote transactions
pub fn parse_vote_transaction(tx: &Transaction) -> Option<ParsedVote> {
    // Check first instruction for a vote
    let message = tx.message();
    let first_instruction = message.instructions.first()?;
    let program_id_index = usize::from(first_instruction.program_id_index);
    let program_id = message.account_keys.get(program_id_index)?;
    if !solana_vote_program::check_id(program_id) {
        return None;
    }
    let first_account = usize::from(*first_instruction.accounts.first()?);
    let key = message.account_keys.get(first_account)?;
    let (vote, switch_proof_hash) = parse_vote_instruction_data(&first_instruction.data)?;
    Some((*key, vote, switch_proof_hash))
}

fn parse_vote_instruction_data(vote_instruction_data: &[u8]) -> Option<(Vote, Option<Hash>)> {
    match limited_deserialize(vote_instruction_data).ok()? {
        VoteInstruction::Vote(vote) => Some((vote, None)),
        VoteInstruction::VoteSwitch(vote, hash) => Some((vote, Some(hash))),
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use solana_sdk::signature::{Keypair, Signer};
    use solana_vote_program::{
        vote_instruction, vote_state::Vote, vote_transaction::new_vote_transaction,
    };

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
