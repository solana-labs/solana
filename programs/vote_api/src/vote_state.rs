//! Vote stte
//! Receive and processes votes from validators

use crate::vote_instruction::Vote;
use crate::{check_id, id};
use bincode::{deserialize, serialize_into, serialized_size, ErrorKind};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::{Account, KeyedAccount};
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use std::collections::VecDeque;

// Maximum number of votes to keep around
pub const MAX_LOCKOUT_HISTORY: usize = 31;
pub const INITIAL_LOCKOUT: usize = 2;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Lockout {
    pub slot: u64,
    pub confirmation_count: u32,
}

impl Lockout {
    pub fn new(vote: &Vote) -> Self {
        Self {
            slot: vote.slot,
            confirmation_count: 1,
        }
    }

    // The number of slots for which this vote is locked
    pub fn lockout(&self) -> u64 {
        (INITIAL_LOCKOUT as u64).pow(self.confirmation_count)
    }

    // The slot height at which this vote expires (cannot vote for any slot
    // less than this)
    pub fn expiration_slot(&self) -> u64 {
        self.slot + self.lockout()
    }
    pub fn is_expired(&self, slot: u64) -> bool {
        self.expiration_slot() < slot
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct VoteState {
    pub votes: VecDeque<Lockout>,
    pub delegate_id: Pubkey,
    pub authorized_voter_id: Pubkey,
    pub root_slot: Option<u64>,
    credits: u64,
}

impl VoteState {
    pub fn new(staker_id: &Pubkey) -> Self {
        let votes = VecDeque::new();
        let credits = 0;
        let root_slot = None;
        Self {
            votes,
            delegate_id: *staker_id,
            authorized_voter_id: *staker_id,
            credits,
            root_slot,
        }
    }

    pub fn max_size() -> usize {
        // Upper limit on the size of the Vote State. Equal to
        // sizeof(VoteState) when votes.len() is MAX_LOCKOUT_HISTORY
        let mut vote_state = Self::default();
        vote_state.votes = VecDeque::from(vec![Lockout::default(); MAX_LOCKOUT_HISTORY]);
        vote_state.root_slot = Some(std::u64::MAX);
        serialized_size(&vote_state).unwrap() as usize
    }

    pub fn deserialize(input: &[u8]) -> Result<Self, InstructionError> {
        deserialize(input).map_err(|_| InstructionError::InvalidAccountData)
    }

    pub fn serialize(&self, output: &mut [u8]) -> Result<(), InstructionError> {
        serialize_into(output, self).map_err(|err| match *err {
            ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
            _ => InstructionError::GenericError,
        })
    }

    pub fn process_vote(&mut self, vote: Vote) {
        // Ignore votes for slots earlier than we already have votes for
        if self
            .votes
            .back()
            .map_or(false, |old_vote| old_vote.slot >= vote.slot)
        {
            return;
        }

        let vote = Lockout::new(&vote);

        // TODO: Integrity checks
        // Verify the vote's bank hash matches what is expected

        self.pop_expired_votes(vote.slot);
        // Once the stack is full, pop the oldest vote and distribute rewards
        if self.votes.len() == MAX_LOCKOUT_HISTORY {
            let vote = self.votes.pop_front().unwrap();
            self.root_slot = Some(vote.slot);
            self.credits += 1;
        }
        self.votes.push_back(vote);
        self.double_lockouts();
    }

    pub fn nth_recent_vote(&self, position: usize) -> Option<&Lockout> {
        if position < self.votes.len() {
            let pos = self.votes.len() - 1 - position;
            self.votes.get(pos)
        } else {
            None
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

    fn pop_expired_votes(&mut self, slot: u64) {
        loop {
            if self.votes.back().map_or(false, |v| v.is_expired(slot)) {
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
    node_id: &Pubkey,
) -> Result<(), InstructionError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(InstructionError::InvalidArgument)?;
    }

    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] should sign the transaction");
        Err(InstructionError::InvalidArgument)?;
    }

    let vote_state = VoteState::deserialize(&keyed_accounts[0].account.data);
    if let Ok(mut vote_state) = vote_state {
        vote_state.delegate_id = *node_id;
        vote_state.serialize(&mut keyed_accounts[0].account.data)?;
    } else {
        error!("account[0] does not valid data");
        Err(InstructionError::InvalidAccountData)?;
    }

    Ok(())
}

/// Authorize the given pubkey to sign votes. This may be called multiple times,
/// but will implicitly withdraw authorization from the previously authorized
/// voter. The default voter is the owner of the vote account's pubkey.
pub fn authorize_voter(
    keyed_accounts: &mut [KeyedAccount],
    voter_id: &Pubkey,
) -> Result<(), InstructionError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(InstructionError::InvalidArgument)?;
    }

    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] should sign the transaction");
        Err(InstructionError::InvalidArgument)?;
    }

    let vote_state = VoteState::deserialize(&keyed_accounts[0].account.data);
    if let Ok(mut vote_state) = vote_state {
        vote_state.authorized_voter_id = *voter_id;
        vote_state.serialize(&mut keyed_accounts[0].account.data)?;
    } else {
        error!("account[0] does not valid data");
        Err(InstructionError::InvalidAccountData)?;
    }

    Ok(())
}

/// Initialize the vote_state for a vote account
/// Assumes that the account is being init as part of a account creation or balance transfer and
/// that the transaction must be signed by the staker's keys
pub fn initialize_account(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(InstructionError::InvalidArgument)?;
    }

    let staker_id = keyed_accounts[0].unsigned_key();
    let vote_state = VoteState::deserialize(&keyed_accounts[0].account.data);
    if let Ok(vote_state) = vote_state {
        if vote_state.delegate_id == Pubkey::default() {
            let vote_state = VoteState::new(staker_id);
            vote_state.serialize(&mut keyed_accounts[0].account.data)?;
        } else {
            error!("account[0] data already initialized");
            Err(InstructionError::InvalidAccountData)?;
        }
    } else {
        error!("account[0] does not have valid data");
        Err(InstructionError::InvalidAccountData)?;
    }

    Ok(())
}

pub fn process_vote(
    keyed_accounts: &mut [KeyedAccount],
    vote: Vote,
) -> Result<(), InstructionError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(InstructionError::InvalidArgument)?;
    }

    let mut vote_state = VoteState::deserialize(&keyed_accounts[0].account.data)?;

    // If no voter was authorized, expect account[0] to be the signer, otherwise account[1].
    let signer_index = if vote_state.authorized_voter_id == *keyed_accounts[0].unsigned_key() {
        0
    } else {
        1
    };

    if keyed_accounts.get(signer_index).is_none() {
        error!("account[{}] not provided", signer_index);
        Err(InstructionError::InvalidArgument)?;
    }
    if keyed_accounts[signer_index].signer_key().is_none() {
        error!("account[{}] should sign the transaction", signer_index);
        Err(InstructionError::InvalidArgument)?;
    }

    vote_state.process_vote(vote);
    vote_state.serialize(&mut keyed_accounts[0].account.data)?;
    Ok(())
}

pub fn clear_credits(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(InstructionError::InvalidArgument)?;
    }

    let mut vote_state = VoteState::deserialize(&keyed_accounts[0].account.data)?;
    vote_state.clear_credits();
    vote_state.serialize(&mut keyed_accounts[0].account.data)?;
    Ok(())
}

pub fn create_vote_account(lamports: u64) -> Account {
    let space = VoteState::max_size();
    Account::new(lamports, space, &id())
}

pub fn initialize_and_deserialize(
    vote_id: &Pubkey,
    vote_account: &mut Account,
) -> Result<VoteState, InstructionError> {
    let mut keyed_accounts = [KeyedAccount::new(vote_id, false, vote_account)];
    initialize_account(&mut keyed_accounts)?;
    let vote_state = VoteState::deserialize(&vote_account.data).unwrap();
    Ok(vote_state)
}

pub fn vote_and_deserialize(
    vote_id: &Pubkey,
    vote_account: &mut Account,
    vote: Vote,
) -> Result<VoteState, InstructionError> {
    let mut keyed_accounts = [KeyedAccount::new(vote_id, true, vote_account)];
    process_vote(&mut keyed_accounts, vote)?;
    let vote_state = VoteState::deserialize(&vote_account.data).unwrap();
    Ok(vote_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_initialize_vote_account() {
        let vote_account_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let bogus_account_id = Keypair::new().pubkey();
        let mut bogus_account = Account::new(100, 0, &id());

        let mut keyed_accounts = [KeyedAccount::new(
            &bogus_account_id,
            false,
            &mut bogus_account,
        )];

        //init should pass
        keyed_accounts[0] = KeyedAccount::new(&vote_account_id, false, &mut vote_account);
        let res = initialize_account(&mut keyed_accounts);
        assert_eq!(res, Ok(()));

        // reinit should fail
        let res = initialize_account(&mut keyed_accounts);
        assert_eq!(res, Err(InstructionError::InvalidAccountData));
    }

    #[test]
    fn test_vote_serialize() {
        let mut buffer: Vec<u8> = vec![0; VoteState::max_size()];
        let mut vote_state = VoteState::default();
        vote_state
            .votes
            .resize(MAX_LOCKOUT_HISTORY, Lockout::default());
        vote_state.serialize(&mut buffer).unwrap();
        assert_eq!(VoteState::deserialize(&buffer).unwrap(), vote_state);
    }

    #[test]
    fn test_voter_registration() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote_state = initialize_and_deserialize(&vote_id, &mut vote_account).unwrap();
        assert_eq!(vote_state.delegate_id, vote_id);
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);
        initialize_and_deserialize(&vote_id, &mut vote_account).unwrap();

        let vote = Vote::new(1);
        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.votes, vec![Lockout::new(&vote)]);
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_vote_signature() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);
        initialize_and_deserialize(&vote_id, &mut vote_account).unwrap();

        let vote = Vote::new(1);
        let mut keyed_accounts = [KeyedAccount::new(&vote_id, false, &mut vote_account)];
        let res = process_vote(&mut keyed_accounts, vote);
        assert_eq!(res, Err(InstructionError::InvalidArgument));
    }

    #[test]
    fn test_vote_without_initialization() {
        let vote_id = Keypair::new().pubkey();
        let mut vote_account = create_vote_account(100);

        let vote = Vote::new(1);
        let res = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone());
        assert_eq!(res, Err(InstructionError::InvalidArgument));
    }

    #[test]
    fn test_vote_lockout() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(&voter_id);

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
        let top_vote = vote_state.votes.front().unwrap().slot;
        vote_state.process_vote(Vote::new(
            vote_state.votes.back().unwrap().expiration_slot(),
        ));
        assert_eq!(Some(top_vote), vote_state.root_slot);

        // Expire everything except the first vote
        let vote = Vote::new(vote_state.votes.front().unwrap().expiration_slot());
        vote_state.process_vote(vote);
        // First vote and new vote are both stored for a total of 2 votes
        assert_eq!(vote_state.votes.len(), 2);
    }

    #[test]
    fn test_vote_double_lockout_after_expiration() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(&voter_id);

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
        let mut vote_state = VoteState::new(&voter_id);

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

    #[test]
    fn test_duplicate_vote() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(&voter_id);
        vote_state.process_vote(Vote::new(0));
        vote_state.process_vote(Vote::new(1));
        vote_state.process_vote(Vote::new(0));
        assert_eq!(vote_state.nth_recent_vote(0).unwrap().slot, 1);
        assert_eq!(vote_state.nth_recent_vote(1).unwrap().slot, 0);
        assert!(vote_state.nth_recent_vote(2).is_none());
    }

    #[test]
    fn test_nth_recent_vote() {
        let voter_id = Keypair::new().pubkey();
        let mut vote_state = VoteState::new(&voter_id);
        for i in 0..MAX_LOCKOUT_HISTORY {
            vote_state.process_vote(Vote::new(i as u64));
        }
        for i in 0..(MAX_LOCKOUT_HISTORY - 1) {
            assert_eq!(
                vote_state.nth_recent_vote(i).unwrap().slot as usize,
                MAX_LOCKOUT_HISTORY - i - 1,
            );
        }
        assert!(vote_state.nth_recent_vote(MAX_LOCKOUT_HISTORY).is_none());
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
