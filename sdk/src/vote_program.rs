//! Vote program
//! Receive and processes votes from validators

use crate::account::{Account, KeyedAccount};
use crate::native_program::ProgramError;
use crate::pubkey::Pubkey;
use bincode::{deserialize, serialize_into, serialized_size, ErrorKind};
use log::*;
use std::collections::VecDeque;

pub const VOTE_PROGRAM_ID: [u8; 32] = [
    132, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == VOTE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&VOTE_PROGRAM_ID)
}

// Maximum number of votes to keep around
pub const MAX_VOTE_HISTORY: usize = 32;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    // TODO: add signature of the state here as well
    /// A vote for height slot_height
    pub slot_height: u64,
}

impl Vote {
    pub fn new(slot_height: u64) -> Self {
        Self { slot_height }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Register a new "vote account" to represent a particular validator in the Vote Contract,
    /// and initialize the VoteState for this "vote account"
    /// * Transaction::keys[0] - the validator id
    /// * Transaction::keys[1] - the new "vote account" to be associated with the validator
    /// identified by keys[0] for voting
    RegisterAccount(Pubkey),
    Vote(Vote),
    /// Clear the credits in the vote account
    /// * Transaction::keys[0] - the "vote account"
    ClearCredits,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteState {
    pub votes: VecDeque<Vote>,
    pub node_id: Pubkey,
    pub voter_id: Pubkey,
    credits: u64,
}

pub fn get_max_size() -> usize {
    // Upper limit on the size of the Vote State. Equal to
    // sizeof(VoteState) when votes.len() is MAX_VOTE_HISTORY
    let mut vote_state = VoteState::default();
    vote_state.votes = VecDeque::from(vec![Vote::default(); MAX_VOTE_HISTORY]);
    serialized_size(&vote_state).unwrap() as usize
}

impl VoteState {
    pub fn new(node_id: Pubkey, voter_id: Pubkey) -> Self {
        let votes = VecDeque::new();
        let credits = 0;
        Self {
            votes,
            node_id,
            voter_id,
            credits,
        }
    }

    pub fn deserialize(input: &[u8]) -> Result<Self, ProgramError> {
        deserialize(input).map_err(|_| ProgramError::InvalidUserdata)
    }

    pub fn serialize(&self, output: &mut [u8]) -> Result<(), ProgramError> {
        serialize_into(output, self).map_err(|err| match *err {
            ErrorKind::SizeLimit => ProgramError::UserdataTooSmall,
            _ => ProgramError::GenericError,
        })
    }

    pub fn process_vote(&mut self, vote: Vote) {
        // TODO: Integrity checks
        // a) Verify the vote's bank hash matches what is expected
        // b) Verify vote is older than previous votes

        // Only keep around the most recent MAX_VOTE_HISTORY votes
        if self.votes.len() == MAX_VOTE_HISTORY {
            self.votes.pop_front();
            self.credits += 1;
        }

        self.votes.push_back(vote);
    }

    /// Number of "credits" owed to this account from the mining pool. Submit this
    /// VoteState to the Rewards program to trade credits for lamports.
    pub fn credits(&self) -> u64 {
        self.credits
    }

    /// Clear any credits.
    pub fn clear_credits(&mut self) {
        self.credits = 0;
    }
}

// TODO: Deprecate the RegisterAccount instruction and its awkward delegation
// semantics.
pub fn register(keyed_accounts: &mut [KeyedAccount], node_id: Pubkey) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }
    let vote_account_id = keyed_accounts[0].signer_key().unwrap();
    let vote_state = VoteState::new(node_id, *vote_account_id);
    vote_state.serialize(&mut keyed_accounts[0].account.userdata)?;

    Ok(())
}

pub fn process_vote(keyed_accounts: &mut [KeyedAccount], vote: Vote) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    let mut vote_state = VoteState::deserialize(&keyed_accounts[0].account.userdata)?;
    vote_state.process_vote(vote);
    vote_state.serialize(&mut keyed_accounts[0].account.userdata)?;
    Ok(())
}

pub fn clear_credits(keyed_accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    let mut vote_state = VoteState::deserialize(&keyed_accounts[0].account.userdata)?;
    vote_state.clear_credits();
    vote_state.serialize(&mut keyed_accounts[0].account.userdata)?;
    Ok(())
}

pub fn create_vote_account(tokens: u64) -> Account {
    let space = get_max_size();
    Account::new(tokens, space, id())
}

pub fn register_and_deserialize(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    vote_account: &mut Account,
) -> Result<VoteState, ProgramError> {
    let mut keyed_accounts = [KeyedAccount::new(vote_id, true, vote_account)];
    register(&mut keyed_accounts, *from_id)?;
    let vote_state = VoteState::deserialize(&vote_account.userdata).unwrap();
    Ok(vote_state)
}

pub fn vote_and_deserialize(
    vote_id: &Pubkey,
    vote_account: &mut Account,
    vote: Vote,
) -> Result<VoteState, ProgramError> {
    let mut keyed_accounts = [KeyedAccount::new(vote_id, true, vote_account)];
    process_vote(&mut keyed_accounts, vote)?;
    let vote_state = VoteState::deserialize(&vote_account.userdata).unwrap();
    Ok(vote_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_vote_serialize() {
        let mut buffer: Vec<u8> = vec![0; get_max_size()];
        let mut vote_state = VoteState::default();
        vote_state.votes.resize(MAX_VOTE_HISTORY, Vote::default());
        vote_state.serialize(&mut buffer).unwrap();
        assert_eq!(VoteState::deserialize(&buffer).unwrap(), vote_state);
    }

    #[test]
    fn test_vote_credits() {
        let mut vote_state = VoteState::default();
        vote_state.votes.resize(MAX_VOTE_HISTORY, Vote::default());
        assert_eq!(vote_state.credits(), 0);
        vote_state.process_vote(Vote::new(42));
        assert_eq!(vote_state.credits(), 1);
        vote_state.process_vote(Vote::new(43));
        assert_eq!(vote_state.credits(), 2);
        vote_state.clear_credits();
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_voter_registration() {
        let from_id = Keypair::new().pubkey();

        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote_state = register_and_deserialize(&from_id, &vote_id, &mut vote_account).unwrap();
        assert_eq!(vote_state.node_id, from_id);
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let from_id = Keypair::new().pubkey();
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);
        register_and_deserialize(&from_id, &vote_id, &mut vote_account).unwrap();

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.votes, vec![vote]);
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_vote_without_registration() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.node_id, Pubkey::default());
        assert_eq!(vote_state.votes, vec![vote]);
    }
}
