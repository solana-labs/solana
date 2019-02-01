//! Vote program
//! Receive and processes votes from validators

use bincode::deserialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::vote_program::{self, Vote, VoteInstruction, VoteProgram};

fn register(keyed_accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    if !vote_program::check_id(&keyed_accounts[1].account.owner) {
        error!("account[1] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    // TODO: a single validator could register multiple "vote accounts"
    // which would clutter the "accounts" structure. See github issue 1654.
    let vote_state = VoteProgram::new(*keyed_accounts[0].signer_key().unwrap());
    vote_state.serialize(&mut keyed_accounts[1].account.userdata)?;

    Ok(())
}

fn process_vote(keyed_accounts: &mut [KeyedAccount], vote: Vote) -> Result<(), ProgramError> {
    if !vote_program::check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    let mut vote_state = VoteProgram::deserialize(&keyed_accounts[0].account.userdata)?;

    // TODO: Integrity checks
    // a) Verify the vote's bank hash matches what is expected
    // b) Verify vote is older than previous votes

    // Only keep around the most recent MAX_VOTE_HISTORY votes
    if vote_state.votes.len() == vote_program::MAX_VOTE_HISTORY {
        vote_state.votes.pop_front();
    }

    vote_state.votes.push_back(vote);
    vote_state.serialize(&mut keyed_accounts[0].account.userdata)?;

    Ok(())
}

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    // all vote instructions require that accounts_keys[0] be a signer
    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] is unsigned");
        Err(ProgramError::InvalidArgument)?;
    }

    match deserialize(data).map_err(|_| ProgramError::InvalidUserdata)? {
        VoteInstruction::RegisterAccount => register(keyed_accounts),
        VoteInstruction::NewVote(vote) => {
            debug!("{:?} by {}", vote, keyed_accounts[0].signer_key().unwrap());
            solana_metrics::submit(
                solana_metrics::influxdb::Point::new("vote-native")
                    .add_field("count", solana_metrics::influxdb::Value::Integer(1))
                    .to_owned(),
            );
            process_vote(keyed_accounts, vote)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::vote_program;

    fn create_vote_program(tokens: u64) -> Account {
        let space = vote_program::get_max_size();
        Account::new(tokens, space, vote_program::id())
    }

    fn register_and_deserialize(
        voter_id: &Pubkey,
        voter_account: &mut Account,
        vote_state_id: &Pubkey,
        vote_state_account: &mut Account,
    ) -> Result<VoteProgram, ProgramError> {
        let mut keyed_accounts = [
            KeyedAccount::new(voter_id, true, voter_account),
            KeyedAccount::new(vote_state_id, false, vote_state_account),
        ];
        register(&mut keyed_accounts)?;
        let vote_state = VoteProgram::deserialize(&vote_state_account.userdata).unwrap();
        Ok(vote_state)
    }

    fn vote_and_deserialize(
        vote_state_id: &Pubkey,
        vote_state_account: &mut Account,
        vote: Vote,
    ) -> Result<VoteProgram, ProgramError> {
        let mut keyed_accounts = [KeyedAccount::new(vote_state_id, true, vote_state_account)];
        process_vote(&mut keyed_accounts, vote)?;
        let vote_state = VoteProgram::deserialize(&vote_state_account.userdata).unwrap();
        Ok(vote_state)
    }

    #[test]
    fn test_voter_registration() {
        let voter_id = Keypair::new().pubkey();
        let mut voter_account = Account::new(100, 0, Pubkey::default());

        let vote_state_id = Keypair::new().pubkey();
        let mut vote_state_account = create_vote_program(100);

        let vote_state = register_and_deserialize(
            &voter_id,
            &mut voter_account,
            &vote_state_id,
            &mut vote_state_account,
        )
        .unwrap();
        assert_eq!(vote_state.node_id, voter_id);
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let voter_id = Keypair::new().pubkey();
        let mut voter_account = Account::new(100, 0, Pubkey::default());

        let vote_state_id = Keypair::new().pubkey();
        let mut vote_state_account = create_vote_program(100);

        let mut keyed_accounts = [
            KeyedAccount::new(&voter_id, true, &mut voter_account),
            KeyedAccount::new(&vote_state_id, false, &mut vote_state_account),
        ];
        register(&mut keyed_accounts).unwrap();

        let vote = Vote::new(1);
        let vote_state =
            vote_and_deserialize(&vote_state_id, &mut vote_state_account, vote.clone()).unwrap();
        assert_eq!(vote_state.votes, vec![vote]);
    }

    #[test]
    // TODO: Should this work?
    fn test_vote_without_registration() {
        let vote_state_id = Keypair::new().pubkey();
        let mut vote_state_account = create_vote_program(100);

        let vote = Vote::new(1);
        let vote_state =
            vote_and_deserialize(&vote_state_id, &mut vote_state_account, vote.clone()).unwrap();
        assert_eq!(vote_state.votes, vec![vote]);
    }
}
