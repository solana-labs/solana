//! Vote program
//! Receive and processes votes from validators

use crate::native_program::ProgramError;
use crate::pubkey::Pubkey;
use bincode::{deserialize, serialize_into, serialized_size, ErrorKind};
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
    /// A vote for height tick_height
    pub tick_height: u64,
}

impl Vote {
    pub fn new(tick_height: u64) -> Self {
        Self { tick_height }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Register a new "vote account" to represent a particular validator in the Vote Contract,
    /// and initialize the VoteState for this "vote account"
    /// * Transaction::keys[0] - the validator id
    /// * Transaction::keys[1] - the new "vote account" to be associated with the validator
    /// identified by keys[0] for voting
    RegisterAccount,
    Vote(Vote),
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteState {
    pub votes: VecDeque<Vote>,
    pub node_id: Pubkey,
    pub staker_id: Pubkey,
    credits: u64,
}

pub fn get_max_size() -> usize {
    // Upper limit on the size of the Vote State. Equal to
    // sizeof(VoteState) when votes.len() is MAX_VOTE_HISTORY
    let mut vote_program = VoteState::default();
    vote_program.votes = VecDeque::from(vec![Vote::default(); MAX_VOTE_HISTORY]);
    serialized_size(&vote_program).unwrap() as usize
}

impl VoteState {
    pub fn new(node_id: Pubkey, staker_id: Pubkey) -> Self {
        let votes = VecDeque::new();
        let credits = 0;
        Self {
            votes,
            node_id,
            staker_id,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
