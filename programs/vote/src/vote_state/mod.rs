//! Vote state, vote program
//! Receive and processes votes from validators
pub use solana_program::vote::state::{vote_state_versions::*, *};
use {
    log::*,
    serde_derive::{Deserialize, Serialize},
    solana_metrics::datapoint_debug,
    solana_program::vote::{error::VoteError, program::id},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::{Epoch, Slot, UnixTimestamp},
        epoch_schedule::EpochSchedule,
        feature_set::{self, FeatureSet},
        hash::Hash,
        instruction::InstructionError,
        pubkey::Pubkey,
        rent::Rent,
        slot_hashes::SlotHash,
        sysvar::clock::Clock,
        transaction_context::{
            BorrowedAccount, IndexOfAccount, InstructionContext, TransactionContext,
        },
    },
    std::{
        cmp::Ordering,
        collections::{HashSet, VecDeque},
        fmt::Debug,
    },
};

#[frozen_abi(digest = "2AuJFjx7SYrJ2ugCfH1jFh3Lr9UHMEPfKwwk1NcjqND1")]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, AbiEnumVisitor, AbiExample)]
pub enum VoteTransaction {
    Vote(Vote),
    VoteStateUpdate(VoteStateUpdate),
    #[serde(with = "serde_compact_vote_state_update")]
    CompactVoteStateUpdate(VoteStateUpdate),
}

impl VoteTransaction {
    pub fn slots(&self) -> Vec<Slot> {
        match self {
            VoteTransaction::Vote(vote) => vote.slots.clone(),
            VoteTransaction::VoteStateUpdate(vote_state_update) => vote_state_update.slots(),
            VoteTransaction::CompactVoteStateUpdate(vote_state_update) => vote_state_update.slots(),
        }
    }

    pub fn slot(&self, i: usize) -> Slot {
        match self {
            VoteTransaction::Vote(vote) => vote.slots[i],
            VoteTransaction::VoteStateUpdate(vote_state_update)
            | VoteTransaction::CompactVoteStateUpdate(vote_state_update) => {
                vote_state_update.lockouts[i].slot()
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            VoteTransaction::Vote(vote) => vote.slots.len(),
            VoteTransaction::VoteStateUpdate(vote_state_update)
            | VoteTransaction::CompactVoteStateUpdate(vote_state_update) => {
                vote_state_update.lockouts.len()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            VoteTransaction::Vote(vote) => vote.slots.is_empty(),
            VoteTransaction::VoteStateUpdate(vote_state_update)
            | VoteTransaction::CompactVoteStateUpdate(vote_state_update) => {
                vote_state_update.lockouts.is_empty()
            }
        }
    }

    pub fn hash(&self) -> Hash {
        match self {
            VoteTransaction::Vote(vote) => vote.hash,
            VoteTransaction::VoteStateUpdate(vote_state_update) => vote_state_update.hash,
            VoteTransaction::CompactVoteStateUpdate(vote_state_update) => vote_state_update.hash,
        }
    }

    pub fn timestamp(&self) -> Option<UnixTimestamp> {
        match self {
            VoteTransaction::Vote(vote) => vote.timestamp,
            VoteTransaction::VoteStateUpdate(vote_state_update)
            | VoteTransaction::CompactVoteStateUpdate(vote_state_update) => {
                vote_state_update.timestamp
            }
        }
    }

    pub fn set_timestamp(&mut self, ts: Option<UnixTimestamp>) {
        match self {
            VoteTransaction::Vote(vote) => vote.timestamp = ts,
            VoteTransaction::VoteStateUpdate(vote_state_update)
            | VoteTransaction::CompactVoteStateUpdate(vote_state_update) => {
                vote_state_update.timestamp = ts
            }
        }
    }

    pub fn last_voted_slot(&self) -> Option<Slot> {
        match self {
            VoteTransaction::Vote(vote) => vote.last_voted_slot(),
            VoteTransaction::VoteStateUpdate(vote_state_update)
            | VoteTransaction::CompactVoteStateUpdate(vote_state_update) => {
                vote_state_update.last_voted_slot()
            }
        }
    }

    pub fn last_voted_slot_hash(&self) -> Option<(Slot, Hash)> {
        Some((self.last_voted_slot()?, self.hash()))
    }
}

impl From<Vote> for VoteTransaction {
    fn from(vote: Vote) -> Self {
        VoteTransaction::Vote(vote)
    }
}

impl From<VoteStateUpdate> for VoteTransaction {
    fn from(vote_state_update: VoteStateUpdate) -> Self {
        VoteTransaction::VoteStateUpdate(vote_state_update)
    }
}

// utility function, used by Stakes, tests
pub fn from<T: ReadableAccount>(account: &T) -> Option<VoteState> {
    VoteState::deserialize(account.data()).ok()
}

// utility function, used by Stakes, tests
pub fn to<T: WritableAccount>(versioned: &VoteStateVersions, account: &mut T) -> Option<()> {
    VoteState::serialize(versioned, account.data_as_mut_slice()).ok()
}

// Updates the vote account state with a new VoteState instance.  This is required temporarily during the
// upgrade of vote account state from V1_14_11 to Current.
fn set_vote_account_state(
    vote_account: &mut BorrowedAccount,
    vote_state: VoteState,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    // Only if vote_state_add_vote_latency feature is enabled should the new version of vote state be stored
    if feature_set.is_active(&feature_set::vote_state_add_vote_latency::id()) {
        // If the account is not large enough to store the vote state, then attempt a realloc to make it large enough.
        // The realloc can only proceed if the vote account has balance sufficient for rent exemption at the new size.
        if (vote_account.get_data().len() < VoteStateVersions::vote_state_size_of(true))
            && (!vote_account
                .is_rent_exempt_at_data_length(VoteStateVersions::vote_state_size_of(true))
                || vote_account
                    .set_data_length(VoteStateVersions::vote_state_size_of(true))
                    .is_err())
        {
            // Account cannot be resized to the size of a vote state as it will not be rent exempt, or failed to be
            // resized for other reasons.  So store the V1_14_11 version.
            return vote_account.set_state(&VoteStateVersions::V1_14_11(Box::new(
                VoteState1_14_11::from(vote_state),
            )));
        }
        // Vote account is large enough to store the newest version of vote state
        vote_account.set_state(&VoteStateVersions::new_current(vote_state))
    // Else when the vote_state_add_vote_latency feature is not enabled, then the V1_14_11 version is stored
    } else {
        vote_account.set_state(&VoteStateVersions::V1_14_11(Box::new(
            VoteState1_14_11::from(vote_state),
        )))
    }
}

fn check_update_vote_state_slots_are_valid(
    vote_state: &VoteState,
    vote_state_update: &mut VoteStateUpdate,
    slot_hashes: &[(Slot, Hash)],
) -> Result<(), VoteError> {
    if vote_state_update.lockouts.is_empty() {
        return Err(VoteError::EmptySlots);
    }

    // If the vote state update is not new enough, return
    if let Some(last_vote_slot) = vote_state.votes.back().map(|lockout| lockout.slot()) {
        if vote_state_update.lockouts.back().unwrap().slot() <= last_vote_slot {
            return Err(VoteError::VoteTooOld);
        }
    }

    let last_vote_state_update_slot = vote_state_update
        .lockouts
        .back()
        .expect("must be nonempty, checked above")
        .slot();

    if slot_hashes.is_empty() {
        return Err(VoteError::SlotsMismatch);
    }
    let earliest_slot_hash_in_history = slot_hashes.last().unwrap().0;

    // Check if the proposed vote is too old to be in the SlotHash history
    if last_vote_state_update_slot < earliest_slot_hash_in_history {
        // If this is the last slot in the vote update, it must be in SlotHashes,
        // otherwise we have no way of confirming if the hash matches
        return Err(VoteError::VoteTooOld);
    }

    // Check if the proposed root is too old
    let original_proposed_root = vote_state_update.root;
    if let Some(new_proposed_root) = original_proposed_root {
        // If the new proposed root `R` is less than the earliest slot hash in the history
        // such that we cannot verify whether the slot was actually was on this fork, set
        // the root to the latest vote in the current vote that's less than R.
        if earliest_slot_hash_in_history > new_proposed_root {
            vote_state_update.root = vote_state.root_slot;
            let mut prev_slot = Slot::MAX;
            let current_root = vote_state_update.root;
            for vote in vote_state.votes.iter().rev() {
                let is_slot_bigger_than_root = current_root
                    .map(|current_root| vote.slot() > current_root)
                    .unwrap_or(true);
                // Ensure we're iterating from biggest to smallest vote in the
                // current vote state
                assert!(vote.slot() < prev_slot && is_slot_bigger_than_root);
                if vote.slot() <= new_proposed_root {
                    vote_state_update.root = Some(vote.slot());
                    break;
                }
                prev_slot = vote.slot();
            }
        }
    }

    // Index into the new proposed vote state's slots, starting with the root if it exists then
    // we use this mutable root to fold checking the root slot into the below loop
    // for performance
    let mut root_to_check = vote_state_update.root;
    let mut vote_state_update_index = 0;

    // index into the slot_hashes, starting at the oldest known
    // slot hash
    let mut slot_hashes_index = slot_hashes.len();

    let mut vote_state_update_indexes_to_filter = vec![];

    // Note:
    //
    // 1) `vote_state_update.lockouts` is sorted from oldest/smallest vote to newest/largest
    // vote, due to the way votes are applied to the vote state (newest votes
    // pushed to the back).
    //
    // 2) Conversely, `slot_hashes` is sorted from newest/largest vote to
    // the oldest/smallest vote
    //
    // Unlike for vote updates, vote state updates here can't only check votes older than the last vote
    // because have to ensure that every slot is actually part of the history, not just the most
    // recent ones
    while vote_state_update_index < vote_state_update.lockouts.len() && slot_hashes_index > 0 {
        let proposed_vote_slot = if let Some(root) = root_to_check {
            root
        } else {
            vote_state_update.lockouts[vote_state_update_index].slot()
        };
        if root_to_check.is_none()
            && vote_state_update_index > 0
            && proposed_vote_slot
                <= vote_state_update.lockouts[vote_state_update_index.checked_sub(1).expect(
                    "`vote_state_update_index` is positive when checking `SlotsNotOrdered`",
                )]
                .slot()
        {
            return Err(VoteError::SlotsNotOrdered);
        }
        let ancestor_slot = slot_hashes[slot_hashes_index
            .checked_sub(1)
            .expect("`slot_hashes_index` is positive when computing `ancestor_slot`")]
        .0;

        // Find if this slot in the proposed vote state exists in the SlotHashes history
        // to confirm if it was a valid ancestor on this fork
        match proposed_vote_slot.cmp(&ancestor_slot) {
            Ordering::Less => {
                if slot_hashes_index == slot_hashes.len() {
                    // The vote slot does not exist in the SlotHashes history because it's too old,
                    // i.e. older than the oldest slot in the history.
                    assert!(proposed_vote_slot < earliest_slot_hash_in_history);
                    if !vote_state.contains_slot(proposed_vote_slot) && root_to_check.is_none() {
                        // If the vote slot is both:
                        // 1) Too old
                        // 2) Doesn't already exist in vote state
                        //
                        // Then filter it out
                        vote_state_update_indexes_to_filter.push(vote_state_update_index);
                    }
                    if let Some(new_proposed_root) = root_to_check {
                        // 1. Because `root_to_check.is_some()`, then we know that
                        // we haven't checked the root yet in this loop, so
                        // `proposed_vote_slot` == `new_proposed_root` == `vote_state_update.root`.
                        assert_eq!(new_proposed_root, proposed_vote_slot);
                        // 2. We know from the assert earlier in the function that
                        // `proposed_vote_slot < earliest_slot_hash_in_history`,
                        // so from 1. we know that `new_proposed_root < earliest_slot_hash_in_history`.
                        assert!(new_proposed_root < earliest_slot_hash_in_history);
                        root_to_check = None;
                    } else {
                        vote_state_update_index = vote_state_update_index.checked_add(1).expect(
                            "`vote_state_update_index` is bounded by `MAX_LOCKOUT_HISTORY` when `proposed_vote_slot` is too old to be in SlotHashes history",
                        );
                    }
                    continue;
                } else {
                    // If the vote slot is new enough to be in the slot history,
                    // but is not part of the slot history, then it must belong to another fork,
                    // which means this vote state update is invalid.
                    if root_to_check.is_some() {
                        return Err(VoteError::RootOnDifferentFork);
                    } else {
                        return Err(VoteError::SlotsMismatch);
                    }
                }
            }
            Ordering::Greater => {
                // Decrement `slot_hashes_index` to find newer slots in the SlotHashes history
                slot_hashes_index = slot_hashes_index
                    .checked_sub(1)
                    .expect("`slot_hashes_index` is positive when finding newer slots in SlotHashes history");
                continue;
            }
            Ordering::Equal => {
                // Once the slot in `vote_state_update.lockouts` is found, bump to the next slot
                // in `vote_state_update.lockouts` and continue. If we were checking the root,
                // start checking the vote state instead.
                if root_to_check.is_some() {
                    root_to_check = None;
                } else {
                    vote_state_update_index = vote_state_update_index
                        .checked_add(1)
                        .expect("`vote_state_update_index` is bounded by `MAX_LOCKOUT_HISTORY` when match is found in SlotHashes history");
                    slot_hashes_index = slot_hashes_index.checked_sub(1).expect(
                        "`slot_hashes_index` is positive when match is found in SlotHashes history",
                    );
                }
            }
        }
    }

    if vote_state_update_index != vote_state_update.lockouts.len() {
        // The last vote slot in the update did not exist in SlotHashes
        return Err(VoteError::SlotsMismatch);
    }

    // This assertion must be true at this point because we can assume by now:
    // 1) vote_state_update_index == vote_state_update.lockouts.len()
    // 2) last_vote_state_update_slot >= earliest_slot_hash_in_history
    // 3) !vote_state_update.lockouts.is_empty()
    //
    // 1) implies that during the last iteration of the loop above,
    // `vote_state_update_index` was equal to `vote_state_update.lockouts.len() - 1`,
    // and was then incremented to `vote_state_update.lockouts.len()`.
    // This means in that last loop iteration,
    // `proposed_vote_slot ==
    //  vote_state_update.lockouts[vote_state_update.lockouts.len() - 1] ==
    //  last_vote_state_update_slot`.
    //
    // Then we know the last comparison `match proposed_vote_slot.cmp(&ancestor_slot)`
    // is equivalent to `match last_vote_state_update_slot.cmp(&ancestor_slot)`. The result
    // of this match to increment `vote_state_update_index` must have been either:
    //
    // 1) The Equal case ran, in which case then we know this assertion must be true
    // 2) The Less case ran, and more specifically the case
    // `proposed_vote_slot < earliest_slot_hash_in_history` ran, which is equivalent to
    // `last_vote_state_update_slot < earliest_slot_hash_in_history`, but this is impossible
    // due to assumption 3) above.
    assert_eq!(
        last_vote_state_update_slot,
        slot_hashes[slot_hashes_index].0
    );

    if slot_hashes[slot_hashes_index].1 != vote_state_update.hash {
        // This means the newest vote in the slot has a match that
        // doesn't match the expected hash for that slot on this
        // fork
        warn!(
            "{} dropped vote {:?} failed to match hash {} {}",
            vote_state.node_pubkey,
            vote_state_update,
            vote_state_update.hash,
            slot_hashes[slot_hashes_index].1
        );
        inc_new_counter_info!("dropped-vote-hash", 1);
        return Err(VoteError::SlotHashMismatch);
    }

    // Filter out the irrelevant votes
    let mut vote_state_update_index = 0;
    let mut filter_votes_index = 0;
    vote_state_update.lockouts.retain(|_lockout| {
        let should_retain = if filter_votes_index == vote_state_update_indexes_to_filter.len() {
            true
        } else if vote_state_update_index == vote_state_update_indexes_to_filter[filter_votes_index]
        {
            filter_votes_index = filter_votes_index.checked_add(1).unwrap();
            false
        } else {
            true
        };

        vote_state_update_index = vote_state_update_index
            .checked_add(1)
            .expect("`vote_state_update_index` is bounded by `MAX_LOCKOUT_HISTORY` when filtering out irrelevant votes");
        should_retain
    });

    Ok(())
}

fn check_slots_are_valid(
    vote_state: &VoteState,
    vote_slots: &[Slot],
    vote_hash: &Hash,
    slot_hashes: &[(Slot, Hash)],
) -> Result<(), VoteError> {
    // index into the vote's slots, starting at the oldest
    // slot
    let mut i = 0;

    // index into the slot_hashes, starting at the oldest known
    // slot hash
    let mut j = slot_hashes.len();

    // Note:
    //
    // 1) `vote_slots` is sorted from oldest/smallest vote to newest/largest
    // vote, due to the way votes are applied to the vote state (newest votes
    // pushed to the back).
    //
    // 2) Conversely, `slot_hashes` is sorted from newest/largest vote to
    // the oldest/smallest vote
    while i < vote_slots.len() && j > 0 {
        // 1) increment `i` to find the smallest slot `s` in `vote_slots`
        // where `s` >= `last_voted_slot`
        if vote_state
            .last_voted_slot()
            .map_or(false, |last_voted_slot| vote_slots[i] <= last_voted_slot)
        {
            i = i
                .checked_add(1)
                .expect("`i` is bounded by `MAX_LOCKOUT_HISTORY` when finding larger slots");
            continue;
        }

        // 2) Find the hash for this slot `s`.
        if vote_slots[i] != slot_hashes[j.checked_sub(1).expect("`j` is positive")].0 {
            // Decrement `j` to find newer slots
            j = j
                .checked_sub(1)
                .expect("`j` is positive when finding newer slots");
            continue;
        }

        // 3) Once the hash for `s` is found, bump `s` to the next slot
        // in `vote_slots` and continue.
        i = i
            .checked_add(1)
            .expect("`i` is bounded by `MAX_LOCKOUT_HISTORY` when hash is found");
        j = j
            .checked_sub(1)
            .expect("`j` is positive when hash is found");
    }

    if j == slot_hashes.len() {
        // This means we never made it to steps 2) or 3) above, otherwise
        // `j` would have been decremented at least once. This means
        // there are not slots in `vote_slots` greater than `last_voted_slot`
        debug!(
            "{} dropped vote slots {:?}, vote hash: {:?} slot hashes:SlotHash {:?}, too old ",
            vote_state.node_pubkey, vote_slots, vote_hash, slot_hashes
        );
        return Err(VoteError::VoteTooOld);
    }
    if i != vote_slots.len() {
        // This means there existed some slot for which we couldn't find
        // a matching slot hash in step 2)
        info!(
            "{} dropped vote slots {:?} failed to match slot hashes: {:?}",
            vote_state.node_pubkey, vote_slots, slot_hashes,
        );
        inc_new_counter_info!("dropped-vote-slot", 1);
        return Err(VoteError::SlotsMismatch);
    }
    if &slot_hashes[j].1 != vote_hash {
        // This means the newest slot in the `vote_slots` has a match that
        // doesn't match the expected hash for that slot on this
        // fork
        warn!(
            "{} dropped vote slots {:?} failed to match hash {} {}",
            vote_state.node_pubkey, vote_slots, vote_hash, slot_hashes[j].1
        );
        inc_new_counter_info!("dropped-vote-hash", 1);
        return Err(VoteError::SlotHashMismatch);
    }
    Ok(())
}

//Ensure `check_update_vote_state_slots_are_valid(&)` runs on the slots in `new_state`
// before `process_new_vote_state()` is called

// This function should guarantee the following about `new_state`:
//
// 1) It's well ordered, i.e. the slots are sorted from smallest to largest,
// and the confirmations sorted from largest to smallest.
// 2) Confirmations `c` on any vote slot satisfy `0 < c <= MAX_LOCKOUT_HISTORY`
// 3) Lockouts are not expired by consecutive votes, i.e. for every consecutive
// `v_i`, `v_{i + 1}` satisfy `v_i.last_locked_out_slot() >= v_{i + 1}`.

// We also guarantee that compared to the current vote state, `new_state`
// introduces no rollback. This means:
//
// 1) The last slot in `new_state` is always greater than any slot in the
// current vote state.
//
// 2) From 1), this means that for every vote `s` in the current state:
//    a) If there exists an `s'` in `new_state` where `s.slot == s'.slot`, then
//    we must guarantee `s.confirmations <= s'.confirmations`
//
//    b) If there does not exist any such `s'` in `new_state`, then there exists
//    some `t` that is the smallest vote in `new_state` where `t.slot > s.slot`.
//    `t` must have expired/popped off s', so it must be guaranteed that
//    `s.last_locked_out_slot() < t`.

// Note these two above checks do not guarantee that the vote state being submitted
// is a vote state that could have been created by iteratively building a tower
// by processing one vote at a time. For instance, the tower:
//
// { slot 0, confirmations: 31 }
// { slot 1, confirmations: 30 }
//
// is a legal tower that could be submitted on top of a previously empty tower. However,
// there is no way to create this tower from the iterative process, because slot 1 would
// have to have at least one other slot on top of it, even if the first 30 votes were all
// popped off.
pub fn process_new_vote_state(
    vote_state: &mut VoteState,
    mut new_state: VecDeque<LandedVote>,
    new_root: Option<Slot>,
    timestamp: Option<i64>,
    epoch: Epoch,
    current_slot: Slot,
    feature_set: Option<&FeatureSet>,
) -> Result<(), VoteError> {
    assert!(!new_state.is_empty());
    if new_state.len() > MAX_LOCKOUT_HISTORY {
        return Err(VoteError::TooManyVotes);
    }

    match (new_root, vote_state.root_slot) {
        (Some(new_root), Some(current_root)) => {
            if new_root < current_root {
                return Err(VoteError::RootRollBack);
            }
        }
        (None, Some(_)) => {
            return Err(VoteError::RootRollBack);
        }
        _ => (),
    }

    let mut previous_vote: Option<&LandedVote> = None;

    // Check that all the votes in the new proposed state are:
    // 1) Strictly sorted from oldest to newest vote
    // 2) The confirmations are strictly decreasing
    // 3) Not zero confirmation votes
    for vote in &new_state {
        if vote.confirmation_count() == 0 {
            return Err(VoteError::ZeroConfirmations);
        } else if vote.confirmation_count() > MAX_LOCKOUT_HISTORY as u32 {
            return Err(VoteError::ConfirmationTooLarge);
        } else if let Some(new_root) = new_root {
            if vote.slot() <= new_root
                &&
                // This check is necessary because
                // https://github.com/ryoqun/solana/blob/df55bfb46af039cbc597cd60042d49b9d90b5961/core/src/consensus.rs#L120
                // always sets a root for even empty towers, which is then hard unwrapped here
                // https://github.com/ryoqun/solana/blob/df55bfb46af039cbc597cd60042d49b9d90b5961/core/src/consensus.rs#L776
                new_root != Slot::default()
            {
                return Err(VoteError::SlotSmallerThanRoot);
            }
        }

        if let Some(previous_vote) = previous_vote {
            if previous_vote.slot() >= vote.slot() {
                return Err(VoteError::SlotsNotOrdered);
            } else if previous_vote.confirmation_count() <= vote.confirmation_count() {
                return Err(VoteError::ConfirmationsNotOrdered);
            } else if vote.slot() > previous_vote.lockout.last_locked_out_slot() {
                return Err(VoteError::NewVoteStateLockoutMismatch);
            }
        }
        previous_vote = Some(vote);
    }

    // Find the first vote in the current vote state for a slot greater
    // than the new proposed root
    let mut current_vote_state_index: usize = 0;
    let mut new_vote_state_index = 0;

    // Accumulate credits earned by newly rooted slots.  The behavior changes with timely_vote_credits: prior to
    // this feature, there was a bug that counted a new root slot as 1 credit even if it had never been voted on.
    // timely_vote_credits fixes this bug by only awarding credits for slots actually voted on and finalized.
    let timely_vote_credits = feature_set.map_or(false, |f| {
        f.is_active(&feature_set::timely_vote_credits::id())
    });
    let mut earned_credits = if timely_vote_credits { 0_u64 } else { 1_u64 };

    if let Some(new_root) = new_root {
        for current_vote in &vote_state.votes {
            // Find the first vote in the current vote state for a slot greater
            // than the new proposed root
            if current_vote.slot() <= new_root {
                if timely_vote_credits || (current_vote.slot() != new_root) {
                    earned_credits = earned_credits
                        .checked_add(vote_state.credits_for_vote_at_index(current_vote_state_index))
                        .expect("`earned_credits` does not overflow");
                }
                current_vote_state_index = current_vote_state_index
                    .checked_add(1)
                    .expect("`current_vote_state_index` is bounded by `MAX_LOCKOUT_HISTORY` when processing new root");
                continue;
            }

            break;
        }
    }

    // For any slots newly added to the new vote state, the vote latency of that slot is not provided by the
    // VoteStateUpdate instruction contents, but instead is computed from the actual latency of the VoteStateUpdate
    // instruction. This prevents other validators from manipulating their own vote latencies within their vote states
    // and forcing the rest of the cluster to accept these possibly fraudulent latency values.  If the
    // timly_vote_credits feature is not enabled then vote latency is set to 0 for new votes.
    //
    // For any slot that is in both the new state and the current state, the vote latency of the new state is taken
    // from the current state.
    //
    // Thus vote latencies are set here for any newly vote-on slots when a VoteStateUpdate instruction is received.
    // They are copied into the new vote state after every VoteStateUpdate for already voted-on slots.
    // And when voted-on slots are rooted, the vote latencies stored in the vote state of all the rooted slots is used
    // to compute credits earned.
    // All validators compute the same vote latencies because all process the same VoteStateUpdate instruction at the
    // same slot, and the only time vote latencies are ever computed is at the time that their slot is first voted on;
    // after that, the latencies are retained unaltered until the slot is rooted.

    // All the votes in our current vote state that are missing from the new vote state
    // must have been expired by later votes. Check that the lockouts match this assumption.
    while current_vote_state_index < vote_state.votes.len()
        && new_vote_state_index < new_state.len()
    {
        let current_vote = &vote_state.votes[current_vote_state_index];
        let new_vote = &mut new_state[new_vote_state_index];

        // If the current slot is less than the new proposed slot, then the
        // new slot must have popped off the old slot, so check that the
        // lockouts are corrects.
        match current_vote.slot().cmp(&new_vote.slot()) {
            Ordering::Less => {
                if current_vote.lockout.last_locked_out_slot() >= new_vote.slot() {
                    return Err(VoteError::LockoutConflict);
                }
                current_vote_state_index = current_vote_state_index
                    .checked_add(1)
                    .expect("`current_vote_state_index` is bounded by `MAX_LOCKOUT_HISTORY` when slot is less than proposed");
            }
            Ordering::Equal => {
                // The new vote state should never have less lockout than
                // the previous vote state for the same slot
                if new_vote.confirmation_count() < current_vote.confirmation_count() {
                    return Err(VoteError::ConfirmationRollBack);
                }

                // Copy the vote slot latency in from the current state to the new state
                new_vote.latency = vote_state.votes[current_vote_state_index].latency;

                current_vote_state_index = current_vote_state_index
                    .checked_add(1)
                    .expect("`current_vote_state_index` is bounded by `MAX_LOCKOUT_HISTORY` when slot is equal to proposed");
                new_vote_state_index = new_vote_state_index
                    .checked_add(1)
                    .expect("`new_vote_state_index` is bounded by `MAX_LOCKOUT_HISTORY` when slot is equal to proposed");
            }
            Ordering::Greater => {
                new_vote_state_index = new_vote_state_index
                    .checked_add(1)
                    .expect("`new_vote_state_index` is bounded by `MAX_LOCKOUT_HISTORY` when slot is greater than proposed");
            }
        }
    }

    // `new_vote_state` passed all the checks, finalize the change by rewriting
    // our state.

    // Now set the vote latencies on new slots not in the current state.  New slots not in the current vote state will
    // have had their latency initialized to 0 by the above loop.  Those will now be updated to their actual latency.
    // If the timely_vote_credits feature is not enabled, then the latency is left as 0 for such slots, which will
    // result in 1 credit per slot when credits are calculated at the time that the slot is rooted.
    if timely_vote_credits {
        for new_vote in new_state.iter_mut() {
            if new_vote.latency == 0 {
                new_vote.latency = VoteState::compute_vote_latency(new_vote.slot(), current_slot);
            }
        }
    }

    if vote_state.root_slot != new_root {
        // Award vote credits based on the number of slots that were voted on and have reached finality
        // For each finalized slot, there was one voted-on slot in the new vote state that was responsible for
        // finalizing it. Each of those votes is awarded 1 credit.
        vote_state.increment_credits(epoch, earned_credits);
    }
    if let Some(timestamp) = timestamp {
        let last_slot = new_state.back().unwrap().slot();
        vote_state.process_timestamp(last_slot, timestamp)?;
    }
    vote_state.root_slot = new_root;
    vote_state.votes = new_state;

    Ok(())
}

pub fn process_vote_unfiltered(
    vote_state: &mut VoteState,
    vote_slots: &[Slot],
    vote: &Vote,
    slot_hashes: &[SlotHash],
    epoch: Epoch,
    current_slot: Slot,
) -> Result<(), VoteError> {
    check_slots_are_valid(vote_state, vote_slots, &vote.hash, slot_hashes)?;
    vote_slots
        .iter()
        .for_each(|s| vote_state.process_next_vote_slot(*s, epoch, current_slot));
    Ok(())
}

pub fn process_vote(
    vote_state: &mut VoteState,
    vote: &Vote,
    slot_hashes: &[SlotHash],
    epoch: Epoch,
    current_slot: Slot,
) -> Result<(), VoteError> {
    if vote.slots.is_empty() {
        return Err(VoteError::EmptySlots);
    }
    let earliest_slot_in_history = slot_hashes.last().map(|(slot, _hash)| *slot).unwrap_or(0);
    let vote_slots = vote
        .slots
        .iter()
        .filter(|slot| **slot >= earliest_slot_in_history)
        .cloned()
        .collect::<Vec<Slot>>();
    if vote_slots.is_empty() {
        return Err(VoteError::VotesTooOldAllFiltered);
    }
    process_vote_unfiltered(
        vote_state,
        &vote_slots,
        vote,
        slot_hashes,
        epoch,
        current_slot,
    )
}

/// "unchecked" functions used by tests and Tower
pub fn process_vote_unchecked(vote_state: &mut VoteState, vote: Vote) -> Result<(), VoteError> {
    if vote.slots.is_empty() {
        return Err(VoteError::EmptySlots);
    }
    let slot_hashes: Vec<_> = vote.slots.iter().rev().map(|x| (*x, vote.hash)).collect();
    process_vote_unfiltered(
        vote_state,
        &vote.slots,
        &vote,
        &slot_hashes,
        vote_state.current_epoch(),
        0,
    )
}

#[cfg(test)]
pub fn process_slot_votes_unchecked(vote_state: &mut VoteState, slots: &[Slot]) {
    for slot in slots {
        process_slot_vote_unchecked(vote_state, *slot);
    }
}

pub fn process_slot_vote_unchecked(vote_state: &mut VoteState, slot: Slot) {
    let _ = process_vote_unchecked(vote_state, Vote::new(vec![slot], Hash::default()));
}

/// Authorize the given pubkey to withdraw or sign votes. This may be called multiple times,
/// but will implicitly withdraw authorization from the previously authorized
/// key
pub fn authorize<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    authorized: &Pubkey,
    vote_authorize: VoteAuthorize,
    signers: &HashSet<Pubkey, S>,
    clock: &Clock,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState = vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    match vote_authorize {
        VoteAuthorize::Voter => {
            let authorized_withdrawer_signer =
                verify_authorized_signer(&vote_state.authorized_withdrawer, signers).is_ok();

            vote_state.set_new_authorized_voter(
                authorized,
                clock.epoch,
                clock
                    .leader_schedule_epoch
                    .checked_add(1)
                    .expect("epoch should be much less than u64::MAX"),
                |epoch_authorized_voter| {
                    // current authorized withdrawer or authorized voter must say "yay"
                    if authorized_withdrawer_signer {
                        Ok(())
                    } else {
                        verify_authorized_signer(&epoch_authorized_voter, signers)
                    }
                },
            )?;
        }
        VoteAuthorize::Withdrawer => {
            // current authorized withdrawer must say "yay"
            verify_authorized_signer(&vote_state.authorized_withdrawer, signers)?;
            vote_state.authorized_withdrawer = *authorized;
        }
    }

    set_vote_account_state(vote_account, vote_state, feature_set)
}

/// Update the node_pubkey, requires signature of the authorized voter
pub fn update_validator_identity<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    node_pubkey: &Pubkey,
    signers: &HashSet<Pubkey, S>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState = vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    // current authorized withdrawer must say "yay"
    verify_authorized_signer(&vote_state.authorized_withdrawer, signers)?;

    // new node must say "yay"
    verify_authorized_signer(node_pubkey, signers)?;

    vote_state.node_pubkey = *node_pubkey;

    set_vote_account_state(vote_account, vote_state, feature_set)
}

/// Update the vote account's commission
pub fn update_commission<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    commission: u8,
    signers: &HashSet<Pubkey, S>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut vote_state: VoteState = vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    // current authorized withdrawer must say "yay"
    verify_authorized_signer(&vote_state.authorized_withdrawer, signers)?;

    vote_state.commission = commission;

    set_vote_account_state(vote_account, vote_state, feature_set)
}

/// Given the current slot and epoch schedule, determine if a commission change
/// is allowed
pub fn is_commission_update_allowed(slot: Slot, epoch_schedule: &EpochSchedule) -> bool {
    // always allowed during warmup epochs
    if let Some(relative_slot) = slot
        .saturating_sub(epoch_schedule.first_normal_slot)
        .checked_rem(epoch_schedule.slots_per_epoch)
    {
        // allowed up to the midpoint of the epoch
        relative_slot.saturating_mul(2) <= epoch_schedule.slots_per_epoch
    } else {
        // no slots per epoch, just allow it, even though this should never happen
        true
    }
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
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    vote_account_index: IndexOfAccount,
    lamports: u64,
    to_account_index: IndexOfAccount,
    signers: &HashSet<Pubkey, S>,
    rent_sysvar: &Rent,
    clock: &Clock,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, vote_account_index)?;
    let vote_state: VoteState = vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    verify_authorized_signer(&vote_state.authorized_withdrawer, signers)?;

    let remaining_balance = vote_account
        .get_lamports()
        .checked_sub(lamports)
        .ok_or(InstructionError::InsufficientFunds)?;

    if remaining_balance == 0 {
        let reject_active_vote_account_close = vote_state
            .epoch_credits
            .last()
            .map(|(last_epoch_with_credits, _, _)| {
                let current_epoch = clock.epoch;
                // if current_epoch - last_epoch_with_credits < 2 then the validator has received credits
                // either in the current epoch or the previous epoch. If it's >= 2 then it has been at least
                // one full epoch since the validator has received credits.
                current_epoch.saturating_sub(*last_epoch_with_credits) < 2
            })
            .unwrap_or(false);

        if reject_active_vote_account_close {
            datapoint_debug!("vote-account-close", ("reject-active", 1, i64));
            return Err(VoteError::ActiveVoteAccountClose.into());
        } else {
            // Deinitialize upon zero-balance
            datapoint_debug!("vote-account-close", ("allow", 1, i64));
            set_vote_account_state(&mut vote_account, VoteState::default(), feature_set)?;
        }
    } else {
        let min_rent_exempt_balance = rent_sysvar.minimum_balance(vote_account.get_data().len());
        if remaining_balance < min_rent_exempt_balance {
            return Err(InstructionError::InsufficientFunds);
        }
    }

    vote_account.checked_sub_lamports(lamports)?;
    drop(vote_account);
    let mut to_account = instruction_context
        .try_borrow_instruction_account(transaction_context, to_account_index)?;
    to_account.checked_add_lamports(lamports)?;
    Ok(())
}

/// Initialize the vote_state for a vote account
/// Assumes that the account is being init as part of a account creation or balance transfer and
/// that the transaction must be signed by the staker's keys
pub fn initialize_account<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    vote_init: &VoteInit,
    signers: &HashSet<Pubkey, S>,
    clock: &Clock,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    if vote_account.get_data().len()
        != VoteStateVersions::vote_state_size_of(
            feature_set.is_active(&feature_set::vote_state_add_vote_latency::id()),
        )
    {
        return Err(InstructionError::InvalidAccountData);
    }
    let versioned = vote_account.get_state::<VoteStateVersions>()?;

    if !versioned.is_uninitialized() {
        return Err(InstructionError::AccountAlreadyInitialized);
    }

    // node must agree to accept this vote account
    verify_authorized_signer(&vote_init.node_pubkey, signers)?;

    set_vote_account_state(vote_account, VoteState::new(vote_init, clock), feature_set)
}

fn verify_and_get_vote_state<S: std::hash::BuildHasher>(
    vote_account: &BorrowedAccount,
    clock: &Clock,
    signers: &HashSet<Pubkey, S>,
) -> Result<VoteState, InstructionError> {
    let versioned = vote_account.get_state::<VoteStateVersions>()?;

    if versioned.is_uninitialized() {
        return Err(InstructionError::UninitializedAccount);
    }

    let mut vote_state = versioned.convert_to_current();
    let authorized_voter = vote_state.get_and_update_authorized_voter(clock.epoch)?;
    verify_authorized_signer(&authorized_voter, signers)?;

    Ok(vote_state)
}

pub fn process_vote_with_account<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    slot_hashes: &[SlotHash],
    clock: &Clock,
    vote: &Vote,
    signers: &HashSet<Pubkey, S>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut vote_state = verify_and_get_vote_state(vote_account, clock, signers)?;

    process_vote(&mut vote_state, vote, slot_hashes, clock.epoch, clock.slot)?;
    if let Some(timestamp) = vote.timestamp {
        vote.slots
            .iter()
            .max()
            .ok_or(VoteError::EmptySlots)
            .and_then(|slot| vote_state.process_timestamp(*slot, timestamp))?;
    }
    set_vote_account_state(vote_account, vote_state, feature_set)
}

pub fn process_vote_state_update<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    slot_hashes: &[SlotHash],
    clock: &Clock,
    vote_state_update: VoteStateUpdate,
    signers: &HashSet<Pubkey, S>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut vote_state = verify_and_get_vote_state(vote_account, clock, signers)?;
    do_process_vote_state_update(
        &mut vote_state,
        slot_hashes,
        clock.epoch,
        clock.slot,
        vote_state_update,
        Some(feature_set),
    )?;
    set_vote_account_state(vote_account, vote_state, feature_set)
}

pub fn do_process_vote_state_update(
    vote_state: &mut VoteState,
    slot_hashes: &[SlotHash],
    epoch: u64,
    slot: u64,
    mut vote_state_update: VoteStateUpdate,
    feature_set: Option<&FeatureSet>,
) -> Result<(), VoteError> {
    check_update_vote_state_slots_are_valid(vote_state, &mut vote_state_update, slot_hashes)?;
    process_new_vote_state(
        vote_state,
        vote_state_update
            .lockouts
            .iter()
            .map(|lockout| LandedVote::from(*lockout))
            .collect(),
        vote_state_update.root,
        vote_state_update.timestamp,
        epoch,
        slot,
        feature_set,
    )
}

// This function is used:
// a. In many tests.
// b. In the genesis tool that initializes a cluster to create the bootstrap validator.
// c. In the ledger tool when creating bootstrap vote accounts.
pub fn create_account_with_authorized(
    node_pubkey: &Pubkey,
    authorized_voter: &Pubkey,
    authorized_withdrawer: &Pubkey,
    commission: u8,
    lamports: u64,
) -> AccountSharedData {
    let mut vote_account = AccountSharedData::new(lamports, VoteState::size_of(), &id());

    let vote_state = VoteState::new(
        &VoteInit {
            node_pubkey: *node_pubkey,
            authorized_voter: *authorized_voter,
            authorized_withdrawer: *authorized_withdrawer,
            commission,
        },
        &Clock::default(),
    );

    VoteState::serialize(
        &VoteStateVersions::Current(Box::new(vote_state)),
        vote_account.data_as_mut_slice(),
    )
    .unwrap();

    vote_account
}

// create_account() should be removed, use create_account_with_authorized() instead
pub fn create_account(
    vote_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
    commission: u8,
    lamports: u64,
) -> AccountSharedData {
    create_account_with_authorized(node_pubkey, vote_pubkey, vote_pubkey, commission, lamports)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::vote_state,
        assert_matches::assert_matches,
        solana_sdk::{
            account::AccountSharedData, account_utils::StateMut, clock::DEFAULT_SLOTS_PER_EPOCH,
            hash::hash, transaction_context::InstructionAccount,
        },
        std::cell::RefCell,
        test_case::test_case,
    };

    const MAX_RECENT_VOTES: usize = 16;

    fn vote_state_new_for_test(auth_pubkey: &Pubkey) -> VoteState {
        VoteState::new(
            &VoteInit {
                node_pubkey: solana_sdk::pubkey::new_rand(),
                authorized_voter: *auth_pubkey,
                authorized_withdrawer: *auth_pubkey,
                commission: 0,
            },
            &Clock::default(),
        )
    }

    fn create_test_account() -> (Pubkey, RefCell<AccountSharedData>) {
        let rent = Rent::default();
        let balance = VoteState::get_rent_exempt_reserve(&rent);
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        (
            vote_pubkey,
            RefCell::new(vote_state::create_account(
                &vote_pubkey,
                &solana_sdk::pubkey::new_rand(),
                0,
                balance,
            )),
        )
    }

    #[test]
    fn test_vote_state_upgrade_from_1_14_11() {
        let mut feature_set = FeatureSet::default();

        // Create an initial vote account that is sized for the 1_14_11 version of vote state, and has only the
        // required lamports for rent exempt minimum at that size
        let node_pubkey = solana_sdk::pubkey::new_rand();
        let withdrawer_pubkey = solana_sdk::pubkey::new_rand();
        let mut vote_state = VoteState::new(
            &VoteInit {
                node_pubkey,
                authorized_voter: withdrawer_pubkey,
                authorized_withdrawer: withdrawer_pubkey,
                commission: 10,
            },
            &Clock::default(),
        );
        // Simulate prior epochs completed with credits and each setting a new authorized voter
        vote_state.increment_credits(0, 100);
        assert_eq!(
            vote_state
                .set_new_authorized_voter(&solana_sdk::pubkey::new_rand(), 0, 1, |_pubkey| Ok(())),
            Ok(())
        );
        vote_state.increment_credits(1, 200);
        assert_eq!(
            vote_state
                .set_new_authorized_voter(&solana_sdk::pubkey::new_rand(), 1, 2, |_pubkey| Ok(())),
            Ok(())
        );
        vote_state.increment_credits(2, 300);
        assert_eq!(
            vote_state
                .set_new_authorized_voter(&solana_sdk::pubkey::new_rand(), 2, 3, |_pubkey| Ok(())),
            Ok(())
        );
        // Simulate votes having occurred
        vec![
            100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116,
            117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133,
            134, 135,
        ]
        .into_iter()
        .for_each(|v| vote_state.process_next_vote_slot(v, 4, 0));

        let version1_14_11_serialized = bincode::serialize(&VoteStateVersions::V1_14_11(Box::new(
            VoteState1_14_11::from(vote_state.clone()),
        )))
        .unwrap();

        let version1_14_11_serialized_len = version1_14_11_serialized.len();
        let rent = Rent::default();
        let lamports = rent.minimum_balance(version1_14_11_serialized_len);
        let mut vote_account =
            AccountSharedData::new(lamports, version1_14_11_serialized_len, &id());
        vote_account.set_data_from_slice(&version1_14_11_serialized);

        // Create a fake TransactionContext with a fake InstructionContext with a single account which is the
        // vote account that was just created
        let transaction_context =
            TransactionContext::new(vec![(node_pubkey, vote_account)], None, 0, 0);
        let mut instruction_context = InstructionContext::default();
        instruction_context.configure(
            &[0],
            &[InstructionAccount {
                index_in_transaction: 0,
                index_in_caller: 0,
                index_in_callee: 0,
                is_signer: false,
                is_writable: true,
            }],
            &[],
        );

        // Get the BorrowedAccount from the InstructionContext which is what is used to manipulate and inspect account
        // state
        let mut borrowed_account = instruction_context
            .try_borrow_instruction_account(&transaction_context, 0)
            .unwrap();

        // Ensure that the vote state started out at 1_14_11
        let vote_state_version = borrowed_account.get_state::<VoteStateVersions>().unwrap();
        assert_matches!(vote_state_version, VoteStateVersions::V1_14_11(_));

        // Convert the vote state to current as would occur during vote instructions
        let converted_vote_state = vote_state_version.convert_to_current();

        // Check to make sure that the vote_state is unchanged
        assert!(vote_state == converted_vote_state);

        let vote_state = converted_vote_state;

        // Now re-set the vote account state; because the feature is not enabled, the old 1_14_11 format should be
        // written out
        assert_eq!(
            set_vote_account_state(&mut borrowed_account, vote_state.clone(), &feature_set),
            Ok(())
        );
        let vote_state_version = borrowed_account.get_state::<VoteStateVersions>().unwrap();
        assert_matches!(vote_state_version, VoteStateVersions::V1_14_11(_));

        // Convert the vote state to current as would occur during vote instructions
        let converted_vote_state = vote_state_version.convert_to_current();

        // Check to make sure that the vote_state is unchanged
        assert_eq!(vote_state, converted_vote_state);

        let vote_state = converted_vote_state;

        // Test that when the feature is enabled, if the vote account does not have sufficient lamports to realloc,
        // the old vote state is written out
        feature_set.activate(&feature_set::vote_state_add_vote_latency::id(), 1);
        assert_eq!(
            set_vote_account_state(&mut borrowed_account, vote_state.clone(), &feature_set),
            Ok(())
        );
        let vote_state_version = borrowed_account.get_state::<VoteStateVersions>().unwrap();
        assert_matches!(vote_state_version, VoteStateVersions::V1_14_11(_));

        // Convert the vote state to current as would occur during vote instructions
        let converted_vote_state = vote_state_version.convert_to_current();

        // Check to make sure that the vote_state is unchanged
        assert_eq!(vote_state, converted_vote_state);

        let vote_state = converted_vote_state;

        // Test that when the feature is enabled, if the vote account does have sufficient lamports, the
        // new vote state is written out
        assert_eq!(
            borrowed_account.set_lamports(rent.minimum_balance(VoteState::size_of())),
            Ok(())
        );
        assert_eq!(
            set_vote_account_state(&mut borrowed_account, vote_state.clone(), &feature_set),
            Ok(())
        );
        let vote_state_version = borrowed_account.get_state::<VoteStateVersions>().unwrap();
        assert_matches!(vote_state_version, VoteStateVersions::Current(_));

        // Convert the vote state to current as would occur during vote instructions
        let converted_vote_state = vote_state_version.convert_to_current();

        // Check to make sure that the vote_state is unchanged
        assert_eq!(vote_state, converted_vote_state);
    }

    #[test]
    fn test_vote_lockout() {
        let (_vote_pubkey, vote_account) = create_test_account();

        let mut vote_state: VoteState =
            StateMut::<VoteStateVersions>::state(&*vote_account.borrow())
                .unwrap()
                .convert_to_current();

        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            process_slot_vote_unchecked(&mut vote_state, (INITIAL_LOCKOUT * i) as u64);
        }

        // The last vote should have been popped b/c it reached a depth of MAX_LOCKOUT_HISTORY
        assert_eq!(vote_state.votes.len(), MAX_LOCKOUT_HISTORY);
        assert_eq!(vote_state.root_slot, Some(0));
        check_lockouts(&vote_state);

        // One more vote that confirms the entire stack,
        // the root_slot should change to the
        // second vote
        let top_vote = vote_state.votes.front().unwrap().slot();
        let slot = vote_state.last_lockout().unwrap().last_locked_out_slot();
        process_slot_vote_unchecked(&mut vote_state, slot);
        assert_eq!(Some(top_vote), vote_state.root_slot);

        // Expire everything except the first vote
        let slot = vote_state
            .votes
            .front()
            .unwrap()
            .lockout
            .last_locked_out_slot();
        process_slot_vote_unchecked(&mut vote_state, slot);
        // First vote and new vote are both stored for a total of 2 votes
        assert_eq!(vote_state.votes.len(), 2);
    }

    #[test]
    fn test_vote_double_lockout_after_expiration() {
        let voter_pubkey = solana_sdk::pubkey::new_rand();
        let mut vote_state = vote_state_new_for_test(&voter_pubkey);

        for i in 0..3 {
            process_slot_vote_unchecked(&mut vote_state, i as u64);
        }

        check_lockouts(&vote_state);

        // Expire the third vote (which was a vote for slot 2). The height of the
        // vote stack is unchanged, so none of the previous votes should have
        // doubled in lockout
        process_slot_vote_unchecked(&mut vote_state, (2 + INITIAL_LOCKOUT + 1) as u64);
        check_lockouts(&vote_state);

        // Vote again, this time the vote stack depth increases, so the votes should
        // double for everybody
        process_slot_vote_unchecked(&mut vote_state, (2 + INITIAL_LOCKOUT + 2) as u64);
        check_lockouts(&vote_state);

        // Vote again, this time the vote stack depth increases, so the votes should
        // double for everybody
        process_slot_vote_unchecked(&mut vote_state, (2 + INITIAL_LOCKOUT + 3) as u64);
        check_lockouts(&vote_state);
    }

    #[test]
    fn test_expire_multiple_votes() {
        let voter_pubkey = solana_sdk::pubkey::new_rand();
        let mut vote_state = vote_state_new_for_test(&voter_pubkey);

        for i in 0..3 {
            process_slot_vote_unchecked(&mut vote_state, i as u64);
        }

        assert_eq!(vote_state.votes[0].confirmation_count(), 3);

        // Expire the second and third votes
        let expire_slot = vote_state.votes[1].slot() + vote_state.votes[1].lockout.lockout() + 1;
        process_slot_vote_unchecked(&mut vote_state, expire_slot);
        assert_eq!(vote_state.votes.len(), 2);

        // Check that the old votes expired
        assert_eq!(vote_state.votes[0].slot(), 0);
        assert_eq!(vote_state.votes[1].slot(), expire_slot);

        // Process one more vote
        process_slot_vote_unchecked(&mut vote_state, expire_slot + 1);

        // Confirmation count for the older first vote should remain unchanged
        assert_eq!(vote_state.votes[0].confirmation_count(), 3);

        // The later votes should still have increasing confirmation counts
        assert_eq!(vote_state.votes[1].confirmation_count(), 2);
        assert_eq!(vote_state.votes[2].confirmation_count(), 1);
    }

    #[test]
    fn test_vote_credits() {
        let voter_pubkey = solana_sdk::pubkey::new_rand();
        let mut vote_state = vote_state_new_for_test(&voter_pubkey);

        for i in 0..MAX_LOCKOUT_HISTORY {
            process_slot_vote_unchecked(&mut vote_state, i as u64);
        }

        assert_eq!(vote_state.credits(), 0);

        process_slot_vote_unchecked(&mut vote_state, MAX_LOCKOUT_HISTORY as u64 + 1);
        assert_eq!(vote_state.credits(), 1);
        process_slot_vote_unchecked(&mut vote_state, MAX_LOCKOUT_HISTORY as u64 + 2);
        assert_eq!(vote_state.credits(), 2);
        process_slot_vote_unchecked(&mut vote_state, MAX_LOCKOUT_HISTORY as u64 + 3);
        assert_eq!(vote_state.credits(), 3);
    }

    #[test]
    fn test_duplicate_vote() {
        let voter_pubkey = solana_sdk::pubkey::new_rand();
        let mut vote_state = vote_state_new_for_test(&voter_pubkey);
        process_slot_vote_unchecked(&mut vote_state, 0);
        process_slot_vote_unchecked(&mut vote_state, 1);
        process_slot_vote_unchecked(&mut vote_state, 0);
        assert_eq!(vote_state.nth_recent_lockout(0).unwrap().slot(), 1);
        assert_eq!(vote_state.nth_recent_lockout(1).unwrap().slot(), 0);
        assert!(vote_state.nth_recent_lockout(2).is_none());
    }

    #[test]
    fn test_nth_recent_lockout() {
        let voter_pubkey = solana_sdk::pubkey::new_rand();
        let mut vote_state = vote_state_new_for_test(&voter_pubkey);
        for i in 0..MAX_LOCKOUT_HISTORY {
            process_slot_vote_unchecked(&mut vote_state, i as u64);
        }
        for i in 0..(MAX_LOCKOUT_HISTORY - 1) {
            assert_eq!(
                vote_state.nth_recent_lockout(i).unwrap().slot() as usize,
                MAX_LOCKOUT_HISTORY - i - 1,
            );
        }
        assert!(vote_state.nth_recent_lockout(MAX_LOCKOUT_HISTORY).is_none());
    }

    fn check_lockouts(vote_state: &VoteState) {
        for (i, vote) in vote_state.votes.iter().enumerate() {
            let num_votes = vote_state
                .votes
                .len()
                .checked_sub(i)
                .expect("`i` is less than `vote_state.votes.len()`");
            assert_eq!(
                vote.lockout.lockout(),
                INITIAL_LOCKOUT.pow(num_votes as u32) as u64
            );
        }
    }

    fn recent_votes(vote_state: &VoteState) -> Vec<Vote> {
        let start = vote_state.votes.len().saturating_sub(MAX_RECENT_VOTES);
        (start..vote_state.votes.len())
            .map(|i| {
                Vote::new(
                    vec![vote_state.votes.get(i).unwrap().slot()],
                    Hash::default(),
                )
            })
            .collect()
    }

    /// check that two accounts with different data can be brought to the same state with one vote submission
    #[test]
    fn test_process_missed_votes() {
        let account_a = solana_sdk::pubkey::new_rand();
        let mut vote_state_a = vote_state_new_for_test(&account_a);
        let account_b = solana_sdk::pubkey::new_rand();
        let mut vote_state_b = vote_state_new_for_test(&account_b);

        // process some votes on account a
        (0..5).for_each(|i| process_slot_vote_unchecked(&mut vote_state_a, i as u64));
        assert_ne!(recent_votes(&vote_state_a), recent_votes(&vote_state_b));

        // as long as b has missed less than "NUM_RECENT" votes both accounts should be in sync
        let slots = (0u64..MAX_RECENT_VOTES as u64).collect();
        let vote = Vote::new(slots, Hash::default());
        let slot_hashes: Vec<_> = vote.slots.iter().rev().map(|x| (*x, vote.hash)).collect();

        assert_eq!(
            process_vote(&mut vote_state_a, &vote, &slot_hashes, 0, 0),
            Ok(())
        );
        assert_eq!(
            process_vote(&mut vote_state_b, &vote, &slot_hashes, 0, 0),
            Ok(())
        );
        assert_eq!(recent_votes(&vote_state_a), recent_votes(&vote_state_b));
    }

    #[test]
    fn test_process_vote_skips_old_vote() {
        let mut vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(0, vote.hash)];
        assert_eq!(
            process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0),
            Ok(())
        );
        let recent = recent_votes(&vote_state);
        assert_eq!(
            process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0),
            Err(VoteError::VoteTooOld)
        );
        assert_eq!(recent, recent_votes(&vote_state));
    }

    #[test]
    fn test_check_slots_are_valid_vote_empty_slot_hashes() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &[]),
            Err(VoteError::VoteTooOld)
        );
    }

    #[test]
    fn test_check_slots_are_valid_new_vote() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &slot_hashes),
            Ok(())
        );
    }

    #[test]
    fn test_check_slots_are_valid_bad_hash() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), hash(vote.hash.as_ref()))];
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &slot_hashes),
            Err(VoteError::SlotHashMismatch)
        );
    }

    #[test]
    fn test_check_slots_are_valid_bad_slot() {
        let vote_state = VoteState::default();

        let vote = Vote::new(vec![1], Hash::default());
        let slot_hashes: Vec<_> = vec![(0, vote.hash)];
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &slot_hashes),
            Err(VoteError::SlotsMismatch)
        );
    }

    #[test]
    fn test_check_slots_are_valid_duplicate_vote() {
        let mut vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(
            process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0),
            Ok(())
        );
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &slot_hashes),
            Err(VoteError::VoteTooOld)
        );
    }

    #[test]
    fn test_check_slots_are_valid_next_vote() {
        let mut vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(
            process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0),
            Ok(())
        );

        let vote = Vote::new(vec![0, 1], Hash::default());
        let slot_hashes: Vec<_> = vec![(1, vote.hash), (0, vote.hash)];
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &slot_hashes),
            Ok(())
        );
    }

    #[test]
    fn test_check_slots_are_valid_next_vote_only() {
        let mut vote_state = VoteState::default();

        let vote = Vote::new(vec![0], Hash::default());
        let slot_hashes: Vec<_> = vec![(*vote.slots.last().unwrap(), vote.hash)];
        assert_eq!(
            process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0),
            Ok(())
        );

        let vote = Vote::new(vec![1], Hash::default());
        let slot_hashes: Vec<_> = vec![(1, vote.hash), (0, vote.hash)];
        assert_eq!(
            check_slots_are_valid(&vote_state, &vote.slots, &vote.hash, &slot_hashes),
            Ok(())
        );
    }
    #[test]
    fn test_process_vote_empty_slots() {
        let mut vote_state = VoteState::default();

        let vote = Vote::new(vec![], Hash::default());
        assert_eq!(
            process_vote(&mut vote_state, &vote, &[], 0, 0),
            Err(VoteError::EmptySlots)
        );
    }

    pub fn process_new_vote_state_from_lockouts(
        vote_state: &mut VoteState,
        new_state: VecDeque<Lockout>,
        new_root: Option<Slot>,
        timestamp: Option<i64>,
        epoch: Epoch,
        feature_set: Option<&FeatureSet>,
    ) -> Result<(), VoteError> {
        process_new_vote_state(
            vote_state,
            new_state.into_iter().map(LandedVote::from).collect(),
            new_root,
            timestamp,
            epoch,
            0,
            feature_set,
        )
    }

    // Test vote credit updates after "one credit per slot" feature is enabled
    #[test]
    fn test_vote_state_update_increment_credits() {
        // Create a new Votestate
        let mut vote_state = VoteState::new(&VoteInit::default(), &Clock::default());

        // Test data: a sequence of groups of votes to simulate having been cast, after each group a vote
        // state update is compared to "normal" vote processing to ensure that credits are earned equally
        let test_vote_groups: Vec<Vec<Slot>> = vec![
            // Initial set of votes that don't dequeue any slots, so no credits earned
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            vec![
                9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
                30, 31,
            ],
            // Now a single vote which should result in the first root and first credit earned
            vec![32],
            // Now another vote, should earn one credit
            vec![33],
            // Two votes in sequence
            vec![34, 35],
            // 3 votes in sequence
            vec![36, 37, 38],
            // 30 votes in sequence
            vec![
                39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59,
                60, 61, 62, 63, 64, 65, 66, 67, 68,
            ],
            // 31 votes in sequence
            vec![
                69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89,
                90, 91, 92, 93, 94, 95, 96, 97, 98, 99,
            ],
            // Votes with expiry
            vec![100, 101, 106, 107, 112, 116, 120, 121, 122, 124],
            // More votes with expiry of a large number of votes
            vec![200, 201],
            vec![
                202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217,
                218, 219, 220, 221, 222, 223, 224, 225, 226,
            ],
            vec![227, 228, 229, 230, 231, 232, 233, 234, 235, 236],
        ];

        let feature_set = FeatureSet::default();

        for vote_group in test_vote_groups {
            // Duplicate vote_state so that the new vote can be applied
            let mut vote_state_after_vote = vote_state.clone();

            process_vote_unchecked(
                &mut vote_state_after_vote,
                Vote {
                    slots: vote_group.clone(),
                    hash: Hash::new_unique(),
                    timestamp: None,
                },
            )
            .unwrap();

            // Now use the resulting new vote state to perform a vote state update on vote_state
            assert_eq!(
                process_new_vote_state(
                    &mut vote_state,
                    vote_state_after_vote.votes,
                    vote_state_after_vote.root_slot,
                    None,
                    0,
                    0,
                    Some(&feature_set)
                ),
                Ok(())
            );

            // And ensure that the credits earned were the same
            assert_eq!(
                vote_state.epoch_credits,
                vote_state_after_vote.epoch_credits
            );
        }
    }

    // Test vote credit updates after "timely vote credits" feature is enabled
    #[test]
    fn test_timely_credits() {
        // Each of the following (Vec<Slot>, Slot, u32) tuples gives a set of slots to cast votes on, a slot in which
        // the vote was cast, and the number of credits that should have been earned by the vote account after this
        // and all prior votes were cast.
        let test_vote_groups: Vec<(Vec<Slot>, Slot, u32)> = vec![
            // Initial set of votes that don't dequeue any slots, so no credits earned
            (
                vec![1, 2, 3, 4, 5, 6, 7, 8],
                9,
                // root: none, no credits earned
                0,
            ),
            (
                vec![
                    9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
                    29, 30, 31,
                ],
                34,
                // lockouts full
                // root: none, no credits earned
                0,
            ),
            // Now a single vote which should result in the first root and first credit earned
            (
                vec![32],
                35,
                // root: 1
                // when slot 1 was voted on in slot 9, it earned 2 credits
                2,
            ),
            // Now another vote, should earn one credit
            (
                vec![33],
                36,
                // root: 2
                // when slot 2 was voted on in slot 9, it earned 3 credits
                2 + 3, // 5
            ),
            // Two votes in sequence
            (
                vec![34, 35],
                37,
                // root: 4
                // when slots 3 and 4 were voted on in slot 9, they earned 4 and 5 credits
                5 + 4 + 5, // 14
            ),
            // 3 votes in sequence
            (
                vec![36, 37, 38],
                39,
                // root: 7
                // slots 5, 6, and 7 earned 6, 7, and 8 credits when voted in slot 9
                14 + 6 + 7 + 8, // 35
            ),
            (
                // 30 votes in sequence
                vec![
                    39, 40, 41, 42, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57,
                    58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68,
                ],
                69,
                // root: 37
                // slot 8 was voted in slot 9, earning 8 credits
                // slots 9 - 25 earned 1 credit when voted in slot 34
                // slot 26, 27, 28, 29, 30, 31 earned 2, 3, 4, 5, 6, 7 credits when voted in slot 34
                // slot 32 earned 7 credits when voted in slot 35
                // slot 33 earned 7 credits when voted in slot 36
                // slot 34 and 35 earned 7 and 8 credits when voted in slot 37
                // slot 36 and 37 earned 7 and 8 credits when voted in slot 39
                35 + 8 + ((25 - 9) + 1) + 2 + 3 + 4 + 5 + 6 + 7 + 7 + 7 + 7 + 8 + 7 + 8, // 131
            ),
            // 31 votes in sequence
            (
                vec![
                    69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88,
                    89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99,
                ],
                100,
                // root: 68
                // slot 38 earned 8 credits when voted in slot 39
                // slot 39 - 60 earned 1 credit each when voted in slot 69
                // slot 61, 62, 63, 64, 65, 66, 67, 68 earned 2, 3, 4, 5, 6, 7, 8, and 8 credits when
                //   voted in slot 69
                131 + 8 + ((60 - 39) + 1) + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 8, // 204
            ),
            // Votes with expiry
            (
                vec![115, 116, 117, 118, 119, 120, 121, 122, 123, 124],
                130,
                // root: 74
                // slots 96 - 114 expire
                // slots 69 - 74 earned 1 credit when voted in slot 100
                204 + ((74 - 69) + 1), // 210
            ),
            // More votes with expiry of a large number of votes
            (
                vec![200, 201],
                202,
                // root: 74
                // slots 119 - 124 expire
                210,
            ),
            (
                vec![
                    202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217,
                    218, 219, 220, 221, 222, 223, 224, 225, 226,
                ],
                227,
                // root: 95
                // slot 75 - 91 earned 1 credit each when voted in slot 100
                // slot 92, 93, 94, 95 earned 2, 3, 4, 5, credits when voted in slot 100
                210 + ((91 - 75) + 1) + 2 + 3 + 4 + 5, // 241
            ),
            (
                vec![227, 228, 229, 230, 231, 232, 233, 234, 235, 236],
                237,
                // root: 205
                // slot 115 - 118 earned 1 credit when voted in slot 130
                // slot 200 and 201 earned 8 credits when voted in slot 202
                // slots 202 - 205 earned 1 credit when voted in slot 227
                241 + 1 + 1 + 1 + 1 + 8 + 8 + 1 + 1 + 1 + 1, // 265
            ),
        ];

        let mut feature_set = FeatureSet::default();
        feature_set.activate(&feature_set::timely_vote_credits::id(), 1);

        // For each vote group, process all vote groups leading up to it and it itself, and ensure that the number of
        // credits earned is correct for both regular votes and vote state updates
        for i in 0..test_vote_groups.len() {
            // Create a new VoteState for vote transaction
            let mut vote_state_1 = VoteState::new(&VoteInit::default(), &Clock::default());
            // Create a new VoteState for vote state update transaction
            let mut vote_state_2 = VoteState::new(&VoteInit::default(), &Clock::default());
            test_vote_groups.iter().take(i + 1).for_each(|vote_group| {
                let vote = Vote {
                    slots: vote_group.0.clone(), //vote_group.0 is the set of slots to cast votes on
                    hash: Hash::new_unique(),
                    timestamp: None,
                };
                let slot_hashes: Vec<_> =
                    vote.slots.iter().rev().map(|x| (*x, vote.hash)).collect();
                assert_eq!(
                    process_vote(
                        &mut vote_state_1,
                        &vote,
                        &slot_hashes,
                        0,
                        vote_group.1 // vote_group.1 is the slot in which the vote was cast
                    ),
                    Ok(())
                );

                assert_eq!(
                    process_new_vote_state(
                        &mut vote_state_2,
                        vote_state_1.votes.clone(),
                        vote_state_1.root_slot,
                        None,
                        0,
                        vote_group.1, // vote_group.1 is the slot in which the vote was cast
                        Some(&feature_set)
                    ),
                    Ok(())
                );
            });

            // Ensure that the credits earned is correct for both vote states
            let vote_group = &test_vote_groups[i];
            assert_eq!(vote_state_1.credits(), vote_group.2 as u64); // vote_group.2 is the expected number of credits
            assert_eq!(vote_state_2.credits(), vote_group.2 as u64); // vote_group.2 is the expected number of credits
        }
    }

    #[test]
    fn test_retroactive_voting_timely_credits() {
        // Each of the following (Vec<(Slot, int)>, Slot, Option<Slot>, u32) tuples gives the following data:
        // Vec<(Slot, int)> -- the set of slots and confirmation_counts that is the VoteStateUpdate
        // Slot -- the slot in which the VoteStateUpdate occurred
        // Option<Slot> -- the root after processing the VoteStateUpdate
        // u32 -- the credits after processing the VoteStateUpdate
        #[allow(clippy::type_complexity)]
        let test_vote_state_updates: Vec<(Vec<(Slot, u32)>, Slot, Option<Slot>, u32)> = vec![
            // VoteStateUpdate to set initial vote state
            (
                vec![(7, 4), (8, 3), (9, 2), (10, 1)],
                11,
                // root: none
                None,
                // no credits earned
                0,
            ),
            // VoteStateUpdate to include the missing slots *prior to previously included slots*
            (
                vec![
                    (1, 10),
                    (2, 9),
                    (3, 8),
                    (4, 7),
                    (5, 6),
                    (6, 5),
                    (7, 4),
                    (8, 3),
                    (9, 2),
                    (10, 1),
                ],
                12,
                // root: none
                None,
                // no credits earned
                0,
            ),
            // Now a single VoteStateUpdate which roots all of the slots from 1 - 10
            (
                vec![
                    (11, 31),
                    (12, 30),
                    (13, 29),
                    (14, 28),
                    (15, 27),
                    (16, 26),
                    (17, 25),
                    (18, 24),
                    (19, 23),
                    (20, 22),
                    (21, 21),
                    (22, 20),
                    (23, 19),
                    (24, 18),
                    (25, 17),
                    (26, 16),
                    (27, 15),
                    (28, 14),
                    (29, 13),
                    (30, 12),
                    (31, 11),
                    (32, 10),
                    (33, 9),
                    (34, 8),
                    (35, 7),
                    (36, 6),
                    (37, 5),
                    (38, 4),
                    (39, 3),
                    (40, 2),
                    (41, 1),
                ],
                42,
                // root: 10
                Some(10),
                // when slots 1 - 6 were voted on in slot 12, they earned 1, 1, 1, 2, 3, and 4 credits
                // when slots 7 - 10 were voted on in slot 11, they earned 6, 7, 8, and 8 credits
                1 + 1 + 1 + 2 + 3 + 4 + 6 + 7 + 8 + 8,
            ),
        ];

        let mut feature_set = FeatureSet::default();
        feature_set.activate(&feature_set::timely_vote_credits::id(), 1);

        // Retroactive voting is only possible with VoteStateUpdate transactions, which is why Vote transactions are
        // not tested here

        // Initial vote state
        let mut vote_state = VoteState::new(&VoteInit::default(), &Clock::default());

        // Process the vote state updates in sequence and ensure that the credits earned after each is processed is
        // correct
        test_vote_state_updates
            .iter()
            .for_each(|vote_state_update| {
                let new_state = vote_state_update
                    .0 // vote_state_update.0 is the set of slots and confirmation_counts that is the VoteStateUpdate
                    .iter()
                    .map(|(slot, confirmation_count)| LandedVote {
                        latency: 0,
                        lockout: Lockout::new_with_confirmation_count(*slot, *confirmation_count),
                    })
                    .collect::<VecDeque<LandedVote>>();
                assert_eq!(
                    process_new_vote_state(
                        &mut vote_state,
                        new_state,
                        vote_state_update.2, // vote_state_update.2 is root after processing the VoteStateUpdate
                        None,
                        0,
                        vote_state_update.1, // vote_state_update.1 is the slot in which the VoteStateUpdate occurred
                        Some(&feature_set)
                    ),
                    Ok(())
                );

                // Ensure that the credits earned is correct
                assert_eq!(vote_state.credits(), vote_state_update.3 as u64);
            });
    }

    #[test]
    fn test_process_new_vote_too_many_votes() {
        let mut vote_state1 = VoteState::default();
        let bad_votes: VecDeque<Lockout> = (0..=MAX_LOCKOUT_HISTORY)
            .map(|slot| {
                Lockout::new_with_confirmation_count(
                    slot as Slot,
                    (MAX_LOCKOUT_HISTORY - slot + 1) as u32,
                )
            })
            .collect();

        let current_epoch = vote_state1.current_epoch();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::TooManyVotes)
        );
    }

    #[test]
    fn test_process_new_vote_state_root_rollback() {
        let mut vote_state1 = VoteState::default();
        for i in 0..MAX_LOCKOUT_HISTORY + 2 {
            process_slot_vote_unchecked(&mut vote_state1, i as Slot);
        }
        assert_eq!(vote_state1.root_slot.unwrap(), 1);

        // Update vote_state2 with a higher slot so that `process_new_vote_state`
        // doesn't panic.
        let mut vote_state2 = vote_state1.clone();
        process_slot_vote_unchecked(&mut vote_state2, MAX_LOCKOUT_HISTORY as Slot + 3);

        // Trying to set a lesser root should error
        let lesser_root = Some(0);

        let current_epoch = vote_state2.current_epoch();
        assert_eq!(
            process_new_vote_state(
                &mut vote_state1,
                vote_state2.votes.clone(),
                lesser_root,
                None,
                current_epoch,
                0,
                None,
            ),
            Err(VoteError::RootRollBack)
        );

        // Trying to set root to None should error
        let none_root = None;
        assert_eq!(
            process_new_vote_state(
                &mut vote_state1,
                vote_state2.votes.clone(),
                none_root,
                None,
                current_epoch,
                0,
                None,
            ),
            Err(VoteError::RootRollBack)
        );
    }

    fn process_new_vote_state_replaced_root_vote_credits(
        feature_set: &FeatureSet,
        expected_credits: u64,
    ) {
        let mut vote_state1 = VoteState::default();

        // Initial vote state: as if 31 votes had occurred on slots 0 - 30 (inclusive)
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                (0..MAX_LOCKOUT_HISTORY)
                    .enumerate()
                    .map(|(index, slot)| Lockout::new_with_confirmation_count(
                        slot as Slot,
                        (MAX_LOCKOUT_HISTORY.checked_sub(index).unwrap()) as u32
                    ))
                    .collect(),
                None,
                None,
                0,
                Some(feature_set),
            ),
            Ok(())
        );

        // Now vote as if new votes on slots 31 and 32 had occurred, yielding a new Root of 1
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                (2..(MAX_LOCKOUT_HISTORY.checked_add(2).unwrap()))
                    .enumerate()
                    .map(|(index, slot)| Lockout::new_with_confirmation_count(
                        slot as Slot,
                        (MAX_LOCKOUT_HISTORY.checked_sub(index).unwrap()) as u32
                    ))
                    .collect(),
                Some(1),
                None,
                0,
                Some(feature_set),
            ),
            Ok(())
        );

        // Vote credits should be 2, since two voted-on slots were "popped off the back" of the tower
        assert_eq!(vote_state1.credits(), 2);

        // Create a new vote state that represents the validator having not voted for a long time, then voting on
        // slots 10001 through 10032 (inclusive) with an entirely new root of 10000 that was never previously voted
        // on.  This is valid because a vote state can include a root that it never voted on (if it votes after a very
        // long delinquency, the new votes will have a root much newer than its most recently voted slot).
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                (10001..(MAX_LOCKOUT_HISTORY.checked_add(10001).unwrap()))
                    .enumerate()
                    .map(|(index, slot)| Lockout::new_with_confirmation_count(
                        slot as Slot,
                        (MAX_LOCKOUT_HISTORY.checked_sub(index).unwrap()) as u32
                    ))
                    .collect(),
                Some(10000),
                None,
                0,
                Some(feature_set),
            ),
            Ok(())
        );

        // The vote is valid, but no vote credits should be awarded because although there is a new root, it does not
        // represent a slot previously voted on.
        assert_eq!(vote_state1.credits(), expected_credits)
    }

    #[test]
    fn test_process_new_vote_state_replaced_root_vote_credits() {
        let mut feature_set = FeatureSet::default();

        // Always use allow_votes_to_directly_update_vote_state feature because VoteStateUpdate is being tested
        feature_set.activate(
            &feature_set::allow_votes_to_directly_update_vote_state::id(),
            1,
        );

        // Test without the timely_vote_credits feature.  The expected credits here of 34 is *incorrect* but is what
        // is expected using vote_state_update_credit_per_dequeue.  With this feature, the credits earned will be
        // calculated as:
        // 2 (from initial vote state)
        // + 31 (for votes which were "popped off of the back of the tower" by the new vote
        // + 1 (just because there is a new root, even though it was never voted on -- this is the flaw)
        feature_set.activate(&feature_set::vote_state_update_credit_per_dequeue::id(), 1);
        process_new_vote_state_replaced_root_vote_credits(&feature_set, 34);

        // Now test using the timely_vote_credits feature.  The expected credits here of 33 is *correct*.  With
        // this feature, the credits earned will be calculated as:
        // 2 (from initial vote state)
        // + 31 (for votes which were "popped off of the back of the tower" by the new vote)
        feature_set.activate(&feature_set::timely_vote_credits::id(), 1);
        process_new_vote_state_replaced_root_vote_credits(&feature_set, 33);
    }

    #[test]
    fn test_process_new_vote_state_zero_confirmations() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 0),
            Lockout::new_with_confirmation_count(1, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::ZeroConfirmations)
        );

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 2),
            Lockout::new_with_confirmation_count(1, 0),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::ZeroConfirmations)
        );
    }

    #[test]
    fn test_process_new_vote_state_confirmations_too_large() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();

        let good_votes: VecDeque<Lockout> = vec![Lockout::new_with_confirmation_count(
            0,
            MAX_LOCKOUT_HISTORY as u32,
        )]
        .into_iter()
        .collect();

        process_new_vote_state_from_lockouts(
            &mut vote_state1,
            good_votes,
            None,
            None,
            current_epoch,
            None,
        )
        .unwrap();

        let mut vote_state1 = VoteState::default();
        let bad_votes: VecDeque<Lockout> = vec![Lockout::new_with_confirmation_count(
            0,
            MAX_LOCKOUT_HISTORY as u32 + 1,
        )]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::ConfirmationTooLarge)
        );
    }

    #[test]
    fn test_process_new_vote_state_slot_smaller_than_root() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();
        let root_slot = 5;

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(root_slot, 2),
            Lockout::new_with_confirmation_count(root_slot + 1, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                Some(root_slot),
                None,
                current_epoch,
                None,
            ),
            Err(VoteError::SlotSmallerThanRoot)
        );

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(root_slot - 1, 2),
            Lockout::new_with_confirmation_count(root_slot + 1, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                Some(root_slot),
                None,
                current_epoch,
                None,
            ),
            Err(VoteError::SlotSmallerThanRoot)
        );
    }

    #[test]
    fn test_process_new_vote_state_slots_not_ordered() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(1, 2),
            Lockout::new_with_confirmation_count(0, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::SlotsNotOrdered)
        );

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(1, 2),
            Lockout::new_with_confirmation_count(1, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::SlotsNotOrdered)
        );
    }

    #[test]
    fn test_process_new_vote_state_confirmations_not_ordered() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 1),
            Lockout::new_with_confirmation_count(1, 2),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::ConfirmationsNotOrdered)
        );

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 1),
            Lockout::new_with_confirmation_count(1, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::ConfirmationsNotOrdered)
        );
    }

    #[test]
    fn test_process_new_vote_state_new_vote_state_lockout_mismatch() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();

        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 2),
            Lockout::new_with_confirmation_count(7, 1),
        ]
        .into_iter()
        .collect();

        // Slot 7 should have expired slot 0
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::NewVoteStateLockoutMismatch)
        );
    }

    #[test]
    fn test_process_new_vote_state_confirmation_rollback() {
        let mut vote_state1 = VoteState::default();
        let current_epoch = vote_state1.current_epoch();
        let votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 4),
            Lockout::new_with_confirmation_count(1, 3),
        ]
        .into_iter()
        .collect();
        process_new_vote_state_from_lockouts(
            &mut vote_state1,
            votes,
            None,
            None,
            current_epoch,
            None,
        )
        .unwrap();

        let votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(0, 4),
            // Confirmation count lowered illegally
            Lockout::new_with_confirmation_count(1, 2),
            Lockout::new_with_confirmation_count(2, 1),
        ]
        .into_iter()
        .collect();
        // Should error because newer vote state should not have lower confirmation the same slot
        // 1
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                votes,
                None,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::ConfirmationRollBack)
        );
    }

    #[test]
    fn test_process_new_vote_state_root_progress() {
        let mut vote_state1 = VoteState::default();
        for i in 0..MAX_LOCKOUT_HISTORY {
            process_slot_vote_unchecked(&mut vote_state1, i as u64);
        }

        assert!(vote_state1.root_slot.is_none());
        let mut vote_state2 = vote_state1.clone();

        // 1) Try to update `vote_state1` with no root,
        // to `vote_state2`, which has a new root, should succeed.
        //
        // 2) Then try to update`vote_state1` with an existing root,
        // to `vote_state2`, which has a newer root, which
        // should succeed.
        for new_vote in MAX_LOCKOUT_HISTORY + 1..=MAX_LOCKOUT_HISTORY + 2 {
            process_slot_vote_unchecked(&mut vote_state2, new_vote as Slot);
            assert_ne!(vote_state1.root_slot, vote_state2.root_slot);

            process_new_vote_state(
                &mut vote_state1,
                vote_state2.votes.clone(),
                vote_state2.root_slot,
                None,
                vote_state2.current_epoch(),
                0,
                None,
            )
            .unwrap();

            assert_eq!(vote_state1, vote_state2);
        }
    }

    #[test]
    fn test_process_new_vote_state_same_slot_but_not_common_ancestor() {
        // It might be possible that during the switch from old vote instructions
        // to new vote instructions, new_state contains votes for slots LESS
        // than the current state, for instance:
        //
        // Current on-chain state: 1, 5
        // New state: 1, 2 (lockout: 4), 3, 5, 7
        //
        // Imagine the validator made two of these votes:
        // 1) The first vote {1, 2, 3} didn't land in the old state, but didn't
        // land on chain
        // 2) A second vote {1, 2, 5} was then submitted, which landed
        //
        //
        // 2 is not popped off in the local tower because 3 doubled the lockout.
        // However, 3 did not land in the on-chain state, so the vote {1, 2, 6}
        // will immediately pop off 2.

        // Construct on-chain vote state
        let mut vote_state1 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state1, &[1, 2, 5]);
        assert_eq!(
            vote_state1
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 5]
        );

        // Construct local tower state
        let mut vote_state2 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state2, &[1, 2, 3, 5, 7]);
        assert_eq!(
            vote_state2
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 2, 3, 5, 7]
        );

        // See that on-chain vote state can update properly
        process_new_vote_state(
            &mut vote_state1,
            vote_state2.votes.clone(),
            vote_state2.root_slot,
            None,
            vote_state2.current_epoch(),
            0,
            None,
        )
        .unwrap();

        assert_eq!(vote_state1, vote_state2);
    }

    #[test]
    fn test_process_new_vote_state_lockout_violation() {
        // Construct on-chain vote state
        let mut vote_state1 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state1, &[1, 2, 4, 5]);
        assert_eq!(
            vote_state1
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 2, 4, 5]
        );

        // Construct conflicting tower state. Vote 4 is missing,
        // but 5 should not have popped off vote 4.
        let mut vote_state2 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state2, &[1, 2, 3, 5, 7]);
        assert_eq!(
            vote_state2
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 2, 3, 5, 7]
        );

        // See that on-chain vote state can update properly
        assert_eq!(
            process_new_vote_state(
                &mut vote_state1,
                vote_state2.votes.clone(),
                vote_state2.root_slot,
                None,
                vote_state2.current_epoch(),
                0,
                None
            ),
            Err(VoteError::LockoutConflict)
        );
    }

    #[test]
    fn test_process_new_vote_state_lockout_violation2() {
        // Construct on-chain vote state
        let mut vote_state1 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state1, &[1, 2, 5, 6, 7]);
        assert_eq!(
            vote_state1
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 5, 6, 7]
        );

        // Construct a new vote state. Violates on-chain state because 8
        // should not have popped off 7
        let mut vote_state2 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state2, &[1, 2, 3, 5, 6, 8]);
        assert_eq!(
            vote_state2
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 2, 3, 5, 6, 8]
        );

        // Both vote states contain `5`, but `5` is not part of the common prefix
        // of both vote states. However, the violation should still be detected.
        assert_eq!(
            process_new_vote_state(
                &mut vote_state1,
                vote_state2.votes.clone(),
                vote_state2.root_slot,
                None,
                vote_state2.current_epoch(),
                0,
                None
            ),
            Err(VoteError::LockoutConflict)
        );
    }

    #[test]
    fn test_process_new_vote_state_expired_ancestor_not_removed() {
        // Construct on-chain vote state
        let mut vote_state1 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state1, &[1, 2, 3, 9]);
        assert_eq!(
            vote_state1
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 9]
        );

        // Example: {1: lockout 8, 9: lockout 2}, vote on 10 will not pop off 1
        // because 9 is not popped off yet
        let mut vote_state2 = vote_state1.clone();
        process_slot_vote_unchecked(&mut vote_state2, 10);

        // Slot 1 has been expired by 10, but is kept alive by its descendant
        // 9 which has not been expired yet.
        assert_eq!(vote_state2.votes[0].slot(), 1);
        assert_eq!(vote_state2.votes[0].lockout.last_locked_out_slot(), 9);
        assert_eq!(
            vote_state2
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![1, 9, 10]
        );

        // Should be able to update vote_state1
        process_new_vote_state(
            &mut vote_state1,
            vote_state2.votes.clone(),
            vote_state2.root_slot,
            None,
            vote_state2.current_epoch(),
            0,
            None,
        )
        .unwrap();
        assert_eq!(vote_state1, vote_state2,);
    }

    #[test]
    fn test_process_new_vote_current_state_contains_bigger_slots() {
        let mut vote_state1 = VoteState::default();
        process_slot_votes_unchecked(&mut vote_state1, &[6, 7, 8]);
        assert_eq!(
            vote_state1
                .votes
                .iter()
                .map(|vote| vote.slot())
                .collect::<Vec<Slot>>(),
            vec![6, 7, 8]
        );

        // Try to process something with lockout violations
        let bad_votes: VecDeque<Lockout> = vec![
            Lockout::new_with_confirmation_count(2, 5),
            // Slot 14 could not have popped off slot 6 yet
            Lockout::new_with_confirmation_count(14, 1),
        ]
        .into_iter()
        .collect();
        let root = Some(1);

        let current_epoch = vote_state1.current_epoch();
        assert_eq!(
            process_new_vote_state_from_lockouts(
                &mut vote_state1,
                bad_votes,
                root,
                None,
                current_epoch,
                None
            ),
            Err(VoteError::LockoutConflict)
        );

        let good_votes: VecDeque<LandedVote> = vec![
            Lockout::new_with_confirmation_count(2, 5).into(),
            Lockout::new_with_confirmation_count(15, 1).into(),
        ]
        .into_iter()
        .collect();

        let current_epoch = vote_state1.current_epoch();
        process_new_vote_state(
            &mut vote_state1,
            good_votes.clone(),
            root,
            None,
            current_epoch,
            0,
            None,
        )
        .unwrap();
        assert_eq!(vote_state1.votes, good_votes);
    }

    #[test]
    fn test_filter_old_votes() {
        let mut vote_state = VoteState::default();
        let old_vote_slot = 1;
        let vote = Vote::new(vec![old_vote_slot], Hash::default());

        // Vote with all slots that are all older than the SlotHashes history should
        // error with `VotesTooOldAllFiltered`
        let slot_hashes = vec![(3, Hash::new_unique()), (2, Hash::new_unique())];
        assert_eq!(
            process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0),
            Err(VoteError::VotesTooOldAllFiltered)
        );

        // Vote with only some slots older than the SlotHashes history should
        // filter out those older slots
        let vote_slot = 2;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;

        let vote = Vote::new(vec![old_vote_slot, vote_slot], vote_slot_hash);
        process_vote(&mut vote_state, &vote, &slot_hashes, 0, 0).unwrap();
        assert_eq!(
            vote_state
                .votes
                .into_iter()
                .map(|vote| vote.lockout)
                .collect::<Vec<Lockout>>(),
            vec![Lockout::new_with_confirmation_count(vote_slot, 1)]
        );
    }

    fn build_slot_hashes(slots: Vec<Slot>) -> Vec<(Slot, Hash)> {
        slots
            .iter()
            .rev()
            .map(|x| (*x, Hash::new_unique()))
            .collect()
    }

    fn build_vote_state(vote_slots: Vec<Slot>, slot_hashes: &[(Slot, Hash)]) -> VoteState {
        let mut vote_state = VoteState::default();

        if !vote_slots.is_empty() {
            let vote_hash = slot_hashes
                .iter()
                .find(|(slot, _hash)| slot == vote_slots.last().unwrap())
                .unwrap()
                .1;
            let vote = Vote::new(vote_slots, vote_hash);
            process_vote_unfiltered(&mut vote_state, &vote.slots, &vote, slot_hashes, 0, 0)
                .unwrap();
        }

        vote_state
    }

    #[test]
    fn test_check_update_vote_state_empty() {
        let empty_slot_hashes = build_slot_hashes(vec![]);
        let empty_vote_state = build_vote_state(vec![], &empty_slot_hashes);

        // Test with empty vote state update, should return EmptySlots error
        let mut vote_state_update = VoteStateUpdate::from(vec![]);
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &empty_vote_state,
                &mut vote_state_update,
                &empty_slot_hashes,
            ),
            Err(VoteError::EmptySlots),
        );

        // Test with non-empty vote state update, should return SlotsMismatch since nothing exists in SlotHashes
        let mut vote_state_update = VoteStateUpdate::from(vec![(0, 1)]);
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &empty_vote_state,
                &mut vote_state_update,
                &empty_slot_hashes,
            ),
            Err(VoteError::SlotsMismatch),
        );
    }

    #[test]
    fn test_check_update_vote_state_too_old() {
        let slot_hashes = build_slot_hashes(vec![1, 2, 3, 4]);
        let latest_vote = 4;
        let vote_state = build_vote_state(vec![1, 2, 3, latest_vote], &slot_hashes);

        // Test with a vote for a slot less than the latest vote in the vote_state,
        // should return error `VoteTooOld`
        let mut vote_state_update = VoteStateUpdate::from(vec![(latest_vote, 1)]);
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::VoteTooOld),
        );

        // Test with a vote state update where the latest slot `X` in the update is
        // 1) Less than the earliest slot in slot_hashes history, AND
        // 2) `X` > latest_vote
        let earliest_slot_in_history = latest_vote + 2;
        let slot_hashes = build_slot_hashes(vec![earliest_slot_in_history]);
        let mut vote_state_update = VoteStateUpdate::from(vec![(earliest_slot_in_history - 1, 1)]);
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::VoteTooOld),
        );
    }

    fn run_test_check_update_vote_state_older_than_history_root(
        earliest_slot_in_history: Slot,
        current_vote_state_slots: Vec<Slot>,
        current_vote_state_root: Option<Slot>,
        vote_state_update_slots_and_lockouts: Vec<(Slot, u32)>,
        vote_state_update_root: Slot,
        expected_root: Option<Slot>,
        expected_vote_state: Vec<Lockout>,
    ) {
        assert!(vote_state_update_root < earliest_slot_in_history);
        assert_eq!(
            expected_root,
            current_vote_state_slots
                .iter()
                .rev()
                .find(|slot| **slot <= vote_state_update_root)
                .cloned()
        );
        let latest_slot_in_history = vote_state_update_slots_and_lockouts
            .last()
            .unwrap()
            .0
            .max(earliest_slot_in_history);
        let mut slot_hashes = build_slot_hashes(
            (current_vote_state_slots.first().copied().unwrap_or(0)..=latest_slot_in_history)
                .collect::<Vec<Slot>>(),
        );

        let mut vote_state = build_vote_state(current_vote_state_slots, &slot_hashes);
        vote_state.root_slot = current_vote_state_root;

        slot_hashes.retain(|slot| slot.0 >= earliest_slot_in_history);
        assert!(!vote_state_update_slots_and_lockouts.is_empty());
        let vote_state_update_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_state_update_slots_and_lockouts.last().unwrap().0)
            .unwrap()
            .1;

        // Test with a `vote_state_update` where the root is less than `earliest_slot_in_history`.
        // Root slot in the `vote_state_update` should be updated to match the root slot in the
        // current vote state
        let mut vote_state_update = VoteStateUpdate::from(vote_state_update_slots_and_lockouts);
        vote_state_update.hash = vote_state_update_hash;
        vote_state_update.root = Some(vote_state_update_root);
        check_update_vote_state_slots_are_valid(&vote_state, &mut vote_state_update, &slot_hashes)
            .unwrap();
        assert_eq!(vote_state_update.root, expected_root);

        // The proposed root slot should become the biggest slot in the current vote state less than
        // `earliest_slot_in_history`.
        assert!(do_process_vote_state_update(
            &mut vote_state,
            &slot_hashes,
            0,
            0,
            vote_state_update.clone(),
            Some(&FeatureSet::all_enabled()),
        )
        .is_ok());
        assert_eq!(vote_state.root_slot, expected_root);
        assert_eq!(
            vote_state
                .votes
                .into_iter()
                .map(|vote| vote.lockout)
                .collect::<Vec<Lockout>>(),
            expected_vote_state,
        );
    }

    #[test]
    fn test_check_update_vote_state_older_than_history_root() {
        // Test when `vote_state_update_root` is in `current_vote_state_slots` but it's not the latest
        // slot
        let earliest_slot_in_history = 5;
        let current_vote_state_slots: Vec<Slot> = vec![1, 2, 3, 4];
        let current_vote_state_root = None;
        let vote_state_update_slots_and_lockouts = vec![(5, 1)];
        let vote_state_update_root = 4;
        let expected_root = Some(4);
        let expected_vote_state = vec![Lockout::new_with_confirmation_count(5, 1)];
        run_test_check_update_vote_state_older_than_history_root(
            earliest_slot_in_history,
            current_vote_state_slots,
            current_vote_state_root,
            vote_state_update_slots_and_lockouts,
            vote_state_update_root,
            expected_root,
            expected_vote_state,
        );

        // Test when `vote_state_update_root` is in `current_vote_state_slots` but it's not the latest
        // slot and the `current_vote_state_root.is_some()`.
        let earliest_slot_in_history = 5;
        let current_vote_state_slots: Vec<Slot> = vec![1, 2, 3, 4];
        let current_vote_state_root = Some(0);
        let vote_state_update_slots_and_lockouts = vec![(5, 1)];
        let vote_state_update_root = 4;
        let expected_root = Some(4);
        let expected_vote_state = vec![Lockout::new_with_confirmation_count(5, 1)];
        run_test_check_update_vote_state_older_than_history_root(
            earliest_slot_in_history,
            current_vote_state_slots,
            current_vote_state_root,
            vote_state_update_slots_and_lockouts,
            vote_state_update_root,
            expected_root,
            expected_vote_state,
        );

        // Test when `vote_state_update_root` is in `current_vote_state_slots` but it's not the latest
        // slot
        let earliest_slot_in_history = 5;
        let current_vote_state_slots: Vec<Slot> = vec![1, 2, 3, 4];
        let current_vote_state_root = Some(0);
        let vote_state_update_slots_and_lockouts = vec![(4, 2), (5, 1)];
        let vote_state_update_root = 3;
        let expected_root = Some(3);
        let expected_vote_state = vec![
            Lockout::new_with_confirmation_count(4, 2),
            Lockout::new_with_confirmation_count(5, 1),
        ];
        run_test_check_update_vote_state_older_than_history_root(
            earliest_slot_in_history,
            current_vote_state_slots,
            current_vote_state_root,
            vote_state_update_slots_and_lockouts,
            vote_state_update_root,
            expected_root,
            expected_vote_state,
        );

        // Test when `vote_state_update_root` is not in `current_vote_state_slots`
        let earliest_slot_in_history = 5;
        let current_vote_state_slots: Vec<Slot> = vec![1, 2, 4];
        let current_vote_state_root = Some(0);
        let vote_state_update_slots_and_lockouts = vec![(4, 2), (5, 1)];
        let vote_state_update_root = 3;
        let expected_root = Some(2);
        let expected_vote_state = vec![
            Lockout::new_with_confirmation_count(4, 2),
            Lockout::new_with_confirmation_count(5, 1),
        ];
        run_test_check_update_vote_state_older_than_history_root(
            earliest_slot_in_history,
            current_vote_state_slots,
            current_vote_state_root,
            vote_state_update_slots_and_lockouts,
            vote_state_update_root,
            expected_root,
            expected_vote_state,
        );

        // Test when the `vote_state_update_root` is smaller than all the slots in
        // `current_vote_state_slots`, no roots should be set.
        let earliest_slot_in_history = 4;
        let current_vote_state_slots: Vec<Slot> = vec![3, 4];
        let current_vote_state_root = None;
        let vote_state_update_slots_and_lockouts = vec![(3, 3), (4, 2), (5, 1)];
        let vote_state_update_root = 2;
        let expected_root = None;
        let expected_vote_state = vec![
            Lockout::new_with_confirmation_count(3, 3),
            Lockout::new_with_confirmation_count(4, 2),
            Lockout::new_with_confirmation_count(5, 1),
        ];
        run_test_check_update_vote_state_older_than_history_root(
            earliest_slot_in_history,
            current_vote_state_slots,
            current_vote_state_root,
            vote_state_update_slots_and_lockouts,
            vote_state_update_root,
            expected_root,
            expected_vote_state,
        );

        // Test when `current_vote_state_slots` is empty, no roots should be set
        let earliest_slot_in_history = 4;
        let current_vote_state_slots: Vec<Slot> = vec![];
        let current_vote_state_root = None;
        let vote_state_update_slots_and_lockouts = vec![(5, 1)];
        let vote_state_update_root = 2;
        let expected_root = None;
        let expected_vote_state = vec![Lockout::new_with_confirmation_count(5, 1)];
        run_test_check_update_vote_state_older_than_history_root(
            earliest_slot_in_history,
            current_vote_state_slots,
            current_vote_state_root,
            vote_state_update_slots_and_lockouts,
            vote_state_update_root,
            expected_root,
            expected_vote_state,
        );
    }

    #[test]
    fn test_check_update_vote_state_slots_not_ordered() {
        let slot_hashes = build_slot_hashes(vec![1, 2, 3, 4]);
        let vote_state = build_vote_state(vec![1], &slot_hashes);

        // Test with a `vote_state_update` where the slots are out of order
        let vote_slot = 3;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let mut vote_state_update = VoteStateUpdate::from(vec![(2, 2), (1, 3), (vote_slot, 1)]);
        vote_state_update.hash = vote_slot_hash;
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::SlotsNotOrdered),
        );

        // Test with a `vote_state_update` where there are multiples of the same slot
        let mut vote_state_update = VoteStateUpdate::from(vec![(2, 2), (2, 2), (vote_slot, 1)]);
        vote_state_update.hash = vote_slot_hash;
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::SlotsNotOrdered),
        );
    }

    #[test]
    fn test_check_update_vote_state_older_than_history_slots_filtered() {
        let slot_hashes = build_slot_hashes(vec![1, 2, 3, 4]);
        let mut vote_state = build_vote_state(vec![1, 2, 3, 4], &slot_hashes);

        // Test with a `vote_state_update` where there:
        // 1) Exists a slot less than `earliest_slot_in_history`
        // 2) This slot does not exist in the vote state already
        // This slot should be filtered out
        let earliest_slot_in_history = 11;
        let slot_hashes = build_slot_hashes(vec![earliest_slot_in_history, 12, 13, 14]);
        let vote_slot = 12;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let missing_older_than_history_slot = earliest_slot_in_history - 1;
        let mut vote_state_update = VoteStateUpdate::from(vec![
            (1, 4),
            (missing_older_than_history_slot, 2),
            (vote_slot, 3),
        ]);
        vote_state_update.hash = vote_slot_hash;
        check_update_vote_state_slots_are_valid(&vote_state, &mut vote_state_update, &slot_hashes)
            .unwrap();

        // Check the earlier slot was filtered out
        assert_eq!(
            vote_state_update
                .clone()
                .lockouts
                .into_iter()
                .collect::<Vec<Lockout>>(),
            vec![
                Lockout::new_with_confirmation_count(1, 4),
                Lockout::new_with_confirmation_count(vote_slot, 3)
            ]
        );
        assert!(do_process_vote_state_update(
            &mut vote_state,
            &slot_hashes,
            0,
            0,
            vote_state_update,
            Some(&FeatureSet::all_enabled()),
        )
        .is_ok());
    }

    #[test]
    fn test_check_update_vote_state_older_than_history_slots_not_filtered() {
        let slot_hashes = build_slot_hashes(vec![4]);
        let mut vote_state = build_vote_state(vec![4], &slot_hashes);

        // Test with a `vote_state_update` where there:
        // 1) Exists a slot less than `earliest_slot_in_history`
        // 2) This slot exists in the vote state already
        // This slot should *NOT* be filtered out
        let earliest_slot_in_history = 11;
        let slot_hashes = build_slot_hashes(vec![earliest_slot_in_history, 12, 13, 14]);
        let vote_slot = 12;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let existing_older_than_history_slot = 4;
        let mut vote_state_update =
            VoteStateUpdate::from(vec![(existing_older_than_history_slot, 3), (vote_slot, 2)]);
        vote_state_update.hash = vote_slot_hash;
        check_update_vote_state_slots_are_valid(&vote_state, &mut vote_state_update, &slot_hashes)
            .unwrap();
        // Check the earlier slot was *NOT* filtered out
        assert_eq!(vote_state_update.lockouts.len(), 2);
        assert_eq!(
            vote_state_update
                .clone()
                .lockouts
                .into_iter()
                .collect::<Vec<Lockout>>(),
            vec![
                Lockout::new_with_confirmation_count(existing_older_than_history_slot, 3),
                Lockout::new_with_confirmation_count(vote_slot, 2)
            ]
        );
        assert!(do_process_vote_state_update(
            &mut vote_state,
            &slot_hashes,
            0,
            0,
            vote_state_update,
            Some(&FeatureSet::all_enabled()),
        )
        .is_ok());
    }

    #[test]
    fn test_check_update_vote_state_older_than_history_slots_filtered_and_not_filtered() {
        let slot_hashes = build_slot_hashes(vec![6]);
        let mut vote_state = build_vote_state(vec![6], &slot_hashes);

        // Test with a `vote_state_update` where there exists both a slot:
        // 1) Less than `earliest_slot_in_history`
        // 2) This slot exists in the vote state already
        // which should not be filtered
        //
        // AND a slot that
        //
        // 1) Less than `earliest_slot_in_history`
        // 2) This slot does not exist in the vote state already
        // which should be filtered
        let earliest_slot_in_history = 11;
        let slot_hashes = build_slot_hashes(vec![earliest_slot_in_history, 12, 13, 14]);
        let vote_slot = 14;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;

        let missing_older_than_history_slot = 4;
        let existing_older_than_history_slot = 6;

        let mut vote_state_update = VoteStateUpdate::from(vec![
            (missing_older_than_history_slot, 4),
            (existing_older_than_history_slot, 3),
            (12, 2),
            (vote_slot, 1),
        ]);
        vote_state_update.hash = vote_slot_hash;
        check_update_vote_state_slots_are_valid(&vote_state, &mut vote_state_update, &slot_hashes)
            .unwrap();
        assert_eq!(vote_state_update.lockouts.len(), 3);
        assert_eq!(
            vote_state_update
                .clone()
                .lockouts
                .into_iter()
                .collect::<Vec<Lockout>>(),
            vec![
                Lockout::new_with_confirmation_count(existing_older_than_history_slot, 3),
                Lockout::new_with_confirmation_count(12, 2),
                Lockout::new_with_confirmation_count(vote_slot, 1)
            ]
        );
        assert!(do_process_vote_state_update(
            &mut vote_state,
            &slot_hashes,
            0,
            0,
            vote_state_update,
            Some(&FeatureSet::all_enabled()),
        )
        .is_ok());
    }

    #[test]
    fn test_check_update_vote_state_slot_not_on_fork() {
        let slot_hashes = build_slot_hashes(vec![2, 4, 6, 8]);
        let vote_state = build_vote_state(vec![2, 4, 6], &slot_hashes);

        // Test with a `vote_state_update` where there:
        // 1) Exists a slot not in the slot hashes history
        // 2) The slot is greater than the earliest slot in the history
        // Thus this slot is not part of the fork and the update should be rejected
        // with error `SlotsMismatch`
        let missing_vote_slot = 3;

        // Have to vote for a slot greater than the last vote in the vote state to avoid VoteTooOld
        // errors
        let vote_slot = vote_state.votes.back().unwrap().slot() + 2;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let mut vote_state_update =
            VoteStateUpdate::from(vec![(missing_vote_slot, 2), (vote_slot, 3)]);
        vote_state_update.hash = vote_slot_hash;
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::SlotsMismatch),
        );

        // Test where some earlier vote slots exist in the history, but others don't
        let missing_vote_slot = 7;
        let mut vote_state_update = VoteStateUpdate::from(vec![
            (2, 5),
            (4, 4),
            (6, 3),
            (missing_vote_slot, 2),
            (vote_slot, 1),
        ]);
        vote_state_update.hash = vote_slot_hash;
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::SlotsMismatch),
        );
    }

    #[test]
    fn test_check_update_vote_state_root_on_different_fork() {
        let slot_hashes = build_slot_hashes(vec![2, 4, 6, 8]);
        let vote_state = build_vote_state(vec![6], &slot_hashes);

        // Test with a `vote_state_update` where:
        // 1) The root is not present in slot hashes history
        // 2) The slot is greater than the earliest slot in the history
        // Thus this slot is not part of the fork and the update should be rejected
        // with error `RootOnDifferentFork`
        let new_root = 3;

        // Have to vote for a slot greater than the last vote in the vote state to avoid VoteTooOld
        // errors, but also this slot must be present in SlotHashes
        let vote_slot = 8;
        assert_eq!(vote_slot, slot_hashes.first().unwrap().0);
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let mut vote_state_update = VoteStateUpdate::from(vec![(vote_slot, 1)]);
        vote_state_update.hash = vote_slot_hash;
        vote_state_update.root = Some(new_root);
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::RootOnDifferentFork),
        );
    }

    #[test]
    fn test_check_update_vote_state_slot_newer_than_slot_history() {
        let slot_hashes = build_slot_hashes(vec![2, 4, 6, 8, 10]);
        let vote_state = build_vote_state(vec![2, 4, 6], &slot_hashes);

        // Test with a `vote_state_update` where there:
        // 1) The last slot in the update is a slot not in the slot hashes history
        // 2) The slot is greater than the newest slot in the slot history
        // Thus this slot is not part of the fork and the update should be rejected
        // with error `SlotsMismatch`
        let missing_vote_slot = slot_hashes.first().unwrap().0 + 1;
        let vote_slot_hash = Hash::new_unique();
        let mut vote_state_update = VoteStateUpdate::from(vec![(8, 2), (missing_vote_slot, 3)]);
        vote_state_update.hash = vote_slot_hash;
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::SlotsMismatch),
        );
    }

    #[test]
    fn test_check_update_vote_state_slot_all_slot_hashes_in_update_ok() {
        let slot_hashes = build_slot_hashes(vec![2, 4, 6, 8]);
        let mut vote_state = build_vote_state(vec![2, 4, 6], &slot_hashes);

        // Test with a `vote_state_update` where every slot in the history is
        // in the update

        // Have to vote for a slot greater than the last vote in the vote state to avoid VoteTooOld
        // errors
        let vote_slot = vote_state.votes.back().unwrap().slot() + 2;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let mut vote_state_update =
            VoteStateUpdate::from(vec![(2, 4), (4, 3), (6, 2), (vote_slot, 1)]);
        vote_state_update.hash = vote_slot_hash;
        check_update_vote_state_slots_are_valid(&vote_state, &mut vote_state_update, &slot_hashes)
            .unwrap();

        // Nothing in the update should have been filtered out
        assert_eq!(
            vote_state_update
                .clone()
                .lockouts
                .into_iter()
                .collect::<Vec<Lockout>>(),
            vec![
                Lockout::new_with_confirmation_count(2, 4),
                Lockout::new_with_confirmation_count(4, 3),
                Lockout::new_with_confirmation_count(6, 2),
                Lockout::new_with_confirmation_count(vote_slot, 1)
            ]
        );

        assert!(do_process_vote_state_update(
            &mut vote_state,
            &slot_hashes,
            0,
            0,
            vote_state_update,
            Some(&FeatureSet::all_enabled()),
        )
        .is_ok());
    }

    #[test]
    fn test_check_update_vote_state_slot_some_slot_hashes_in_update_ok() {
        let slot_hashes = build_slot_hashes(vec![2, 4, 6, 8, 10]);
        let mut vote_state = build_vote_state(vec![6], &slot_hashes);

        // Test with a `vote_state_update` where only some slots in the history are
        // in the update, and others slots in the history are missing.

        // Have to vote for a slot greater than the last vote in the vote state to avoid VoteTooOld
        // errors
        let vote_slot = vote_state.votes.back().unwrap().slot() + 2;
        let vote_slot_hash = slot_hashes
            .iter()
            .find(|(slot, _hash)| *slot == vote_slot)
            .unwrap()
            .1;
        let mut vote_state_update = VoteStateUpdate::from(vec![(4, 2), (vote_slot, 1)]);
        vote_state_update.hash = vote_slot_hash;
        check_update_vote_state_slots_are_valid(&vote_state, &mut vote_state_update, &slot_hashes)
            .unwrap();

        // Nothing in the update should have been filtered out
        assert_eq!(
            vote_state_update
                .clone()
                .lockouts
                .into_iter()
                .collect::<Vec<Lockout>>(),
            vec![
                Lockout::new_with_confirmation_count(4, 2),
                Lockout::new_with_confirmation_count(vote_slot, 1)
            ]
        );

        // Because 6 from the original VoteState
        // should not have been popped off in the proposed state,
        // we should get a lockout conflict
        assert_eq!(
            do_process_vote_state_update(
                &mut vote_state,
                &slot_hashes,
                0,
                0,
                vote_state_update,
                Some(&FeatureSet::all_enabled())
            ),
            Err(VoteError::LockoutConflict)
        );
    }

    #[test]
    fn test_check_update_vote_state_slot_hash_mismatch() {
        let slot_hashes = build_slot_hashes(vec![2, 4, 6, 8]);
        let vote_state = build_vote_state(vec![2, 4, 6], &slot_hashes);

        // Test with a `vote_state_update` where the hash is mismatched

        // Have to vote for a slot greater than the last vote in the vote state to avoid VoteTooOld
        // errors
        let vote_slot = vote_state.votes.back().unwrap().slot() + 2;
        let vote_slot_hash = Hash::new_unique();
        let mut vote_state_update =
            VoteStateUpdate::from(vec![(2, 4), (4, 3), (6, 2), (vote_slot, 1)]);
        vote_state_update.hash = vote_slot_hash;
        assert_eq!(
            check_update_vote_state_slots_are_valid(
                &vote_state,
                &mut vote_state_update,
                &slot_hashes,
            ),
            Err(VoteError::SlotHashMismatch),
        );
    }

    #[test_case(0, true; "first slot")]
    #[test_case(DEFAULT_SLOTS_PER_EPOCH / 2, true; "halfway through epoch")]
    #[test_case((DEFAULT_SLOTS_PER_EPOCH / 2).saturating_add(1), false; "halfway through epoch plus one")]
    #[test_case(DEFAULT_SLOTS_PER_EPOCH.saturating_sub(1), false; "last slot in epoch")]
    #[test_case(DEFAULT_SLOTS_PER_EPOCH, true; "first slot in second epoch")]
    fn test_epoch_half_check(slot: Slot, expected_allowed: bool) {
        let epoch_schedule = EpochSchedule::without_warmup();
        assert_eq!(
            is_commission_update_allowed(slot, &epoch_schedule),
            expected_allowed
        );
    }

    #[test]
    fn test_warmup_epoch_half_check_with_warmup() {
        let epoch_schedule = EpochSchedule::default();
        let first_normal_slot = epoch_schedule.first_normal_slot;
        // first slot works
        assert!(is_commission_update_allowed(0, &epoch_schedule));
        // right before first normal slot works, since all warmup slots allow
        // commission updates
        assert!(is_commission_update_allowed(
            first_normal_slot - 1,
            &epoch_schedule
        ));
    }

    #[test_case(0, true; "first slot")]
    #[test_case(DEFAULT_SLOTS_PER_EPOCH / 2, true; "halfway through epoch")]
    #[test_case((DEFAULT_SLOTS_PER_EPOCH / 2).saturating_add(1), false; "halfway through epoch plus one")]
    #[test_case(DEFAULT_SLOTS_PER_EPOCH.saturating_sub(1), false; "last slot in epoch")]
    #[test_case(DEFAULT_SLOTS_PER_EPOCH, true; "first slot in second epoch")]
    fn test_epoch_half_check_with_warmup(slot: Slot, expected_allowed: bool) {
        let epoch_schedule = EpochSchedule::default();
        let first_normal_slot = epoch_schedule.first_normal_slot;
        assert_eq!(
            is_commission_update_allowed(first_normal_slot.saturating_add(slot), &epoch_schedule),
            expected_allowed
        );
    }
}
