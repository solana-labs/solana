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
pub const INITIAL_LOCKOUT: usize = 2;

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

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct StoredVote {
    pub slot_height: u64,
    pub confirmation_count: u32,
}

impl StoredVote {
    pub fn new(vote: &Vote) -> Self {
        Self {
            slot_height: vote.slot_height,
            confirmation_count: 1,
        }
    }

    // The number of slots for which this vote is locked
    pub fn lockout(&self) -> u64 {
        (INITIAL_LOCKOUT as u64).pow(self.confirmation_count)
    }

    // The slot height at which this vote expires (cannot vote for any slot
    // less than this)
    pub fn lock_height(&self) -> u64 {
        self.slot_height + self.lockout() as u64
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
    /// Clear the credits in the vote account
    /// * Transaction::keys[0] - the "vote account"
    ClearCredits,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteState {
    pub votes: VecDeque<StoredVote>,
    pub node_id: Pubkey,
    pub staker_id: Pubkey,
    pub max_lockout_slot_height: u64,
    credits: u64,
}

pub fn get_max_size() -> usize {
    // Upper limit on the size of the Vote State. Equal to
    // sizeof(VoteState) when votes.len() is MAX_VOTE_HISTORY
    let mut vote_state = VoteState::default();
    vote_state.votes = VecDeque::from(vec![StoredVote::default(); MAX_VOTE_HISTORY]);
    serialized_size(&vote_state).unwrap() as usize
}

impl VoteState {
    pub fn new(node_id: Pubkey, staker_id: Pubkey) -> Self {
        let votes = VecDeque::new();
        let credits = 0;
        let max_lockout_slot_height = 0;
        Self {
            votes,
            node_id,
            staker_id,
            credits,
            max_lockout_slot_height,
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
        // Ignore votes for slots earlier than we already have votes for
        if self
            .votes
            .back()
            .map_or(false, |old_vote| old_vote.slot_height >= vote.slot_height)
        {
            return;
        }

        let vote = StoredVote::new(&vote);

        // TODO: Integrity checks
        // Verify the vote's bank hash matches what is expected

        self.pop_expired_votes(vote.slot_height);
        self.votes.push_back(vote);
        self.double_lockouts();

        // Once the stack is full, pop the oldest vote and distribute rewards
        if self.votes.len() == MAX_VOTE_HISTORY {
            let vote = self.votes.pop_front().unwrap();
            self.max_lockout_slot_height = vote.slot_height;
            self.credits += 1;
        }
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

    fn pop_expired_votes(&mut self, slot_height: u64) {
        loop {
            if self
                .votes
                .back()
                .map_or(false, |v| v.lock_height() < slot_height)
            {
                self.votes.pop_back();
            } else {
                break;
            }
        }
    }

    fn double_lockouts(&mut self) {
        let stack_depth = self.votes.len();
        for (i, v) in self.votes.iter_mut().enumerate() {
            // Don't increase the lockout for this vote until we get more confirmations
            // than the max number of confirmations this vote has seen
            if stack_depth > i + v.confirmation_count as usize {
                v.confirmation_count += 1;
            } else {
                break;
            }
        }
    }
}

// TODO: Deprecate the RegisterAccount instruction and its awkward delegation
// semantics.
pub fn register(keyed_accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[1].account.owner) {
        error!("account[1] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    // TODO: This assumes keyed_accounts[0] is the SystemInstruction::CreateAccount
    // that created keyed_accounts[1]. Putting any other signed instruction in
    // keyed_accounts[0] would allow the owner to highjack the vote account and
    // insert itself into the leader rotation.
    let from_id = keyed_accounts[0].signer_key().unwrap();

    // Awkwardly configure the voting account to claim that the account that
    // initially funded it is both the identity of the staker and the fullnode
    // that should sign blocks on behalf of the staker.
    let vote_state = VoteState::new(*from_id, *from_id);
    vote_state.serialize(&mut keyed_accounts[1].account.userdata)?;

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
    from_account: &mut Account,
    vote_id: &Pubkey,
    vote_account: &mut Account,
) -> Result<VoteState, ProgramError> {
    let mut keyed_accounts = [
        KeyedAccount::new(from_id, true, from_account),
        KeyedAccount::new(vote_id, false, vote_account),
    ];
    register(&mut keyed_accounts)?;
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
        vote_state
            .votes
            .resize(MAX_VOTE_HISTORY, StoredVote::default());
        vote_state.serialize(&mut buffer).unwrap();
        assert_eq!(VoteState::deserialize(&buffer).unwrap(), vote_state);
    }

    #[test]
    fn test_voter_registration() {
        let from_id = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, Pubkey::default());

        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote_state =
            register_and_deserialize(&from_id, &mut from_account, &vote_id, &mut vote_account)
                .unwrap();
        assert_eq!(vote_state.node_id, from_id);
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let from_id = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, Pubkey::default());

        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);
        register_and_deserialize(&from_id, &mut from_account, &vote_id, &mut vote_account).unwrap();

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.votes, vec![StoredVote::new(&vote)]);
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_vote_without_registration() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.node_id, Pubkey::default());
        assert_eq!(vote_state.votes, vec![StoredVote::new(&vote)]);
    }

    #[test]
    fn test_vote_lockout() {
        let voter_id = Keypair::new().pubkey();
        let staker_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(voter_id, staker_id);

        for i in 0..MAX_VOTE_HISTORY {
            vote_state.process_vote(Vote::new((INITIAL_LOCKOUT as usize * i) as u64));
        }

        // The last vote should have been popped b/c it reached a depth of MAX_VOTE_HISTORY
        assert_eq!(vote_state.votes.len(), MAX_VOTE_HISTORY - 1);
        assert_eq!(vote_state.max_lockout_slot_height, 0);
        for (i, vote) in vote_state.votes.iter().enumerate() {
            let num_lockouts = MAX_VOTE_HISTORY - 1 - i;
            assert_eq!(
                vote.lockout(),
                INITIAL_LOCKOUT.pow(num_lockouts as u32) as u64
            );
        }

        // One more vote that confirms the entire stack,
        // the max_lockout_slot_height should change to the
        // second vote
        let top_vote = vote_state.votes.front().unwrap().slot_height;
        vote_state.process_vote(Vote::new(vote_state.votes.back().unwrap().lock_height()));
        assert_eq!(top_vote, vote_state.max_lockout_slot_height);

        // Expire everything except the first vote
        let vote = Vote::new(vote_state.votes.front().unwrap().lock_height());
        vote_state.process_vote(vote);
        // First vote and new vote are both stored for a total of 2 votes
        assert_eq!(vote_state.votes.len(), 2);
    }

    #[test]
    fn test_vote_double_lockout_after_expiration() {
        let voter_id = Keypair::new().pubkey();
        let staker_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(voter_id, staker_id);

        for i in 0..3 {
            let vote = Vote::new(i as u64);
            vote_state.process_vote(vote);
        }

        // Check the lockouts for first and second votes. Lockouts should be
        // INITIAL_LOCKOUT^3 and INITIAL_LOCKOUT^2 respectively
        assert_eq!(vote_state.votes[0].lockout(), INITIAL_LOCKOUT.pow(3) as u64);
        assert_eq!(vote_state.votes[1].lockout(), INITIAL_LOCKOUT.pow(2) as u64);

        // Expire the third vote (which was a vote for slot 2). The height of the
        // vote stack is unchanged, so none of the previous votes should have
        // doubled in lockout
        vote_state.process_vote(Vote::new((2 + INITIAL_LOCKOUT + 1) as u64));

        assert_eq!(vote_state.votes[0].lockout(), INITIAL_LOCKOUT.pow(3) as u64);
        assert_eq!(vote_state.votes[1].lockout(), INITIAL_LOCKOUT.pow(2) as u64);
        assert_eq!(vote_state.votes[2].lockout(), INITIAL_LOCKOUT as u64);

        // Vote again, this time the vote stack depth increases, so the lockouts should
        // double for everybody
        vote_state.process_vote(Vote::new((2 + INITIAL_LOCKOUT + 2) as u64));
        assert_eq!(vote_state.votes[0].lockout(), INITIAL_LOCKOUT.pow(4) as u64);
        assert_eq!(vote_state.votes[1].lockout(), INITIAL_LOCKOUT.pow(3) as u64);
        assert_eq!(vote_state.votes[2].lockout(), INITIAL_LOCKOUT.pow(2) as u64);
        assert_eq!(vote_state.votes[3].lockout(), INITIAL_LOCKOUT as u64);
    }

    #[test]
    fn test_vote_credits() {
        let voter_id = Keypair::new().pubkey();
        let staker_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(voter_id, staker_id);

        for i in 0..MAX_VOTE_HISTORY - 1 {
            vote_state.process_vote(Vote::new(i as u64));
        }

        assert_eq!(vote_state.credits, 0);

        vote_state.process_vote(Vote::new(MAX_VOTE_HISTORY as u64));
        assert_eq!(vote_state.credits, 1);
        vote_state.process_vote(Vote::new(MAX_VOTE_HISTORY as u64 + 1));
        assert_eq!(vote_state.credits(), 2);
        vote_state.process_vote(Vote::new(MAX_VOTE_HISTORY as u64 + 2));
        assert_eq!(vote_state.credits(), 3);
        vote_state.clear_credits();
        assert_eq!(vote_state.credits(), 0);
    }
}
