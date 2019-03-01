//! Vote program
//! Receive and processes votes from validators

use crate::account::{Account, KeyedAccount};
use crate::native_program::ProgramError;
use crate::pubkey::Pubkey;
use crate::transaction_builder::BuilderInstruction;
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
pub const MAX_LOCKOUT_HISTORY: usize = 31;
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
pub struct Lockout {
    pub slot_height: u64,
    pub confirmation_count: u32,
}

impl Lockout {
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
    pub fn expiration_slot_height(&self) -> u64 {
        self.slot_height + self.lockout()
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// * Transaction::keys[0] - the staker id that is also assumed to be the delegate by default
    /// * Transaction::keys[1] - the new "vote account" to be associated with the delegate
    InitializeAccount,
    /// `Delegate` or `Assign` A staking account to a particular node
    DelegateStake(Pubkey),
    Vote(Vote),
    /// Clear the credits in the vote account
    /// * Transaction::keys[0] - the "vote account"
    ClearCredits,
}

impl VoteInstruction {
    pub fn new_clear_credits(vote_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(id(), &VoteInstruction::ClearCredits, vec![(vote_id, true)])
    }
    pub fn new_delegate_stake(vote_id: Pubkey, delegate_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            id(),
            &VoteInstruction::DelegateStake(delegate_id),
            vec![(vote_id, true)],
        )
    }
    pub fn new_initialize_account(vote_id: Pubkey, staker_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            id(),
            &VoteInstruction::InitializeAccount,
            vec![(staker_id, true), (vote_id, false)],
        )
    }
    pub fn new_vote(vote_id: Pubkey, vote: Vote) -> BuilderInstruction {
        BuilderInstruction::new(id(), &VoteInstruction::Vote(vote), vec![(vote_id, true)])
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteState {
    pub votes: VecDeque<Lockout>,
    pub delegate_id: Pubkey,
    pub root_slot: Option<u64>,
    credits: u64,
}

pub fn get_max_size() -> usize {
    // Upper limit on the size of the Vote State. Equal to
    // sizeof(VoteState) when votes.len() is MAX_LOCKOUT_HISTORY
    let mut vote_state = VoteState::default();
    vote_state.votes = VecDeque::from(vec![Lockout::default(); MAX_LOCKOUT_HISTORY]);
    vote_state.root_slot = Some(std::u64::MAX);
    serialized_size(&vote_state).unwrap() as usize
}

impl VoteState {
    pub fn new(delegate_id: Pubkey) -> Self {
        let votes = VecDeque::new();
        let credits = 0;
        let root_slot = None;
        Self {
            votes,
            delegate_id,
            credits,
            root_slot,
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

        let vote = Lockout::new(&vote);

        // TODO: Integrity checks
        // Verify the vote's bank hash matches what is expected

        self.pop_expired_votes(vote.slot_height);
        // Once the stack is full, pop the oldest vote and distribute rewards
        if self.votes.len() == MAX_LOCKOUT_HISTORY {
            let vote = self.votes.pop_front().unwrap();
            self.root_slot = Some(vote.slot_height);
            self.credits += 1;
        }
        self.votes.push_back(vote);
        self.double_lockouts();
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
                .map_or(false, |v| v.expiration_slot_height() < slot_height)
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

pub fn delegate_stake(
    keyed_accounts: &mut [KeyedAccount],
    node_id: Pubkey,
) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] should sign the transaction");
        Err(ProgramError::InvalidArgument)?;
    }

    let vote_state = VoteState::deserialize(&keyed_accounts[0].account.userdata);
    if let Ok(mut vote_state) = vote_state {
        vote_state.delegate_id = node_id;
        vote_state.serialize(&mut keyed_accounts[0].account.userdata)?;
    } else {
        error!("account[0] does not valid userdata");
        Err(ProgramError::InvalidUserdata)?;
    }

    Ok(())
}

/// Initialize the vote_state for a staking account
/// Assumes that the account is being init as part of a account creation or balance transfer and
/// that the transaction must be signed by the staker's keys
pub fn initialize_account(keyed_accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[1].account.owner) {
        error!("account[1] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] should sign the transaction");
        Err(ProgramError::InvalidArgument)?;
    }

    let staker_id = keyed_accounts[0].signer_key().unwrap();
    let vote_state = VoteState::deserialize(&keyed_accounts[1].account.userdata);
    if let Ok(vote_state) = vote_state {
        if vote_state.delegate_id == Pubkey::default() {
            let vote_state = VoteState::new(*staker_id);
            vote_state.serialize(&mut keyed_accounts[1].account.userdata)?;
        } else {
            error!("account[1] userdata already initialized");
            Err(ProgramError::InvalidUserdata)?;
        }
    } else {
        error!("account[1] does not have valid userdata");
        Err(ProgramError::InvalidUserdata)?;
    }

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

pub fn initialize_and_deserialize(
    from_id: &Pubkey,
    from_account: &mut Account,
    vote_id: &Pubkey,
    vote_account: &mut Account,
) -> Result<VoteState, ProgramError> {
    let mut keyed_accounts = [
        KeyedAccount::new(from_id, true, from_account),
        KeyedAccount::new(vote_id, false, vote_account),
    ];
    initialize_account(&mut keyed_accounts)?;
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
    fn test_initialize_staking_account() {
        let staker_id = Keypair::new().pubkey();
        let mut staker_account = Account::new(100, 0, Pubkey::default());
        let mut bogus_staker_account = Account::new(100, 0, Pubkey::default());

        let staking_account_id = Keypair::new().pubkey();
        let mut staking_account = create_vote_account(100);

        let bogus_account_id = Keypair::new().pubkey();
        let mut bogus_account = Account::new(100, 0, id());

        let mut keyed_accounts = [
            KeyedAccount::new(&staker_id, false, &mut bogus_staker_account),
            KeyedAccount::new(&bogus_account_id, false, &mut bogus_account),
        ];

        //staker is not signer
        let res = initialize_account(&mut keyed_accounts);
        assert_eq!(res, Err(ProgramError::InvalidArgument));

        //space was never initialized, deserialize will fail
        keyed_accounts[0] = KeyedAccount::new(&staker_id, true, &mut staker_account);
        let res = initialize_account(&mut keyed_accounts);
        assert_eq!(res, Err(ProgramError::InvalidUserdata));

        //init should pass
        keyed_accounts[1] = KeyedAccount::new(&staking_account_id, false, &mut staking_account);
        let res = initialize_account(&mut keyed_accounts);
        assert_eq!(res, Ok(()));

        // reinit should fail
        let res = initialize_account(&mut keyed_accounts);
        assert_eq!(res, Err(ProgramError::InvalidUserdata));
    }

    #[test]
    fn test_vote_serialize() {
        let mut buffer: Vec<u8> = vec![0; get_max_size()];
        let mut vote_state = VoteState::default();
        vote_state
            .votes
            .resize(MAX_LOCKOUT_HISTORY, Lockout::default());
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
            initialize_and_deserialize(&from_id, &mut from_account, &vote_id, &mut vote_account)
                .unwrap();
        assert_eq!(vote_state.delegate_id, from_id);
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let from_id = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, Pubkey::default());
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);
        initialize_and_deserialize(&from_id, &mut from_account, &vote_id, &mut vote_account)
            .unwrap();

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.votes, vec![Lockout::new(&vote)]);
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_vote_without_initialization() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.delegate_id, Pubkey::default());
        assert_eq!(vote_state.votes, vec![Lockout::new(&vote)]);
    }

    #[test]
    fn test_vote_lockout() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(voter_id);

        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            vote_state.process_vote(Vote::new((INITIAL_LOCKOUT as usize * i) as u64));
        }

        // The last vote should have been popped b/c it reached a depth of MAX_LOCKOUT_HISTORY
        assert_eq!(vote_state.votes.len(), MAX_LOCKOUT_HISTORY);
        assert_eq!(vote_state.root_slot, Some(0));
        check_lockouts(&vote_state);

        // One more vote that confirms the entire stack,
        // the root_slot should change to the
        // second vote
        let top_vote = vote_state.votes.front().unwrap().slot_height;
        vote_state.process_vote(Vote::new(
            vote_state.votes.back().unwrap().expiration_slot_height(),
        ));
        assert_eq!(Some(top_vote), vote_state.root_slot);

        // Expire everything except the first vote
        let vote = Vote::new(vote_state.votes.front().unwrap().expiration_slot_height());
        vote_state.process_vote(vote);
        // First vote and new vote are both stored for a total of 2 votes
        assert_eq!(vote_state.votes.len(), 2);
    }

    #[test]
    fn test_vote_double_lockout_after_expiration() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(voter_id);

        for i in 0..3 {
            let vote = Vote::new(i as u64);
            vote_state.process_vote(vote);
        }

        // Check the lockouts for first and second votes. Lockouts should be
        // INITIAL_LOCKOUT^3 and INITIAL_LOCKOUT^2 respectively
        check_lockouts(&vote_state);

        // Expire the third vote (which was a vote for slot 2). The height of the
        // vote stack is unchanged, so none of the previous votes should have
        // doubled in lockout
        vote_state.process_vote(Vote::new((2 + INITIAL_LOCKOUT + 1) as u64));
        check_lockouts(&vote_state);

        // Vote again, this time the vote stack depth increases, so the lockouts should
        // double for everybody
        vote_state.process_vote(Vote::new((2 + INITIAL_LOCKOUT + 2) as u64));
        check_lockouts(&vote_state);
    }

    #[test]
    fn test_vote_credits() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(voter_id);

        for i in 0..MAX_LOCKOUT_HISTORY {
            vote_state.process_vote(Vote::new(i as u64));
        }

        assert_eq!(vote_state.credits, 0);

        vote_state.process_vote(Vote::new(MAX_LOCKOUT_HISTORY as u64 + 1));
        assert_eq!(vote_state.credits, 1);
        vote_state.process_vote(Vote::new(MAX_LOCKOUT_HISTORY as u64 + 2));
        assert_eq!(vote_state.credits(), 2);
        vote_state.process_vote(Vote::new(MAX_LOCKOUT_HISTORY as u64 + 3));
        assert_eq!(vote_state.credits(), 3);
        vote_state.clear_credits();
        assert_eq!(vote_state.credits(), 0);
    }

    fn check_lockouts(vote_state: &VoteState) {
        for (i, vote) in vote_state.votes.iter().enumerate() {
            let num_lockouts = vote_state.votes.len() - i;
            assert_eq!(
                vote.lockout(),
                INITIAL_LOCKOUT.pow(num_lockouts as u32) as u64
            );
        }
    }
}
