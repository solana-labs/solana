//! Vote state, vote program
//! Receive and processes votes from validators
use crate::authorized_voters::AuthorizedVoters;
use crate::{id, vote_instruction::VoteError};
use bincode::{deserialize, serialize_into, serialized_size, ErrorKind};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::{Account, KeyedAccount},
    account_utils::State,
    clock::{Epoch, Slot, UnixTimestamp},
    epoch_schedule::MAX_LEADER_SCHEDULE_EPOCH_OFFSET,
    hash::Hash,
    instruction::InstructionError,
    pubkey::Pubkey,
    rent::Rent,
    slot_hashes::SlotHash,
    sysvar::clock::Clock,
};
use std::boxed::Box;
use std::collections::{HashSet, VecDeque};

mod vote_state_0_23_5;
pub mod vote_state_versions;
pub use vote_state_versions::*;

// Maximum number of votes to keep around, tightly coupled with epoch_schedule::MIN_SLOTS_PER_EPOCH
pub const MAX_LOCKOUT_HISTORY: usize = 31;
pub const INITIAL_LOCKOUT: usize = 2;

// Maximum number of credits history to keep around
//  smaller numbers makes
pub const MAX_EPOCH_CREDITS_HISTORY: usize = 64;

// Frequency of timestamp Votes. In v0.22.0, this is approximately 30min with cluster clock
// defaults, intended to limit block time drift to < 1hr
pub const TIMESTAMP_SLOT_INTERVAL: u64 = 4500;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    /// A stack of votes starting with the oldest vote
    pub slots: Vec<Slot>,
    /// signature of the bank's state at the last slot
    pub hash: Hash,
    /// processing timestamp of last slot
    pub timestamp: Option<UnixTimestamp>,
}

impl Vote {
    pub fn new(slots: Vec<Slot>, hash: Hash) -> Self {
        Self {
            slots,
            hash,
            timestamp: None,
        }
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

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct VoteInit {
    pub node_pubkey: Pubkey,
    pub authorized_voter: Pubkey,
    pub authorized_withdrawer: Pubkey,
    pub commission: u8,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum VoteAuthorize {
    Voter,
    Withdrawer,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct BlockTimestamp {
    pub slot: Slot,
    pub timestamp: UnixTimestamp,
}

// this is how many epochs a voter can be remembered for slashing
const MAX_ITEMS: usize = 32;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct CircBuf<I> {
    buf: [I; MAX_ITEMS],
    /// next pointer
    idx: usize,
    is_empty: bool,
}

impl<I: Default + Copy> Default for CircBuf<I> {
    fn default() -> Self {
        Self {
            buf: [I::default(); MAX_ITEMS],
            idx: MAX_ITEMS - 1,
            is_empty: true,
        }
    }
}

impl<I> CircBuf<I> {
    pub fn append(&mut self, item: I) {
        // remember prior delegate and when we switched, to support later slashing
        self.idx += 1;
        self.idx %= MAX_ITEMS;

        self.buf[self.idx] = item;
        self.is_empty = false;
    }

    pub fn buf(&self) -> &[I; MAX_ITEMS] {
        &self.buf
    }

    pub fn last(&self) -> Option<&I> {
        if !self.is_empty {
            Some(&self.buf[self.idx])
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct VoteState {
    /// the node that votes in this account
    pub node_pubkey: Pubkey,

    /// the signer for withdrawals
    pub authorized_withdrawer: Pubkey,
    /// percentage (0-100) that represents what part of a rewards
    ///  payout should be given to this VoteAccount
    pub commission: u8,

    pub votes: VecDeque<Lockout>,
    pub root_slot: Option<u64>,

    /// the signer for vote transactions
    authorized_voters: AuthorizedVoters,

    /// history of prior authorized voters and the epochs for which
    /// they were set, the bottom end of the range is inclusive,
    /// the top of the range is exclusive
    prior_voters: CircBuf<(Pubkey, Epoch, Epoch)>,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    epoch_credits: Vec<(Epoch, u64, u64)>,

    /// most recent timestamp submitted with a vote
    pub last_timestamp: BlockTimestamp,
}

impl VoteState {
    pub fn new(vote_init: &VoteInit, clock: &Clock) -> Self {
        Self {
            node_pubkey: vote_init.node_pubkey,
            authorized_voters: AuthorizedVoters::new(clock.epoch, vote_init.authorized_voter),
            authorized_withdrawer: vote_init.authorized_withdrawer,
            commission: vote_init.commission,
            ..VoteState::default()
        }
    }

    pub fn get_authorized_voter(&self, epoch: u64) -> Option<Pubkey> {
        self.authorized_voters.get_authorized_voter(epoch)
    }

    pub fn authorized_voters(&self) -> &AuthorizedVoters {
        &self.authorized_voters
    }

    pub fn prior_voters(&mut self) -> &CircBuf<(Pubkey, Epoch, Epoch)> {
        &self.prior_voters
    }

    pub fn get_rent_exempt_reserve(rent: &Rent) -> u64 {
        rent.minimum_balance(VoteState::size_of())
    }

    pub fn size_of() -> usize {
        // Upper limit on the size of the Vote State. Equal to
        // size_of(VoteState) when votes.len() is MAX_LOCKOUT_HISTORY
        let vote_state = VoteStateVersions::Current(Box::new(Self::get_max_sized_vote_state()));
        serialized_size(&vote_state).unwrap() as usize
    }

    // utility function, used by Stakes, tests
    pub fn from(account: &Account) -> Option<VoteState> {
        Self::deserialize(&account.data).ok()
    }

    // utility function, used by Stakes, tests
    pub fn to(versioned: &VoteStateVersions, account: &mut Account) -> Option<()> {
        Self::serialize(versioned, &mut account.data).ok()
    }

    pub fn deserialize(input: &[u8]) -> Result<Self, InstructionError> {
        deserialize::<VoteStateVersions>(&input)
            .map(|versioned| versioned.convert_to_current())
            .map_err(|_| InstructionError::InvalidAccountData)
    }

    pub fn serialize(
        versioned: &VoteStateVersions,
        output: &mut [u8],
    ) -> Result<(), InstructionError> {
        serialize_into(output, versioned).map_err(|err| match *err {
            ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
            _ => InstructionError::GenericError,
        })
    }

    pub fn credits_from(account: &Account) -> Option<u64> {
        Self::from(account).map(|state| state.credits())
    }

    /// returns commission split as (voter_portion, staker_portion, was_split) tuple
    ///
    ///  if commission calculation is 100% one way or other,
    ///   indicate with false for was_split
    pub fn commission_split(&self, on: f64) -> (f64, f64, bool) {
        match self.commission.min(100) {
            0 => (0.0, on, false),
            100 => (on, 0.0, false),
            split => {
                let mine = on * f64::from(split) / f64::from(100);
                (mine, on - mine, true)
            }
        }
    }

    fn get_max_sized_vote_state() -> VoteState {
        let mut vote_state = Self::default();
        vote_state.votes = VecDeque::from(vec![Lockout::default(); MAX_LOCKOUT_HISTORY]);
        vote_state.root_slot = Some(std::u64::MAX);
        vote_state.epoch_credits = vec![(0, 0, 0); MAX_EPOCH_CREDITS_HISTORY];
        let mut authorized_voters = AuthorizedVoters::default();
        for i in 0..=MAX_LEADER_SCHEDULE_EPOCH_OFFSET {
            authorized_voters.insert(i, Pubkey::new_rand());
        }
        vote_state.authorized_voters = authorized_voters;
        vote_state
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
            debug!(
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
        // increment credits, record by epoch

        // never seen a credit
        if self.epoch_credits.is_empty() {
            self.epoch_credits.push((epoch, 0, 0));
        } else if epoch != self.epoch_credits.last().unwrap().0 {
            let (_, credits, prev_credits) = *self.epoch_credits.last().unwrap();

            if credits != prev_credits {
                // if credits were earned previous epoch
                // append entry at end of list for the new epoch
                self.epoch_credits.push((epoch, credits, credits));
            } else {
                // else just move the current epoch
                self.epoch_credits.last_mut().unwrap().0 = epoch;
            }

            // if stakers do not claim before the epoch goes away they lose the
            //  credits...
            if self.epoch_credits.len() > MAX_EPOCH_CREDITS_HISTORY {
                self.epoch_credits.remove(0);
            }
        }

        self.epoch_credits.last_mut().unwrap().1 += 1;
    }

    /// "unchecked" functions used by tests and Tower
    pub fn process_vote_unchecked(&mut self, vote: &Vote) {
        let slot_hashes: Vec<_> = vote.slots.iter().rev().map(|x| (*x, vote.hash)).collect();
        let _ignored = self.process_vote(vote, &slot_hashes, self.current_epoch());
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

    fn current_epoch(&self) -> Epoch {
        if self.epoch_credits.is_empty() {
            0
        } else {
            self.epoch_credits.last().unwrap().0
        }
    }

    /// Number of "credits" owed to this account from the mining pool. Submit this
    /// VoteState to the Rewards program to trade credits for lamports.
    pub fn credits(&self) -> u64 {
        if self.epoch_credits.is_empty() {
            0
        } else {
            self.epoch_credits.last().unwrap().1
        }
    }

    /// Number of "credits" owed to this account from the mining pool on a per-epoch basis,
    ///  starting from credits observed.
    /// Each tuple of (Epoch, u64, u64) is read as (epoch, credits, prev_credits), where
    ///   credits for each epoch is credits - prev_credits; while redundant this makes
    ///   calculating rewards over partial epochs nice and simple
    pub fn epoch_credits(&self) -> &Vec<(Epoch, u64, u64)> {
        &self.epoch_credits
    }

    fn set_new_authorized_voter<F>(
        &mut self,
        authorized_pubkey: &Pubkey,
        current_epoch: u64,
        target_epoch: u64,
        verify: F,
    ) -> Result<(), InstructionError>
    where
        F: Fn(Pubkey) -> Result<(), InstructionError>,
    {
        let epoch_authorized_voter = self.get_and_update_authorized_voter(current_epoch).expect(
            "the clock epoch is monotonically increasing, so authorized voter must be known",
        );

        verify(epoch_authorized_voter)?;

        // The offset in slots `n` on which the target_epoch
        // (default value `DEFAULT_LEADER_SCHEDULE_SLOT_OFFSET`) is
        // calculated is the number of slots available from the
        // first slot `S` of an epoch in which to set a new voter for
        // the epoch at `S` + `n`
        if self.authorized_voters.contains(target_epoch) {
            return Err(VoteError::TooSoonToReauthorize.into());
        }

        // Get the latest authorized_voter
        let (latest_epoch, latest_authorized_pubkey) = self.authorized_voters.last().expect(
            "Earlier call to `get_and_update_authorized_voter()` guarantees
            at least the voter for `epoch` exists in the map",
        );

        // If we're not setting the same pubkey as authorized pubkey again,
        // then update the list of prior voters to mark the expiration
        // of the old authorized pubkey
        if latest_authorized_pubkey != authorized_pubkey {
            // Update the epoch ranges of authorized pubkeys that will be expired
            let epoch_of_last_authorized_switch =
                self.prior_voters.last().map(|range| range.2).unwrap_or(0);

            // target_epoch must:
            // 1) Be monotonically increasing due to the clock always
            //    moving forward
            // 2) not be equal to latest epoch otherwise this
            //    function would have returned TooSoonToReauthorize error
            //    above
            assert!(target_epoch > *latest_epoch);

            // Commit the new state
            self.prior_voters.append((
                *latest_authorized_pubkey,
                epoch_of_last_authorized_switch,
                target_epoch,
            ));
        }

        self.authorized_voters
            .insert(target_epoch, *authorized_pubkey);

        Ok(())
    }

    fn get_and_update_authorized_voter(&mut self, current_epoch: u64) -> Option<Pubkey> {
        let pubkey = self
            .authorized_voters
            .get_and_cache_authorized_voter_for_epoch(current_epoch)
            .expect(
                "Internal functions should
        only call this will monotonically increasing current_epoch",
            );
        self.authorized_voters
            .purge_authorized_voters(current_epoch);
        Some(pubkey)
    }

    fn pop_expired_votes(&mut self, slot: Slot) {
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

    pub fn process_timestamp(
        &mut self,
        slot: Slot,
        timestamp: UnixTimestamp,
    ) -> Result<(), VoteError> {
        if (slot < self.last_timestamp.slot || timestamp < self.last_timestamp.timestamp)
            || ((slot == self.last_timestamp.slot || timestamp == self.last_timestamp.timestamp)
                && BlockTimestamp { slot, timestamp } != self.last_timestamp
                && self.last_timestamp.slot != 0)
        {
            return Err(VoteError::TimestampTooOld);
        }
        self.last_timestamp = BlockTimestamp { slot, timestamp };
        Ok(())
    }
}

/// Authorize the given pubkey to withdraw or sign votes. This may be called multiple times,
/// but will implicitly withdraw authorization from the previously authorized
/// key
pub fn authorize<S: std::hash::BuildHasher>(
    vote_account: &KeyedAccount,
    authorized: &Pubkey,
    vote_authorize: VoteAuthorize,
    signers: &HashSet<Pubkey, S>,
    clock: &Clock,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState =
        State::<VoteStateVersions>::state(vote_account)?.convert_to_current();

    // current authorized signer must say "yay"
    match vote_authorize {
        VoteAuthorize::Voter => {
            vote_state.set_new_authorized_voter(
                authorized,
                clock.epoch,
                clock.leader_schedule_epoch + 1,
                |epoch_authorized_voter| verify_authorized_signer(&epoch_authorized_voter, signers),
            )?;
        }
        VoteAuthorize::Withdrawer => {
            verify_authorized_signer(&vote_state.authorized_withdrawer, signers)?;
            vote_state.authorized_withdrawer = *authorized;
        }
    }

    vote_account.set_state(&VoteStateVersions::Current(Box::new(vote_state)))
}

/// Update the node_pubkey, requires signature of the authorized voter
pub fn update_node<S: std::hash::BuildHasher>(
    vote_account: &KeyedAccount,
    node_pubkey: &Pubkey,
    signers: &HashSet<Pubkey, S>,
    clock: &Clock,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState =
        State::<VoteStateVersions>::state(vote_account)?.convert_to_current();
    let authorized_voter = vote_state
        .get_and_update_authorized_voter(clock.epoch)
        .expect("the clock epoch is monotonically increasing, so authorized voter must be known");

    // current authorized voter must say "yay"
    verify_authorized_signer(&authorized_voter, signers)?;

    vote_state.node_pubkey = *node_pubkey;

    vote_account.set_state(&VoteStateVersions::Current(Box::new(vote_state)))
}

fn verify_authorized_signer<S: std::hash::BuildHasher>(
    authorized: &Pubkey,
    signers: &HashSet<Pubkey, S>,
) -> Result<(), InstructionError> {
    if signers.contains(authorized) {
        Ok(())
    } else {
        Err(InstructionError::MissingRequiredSignature)
    }
}

/// Withdraw funds from the vote account
pub fn withdraw<S: std::hash::BuildHasher>(
    vote_account: &KeyedAccount,
    lamports: u64,
    to_account: &KeyedAccount,
    signers: &HashSet<Pubkey, S>,
) -> Result<(), InstructionError> {
    let vote_state: VoteState =
        State::<VoteStateVersions>::state(vote_account)?.convert_to_current();

    verify_authorized_signer(&vote_state.authorized_withdrawer, signers)?;

    if vote_account.lamports()? < lamports {
        return Err(InstructionError::InsufficientFunds);
    }
    vote_account.try_account_ref_mut()?.lamports -= lamports;
    to_account.try_account_ref_mut()?.lamports += lamports;
    Ok(())
}

/// Initialize the vote_state for a vote account
/// Assumes that the account is being init as part of a account creation or balance transfer and
/// that the transaction must be signed by the staker's keys
pub fn initialize_account(
    vote_account: &KeyedAccount,
    vote_init: &VoteInit,
    clock: &Clock,
) -> Result<(), InstructionError> {
    let versioned = State::<VoteStateVersions>::state(vote_account)?;

    if !versioned.is_uninitialized() {
        return Err(InstructionError::AccountAlreadyInitialized);
    }

    vote_account.set_state(&VoteStateVersions::Current(Box::new(VoteState::new(
        vote_init, clock,
    ))))
}

pub fn process_vote<S: std::hash::BuildHasher>(
    vote_account: &KeyedAccount,
    slot_hashes: &[SlotHash],
    clock: &Clock,
    vote: &Vote,
    signers: &HashSet<Pubkey, S>,
) -> Result<(), InstructionError> {
    let versioned = State::<VoteStateVersions>::state(vote_account)?;

    if versioned.is_uninitialized() {
        return Err(InstructionError::UninitializedAccount);
    }

    let mut vote_state = versioned.convert_to_current();
    let authorized_voter = vote_state
        .get_and_update_authorized_voter(clock.epoch)
        .expect("the clock epoch is monotonically increasinig, so authorized voter must be known");
    verify_authorized_signer(&authorized_voter, signers)?;

    vote_state.process_vote(vote, slot_hashes, clock.epoch)?;
    if let Some(timestamp) = vote.timestamp {
        vote.slots
            .iter()
            .max()
            .ok_or_else(|| VoteError::EmptySlots)
            .and_then(|slot| vote_state.process_timestamp(*slot, timestamp))?;
    }
    vote_account.set_state(&VoteStateVersions::Current(Box::new(vote_state)))
}

// utility function, used by Bank, tests
pub fn create_account(
    vote_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
    commission: u8,
    lamports: u64,
) -> Account {
    let mut vote_account = Account::new(lamports, VoteState::size_of(), &id());

    let vote_state = VoteState::new(
        &VoteInit {
            node_pubkey: *node_pubkey,
            authorized_voter: *vote_pubkey,
            authorized_withdrawer: *vote_pubkey,
            commission,
        },
        &Clock::default(),
    );

    let versioned = VoteStateVersions::Current(Box::new(vote_state));
    VoteState::to(&versioned, &mut vote_account).unwrap();

    vote_account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vote_state;
    use solana_sdk::{
        account::{get_signers, Account},
        account_utils::StateMut,
        hash::hash,
        program_utils::next_keyed_account,
    };
    use std::cell::RefCell;

    const MAX_RECENT_VOTES: usize = 16;

    impl VoteState {
        pub fn new_for_test(auth_pubkey: &Pubkey) -> Self {
            Self::new(
                &VoteInit {
                    node_pubkey: Pubkey::new_rand(),
                    authorized_voter: *auth_pubkey,
                    authorized_withdrawer: *auth_pubkey,
                    commission: 0,
                },
                &Clock::default(),
            )
        }
    }

    #[test]
    fn test_initialize_vote_account() {
        let vote_account_pubkey = Pubkey::new_rand();
        let vote_account = Account::new_ref(100, VoteState::size_of(), &id());

        let node_pubkey = Pubkey::new_rand();

        //init should pass
        let vote_account = KeyedAccount::new(&vote_account_pubkey, false, &vote_account);
        let res = initialize_account(
            &vote_account,
            &VoteInit {
                node_pubkey,
                authorized_voter: vote_account_pubkey,
                authorized_withdrawer: vote_account_pubkey,
                commission: 0,
            },
            &Clock::default(),
        );
        assert_eq!(res, Ok(()));

        // reinit should fail
        let res = initialize_account(
            &vote_account,
            &VoteInit {
                node_pubkey,
                authorized_voter: vote_account_pubkey,
                authorized_withdrawer: vote_account_pubkey,
                commission: 0,
            },
            &Clock::default(),
        );
        assert_eq!(res, Err(InstructionError::AccountAlreadyInitialized));
    }

    fn create_test_account() -> (Pubkey, RefCell<Account>) {
        let vote_pubkey = Pubkey::new_rand();
        (
            vote_pubkey,
            RefCell::new(vote_state::create_account(
                &vote_pubkey,
                &Pubkey::new_rand(),
                0,
                100,
            )),
        )
    }

    fn simulate_process_vote(
        vote_pubkey: &Pubkey,
        vote_account: &RefCell<Account>,
        vote: &Vote,
        slot_hashes: &[SlotHash],
        epoch: Epoch,
    ) -> Result<VoteState, InstructionError> {
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        process_vote(
            &keyed_accounts[0],
            slot_hashes,
            &Clock {
                epoch,
                ..Clock::default()
            },
            &vote.clone(),
            &signers,
        )?;
        StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
            .map(|versioned| versioned.convert_to_current())
    }

    /// exercises all the keyed accounts stuff
    fn simulate_process_vote_unchecked(
        vote_pubkey: &Pubkey,
        vote_account: &RefCell<Account>,
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
        let versioned = VoteStateVersions::Current(Box::new(vote_state));
        assert!(VoteState::serialize(&versioned, &mut buffer[0..4]).is_err());
        VoteState::serialize(&versioned, &mut buffer).unwrap();
        assert_eq!(
            VoteStateVersions::Current(Box::new(VoteState::deserialize(&buffer).unwrap())),
            versioned
        );
    }

    #[test]
    fn test_voter_registration() {
        let (vote_pubkey, vote_account) = create_test_account();

        let vote_state: VoteState = StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
            .unwrap()
            .convert_to_current();
        assert_eq!(vote_state.authorized_voters.len(), 1);
        assert_eq!(
            *vote_state.authorized_voters.first().unwrap().1,
            vote_pubkey
        );
        assert!(vote_state.votes.is_empty());
    }

    #[test]
    fn test_vote() {
        let (vote_pubkey, vote_account) = create_test_account();

        let vote = Vote::new(vec![1], Hash::default());
        let vote_state =
            simulate_process_vote_unchecked(&vote_pubkey, &vote_account, &vote).unwrap();
        assert_eq!(
            vote_state.votes,
            vec![Lockout::new(*vote.slots.last().unwrap())]
        );
        assert_eq!(vote_state.credits(), 0);
    }

    #[test]
    fn test_vote_slot_hashes() {
        let (vote_pubkey, vote_account) = create_test_account();

        let hash = hash(&[0u8]);
        let vote = Vote::new(vec![0], hash);

        // wrong hash
        assert_eq!(
            simulate_process_vote(
                &vote_pubkey,
                &vote_account,
                &vote,
                &[(0, Hash::default())],
                0,
            ),
            Err(VoteError::SlotHashMismatch.into())
        );

        // wrong slot
        assert_eq!(
            simulate_process_vote(&vote_pubkey, &vote_account, &vote, &[(1, hash)], 0),
            Err(VoteError::SlotsMismatch.into())
        );

        // empty slot_hashes
        assert_eq!(
            simulate_process_vote(&vote_pubkey, &vote_account, &vote, &[], 0),
            Err(VoteError::VoteTooOld.into())
        );
    }

    #[test]
    fn test_vote_update_node_id() {
        let (vote_pubkey, vote_account) = create_test_account();

        let node_pubkey = Pubkey::new_rand();

        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, false, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = update_node(
            &keyed_accounts[0],
            &node_pubkey,
            &signers,
            &Clock::default(),
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));
        let vote_state: VoteState = StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
            .unwrap()
            .convert_to_current();
        assert!(vote_state.node_pubkey != node_pubkey);

        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = update_node(
            &keyed_accounts[0],
            &node_pubkey,
            &signers,
            &Clock::default(),
        );
        assert_eq!(res, Ok(()));
        let vote_state: VoteState = StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
            .unwrap()
            .convert_to_current();
        assert_eq!(vote_state.node_pubkey, node_pubkey);

        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let mut clock = Clock::default();
        clock.epoch += 10;
        let res = update_node(&keyed_accounts[0], &node_pubkey, &signers, &clock);
        assert_eq!(res, Ok(()));
        let vote_state: VoteState = StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
            .unwrap()
            .convert_to_current();
        assert_eq!(vote_state.node_pubkey, node_pubkey);
    }

    #[test]
    fn test_vote_signature() {
        let (vote_pubkey, vote_account) = create_test_account();
        let vote = Vote::new(vec![1], Hash::default());

        // unsigned
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, false, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = process_vote(
            &keyed_accounts[0],
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock {
                epoch: 1,
                leader_schedule_epoch: 2,
                ..Clock::default()
            },
            &vote,
            &signers,
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        // signed
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = process_vote(
            &keyed_accounts[0],
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock {
                epoch: 1,
                leader_schedule_epoch: 2,
                ..Clock::default()
            },
            &vote,
            &signers,
        );
        assert_eq!(res, Ok(()));

        // another voter, unsigned
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, false, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let authorized_voter_pubkey = Pubkey::new_rand();
        let res = authorize(
            &keyed_accounts[0],
            &authorized_voter_pubkey,
            VoteAuthorize::Voter,
            &signers,
            &Clock {
                epoch: 1,
                leader_schedule_epoch: 2,
                ..Clock::default()
            },
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = authorize(
            &keyed_accounts[0],
            &authorized_voter_pubkey,
            VoteAuthorize::Voter,
            &signers,
            &Clock {
                epoch: 1,
                leader_schedule_epoch: 2,
                ..Clock::default()
            },
        );
        assert_eq!(res, Ok(()));

        // Already set an authorized voter earlier for leader_schedule_epoch == 2
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = authorize(
            &keyed_accounts[0],
            &authorized_voter_pubkey,
            VoteAuthorize::Voter,
            &signers,
            &Clock {
                epoch: 1,
                leader_schedule_epoch: 2,
                ..Clock::default()
            },
        );
        assert_eq!(res, Err(VoteError::TooSoonToReauthorize.into()));

        // verify authorized_voter_pubkey can authorize authorized_voter_pubkey ;)
        let authorized_voter_account = RefCell::new(Account::default());
        let keyed_accounts = &[
            KeyedAccount::new(&vote_pubkey, false, &vote_account),
            KeyedAccount::new(&authorized_voter_pubkey, true, &authorized_voter_account),
        ];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = authorize(
            &keyed_accounts[0],
            &authorized_voter_pubkey,
            VoteAuthorize::Voter,
            &signers,
            &Clock {
                // The authorized voter was set when leader_schedule_epoch == 2, so will
                // take effect when epoch == 3
                epoch: 3,
                leader_schedule_epoch: 4,
                ..Clock::default()
            },
        );
        assert_eq!(res, Ok(()));

        // authorize another withdrawer
        // another voter
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let authorized_withdrawer_pubkey = Pubkey::new_rand();
        let res = authorize(
            &keyed_accounts[0],
            &authorized_withdrawer_pubkey,
            VoteAuthorize::Withdrawer,
            &signers,
            &Clock {
                epoch: 3,
                leader_schedule_epoch: 4,
                ..Clock::default()
            },
        );
        assert_eq!(res, Ok(()));

        // verify authorized_withdrawer can authorize authorized_withdrawer ;)
        let withdrawer_account = RefCell::new(Account::default());
        let keyed_accounts = &[
            KeyedAccount::new(&vote_pubkey, false, &vote_account),
            KeyedAccount::new(&authorized_withdrawer_pubkey, true, &withdrawer_account),
        ];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = authorize(
            &keyed_accounts[0],
            &authorized_withdrawer_pubkey,
            VoteAuthorize::Withdrawer,
            &signers,
            &Clock {
                epoch: 3,
                leader_schedule_epoch: 4,
                ..Clock::default()
            },
        );
        assert_eq!(res, Ok(()));

        // not signed by authorized voter
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let vote = Vote::new(vec![2], Hash::default());
        let res = process_vote(
            &keyed_accounts[0],
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock {
                epoch: 3,
                leader_schedule_epoch: 4,
                ..Clock::default()
            },
            &vote,
            &signers,
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        // signed by authorized voter
        let authorized_voter_account = RefCell::new(Account::default());
        let keyed_accounts = &[
            KeyedAccount::new(&vote_pubkey, false, &vote_account),
            KeyedAccount::new(&authorized_voter_pubkey, true, &authorized_voter_account),
        ];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let vote = Vote::new(vec![2], Hash::default());
        let res = process_vote(
            &keyed_accounts[0],
            &[(*vote.slots.last().unwrap(), vote.hash)],
            &Clock {
                epoch: 3,
                leader_schedule_epoch: 4,
                ..Clock::default()
            },
            &vote,
            &signers,
        );
        assert_eq!(res, Ok(()));
    }

    #[test]
    fn test_vote_without_initialization() {
        let vote_pubkey = Pubkey::new_rand();
        let vote_account = RefCell::new(Account::new(100, VoteState::size_of(), &id()));

        let res = simulate_process_vote_unchecked(
            &vote_pubkey,
            &vote_account,
            &Vote::new(vec![1], Hash::default()),
        );
        assert_eq!(res, Err(InstructionError::UninitializedAccount));
    }

    #[test]
    fn test_vote_lockout() {
        let (_vote_pubkey, vote_account) = create_test_account();

        let mut vote_state: VoteState =
            StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
                .unwrap()
                .convert_to_current();

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
        let mut vote_state = VoteState::new_for_test(&voter_pubkey);

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
        let mut vote_state = VoteState::new_for_test(&voter_pubkey);

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
        let mut vote_state = VoteState::new_for_test(&voter_pubkey);

        for i in 0..MAX_LOCKOUT_HISTORY {
            vote_state.process_slot_vote_unchecked(i as u64);
        }

        assert_eq!(vote_state.credits(), 0);

        vote_state.process_slot_vote_unchecked(MAX_LOCKOUT_HISTORY as u64 + 1);
        assert_eq!(vote_state.credits(), 1);
        vote_state.process_slot_vote_unchecked(MAX_LOCKOUT_HISTORY as u64 + 2);
        assert_eq!(vote_state.credits(), 2);
        vote_state.process_slot_vote_unchecked(MAX_LOCKOUT_HISTORY as u64 + 3);
        assert_eq!(vote_state.credits(), 3);
    }

    #[test]
    fn test_duplicate_vote() {
        let voter_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::new_for_test(&voter_pubkey);
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
        let mut vote_state = VoteState::new_for_test(&voter_pubkey);
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
        let mut vote_state_a = VoteState::new_for_test(&account_a);
        let account_b = Pubkey::new_rand();
        let mut vote_state_b = VoteState::new_for_test(&account_b);

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
        let mut vote_state = VoteState::default();

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
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &vec![]),
            Err(VoteError::VoteTooOld)
        );
    }

    #[test]
    fn test_check_slots_are_valid_new_vote() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Ok(())
        );
    }

    #[test]
    fn test_check_slots_are_valid_bad_hash() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), hash(vote.hash.as_ref()))];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Err(VoteError::SlotHashMismatch)
        );
    }

    #[test]
    fn test_check_slots_are_valid_bad_slot() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![1], Hash::default());
        let slot_hashes: Vec<_> = vec![(0, vote.hash)];
        assert_eq!(
            vote_state.check_slots_are_valid(&vote, &slot_hashes),
            Err(VoteError::SlotsMismatch)
        );
    }

    #[test]
    fn test_check_slots_are_valid_duplicate_vote() {
        let mut vote_state = VoteState::default();

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
        let mut vote_state = VoteState::default();

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
        let mut vote_state = VoteState::default();

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
    fn test_process_vote_empty_slots() {
        let mut vote_state = VoteState::default();

        let vote = Vote::new(vec![], Hash::default());
        assert_eq!(
            vote_state.process_vote(&vote, &[], 0),
            Err(VoteError::EmptySlots)
        );
    }

    #[test]
    fn test_vote_state_commission_split() {
        let vote_state = VoteState::default();

        assert_eq!(vote_state.commission_split(1.0), (0.0, 1.0, false));

        let mut vote_state = VoteState::default();
        vote_state.commission = std::u8::MAX;
        assert_eq!(vote_state.commission_split(1.0), (1.0, 0.0, false));

        vote_state.commission = 50;
        let (voter_portion, staker_portion, was_split) = vote_state.commission_split(10.0);

        assert_eq!(
            (voter_portion.round(), staker_portion.round(), was_split),
            (5.0, 5.0, true)
        );
    }

    #[test]
    fn test_vote_state_withdraw() {
        let (vote_pubkey, vote_account) = create_test_account();

        // unsigned request
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, false, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = withdraw(
            &keyed_accounts[0],
            0,
            &KeyedAccount::new(
                &Pubkey::new_rand(),
                false,
                &RefCell::new(Account::default()),
            ),
            &signers,
        );
        assert_eq!(res, Err(InstructionError::MissingRequiredSignature));

        // insufficient funds
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = withdraw(
            &keyed_accounts[0],
            101,
            &KeyedAccount::new(
                &Pubkey::new_rand(),
                false,
                &RefCell::new(Account::default()),
            ),
            &signers,
        );
        assert_eq!(res, Err(InstructionError::InsufficientFunds));

        // all good
        let to_account = RefCell::new(Account::default());
        let lamports = vote_account.borrow().lamports;
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = withdraw(
            &keyed_accounts[0],
            lamports,
            &KeyedAccount::new(&Pubkey::new_rand(), false, &to_account),
            &signers,
        );
        assert_eq!(res, Ok(()));
        assert_eq!(vote_account.borrow().lamports, 0);
        assert_eq!(to_account.borrow().lamports, lamports);

        // reset balance, verify that authorized_withdrawer works
        vote_account.borrow_mut().lamports = lamports;

        // authorize authorized_withdrawer
        let authorized_withdrawer_pubkey = Pubkey::new_rand();
        let keyed_accounts = &[KeyedAccount::new(&vote_pubkey, true, &vote_account)];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let res = authorize(
            &keyed_accounts[0],
            &authorized_withdrawer_pubkey,
            VoteAuthorize::Withdrawer,
            &signers,
            &Clock::default(),
        );
        assert_eq!(res, Ok(()));

        // withdraw using authorized_withdrawer to authorized_withdrawer's account
        let withdrawer_account = RefCell::new(Account::default());
        let keyed_accounts = &[
            KeyedAccount::new(&vote_pubkey, false, &vote_account),
            KeyedAccount::new(&authorized_withdrawer_pubkey, true, &withdrawer_account),
        ];
        let signers: HashSet<Pubkey> = get_signers(keyed_accounts);
        let keyed_accounts = &mut keyed_accounts.iter();
        let vote_keyed_account = next_keyed_account(keyed_accounts).unwrap();
        let withdrawer_keyed_account = next_keyed_account(keyed_accounts).unwrap();
        let res = withdraw(
            vote_keyed_account,
            lamports,
            withdrawer_keyed_account,
            &signers,
        );
        assert_eq!(res, Ok(()));
        assert_eq!(vote_account.borrow().lamports, 0);
        assert_eq!(withdrawer_account.borrow().lamports, lamports);
    }

    #[test]
    fn test_vote_state_epoch_credits() {
        let mut vote_state = VoteState::default();

        assert_eq!(vote_state.credits(), 0);
        assert_eq!(vote_state.epoch_credits().clone(), vec![]);

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

        while expected.len() > MAX_EPOCH_CREDITS_HISTORY {
            expected.remove(0);
        }

        assert_eq!(vote_state.credits(), credits);
        assert_eq!(vote_state.epoch_credits().clone(), expected);
    }

    #[test]
    fn test_vote_state_epoch0_no_credits() {
        let mut vote_state = VoteState::default();

        assert_eq!(vote_state.epoch_credits().len(), 0);
        vote_state.increment_credits(1);
        assert_eq!(vote_state.epoch_credits().len(), 1);

        vote_state.increment_credits(2);
        assert_eq!(vote_state.epoch_credits().len(), 2);
    }

    #[test]
    fn test_vote_state_increment_credits() {
        let mut vote_state = VoteState::default();

        let credits = (MAX_EPOCH_CREDITS_HISTORY + 2) as u64;
        for i in 0..credits {
            vote_state.increment_credits(i as u64);
        }
        assert_eq!(vote_state.credits(), credits);
        assert!(vote_state.epoch_credits().len() <= MAX_EPOCH_CREDITS_HISTORY);
    }

    #[test]
    fn test_vote_process_timestamp() {
        let (slot, timestamp) = (15, 1575412285);
        let mut vote_state = VoteState::default();
        vote_state.last_timestamp = BlockTimestamp { slot, timestamp };

        assert_eq!(
            vote_state.process_timestamp(slot - 1, timestamp + 1),
            Err(VoteError::TimestampTooOld)
        );
        assert_eq!(
            vote_state.last_timestamp,
            BlockTimestamp { slot, timestamp }
        );
        assert_eq!(
            vote_state.process_timestamp(slot + 1, timestamp - 1),
            Err(VoteError::TimestampTooOld)
        );
        assert_eq!(
            vote_state.process_timestamp(slot + 1, timestamp),
            Err(VoteError::TimestampTooOld)
        );
        assert_eq!(
            vote_state.process_timestamp(slot, timestamp + 1),
            Err(VoteError::TimestampTooOld)
        );
        assert_eq!(vote_state.process_timestamp(slot, timestamp), Ok(()));
        assert_eq!(
            vote_state.last_timestamp,
            BlockTimestamp { slot, timestamp }
        );
        assert_eq!(
            vote_state.process_timestamp(slot + 1, timestamp + 1),
            Ok(())
        );
        assert_eq!(
            vote_state.last_timestamp,
            BlockTimestamp {
                slot: slot + 1,
                timestamp: timestamp + 1
            }
        );

        // Test initial vote
        vote_state.last_timestamp = BlockTimestamp::default();
        assert_eq!(vote_state.process_timestamp(0, timestamp), Ok(()));
    }

    #[test]
    fn test_get_and_update_authorized_voter() {
        let original_voter = Pubkey::new_rand();
        let mut vote_state = VoteState::new(
            &VoteInit {
                node_pubkey: original_voter,
                authorized_voter: original_voter,
                authorized_withdrawer: original_voter,
                commission: 0,
            },
            &Clock::default(),
        );

        // If no new authorized voter was set, the same authorized voter
        // is locked into the next epoch
        assert_eq!(
            vote_state.get_and_update_authorized_voter(1).unwrap(),
            original_voter
        );

        // Try to get the authorized voter for epoch 5, implies
        // the authorized voter for epochs 1-4 were unchanged
        assert_eq!(
            vote_state.get_and_update_authorized_voter(5).unwrap(),
            original_voter
        );

        // Authorized voter for expired epoch 0..5 should have been
        // purged and no longer queryable
        assert_eq!(vote_state.authorized_voters.len(), 1);
        for i in 0..5 {
            assert!(vote_state
                .authorized_voters
                .get_authorized_voter(i)
                .is_none());
        }

        // Set an authorized voter change at slot 7
        let new_authorized_voter = Pubkey::new_rand();
        vote_state
            .set_new_authorized_voter(&new_authorized_voter, 5, 7, |_| Ok(()))
            .unwrap();

        // Try to get the authorized voter for epoch 6, unchanged
        assert_eq!(
            vote_state.get_and_update_authorized_voter(6).unwrap(),
            original_voter
        );

        // Try to get the authorized voter for epoch 7 and onwards, should
        // be the new authorized voter
        for i in 7..10 {
            assert_eq!(
                vote_state.get_and_update_authorized_voter(i).unwrap(),
                new_authorized_voter
            );
        }
        assert_eq!(vote_state.authorized_voters.len(), 1);
    }

    #[test]
    fn test_set_new_authorized_voter() {
        let original_voter = Pubkey::new_rand();
        let epoch_offset = 15;
        let mut vote_state = VoteState::new(
            &VoteInit {
                node_pubkey: original_voter,
                authorized_voter: original_voter,
                authorized_withdrawer: original_voter,
                commission: 0,
            },
            &Clock::default(),
        );

        assert!(vote_state.prior_voters.last().is_none());

        let new_voter = Pubkey::new_rand();
        // Set a new authorized voter
        vote_state
            .set_new_authorized_voter(&new_voter, 0, 0 + epoch_offset, |_| Ok(()))
            .unwrap();

        assert_eq!(vote_state.prior_voters.idx, 0);
        assert_eq!(
            vote_state.prior_voters.last(),
            Some(&(original_voter, 0, 0 + epoch_offset))
        );

        // Trying to set authorized voter for same epoch again should fail
        assert_eq!(
            vote_state.set_new_authorized_voter(&new_voter, 0, epoch_offset, |_| Ok(())),
            Err(VoteError::TooSoonToReauthorize.into())
        );

        // Setting the same authorized voter again should succeed
        vote_state
            .set_new_authorized_voter(&new_voter, 2, 2 + epoch_offset, |_| Ok(()))
            .unwrap();

        // Set a third and fourth authorized voter
        let new_voter2 = Pubkey::new_rand();
        vote_state
            .set_new_authorized_voter(&new_voter2, 3, 3 + epoch_offset, |_| Ok(()))
            .unwrap();
        assert_eq!(vote_state.prior_voters.idx, 1);
        assert_eq!(
            vote_state.prior_voters.last(),
            Some(&(new_voter, epoch_offset, 3 + epoch_offset))
        );

        let new_voter3 = Pubkey::new_rand();
        vote_state
            .set_new_authorized_voter(&new_voter3, 6, 6 + epoch_offset, |_| Ok(()))
            .unwrap();
        assert_eq!(vote_state.prior_voters.idx, 2);
        assert_eq!(
            vote_state.prior_voters.last(),
            Some(&(new_voter2, 3 + epoch_offset, 6 + epoch_offset))
        );

        // Check can set back to original voter
        vote_state
            .set_new_authorized_voter(&original_voter, 9, 9 + epoch_offset, |_| Ok(()))
            .unwrap();

        // Run with these voters for a while, check the ranges of authorized
        // voters is correct
        for i in 9..epoch_offset {
            assert_eq!(
                vote_state.get_and_update_authorized_voter(i).unwrap(),
                original_voter
            );
        }
        for i in epoch_offset..3 + epoch_offset {
            assert_eq!(
                vote_state.get_and_update_authorized_voter(i).unwrap(),
                new_voter
            );
        }
        for i in 3 + epoch_offset..6 + epoch_offset {
            assert_eq!(
                vote_state.get_and_update_authorized_voter(i).unwrap(),
                new_voter2
            );
        }
        for i in 6 + epoch_offset..9 + epoch_offset {
            assert_eq!(
                vote_state.get_and_update_authorized_voter(i).unwrap(),
                new_voter3
            );
        }
        for i in 9 + epoch_offset..=10 + epoch_offset {
            assert_eq!(
                vote_state.get_and_update_authorized_voter(i).unwrap(),
                original_voter
            );
        }
    }

    #[test]
    fn test_authorized_voter_is_locked_within_epoch() {
        let original_voter = Pubkey::new_rand();
        let mut vote_state = VoteState::new(
            &VoteInit {
                node_pubkey: original_voter,
                authorized_voter: original_voter,
                authorized_withdrawer: original_voter,
                commission: 0,
            },
            &Clock::default(),
        );

        // Test that it's not possible to set a new authorized
        // voter within the same epoch, even if none has been
        // explicitly set before
        let new_voter = Pubkey::new_rand();
        assert_eq!(
            vote_state.set_new_authorized_voter(&new_voter, 1, 1, |_| Ok(())),
            Err(VoteError::TooSoonToReauthorize.into())
        );

        assert_eq!(vote_state.get_authorized_voter(1), Some(original_voter));

        // Set a new authorized voter for a future epoch
        assert_eq!(
            vote_state.set_new_authorized_voter(&new_voter, 1, 2, |_| Ok(())),
            Ok(())
        );

        // Test that it's not possible to set a new authorized
        // voter within the same epoch, even if none has been
        // explicitly set before
        assert_eq!(
            vote_state.set_new_authorized_voter(&original_voter, 3, 3, |_| Ok(())),
            Err(VoteError::TooSoonToReauthorize.into())
        );

        assert_eq!(vote_state.get_authorized_voter(3), Some(new_voter));
    }

    #[test]
    fn test_vote_state_max_size() {
        let mut max_sized_data = vec![0; VoteState::size_of()];
        let vote_state = VoteState::get_max_sized_vote_state();
        let (start_leader_schedule_epoch, _) = vote_state.authorized_voters.last().unwrap();
        let start_current_epoch =
            start_leader_schedule_epoch - MAX_LEADER_SCHEDULE_EPOCH_OFFSET + 1;

        let mut vote_state = Some(vote_state);
        for i in start_current_epoch..start_current_epoch + 2 * MAX_LEADER_SCHEDULE_EPOCH_OFFSET {
            vote_state.as_mut().map(|vote_state| {
                vote_state.set_new_authorized_voter(
                    &Pubkey::new_rand(),
                    i,
                    i + MAX_LEADER_SCHEDULE_EPOCH_OFFSET,
                    |_| Ok(()),
                )
            });

            let versioned = VoteStateVersions::Current(Box::new(vote_state.take().unwrap()));
            VoteState::serialize(&versioned, &mut max_sized_data).unwrap();
            vote_state = Some(versioned.convert_to_current());
        }
    }
}
