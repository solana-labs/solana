//! Vote state, vote program
//! Receive and processes votes from validators
use crate::id;
use bincode::{deserialize, serialize_into, serialized_size, ErrorKind};
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::sysvar::slot_hashes::SlotHash;
use solana_sdk::{
    account::{Account, KeyedAccount},
    account_utils::State,
    clock::{Epoch, Slot},
    hash::Hash,
    instruction::InstructionError,
    instruction_processor_utils::DecodeError,
    pubkey::Pubkey,
    sysvar::clock::Clock,
};
use std::collections::VecDeque;

// Maximum number of votes to keep around
pub const MAX_LOCKOUT_HISTORY: usize = 31;
pub const INITIAL_LOCKOUT: usize = 2;

// Maximum number of credits history to keep around
//  smaller numbers makes
pub const MAX_EPOCH_CREDITS_HISTORY: usize = 64;

/// Reasons the stake might have had an error
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum VoteError {
    VoteTooOld,
    SlotsMismatch,
    SlotHashMismatch,
    EmptySlots,
}
impl<E> DecodeError<E> for VoteError {
    fn type_of() -> &'static str {
        "VoteError"
    }
}

impl std::fmt::Display for VoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                VoteError::VoteTooOld => "vote already recorded or not in slot hashes history",
                VoteError::SlotsMismatch => "vote slots do not match bank history",
                VoteError::SlotHashMismatch => "vote hash does not match bank hash",
                VoteError::EmptySlots => "vote has no slots, invalid",
            }
        )
    }
}
impl std::error::Error for VoteError {}

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    /// A stack of votes starting with the oldest vote
    pub slots: Vec<Slot>,
    /// signature of the bank's state at the last slot
    pub hash: Hash,
}

impl Vote {
    pub fn new(slots: Vec<Slot>, hash: Hash) -> Self {
        Self { slots, hash }
    }
}

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Lockout {
    pub slot: Slot,
    pub confirmation_count: u32,
}

impl Lockout {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            confirmation_count: 1,
        }
    }

    // The number of slots for which this vote is locked
    pub fn lockout(&self) -> u64 {
        (INITIAL_LOCKOUT as u64).pow(self.confirmation_count)
    }

    // The slot height at which this vote expires (cannot vote for any slot
    // less than this)
    pub fn expiration_slot(&self) -> Slot {
        self.slot + self.lockout()
    }
    pub fn is_expired(&self, slot: Slot) -> bool {
        self.expiration_slot() < slot
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct VoteState {
    pub votes: VecDeque<Lockout>,
    pub node_pubkey: Pubkey,
    pub authorized_voter_pubkey: Pubkey,
    /// fraction of std::u8::MAX that represents what part of a rewards
    ///  payout should be given to this VoteAccount
    pub commission: u8,
    pub root_slot: Option<u64>,

    /// clock epoch
    epoch: Epoch,
    /// clock credits earned, monotonically increasing
    credits: u64,

    /// credits as of previous epoch
    last_epoch_credits: u64,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    epoch_credits: Vec<(Epoch, u64, u64)>,
}

impl VoteState {
    pub fn new(vote_pubkey: &Pubkey, node_pubkey: &Pubkey, commission: u8) -> Self {
        Self {
            node_pubkey: *node_pubkey,
            authorized_voter_pubkey: *vote_pubkey,
            commission,
            ..VoteState::default()
        }
    }

    pub fn size_of() -> usize {
        // Upper limit on the size of the Vote State. Equal to
        // size_of(VoteState) when votes.len() is MAX_LOCKOUT_HISTORY
        let mut vote_state = Self::default();
        vote_state.votes = VecDeque::from(vec![Lockout::default(); MAX_LOCKOUT_HISTORY]);
        vote_state.root_slot = Some(std::u64::MAX);
        vote_state.epoch_credits = vec![(0, 0, 0); MAX_EPOCH_CREDITS_HISTORY];
        serialized_size(&vote_state).unwrap() as usize
    }

    // utility function, used by Stakes, tests
    pub fn from(account: &Account) -> Option<VoteState> {
        Self::deserialize(&account.data).ok()
    }

    // utility function, used by Stakes, tests
    pub fn to(&self, account: &mut Account) -> Option<()> {
        Self::serialize(self, &mut account.data).ok()
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

    // utility function, used by Stakes, tests
    pub fn credits_from(account: &Account) -> Option<u64> {
        Self::from(account).map(|state| state.credits())
    }

    /// returns commission split as (voter_portion, staker_portion, was_split) tuple
    ///
    ///  if commission calculation is 100% one way or other,
    ///   indicate with false for was_split
    pub fn commission_split(&self, on: f64) -> (f64, f64, bool) {
        match self.commission {
            0 => (0.0, on, false),
            std::u8::MAX => (on, 0.0, false),
            split => {
                let mine = on * f64::from(split) / f64::from(std::u8::MAX);
                (mine, on - mine, true)
            }
        }
    }
    fn check_slots_are_valid(
        &self,
        vote: &Vote,
        slot_hashes: &[(Slot, Hash)],
    ) -> Result<(), VoteError> {
        let mut i = 0; // index into the vote's slots
        let mut j = slot_hashes.len(); // index into the slot_hashes
        while i < vote.slots.len() && j > 0 {
            // find the most recent "new" slot in the vote
            if self
                .votes
                .back()
                .map_or(false, |old_vote| old_vote.slot >= vote.slots[i])
            {
                i += 1;
                continue;
            }
            if vote.slots[i] != slot_hashes[j - 1].0 {
                j -= 1;
                continue;
            }
            i += 1;
            j -= 1;
        }
        if j == slot_hashes.len() {
            warn!(
                "{} dropped vote {:?} too old: {:?} ",
                self.node_pubkey, vote, slot_hashes
            );
            return Err(VoteError::VoteTooOld);
        }
        if i != vote.slots.len() {
            warn!(
                "{} dropped vote {:?} failed to match slot:  {:?}",
                self.node_pubkey, vote, slot_hashes,
            );
            return Err(VoteError::SlotsMismatch);
        }
        if slot_hashes[j].1 != vote.hash {
            warn!(
                "{} dropped vote {:?} failed to match hash {} {}",
                self.node_pubkey, vote, vote.hash, slot_hashes[j].1
            );
            return Err(VoteError::SlotHashMismatch);
        }
        Ok(())
    }
    pub fn process_vote(
        &mut self,
        vote: &Vote,
        slot_hashes: &[SlotHash],
        epoch: Epoch,
    ) -> Result<(), VoteError> {
        if vote.slots.is_empty() {
            return Err(VoteError::EmptySlots);
        }
        self.check_slots_are_valid(vote, slot_hashes)?;

        vote.slots.iter().for_each(|s| self.process_slot(*s, epoch));
        Ok(())
    }

    pub fn process_slot(&mut self, slot: Slot, epoch: Epoch) {
        // Ignore votes for slots earlier than we already have votes for
        if self
            .votes
            .back()
            .map_or(false, |old_vote| old_vote.slot >= slot)
        {
            return;
        }

        let vote = Lockout::new(slot);

        self.pop_expired_votes(slot);

        // Once the stack is full, pop the oldest lockout and distribute rewards
        if self.votes.len() == MAX_LOCKOUT_HISTORY {
            let vote = self.votes.pop_front().unwrap();
            self.root_slot = Some(vote.slot);

            self.increment_credits(epoch);
        }
        self.votes.push_back(vote);
        self.double_lockouts();
    }

    /// increment credits, record credits for last epoch if new epoch
    pub fn increment_credits(&mut self, epoch: Epoch) {
        // record credits by epoch

        if epoch != self.epoch {
            // encode the delta, but be able to return partial for stakers who
            //   attach halfway through an epoch
            self.epoch_credits
                .push((self.epoch, self.credits, self.last_epoch_credits));
            // if stakers do not claim before the epoch goes away they lose the
            //  credits...
            if self.epoch_credits.len() > MAX_EPOCH_CREDITS_HISTORY {
                self.epoch_credits.remove(0);
            }
            self.epoch = epoch;
            self.last_epoch_credits = self.credits;
        }

        self.credits += 1;
    }

    /// "unchecked" functions used by tests and Tower
    pub fn process_vote_unchecked(&mut self, vote: &Vote) {
        let slot_hashes: Vec<_> = vote.slots.iter().rev().map(|x| (*x, vote.hash)).collect();
        let _ignored = self.process_vote(vote, &slot_hashes, self.epoch);
    }
    pub fn process_slot_vote_unchecked(&mut self, slot: Slot) {
        self.process_vote_unchecked(&Vote::new(vec![slot], Hash::default()));
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

    /// Number of "credits" owed to this account from the mining pool on a per-epoch basis,
    ///  starting from credits observed.
    /// Each tuple of (Epoch, u64, u64) is read as (epoch, credits, prev_credits), where
    ///   credits for each epoch is credits - prev_credits; while redundant this makes
    ///   calculating rewards over partial epochs nice and simple
    pub fn epoch_credits(&self) -> impl Iterator<Item = &(Epoch, u64, u64)> {
        self.epoch_credits.iter()
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
            }
        }
    }
}

/// Authorize the given pubkey to sign votes. This may be called multiple times,
/// but will implicitly withdraw authorization from the previously authorized
/// voter. The default voter is the owner of the vote account's pubkey.
pub fn authorize_voter(
    vote_account: &mut KeyedAccount,
    other_signers: &[KeyedAccount],
    authorized_voter_pubkey: &Pubkey,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState = vote_account.state()?;

    // clock authorized signer must say "yay"
    let authorized = Some(&vote_state.authorized_voter_pubkey);
    if vote_account.signer_key() != authorized
        && other_signers
            .iter()
            .all(|account| account.signer_key() != authorized)
    {
        return Err(InstructionError::MissingRequiredSignature);
    }

    vote_state.authorized_voter_pubkey = *authorized_voter_pubkey;
    vote_account.set_state(&vote_state)
}

/// Withdraw funds from the vote account
pub fn withdraw(
    vote_account: &mut KeyedAccount,
    lamports: u64,
    to_account: &mut KeyedAccount,
) -> Result<(), InstructionError> {
    if vote_account.signer_key().is_none() {
        return Err(InstructionError::MissingRequiredSignature);
    }
    if vote_account.account.lamports < lamports {
        return Err(InstructionError::InsufficientFunds);
    }
    vote_account.account.lamports -= lamports;
    to_account.account.lamports += lamports;
    Ok(())
}

/// Initialize the vote_state for a vote account
/// Assumes that the account is being init as part of a account creation or balance transfer and
/// that the transaction must be signed by the staker's keys
pub fn initialize_account(
    vote_account: &mut KeyedAccount,
    node_pubkey: &Pubkey,
    commission: u8,
) -> Result<(), InstructionError> {
    let vote_state: VoteState = vote_account.state()?;

    if vote_state.authorized_voter_pubkey != Pubkey::default() {
        return Err(InstructionError::AccountAlreadyInitialized);
    }
    vote_account.set_state(&VoteState::new(
        vote_account.unsigned_key(),
        node_pubkey,
        commission,
    ))
}

pub fn process_vote(
    vote_account: &mut KeyedAccount,
    slot_hashes: &[SlotHash],
    clock: &Clock,
    other_signers: &[KeyedAccount],
    vote: &Vote,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState = vote_account.state()?;

    if vote_state.authorized_voter_pubkey == Pubkey::default() {
        return Err(InstructionError::UninitializedAccount);
    }

    let authorized = Some(&vote_state.authorized_voter_pubkey);
    // find a signer that matches the authorized_voter_pubkey
    if vote_account.signer_key() != authorized
        && other_signers
            .iter()
            .all(|account| account.signer_key() != authorized)
    {
        return Err(InstructionError::MissingRequiredSignature);
    }

    vote_state.process_vote(vote, slot_hashes, clock.epoch)?;
    vote_account.set_state(&vote_state)
}

// utility function, used by Bank, tests
pub fn create_account(
    vote_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
    commission: u8,
    lamports: u64,
) -> Account {
    let mut vote_account = Account::new(lamports, VoteState::size_of(), &id());

    VoteState::new(vote_pubkey, node_pubkey, commission)
        .to(&mut vote_account)
        .unwrap();

    vote_account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vote_state;
    use solana_sdk::account::Account;
    use solana_sdk::account_utils::State;
    use solana_sdk::hash::hash;

    const MAX_RECENT_VOTES: usize = 16;

    #[test]
    fn test_initialize_vote_account() {
        let vote_account_pubkey = Pubkey::new_rand();
        let mut vote_account = Account::new(100, VoteState::size_of(), &id());

        let node_pubkey = Pubkey::new_rand();

        //init should pass
        let mut vote_account = KeyedAccount::new(&vote_account_pubkey, false, &mut vote_account);
        let res = initialize_account(&mut vote_account, &node_pubkey, 0);
        assert_eq!(res, Ok(()));

        // reinit should fail
        let res = initialize_account(&mut vote_account, &node_pubkey, 0);
        assert_eq!(res, Err(InstructionError::AccountAlreadyInitialized));
    }

    fn create_test_account() -> (Pubkey, Account) {
        let vote_pubkey = Pubkey::new_rand();
        (
            vote_pubkey,
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100),
        )
    }

    fn simulate_process_vote(
        vote_pubkey: &Pubkey,
        vote_account: &mut Account,
        vote: &Vote,
        slot_hashes: &[SlotHash],
        epoch: u64,
    ) -> Result<VoteState, InstructionError> {
        process_vote(
            &mut KeyedAccount::new(vote_pubkey, true, vote_account),
            slot_hashes,
            &Clock {
                epoch,
                ..Clock::default()
            },
            &[],
            &vote.clone(),
        )?;
        vote_account.state()
    }

    /// exercises all the keyed accounts stuff
    fn simulate_process_vote_unchecked(
        vote_pubkey: &Pubkey,
        vote_account: &mut Account,
        vote: &Vote,
    ) -> Result<VoteState, InstructionError> {
        simulate_process_vote(
            vote_pubkey,
            vote_account,
            vote,
            &[(*vote.slots.last().unwrap(), vote.hash)],
            0,
        )
    }

    #[test]
    fn test_vote_serialize() {
        let mut buffer: Vec<u8> = vec![0; VoteState::size_of()];
        let mut vote_state = VoteState::default();
        vote_state
            .votes
            .resize(MAX_LOCKOUT_HISTORY, Lockout::default());
        assert!(vote_state.serialize(&mut buffer[0..4]).is_err());
        vote_state.serialize(&mut buffer).unwrap();
        assert_eq!(VoteState::deserialize(&buffer).unwrap(), vote_state);
    }

    #[test]
    fn test_voter_registration() {
        let (vote_pubkey, vote_account) = create_test_account();

        let vote_state: VoteState = vote_account.state().unwrap();
        assert_eq!(vote_state.authorized_voter_pubkey, vote_pubkey);
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let (vote_pubkey, mut vote_account) = create_test_account();

        let vote = Vote::new(vec![1], Hash::default());
        let vote_state =
            simulate_process_vote_unchecked(&vote_pubkey, &mut vote_account, &vote).unwrap();
        assert_eq!(
            vote_state.votes,
            vec![Lockout::new(*vote.slots.last().unwrap())]
        );
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_vote_slot_hashes() {
        let (vote_pubkey, mut vote_account) = create_test_account();

        let hash = hash(&[0u8]);
        let vote = Vote::new(vec![0], hash);

        // wrong hash
        assert_eq!(
            simulate_process_vote(
                &vote_pubkey,
                &mut vote_account,
                &vote,
                &[(0, Hash::default())],
                0,
            ),
            Err(VoteError::SlotHashMismatch.into())
        );

        // wrong slot
        assert_eq!(
            simulate_process_vote(&vote_pubkey, &mut vote_account, &vote, &[(1, hash)], 0),
            Err(VoteError::SlotsMismatch.into())
        );

        // empty slot_hashes
        assert_eq!(
            simulate_process_vote(&vote_pubkey, &mut vote_account, &vote, &[], 0),
            Err(VoteError::VoteTooOld.into())
        );
    }

    #[test]
    fn test_vote_signature() {
        let (vote_pubkey, mut vote_account) = create_test_account();

        let vote = Vote::new(vec![1], Hash::default());

        // unsigned
        let res = process_vote(
            &mut KeyedAccount::new(&vote_pubkey, false, &mut vote_account),
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock::default(),
            &[],
            &vote,
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        // unsigned
        let res = process_vote(
            &mut KeyedAccount::new(&vote_pubkey, true, &mut vote_account),
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock::default(),
            &[],
            &vote,
        );
        assert_eq!(res, Ok(()));

        // another voter
        let authorized_voter_pubkey = Pubkey::new_rand();
        let res = authorize_voter(
            &mut KeyedAccount::new(&vote_pubkey, false, &mut vote_account),
            &[],
            &authorized_voter_pubkey,
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        let res = authorize_voter(
            &mut KeyedAccount::new(&vote_pubkey, true, &mut vote_account),
            &[],
            &authorized_voter_pubkey,
        );
        assert_eq!(res, Ok(()));
        // verify authorized_voter_pubkey can authorize authorized_voter_pubkey ;)
        let res = authorize_voter(
            &mut KeyedAccount::new(&vote_pubkey, false, &mut vote_account),
            &[KeyedAccount::new(
                &authorized_voter_pubkey,
                true,
                &mut Account::default(),
            )],
            &authorized_voter_pubkey,
        );
        assert_eq!(res, Ok(()));

        // not signed by authorized voter
        let vote = Vote::new(vec![2], Hash::default());
        let res = process_vote(
            &mut KeyedAccount::new(&vote_pubkey, true, &mut vote_account),
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock::default(),
            &[],
            &vote,
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        // signed by authorized voter
        let vote = Vote::new(vec![2], Hash::default());
        let res = process_vote(
            &mut KeyedAccount::new(&vote_pubkey, false, &mut vote_account),
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock::default(),
            &[KeyedAccount::new(
                &authorized_voter_pubkey,
                true,
                &mut Account::default(),
            )],
            &vote,
        );
        assert_eq!(res, Ok(()));
    }

    #[test]
    fn test_vote_without_initialization() {
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account = Account::new(100, VoteState::size_of(), &id());

        let res = simulate_process_vote_unchecked(
            &vote_pubkey,
            &mut vote_account,
            &Vote::new(vec![1], Hash::default()),
        );
        assert_eq!(res, Err(InstructionError::UninitializedAccount));
    }

    #[test]
    fn test_vote_lockout() {
        let (_vote_pubkey, vote_account) = create_test_account();

        let mut vote_state: VoteState = vote_account.state().unwrap();

        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            vote_state.process_slot_vote_unchecked((INITIAL_LOCKOUT as usize * i) as u64);
        }

        // The last vote should have been popped b/c it reached a depth of MAX_LOCKOUT_HISTORY
        assert_eq!(vote_state.votes.len(), MAX_LOCKOUT_HISTORY);
        assert_eq!(vote_state.root_slot, Some(0));
        check_lockouts(&vote_state);

        // One more vote that confirms the entire stack,
        // the root_slot should change to the
        // second vote
        let top_vote = vote_state.votes.front().unwrap().slot;
        vote_state.process_slot_vote_unchecked(vote_state.votes.back().unwrap().expiration_slot());
        assert_eq!(Some(top_vote), vote_state.root_slot);

        // Expire everything except the first vote
        vote_state.process_slot_vote_unchecked(vote_state.votes.front().unwrap().expiration_slot());
        // First vote and new vote are both stored for a total of 2 votes
        assert_eq!(vote_state.votes.len(), 2);
    }

    #[test]
    fn test_vote_double_lockout_after_expiration() {
        let voter_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::new(&voter_pubkey, &Pubkey::new_rand(), 0);

        for i in 0..3 {
            vote_state.process_slot_vote_unchecked(i as u64);
        }

        check_lockouts(&vote_state);

        // Expire the third vote (which was a vote for slot 2). The height of the
        // vote stack is unchanged, so none of the previous votes should have
        // doubled in lockout
        vote_state.process_slot_vote_unchecked((2 + INITIAL_LOCKOUT + 1) as u64);
        check_lockouts(&vote_state);

        // Vote again, this time the vote stack depth increases, so the votes should
        // double for everybody
        vote_state.process_slot_vote_unchecked((2 + INITIAL_LOCKOUT + 2) as u64);
        check_lockouts(&vote_state);

        // Vote again, this time the vote stack depth increases, so the votes should
        // double for everybody
        vote_state.process_slot_vote_unchecked((2 + INITIAL_LOCKOUT + 3) as u64);
        check_lockouts(&vote_state);
    }

    #[test]
    fn test_expire_multiple_votes() {
        let voter_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::new(&voter_pubkey, &Pubkey::new_rand(), 0);

        for i in 0..3 {
            vote_state.process_slot_vote_unchecked(i as u64);
        }

        assert_eq!(vote_state.votes[0].confirmation_count, 3);

        // Expire the second and third votes
        let expire_slot = vote_state.votes[1].slot + vote_state.votes[1].lockout() + 1;
        vote_state.process_slot_vote_unchecked(expire_slot);
        assert_eq!(vote_state.votes.len(), 2);

        // Check that the old votes expired
        assert_eq!(vote_state.votes[0].slot, 0);
        assert_eq!(vote_state.votes[1].slot, expire_slot);

        // Process one more vote
        vote_state.process_slot_vote_unchecked(expire_slot + 1);

        // Confirmation count for the older first vote should remain unchanged
        assert_eq!(vote_state.votes[0].confirmation_count, 3);

        // The later votes should still have increasing confirmation counts
        assert_eq!(vote_state.votes[1].confirmation_count, 2);
        assert_eq!(vote_state.votes[2].confirmation_count, 1);
    }

    #[test]
    fn test_vote_credits() {
        let voter_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::new(&voter_pubkey, &Pubkey::new_rand(), 0);

        for i in 0..MAX_LOCKOUT_HISTORY {
            vote_state.process_slot_vote_unchecked(i as u64);
        }

        assert_eq!(vote_state.credits, 0);

        vote_state.process_slot_vote_unchecked(MAX_LOCKOUT_HISTORY as u64 + 1);
        assert_eq!(vote_state.credits, 1);
        vote_state.process_slot_vote_unchecked(MAX_LOCKOUT_HISTORY as u64 + 2);
        assert_eq!(vote_state.credits(), 2);
        vote_state.process_slot_vote_unchecked(MAX_LOCKOUT_HISTORY as u64 + 3);
        assert_eq!(vote_state.credits(), 3);
    }

    #[test]
    fn test_duplicate_vote() {
        let voter_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::new(&voter_pubkey, &Pubkey::new_rand(), 0);
        vote_state.process_slot_vote_unchecked(0);
        vote_state.process_slot_vote_unchecked(1);
        vote_state.process_slot_vote_unchecked(0);
        assert_eq!(vote_state.nth_recent_vote(0).unwrap().slot, 1);
        assert_eq!(vote_state.nth_recent_vote(1).unwrap().slot, 0);
        assert!(vote_state.nth_recent_vote(2).is_none());
    }

    #[test]
    fn test_nth_recent_vote() {
        let voter_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::new(&voter_pubkey, &Pubkey::new_rand(), 0);
        for i in 0..MAX_LOCKOUT_HISTORY {
            vote_state.process_slot_vote_unchecked(i as u64);
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
            let num_votes = vote_state.votes.len() - i;
            assert_eq!(vote.lockout(), INITIAL_LOCKOUT.pow(num_votes as u32) as u64);
        }
    }

    fn recent_votes(vote_state: &VoteState) -> Vec<Vote> {
        let start = vote_state.votes.len().saturating_sub(MAX_RECENT_VOTES);
        (start..vote_state.votes.len())
            .map(|i| Vote::new(vec![vote_state.votes.get(i).unwrap().slot], Hash::default()))
            .collect()
    }

    /// check that two accounts with different data can be brought to the same state with one vote submission
    #[test]
    fn test_process_missed_votes() {
        let account_a = Pubkey::new_rand();
        let mut vote_state_a = VoteState::new(&account_a, &Pubkey::new_rand(), 0);
        let account_b = Pubkey::new_rand();
        let mut vote_state_b = VoteState::new(&account_b, &Pubkey::new_rand(), 0);

        // process some votes on account a
        (0..5)
            .into_iter()
            .for_each(|i| vote_state_a.process_slot_vote_unchecked(i as u64));
        assert_ne!(recent_votes(&vote_state_a), recent_votes(&vote_state_b));

        // as long as b has missed less than "NUM_RECENT" votes both accounts should be in sync
        let slots = (0u64..MAX_RECENT_VOTES as u64).into_iter().collect();
        let vote = Vote::new(slots, Hash::default());
        let slot_hashes: Vec<_> = vote.slots.iter().rev().map(|x| (*x, vote.hash)).collect();

        assert_eq!(vote_state_a.process_vote(&vote, &slot_hashes, 0), Ok(()));
        assert_eq!(vote_state_b.process_vote(&vote, &slot_hashes, 0), Ok(()));
        assert_eq!(recent_votes(&vote_state_a), recent_votes(&vote_state_b));
    }

    #[test]
    fn test_process_vote_skips_old_vote() {
        let mut vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(0, vote.hash)];
        assert_eq!(vote_state.process_vote(&vote, &slot_hashes, 0), Ok(()));
        let recent = recent_votes(&vote_state);
        assert_eq!(
            vote_state.process_vote(&vote, &slot_hashes, 0),
            Err(VoteError::VoteTooOld)
        );
        assert_eq!(recent, recent_votes(&vote_state));
    }

    #[test]
    fn test_check_slots_are_valid_vote_empty_slot_hashes() {
        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &vec![]),
            Err(VoteError::VoteTooOld)
        );
    }

    #[test]
    fn test_check_slots_are_valid_new_vote() {
        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Ok(())
        );
    }

    #[test]
    fn test_check_slots_are_valid_bad_hash() {
        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), hash(vote.hash.as_ref()))];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Err(VoteError::SlotHashMismatch)
        );
    }

    #[test]
    fn test_check_slots_are_valid_bad_slot() {
        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![1], Hash::default());
        let slot_hashes: Vec<_> = vec![(0, vote.hash)];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Err(VoteError::SlotsMismatch)
        );
    }

    #[test]
    fn test_check_slots_are_valid_duplicate_vote() {
        let mut vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(vote_state.process_vote(&vote, &slot_hashes, 0), Ok(()));
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Err(VoteError::VoteTooOld)
        );
    }

    #[test]
    fn test_check_slots_are_valid_next_vote() {
        let mut vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(vote_state.process_vote(&vote, &slot_hashes, 0), Ok(()));

        let vote = Vote::new(vec![0, 1], Hash::default());
        let slot_hashes: Vec<_> = vec![(1, vote.hash), (0, vote.hash)];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Ok(())
        );
    }

    #[test]
    fn test_check_slots_are_valid_next_vote_only() {
        let mut vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(vote_state.process_vote(&vote, &slot_hashes, 0), Ok(()));

        let vote = Vote::new(vec![1], Hash::default());
        let slot_hashes: Vec<_> = vec![(1, vote.hash), (0, vote.hash)];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Ok(())
        );
    }

    #[test]
    fn test_vote_state_commission_split() {
        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), 0);

        assert_eq!(vote_state.commission_split(1.0), (0.0, 1.0, false));

        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), std::u8::MAX);
        assert_eq!(vote_state.commission_split(1.0), (1.0, 0.0, false));

        let vote_state = VoteState::new(&Pubkey::default(), &Pubkey::default(), std::u8::MAX / 2);
        let (voter_portion, staker_portion, was_split) = vote_state.commission_split(10.0);

        assert_eq!(
            (voter_portion.round(), staker_portion.round(), was_split),
            (5.0, 5.0, true)
        );
    }

    #[test]
    fn test_vote_state_withdraw() {
        let (vote_pubkey, mut vote_account) = create_test_account();

        // unsigned
        let res = withdraw(
            &mut KeyedAccount::new(&vote_pubkey, false, &mut vote_account),
            0,
            &mut KeyedAccount::new(&Pubkey::new_rand(), false, &mut Account::default()),
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        // insufficient funds
        let res = withdraw(
            &mut KeyedAccount::new(&vote_pubkey, true, &mut vote_account),
            101,
            &mut KeyedAccount::new(&Pubkey::new_rand(), false, &mut Account::default()),
        );
        assert_eq!(res, Err(InstructionError::InsufficientFunds));

        // all good
        let mut to_account = Account::default();
        let lamports = vote_account.lamports;
        let res = withdraw(
            &mut KeyedAccount::new(&vote_pubkey, true, &mut vote_account),
            lamports,
            &mut KeyedAccount::new(&Pubkey::new_rand(), false, &mut to_account),
        );
        assert_eq!(res, Ok(()));
        assert_eq!(vote_account.lamports, 0);
        assert_eq!(to_account.lamports, lamports);
    }

    #[test]
    fn test_vote_state_epoch_credits() {
        let mut vote_state = VoteState::default();

        assert_eq!(vote_state.credits(), 0);
        assert_eq!(
            vote_state
                .epoch_credits()
                .cloned()
                .collect::<Vec<(Epoch, u64, u64)>>(),
            vec![]
        );

        let mut expected = vec![];
        let mut credits = 0;
        let epochs = (MAX_EPOCH_CREDITS_HISTORY + 2) as u64;
        for epoch in 0..epochs {
            for _j in 0..epoch {
                vote_state.increment_credits(epoch);
                credits += 1;
            }
            expected.push((epoch, credits, credits - epoch));
        }
        expected.pop(); // last one doesn't count, doesn't get saved off
        while expected.len() > MAX_EPOCH_CREDITS_HISTORY {
            expected.remove(0);
        }

        assert_eq!(vote_state.credits(), credits);
        assert_eq!(
            vote_state
                .epoch_credits()
                .cloned()
                .collect::<Vec<(Epoch, u64, u64)>>(),
            expected
        );
    }

}
