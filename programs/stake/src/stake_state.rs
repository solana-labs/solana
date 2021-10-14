//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

use {
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::{State, StateMut},
        clock::{Clock, Epoch},
        feature_set::stake_merge_with_unmatched_credits_observed,
        ic_msg,
        instruction::{checked_add, InstructionError},
        keyed_account::KeyedAccount,
        process_instruction::InvokeContext,
        pubkey::Pubkey,
        rent::{Rent, ACCOUNT_STORAGE_OVERHEAD},
        stake::{
            config::Config,
            instruction::{LockupArgs, StakeError},
            program::id,
        },
        stake_history::{StakeHistory, StakeHistoryEntry},
    },
    solana_vote_program::vote_state::{VoteState, VoteStateVersions},
    std::{collections::HashSet, convert::TryFrom},
};

#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::state` or `solana_program::stake::state` instead"
)]
pub use solana_sdk::stake::state::*;

#[derive(Debug)]
pub enum SkippedReason {
    ZeroPoints,
    ZeroPointValue,
    ZeroReward,
    ZeroCreditsAndReturnZero,
    ZeroCreditsAndReturnCurrent,
}

impl From<SkippedReason> for InflationPointCalculationEvent {
    fn from(reason: SkippedReason) -> Self {
        InflationPointCalculationEvent::Skipped(reason)
    }
}

#[derive(Debug)]
pub enum InflationPointCalculationEvent {
    CalculatedPoints(u64, u128, u128, u128),
    SplitRewards(u64, u64, u64, PointValue),
    EffectiveStakeAtRewardedEpoch(u64),
    RentExemptReserve(u64),
    Delegation(Delegation, Pubkey),
    Commission(u8),
    CreditsObserved(u64, Option<u64>),
    Skipped(SkippedReason),
}

pub(crate) fn null_tracer() -> Option<impl Fn(&InflationPointCalculationEvent)> {
    None::<fn(&_)>
}

// utility function, used by Stakes, tests
pub fn from<T: ReadableAccount + StateMut<StakeState>>(account: &T) -> Option<StakeState> {
    account.state().ok()
}

pub fn stake_from<T: ReadableAccount + StateMut<StakeState>>(account: &T) -> Option<Stake> {
    from(account).and_then(|state: StakeState| state.stake())
}

pub fn delegation_from(account: &AccountSharedData) -> Option<Delegation> {
    from(account).and_then(|state: StakeState| state.delegation())
}

pub fn authorized_from(account: &AccountSharedData) -> Option<Authorized> {
    from(account).and_then(|state: StakeState| state.authorized())
}

pub fn lockup_from<T: ReadableAccount + StateMut<StakeState>>(account: &T) -> Option<Lockup> {
    from(account).and_then(|state: StakeState| state.lockup())
}

pub fn meta_from(account: &AccountSharedData) -> Option<Meta> {
    from(account).and_then(|state: StakeState| state.meta())
}

fn redelegate(
    stake: &mut Stake,
    stake_lamports: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    clock: &Clock,
    stake_history: &StakeHistory,
    config: &Config,
    can_reverse_deactivation: bool,
) -> Result<(), StakeError> {
    // If stake is currently active:
    if stake.stake(clock.epoch, Some(stake_history)) != 0 {
        // If pubkey of new voter is the same as current,
        // and we are scheduled to start deactivating this epoch,
        // we rescind deactivation
        if stake.delegation.voter_pubkey == *voter_pubkey
            && clock.epoch == stake.delegation.deactivation_epoch
            && can_reverse_deactivation
        {
            stake.delegation.deactivation_epoch = std::u64::MAX;
            return Ok(());
        } else {
            // can't redelegate to another pubkey if stake is active.
            return Err(StakeError::TooSoonToRedelegate);
        }
    }
    // Either the stake is freshly activated, is active but has been
    // deactivated this epoch, or has fully de-activated.
    // Redelegation implies either re-activation or un-deactivation

    stake.delegation.stake = stake_lamports;
    stake.delegation.activation_epoch = clock.epoch;
    stake.delegation.deactivation_epoch = std::u64::MAX;
    stake.delegation.voter_pubkey = *voter_pubkey;
    stake.delegation.warmup_cooldown_rate = config.warmup_cooldown_rate;
    stake.credits_observed = vote_state.credits();
    Ok(())
}

fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: Epoch,
    config: &Config,
) -> Stake {
    Stake {
        delegation: Delegation::new(
            voter_pubkey,
            stake,
            activation_epoch,
            config.warmup_cooldown_rate,
        ),
        credits_observed: vote_state.credits(),
    }
}

/// captures a rewards round as lamports to be awarded
///  and the total points over which those lamports
///  are to be distributed
//  basically read as rewards/points, but in integers instead of as an f64
#[derive(Clone, Debug, PartialEq)]
pub struct PointValue {
    pub rewards: u64, // lamports to split
    pub points: u128, // over these points
}

fn redeem_stake_rewards(
    rewarded_epoch: Epoch,
    stake: &mut Stake,
    point_value: &PointValue,
    vote_state: &VoteState,
    stake_history: Option<&StakeHistory>,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    fix_activating_credits_observed: bool,
) -> Option<(u64, u64)> {
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
        inflation_point_calc_tracer(&InflationPointCalculationEvent::CreditsObserved(
            stake.credits_observed,
            None,
        ));
    }
    calculate_stake_rewards(
        rewarded_epoch,
        stake,
        point_value,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
        fix_activating_credits_observed,
    )
    .map(|(stakers_reward, voters_reward, credits_observed)| {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
            inflation_point_calc_tracer(&InflationPointCalculationEvent::CreditsObserved(
                stake.credits_observed,
                Some(credits_observed),
            ));
        }
        stake.credits_observed = credits_observed;
        stake.delegation.stake += stakers_reward;
        (stakers_reward, voters_reward)
    })
}

fn calculate_stake_points(
    stake: &Stake,
    vote_state: &VoteState,
    stake_history: Option<&StakeHistory>,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
) -> u128 {
    calculate_stake_points_and_credits(
        stake,
        vote_state,
        stake_history,
        inflation_point_calc_tracer,
    )
    .0
}

/// for a given stake and vote_state, calculate how many
///   points were earned (credits * stake) and new value
///   for credits_observed were the points paid
fn calculate_stake_points_and_credits(
    stake: &Stake,
    new_vote_state: &VoteState,
    stake_history: Option<&StakeHistory>,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
) -> (u128, u64) {
    // if there is no newer credits since observed, return no point
    if new_vote_state.credits() <= stake.credits_observed {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroCreditsAndReturnCurrent.into());
        }
        return (0, stake.credits_observed);
    }

    let mut points = 0;
    let mut new_credits_observed = stake.credits_observed;

    for (epoch, final_epoch_credits, initial_epoch_credits) in
        new_vote_state.epoch_credits().iter().copied()
    {
        let stake_amount = u128::from(stake.delegation.stake(epoch, stake_history));

        // figure out how much this stake has seen that
        //   for which the vote account has a record
        let earned_credits = if stake.credits_observed < initial_epoch_credits {
            // the staker observed the entire epoch
            final_epoch_credits - initial_epoch_credits
        } else if stake.credits_observed < final_epoch_credits {
            // the staker registered sometime during the epoch, partial credit
            final_epoch_credits - new_credits_observed
        } else {
            // the staker has already observed or been redeemed this epoch
            //  or was activated after this epoch
            0
        };
        let earned_credits = u128::from(earned_credits);

        // don't want to assume anything about order of the iterator...
        new_credits_observed = new_credits_observed.max(final_epoch_credits);

        // finally calculate points for this epoch
        let earned_points = stake_amount * earned_credits;
        points += earned_points;

        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&InflationPointCalculationEvent::CalculatedPoints(
                epoch,
                stake_amount,
                earned_credits,
                earned_points,
            ));
        }
    }

    (points, new_credits_observed)
}

/// for a given stake and vote_state, calculate what distributions and what updates should be made
/// returns a tuple in the case of a payout of:
///   * staker_rewards to be distributed
///   * voter_rewards to be distributed
///   * new value for credits_observed in the stake
/// returns None if there's no payout or if any deserved payout is < 1 lamport
fn calculate_stake_rewards(
    rewarded_epoch: Epoch,
    stake: &Stake,
    point_value: &PointValue,
    vote_state: &VoteState,
    stake_history: Option<&StakeHistory>,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    fix_activating_credits_observed: bool,
) -> Option<(u64, u64, u64)> {
    let (points, credits_observed) = calculate_stake_points_and_credits(
        stake,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
    );

    // Drive credits_observed forward unconditionally when rewards are disabled
    // or when this is the stake's activation epoch
    if point_value.rewards == 0
        || (fix_activating_credits_observed && stake.delegation.activation_epoch == rewarded_epoch)
    {
        return Some((0, 0, credits_observed));
    }

    if points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
            inflation_point_calc_tracer(&SkippedReason::ZeroPoints.into());
        }
        return None;
    }
    if point_value.points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
            inflation_point_calc_tracer(&SkippedReason::ZeroPointValue.into());
        }
        return None;
    }

    let rewards = points
        .checked_mul(u128::from(point_value.rewards))
        .unwrap()
        .checked_div(point_value.points)
        .unwrap();

    let rewards = u64::try_from(rewards).unwrap();

    // don't bother trying to split if fractional lamports got truncated
    if rewards == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
            inflation_point_calc_tracer(&SkippedReason::ZeroReward.into());
        }
        return None;
    }
    let (voter_rewards, staker_rewards, is_split) = vote_state.commission_split(rewards);
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
        inflation_point_calc_tracer(&InflationPointCalculationEvent::SplitRewards(
            rewards,
            voter_rewards,
            staker_rewards,
            (*point_value).clone(),
        ));
    }

    if (voter_rewards == 0 || staker_rewards == 0) && is_split {
        // don't collect if we lose a whole lamport somewhere
        //  is_split means there should be tokens on both sides,
        //  uncool to move credits_observed if one side didn't get paid
        return None;
    }

    Some((staker_rewards, voter_rewards, credits_observed))
}

pub trait StakeAccount {
    fn initialize(
        &self,
        authorized: &Authorized,
        lockup: &Lockup,
        rent: &Rent,
    ) -> Result<(), InstructionError>;
    fn authorize(
        &self,
        signers: &HashSet<Pubkey>,
        new_authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        require_custodian_for_locked_stake_authorize: bool,
        clock: &Clock,
        custodian: Option<&Pubkey>,
    ) -> Result<(), InstructionError>;
    fn authorize_with_seed(
        &self,
        authority_base: &KeyedAccount,
        authority_seed: &str,
        authority_owner: &Pubkey,
        new_authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        require_custodian_for_locked_stake_authorize: bool,
        clock: &Clock,
        custodian: Option<&Pubkey>,
    ) -> Result<(), InstructionError>;
    fn delegate(
        &self,
        vote_account: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        config: &Config,
        signers: &HashSet<Pubkey>,
        can_reverse_deactivation: bool,
    ) -> Result<(), InstructionError>;
    fn deactivate(&self, clock: &Clock, signers: &HashSet<Pubkey>) -> Result<(), InstructionError>;
    fn set_lockup(
        &self,
        lockup: &LockupArgs,
        signers: &HashSet<Pubkey>,
        clock: Option<&Clock>,
    ) -> Result<(), InstructionError>;
    fn split(
        &self,
        lamports: u64,
        split_stake: &KeyedAccount,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn merge(
        &self,
        invoke_context: &dyn InvokeContext,
        source_stake: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        signers: &HashSet<Pubkey>,
        can_merge_expired_lockups: bool,
    ) -> Result<(), InstructionError>;
    fn withdraw(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        withdraw_authority: &KeyedAccount,
        custodian: Option<&KeyedAccount>,
        prevent_withdraw_to_zero: bool,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn initialize(
        &self,
        authorized: &Authorized,
        lockup: &Lockup,
        rent: &Rent,
    ) -> Result<(), InstructionError> {
        if self.data_len()? != std::mem::size_of::<StakeState>() {
            return Err(InstructionError::InvalidAccountData);
        }
        if let StakeState::Uninitialized = self.state()? {
            let rent_exempt_reserve = rent.minimum_balance(self.data_len()?);

            if rent_exempt_reserve < self.lamports()? {
                self.set_state(&StakeState::Initialized(Meta {
                    rent_exempt_reserve,
                    authorized: *authorized,
                    lockup: *lockup,
                }))
            } else {
                Err(InstructionError::InsufficientFunds)
            }
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }

    /// Authorize the given pubkey to manage stake (deactivate, withdraw). This may be called
    /// multiple times, but will implicitly withdraw authorization from the previously authorized
    /// staker. The default staker is the owner of the stake account's pubkey.
    fn authorize(
        &self,
        signers: &HashSet<Pubkey>,
        new_authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        require_custodian_for_locked_stake_authorize: bool,
        clock: &Clock,
        custodian: Option<&Pubkey>,
    ) -> Result<(), InstructionError> {
        match self.state()? {
            StakeState::Stake(mut meta, stake) => {
                meta.authorized.authorize(
                    signers,
                    new_authority,
                    stake_authorize,
                    if require_custodian_for_locked_stake_authorize {
                        Some((&meta.lockup, clock, custodian))
                    } else {
                        None
                    },
                )?;
                self.set_state(&StakeState::Stake(meta, stake))
            }
            StakeState::Initialized(mut meta) => {
                meta.authorized.authorize(
                    signers,
                    new_authority,
                    stake_authorize,
                    if require_custodian_for_locked_stake_authorize {
                        Some((&meta.lockup, clock, custodian))
                    } else {
                        None
                    },
                )?;
                self.set_state(&StakeState::Initialized(meta))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }
    fn authorize_with_seed(
        &self,
        authority_base: &KeyedAccount,
        authority_seed: &str,
        authority_owner: &Pubkey,
        new_authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        require_custodian_for_locked_stake_authorize: bool,
        clock: &Clock,
        custodian: Option<&Pubkey>,
    ) -> Result<(), InstructionError> {
        let mut signers = HashSet::default();
        if let Some(base_pubkey) = authority_base.signer_key() {
            signers.insert(Pubkey::create_with_seed(
                base_pubkey,
                authority_seed,
                authority_owner,
            )?);
        }
        self.authorize(
            &signers,
            new_authority,
            stake_authorize,
            require_custodian_for_locked_stake_authorize,
            clock,
            custodian,
        )
    }
    fn delegate(
        &self,
        vote_account: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        config: &Config,
        signers: &HashSet<Pubkey>,
        can_reverse_deactivation: bool,
    ) -> Result<(), InstructionError> {
        if vote_account.owner()? != solana_vote_program::id() {
            return Err(InstructionError::IncorrectProgramId);
        }

        match self.state()? {
            StakeState::Initialized(meta) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                let stake = new_stake(
                    self.lamports()?.saturating_sub(meta.rent_exempt_reserve), // can't stake the rent ;)
                    vote_account.unsigned_key(),
                    &State::<VoteStateVersions>::state(vote_account)?.convert_to_current(),
                    clock.epoch,
                    config,
                );
                self.set_state(&StakeState::Stake(meta, stake))
            }
            StakeState::Stake(meta, mut stake) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                redelegate(
                    &mut stake,
                    self.lamports()?.saturating_sub(meta.rent_exempt_reserve), // can't stake the rent ;)
                    vote_account.unsigned_key(),
                    &State::<VoteStateVersions>::state(vote_account)?.convert_to_current(),
                    clock,
                    stake_history,
                    config,
                    can_reverse_deactivation,
                )?;
                self.set_state(&StakeState::Stake(meta, stake))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }
    fn deactivate(&self, clock: &Clock, signers: &HashSet<Pubkey>) -> Result<(), InstructionError> {
        if let StakeState::Stake(meta, mut stake) = self.state()? {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            stake.deactivate(clock.epoch)?;

            self.set_state(&StakeState::Stake(meta, stake))
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn set_lockup(
        &self,
        lockup: &LockupArgs,
        signers: &HashSet<Pubkey>,
        clock: Option<&Clock>,
    ) -> Result<(), InstructionError> {
        match self.state()? {
            StakeState::Initialized(mut meta) => {
                meta.set_lockup(lockup, signers, clock)?;
                self.set_state(&StakeState::Initialized(meta))
            }
            StakeState::Stake(mut meta, stake) => {
                meta.set_lockup(lockup, signers, clock)?;
                self.set_state(&StakeState::Stake(meta, stake))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }

    fn split(
        &self,
        lamports: u64,
        split: &KeyedAccount,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        if split.owner()? != id() {
            return Err(InstructionError::IncorrectProgramId);
        }
        if split.data_len()? != std::mem::size_of::<StakeState>() {
            return Err(InstructionError::InvalidAccountData);
        }

        if let StakeState::Uninitialized = split.state()? {
            // verify enough account lamports
            if lamports > self.lamports()? {
                return Err(InstructionError::InsufficientFunds);
            }

            match self.state()? {
                StakeState::Stake(meta, mut stake) => {
                    meta.authorized.check(signers, StakeAuthorize::Staker)?;
                    let split_rent_exempt_reserve = calculate_split_rent_exempt_reserve(
                        meta.rent_exempt_reserve,
                        self.data_len()? as u64,
                        split.data_len()? as u64,
                    );

                    // verify enough lamports for rent and more than 0 stake in new split account
                    if lamports <= split_rent_exempt_reserve.saturating_sub(split.lamports()?)
                        // if not full withdrawal
                        || (lamports != self.lamports()?
                            // verify more than 0 stake left in previous stake
                            && checked_add(lamports, meta.rent_exempt_reserve)? >= self.lamports()?)
                    {
                        return Err(InstructionError::InsufficientFunds);
                    }
                    // split the stake, subtract rent_exempt_balance unless
                    // the destination account already has those lamports
                    // in place.
                    // this means that the new stake account will have a stake equivalent to
                    // lamports minus rent_exempt_reserve if it starts out with a zero balance
                    let (remaining_stake_delta, split_stake_amount) = if lamports
                        == self.lamports()?
                    {
                        // If split amount equals the full source stake, the new split stake must
                        // equal the same amount, regardless of any current lamport balance in the
                        // split account. Since split accounts retain the state of their source
                        // account, this prevents any magic activation of stake by prefunding the
                        // split account.
                        // The new split stake also needs to ignore any positive delta between the
                        // original rent_exempt_reserve and the split_rent_exempt_reserve, in order
                        // to prevent magic activation of stake by splitting between accounts of
                        // different sizes.
                        let remaining_stake_delta =
                            lamports.saturating_sub(meta.rent_exempt_reserve);
                        (remaining_stake_delta, remaining_stake_delta)
                    } else {
                        // Otherwise, the new split stake should reflect the entire split
                        // requested, less any lamports needed to cover the split_rent_exempt_reserve
                        (
                            lamports,
                            lamports - split_rent_exempt_reserve.saturating_sub(split.lamports()?),
                        )
                    };
                    let split_stake = stake.split(remaining_stake_delta, split_stake_amount)?;
                    let mut split_meta = meta;
                    split_meta.rent_exempt_reserve = split_rent_exempt_reserve;

                    self.set_state(&StakeState::Stake(meta, stake))?;
                    split.set_state(&StakeState::Stake(split_meta, split_stake))?;
                }
                StakeState::Initialized(meta) => {
                    meta.authorized.check(signers, StakeAuthorize::Staker)?;
                    let split_rent_exempt_reserve = calculate_split_rent_exempt_reserve(
                        meta.rent_exempt_reserve,
                        self.data_len()? as u64,
                        split.data_len()? as u64,
                    );

                    // enough lamports for rent and more than 0 stake in new split account
                    if lamports <= split_rent_exempt_reserve.saturating_sub(split.lamports()?)
                        // if not full withdrawal
                        || (lamports != self.lamports()?
                            // verify more than 0 stake left in previous stake
                            && checked_add(lamports, meta.rent_exempt_reserve)? >= self.lamports()?)
                    {
                        return Err(InstructionError::InsufficientFunds);
                    }

                    let mut split_meta = meta;
                    split_meta.rent_exempt_reserve = split_rent_exempt_reserve;
                    split.set_state(&StakeState::Initialized(split_meta))?;
                }
                StakeState::Uninitialized => {
                    if !signers.contains(self.unsigned_key()) {
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                }
                _ => return Err(InstructionError::InvalidAccountData),
            }

            // Deinitialize state upon zero balance
            if lamports == self.lamports()? {
                self.set_state(&StakeState::Uninitialized)?;
            }

            split
                .try_account_ref_mut()?
                .checked_add_lamports(lamports)?;
            self.try_account_ref_mut()?.checked_sub_lamports(lamports)?;
            Ok(())
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }

    fn merge(
        &self,
        invoke_context: &dyn InvokeContext,
        source_account: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        signers: &HashSet<Pubkey>,
        can_merge_expired_lockups: bool,
    ) -> Result<(), InstructionError> {
        // Ensure source isn't spoofed
        if source_account.owner()? != id() {
            return Err(InstructionError::IncorrectProgramId);
        }
        // Close the self-reference loophole
        if source_account.unsigned_key() == self.unsigned_key() {
            return Err(InstructionError::InvalidArgument);
        }

        ic_msg!(invoke_context, "Checking if destination stake is mergeable");
        let stake_merge_kind =
            MergeKind::get_if_mergeable(invoke_context, self, clock, stake_history)?;
        let meta = stake_merge_kind.meta();

        // Authorized staker is allowed to split/merge accounts
        meta.authorized.check(signers, StakeAuthorize::Staker)?;

        ic_msg!(invoke_context, "Checking if source stake is mergeable");
        let source_merge_kind =
            MergeKind::get_if_mergeable(invoke_context, source_account, clock, stake_history)?;

        let clock = if can_merge_expired_lockups {
            Some(clock)
        } else {
            None
        };

        ic_msg!(invoke_context, "Merging stake accounts");
        if let Some(merged_state) =
            stake_merge_kind.merge(invoke_context, source_merge_kind, clock)?
        {
            self.set_state(&merged_state)?;
        }

        // Source is about to be drained, deinitialize its state
        source_account.set_state(&StakeState::Uninitialized)?;

        // Drain the source stake account
        let lamports = source_account.lamports()?;
        source_account
            .try_account_ref_mut()?
            .checked_sub_lamports(lamports)?;
        self.try_account_ref_mut()?.checked_add_lamports(lamports)?;
        Ok(())
    }

    fn withdraw(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        withdraw_authority: &KeyedAccount,
        custodian: Option<&KeyedAccount>,
        prevent_withdraw_to_zero: bool,
    ) -> Result<(), InstructionError> {
        let mut signers = HashSet::new();
        let withdraw_authority_pubkey = withdraw_authority
            .signer_key()
            .ok_or(InstructionError::MissingRequiredSignature)?;
        signers.insert(*withdraw_authority_pubkey);

        let (lockup, reserve, is_staked) = match self.state()? {
            StakeState::Stake(meta, stake) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Withdrawer)?;
                // if we have a deactivation epoch and we're in cooldown
                let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                    stake.delegation.stake(clock.epoch, Some(stake_history))
                } else {
                    // Assume full stake if the stake account hasn't been
                    //  de-activated, because in the future the exposed stake
                    //  might be higher than stake.stake() due to warmup
                    stake.delegation.stake
                };

                let staked_and_reserve = checked_add(staked, meta.rent_exempt_reserve)?;
                (meta.lockup, staked_and_reserve, staked != 0)
            }
            StakeState::Initialized(meta) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Withdrawer)?;
                let reserve = if prevent_withdraw_to_zero {
                    checked_add(meta.rent_exempt_reserve, 1)? // stake accounts must have a balance > rent_exempt_reserve
                } else {
                    meta.rent_exempt_reserve
                };

                (meta.lockup, reserve, false)
            }
            StakeState::Uninitialized => {
                if !signers.contains(self.unsigned_key()) {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                (Lockup::default(), 0, false) // no lockup, no restrictions
            }
            _ => return Err(InstructionError::InvalidAccountData),
        };

        // verify that lockup has expired or that the withdrawal is signed by
        //   the custodian, both epoch and unix_timestamp must have passed
        let custodian_pubkey = custodian.and_then(|keyed_account| keyed_account.signer_key());
        if lockup.is_in_force(clock, custodian_pubkey) {
            return Err(StakeError::LockupInForce.into());
        }

        let lamports_and_reserve = checked_add(lamports, reserve)?;
        // if the stake is active, we mustn't allow the account to go away
        if is_staked // line coverage for branch coverage
            && lamports_and_reserve > self.lamports()?
        {
            return Err(InstructionError::InsufficientFunds);
        }

        if lamports != self.lamports()? // not a full withdrawal
            && lamports_and_reserve > self.lamports()?
        {
            assert!(!is_staked);
            return Err(InstructionError::InsufficientFunds);
        }

        // Deinitialize state upon zero balance
        if lamports == self.lamports()? {
            self.set_state(&StakeState::Uninitialized)?;
        }

        self.try_account_ref_mut()?.checked_sub_lamports(lamports)?;
        to.try_account_ref_mut()?.checked_add_lamports(lamports)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
enum MergeKind {
    Inactive(Meta, u64),
    ActivationEpoch(Meta, Stake),
    FullyActive(Meta, Stake),
}

impl MergeKind {
    fn meta(&self) -> &Meta {
        match self {
            Self::Inactive(meta, _) => meta,
            Self::ActivationEpoch(meta, _) => meta,
            Self::FullyActive(meta, _) => meta,
        }
    }

    fn active_stake(&self) -> Option<&Stake> {
        match self {
            Self::Inactive(_, _) => None,
            Self::ActivationEpoch(_, stake) => Some(stake),
            Self::FullyActive(_, stake) => Some(stake),
        }
    }

    fn get_if_mergeable(
        invoke_context: &dyn InvokeContext,
        stake_keyed_account: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
    ) -> Result<Self, InstructionError> {
        match stake_keyed_account.state()? {
            StakeState::Stake(meta, stake) => {
                // stake must not be in a transient state. Transient here meaning
                // activating or deactivating with non-zero effective stake.
                let status = stake
                    .delegation
                    .stake_activating_and_deactivating(clock.epoch, Some(stake_history));

                match (status.effective, status.activating, status.deactivating) {
                    (0, 0, 0) => Ok(Self::Inactive(meta, stake_keyed_account.lamports()?)),
                    (0, _, _) => Ok(Self::ActivationEpoch(meta, stake)),
                    (_, 0, 0) => Ok(Self::FullyActive(meta, stake)),
                    _ => {
                        let err = StakeError::MergeTransientStake;
                        ic_msg!(invoke_context, "{}", err);
                        Err(err.into())
                    }
                }
            }
            StakeState::Initialized(meta) => {
                Ok(Self::Inactive(meta, stake_keyed_account.lamports()?))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }

    fn metas_can_merge(
        invoke_context: &dyn InvokeContext,
        stake: &Meta,
        source: &Meta,
        clock: Option<&Clock>,
    ) -> Result<(), InstructionError> {
        let can_merge_lockups = match clock {
            // pre-v4 behavior. lockups must match, even when expired
            None => stake.lockup == source.lockup,
            // v4 behavior. lockups may mismatch so long as both have expired
            Some(clock) => {
                stake.lockup == source.lockup
                    || (!stake.lockup.is_in_force(clock, None)
                        && !source.lockup.is_in_force(clock, None))
            }
        };
        // `rent_exempt_reserve` has no bearing on the mergeability of accounts,
        // as the source account will be culled by runtime once the operation
        // succeeds. Considering it here would needlessly prevent merging stake
        // accounts with differing data lengths, which already exist in the wild
        // due to an SDK bug
        if stake.authorized == source.authorized && can_merge_lockups {
            Ok(())
        } else {
            ic_msg!(invoke_context, "Unable to merge due to metadata mismatch");
            Err(StakeError::MergeMismatch.into())
        }
    }

    fn active_delegations_can_merge(
        invoke_context: &dyn InvokeContext,
        stake: &Delegation,
        source: &Delegation,
    ) -> Result<(), InstructionError> {
        if stake.voter_pubkey != source.voter_pubkey {
            ic_msg!(invoke_context, "Unable to merge due to voter mismatch");
            Err(StakeError::MergeMismatch.into())
        } else if (stake.warmup_cooldown_rate - source.warmup_cooldown_rate).abs() < f64::EPSILON
            && stake.deactivation_epoch == Epoch::MAX
            && source.deactivation_epoch == Epoch::MAX
        {
            Ok(())
        } else {
            ic_msg!(invoke_context, "Unable to merge due to stake deactivation");
            Err(StakeError::MergeMismatch.into())
        }
    }

    // Remove this when the `stake_merge_with_unmatched_credits_observed` feature is removed
    fn active_stakes_can_merge(
        invoke_context: &dyn InvokeContext,
        stake: &Stake,
        source: &Stake,
    ) -> Result<(), InstructionError> {
        Self::active_delegations_can_merge(invoke_context, &stake.delegation, &source.delegation)?;
        // `credits_observed` MUST match to prevent earning multiple rewards
        // from a stake account by merging it into another stake account that
        // is small enough to not be paid out every epoch. This would effectively
        // reset the larger stake accounts `credits_observed` to that of the
        // smaller account.
        if stake.credits_observed == source.credits_observed {
            Ok(())
        } else {
            ic_msg!(
                invoke_context,
                "Unable to merge due to credits observed mismatch"
            );
            Err(StakeError::MergeMismatch.into())
        }
    }

    fn merge(
        self,
        invoke_context: &dyn InvokeContext,
        source: Self,
        clock: Option<&Clock>,
    ) -> Result<Option<StakeState>, InstructionError> {
        Self::metas_can_merge(invoke_context, self.meta(), source.meta(), clock)?;
        self.active_stake()
            .zip(source.active_stake())
            .map(|(stake, source)| {
                if invoke_context
                    .is_feature_active(&stake_merge_with_unmatched_credits_observed::id())
                {
                    Self::active_delegations_can_merge(
                        invoke_context,
                        &stake.delegation,
                        &source.delegation,
                    )
                } else {
                    Self::active_stakes_can_merge(invoke_context, stake, source)
                }
            })
            .unwrap_or(Ok(()))?;
        let merged_state = match (self, source) {
            (Self::Inactive(_, _), Self::Inactive(_, _)) => None,
            (Self::Inactive(_, _), Self::ActivationEpoch(_, _)) => None,
            (Self::ActivationEpoch(meta, mut stake), Self::Inactive(_, source_lamports)) => {
                stake.delegation.stake = checked_add(stake.delegation.stake, source_lamports)?;
                Some(StakeState::Stake(meta, stake))
            }
            (
                Self::ActivationEpoch(meta, mut stake),
                Self::ActivationEpoch(source_meta, source_stake),
            ) => {
                let source_lamports = checked_add(
                    source_meta.rent_exempt_reserve,
                    source_stake.delegation.stake,
                )?;
                merge_delegation_stake_and_credits_observed(
                    invoke_context,
                    &mut stake,
                    source_lamports,
                    source_stake.credits_observed,
                )?;
                Some(StakeState::Stake(meta, stake))
            }
            (Self::FullyActive(meta, mut stake), Self::FullyActive(_, source_stake)) => {
                // Don't stake the source account's `rent_exempt_reserve` to
                // protect against the magic activation loophole. It will
                // instead be moved into the destination account as extra,
                // withdrawable `lamports`
                merge_delegation_stake_and_credits_observed(
                    invoke_context,
                    &mut stake,
                    source_stake.delegation.stake,
                    source_stake.credits_observed,
                )?;
                Some(StakeState::Stake(meta, stake))
            }
            _ => return Err(StakeError::MergeMismatch.into()),
        };
        Ok(merged_state)
    }
}

fn merge_delegation_stake_and_credits_observed(
    invoke_context: &dyn InvokeContext,
    stake: &mut Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Result<(), InstructionError> {
    if invoke_context.is_feature_active(&stake_merge_with_unmatched_credits_observed::id()) {
        stake.credits_observed =
            stake_weighted_credits_observed(stake, absorbed_lamports, absorbed_credits_observed)
                .ok_or(InstructionError::ArithmeticOverflow)?;
    }
    stake.delegation.stake = checked_add(stake.delegation.stake, absorbed_lamports)?;
    Ok(())
}

/// Calculate the effective credits observed for two stakes when merging
///
/// When merging two `ActivationEpoch` or `FullyActive` stakes, the credits
/// observed of the merged stake is the weighted average of the two stakes'
/// credits observed.
///
/// This is because we can derive the effective credits_observed by reversing the staking
/// rewards equation, _while keeping the rewards unchanged after merge (i.e. strong
/// requirement)_, like below:
///
/// a(N) => account, r => rewards, s => stake, c => credits:
/// assume:
///   a3 = merge(a1, a2)
/// then:
///   a3.s = a1.s + a2.s
///
/// Next, given:
///   aN.r = aN.c * aN.s (for every N)
/// finally:
///        a3.r = a1.r + a2.r
/// a3.c * a3.s = a1.c * a1.s + a2.c * a2.s
///        a3.c = (a1.c * a1.s + a2.c * a2.s) / (a1.s + a2.s)     // QED
///
/// (For this discussion, we omitted irrelevant variables, including distance
///  calculation against vote_account and point indirection.)
fn stake_weighted_credits_observed(
    stake: &Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Option<u64> {
    if stake.credits_observed == absorbed_credits_observed {
        Some(stake.credits_observed)
    } else {
        let total_stake = u128::from(stake.delegation.stake.checked_add(absorbed_lamports)?);
        let stake_weighted_credits =
            u128::from(stake.credits_observed).checked_mul(u128::from(stake.delegation.stake))?;
        let absorbed_weighted_credits =
            u128::from(absorbed_credits_observed).checked_mul(u128::from(absorbed_lamports))?;
        // Discard fractional credits as a merge side-effect friction by taking
        // the ceiling, done by adding `denominator - 1` to the numerator.
        let total_weighted_credits = stake_weighted_credits
            .checked_add(absorbed_weighted_credits)?
            .checked_add(total_stake)?
            .checked_sub(1)?;
        u64::try_from(total_weighted_credits.checked_div(total_stake)?).ok()
    }
}

// utility function, used by runtime
// returns a tuple of (stakers_reward,voters_reward)
#[doc(hidden)]
#[deprecated(note = "remove after optimize_epoch_boundary_updates feature is active")]
pub fn redeem_rewards_slow(
    rewarded_epoch: Epoch,
    stake_account: &mut AccountSharedData,
    vote_account: &mut AccountSharedData,
    vote_state: &VoteState,
    point_value: &PointValue,
    stake_history: Option<&StakeHistory>,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    fix_activating_credits_observed: bool,
) -> Result<(u64, u64), InstructionError> {
    if let StakeState::Stake(meta, mut stake) = stake_account.state()? {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(
                &InflationPointCalculationEvent::EffectiveStakeAtRewardedEpoch(
                    stake.stake(rewarded_epoch, stake_history),
                ),
            );
            inflation_point_calc_tracer(&InflationPointCalculationEvent::RentExemptReserve(
                meta.rent_exempt_reserve,
            ));
            inflation_point_calc_tracer(&InflationPointCalculationEvent::Commission(
                vote_state.commission,
            ));
        }

        if let Some((stakers_reward, voters_reward)) = redeem_stake_rewards(
            rewarded_epoch,
            &mut stake,
            point_value,
            vote_state,
            stake_history,
            inflation_point_calc_tracer,
            fix_activating_credits_observed,
        ) {
            stake_account.checked_add_lamports(stakers_reward)?;
            vote_account.checked_add_lamports(voters_reward)?;

            stake_account.set_state(&StakeState::Stake(meta, stake))?;

            Ok((stakers_reward, voters_reward))
        } else {
            Err(StakeError::NoCreditsToRedeem.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

// utility function, used by runtime
#[doc(hidden)]
#[deprecated(note = "remove after optimize_epoch_boundary_updates feature is active")]
pub fn calculate_points_slow(
    stake_account: &AccountSharedData,
    vote_account: &AccountSharedData,
    stake_history: Option<&StakeHistory>,
) -> Result<u128, InstructionError> {
    if let StakeState::Stake(_meta, stake) = stake_account.state()? {
        let vote_state: VoteState =
            StateMut::<VoteStateVersions>::state(vote_account)?.convert_to_current();

        Ok(calculate_stake_points(
            &stake,
            &vote_state,
            stake_history,
            null_tracer(),
        ))
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

// utility function, used by runtime
// returns a tuple of (stakers_reward,voters_reward)
#[doc(hidden)]
pub fn redeem_rewards(
    rewarded_epoch: Epoch,
    stake_state: StakeState,
    stake_account: &mut AccountSharedData,
    vote_state: &VoteState,
    point_value: &PointValue,
    stake_history: Option<&StakeHistory>,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    fix_activating_credits_observed: bool,
) -> Result<(u64, u64), InstructionError> {
    if let StakeState::Stake(meta, mut stake) = stake_state {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(
                &InflationPointCalculationEvent::EffectiveStakeAtRewardedEpoch(
                    stake.stake(rewarded_epoch, stake_history),
                ),
            );
            inflation_point_calc_tracer(&InflationPointCalculationEvent::RentExemptReserve(
                meta.rent_exempt_reserve,
            ));
            inflation_point_calc_tracer(&InflationPointCalculationEvent::Commission(
                vote_state.commission,
            ));
        }

        if let Some((stakers_reward, voters_reward)) = redeem_stake_rewards(
            rewarded_epoch,
            &mut stake,
            point_value,
            vote_state,
            stake_history,
            inflation_point_calc_tracer,
            fix_activating_credits_observed,
        ) {
            stake_account.checked_add_lamports(stakers_reward)?;
            stake_account.set_state(&StakeState::Stake(meta, stake))?;

            Ok((stakers_reward, voters_reward))
        } else {
            Err(StakeError::NoCreditsToRedeem.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

// utility function, used by runtime
#[doc(hidden)]
pub fn calculate_points(
    stake_state: &StakeState,
    vote_state: &VoteState,
    stake_history: Option<&StakeHistory>,
) -> Result<u128, InstructionError> {
    if let StakeState::Stake(_meta, stake) = stake_state {
        Ok(calculate_stake_points(
            stake,
            vote_state,
            stake_history,
            null_tracer(),
        ))
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

// utility function, used by Split
//This emulates current Rent math in order to preserve backward compatibility. In the future, and
//to support variable rent, the Split instruction should pass in the Rent sysvar instead.
fn calculate_split_rent_exempt_reserve(
    source_rent_exempt_reserve: u64,
    source_data_len: u64,
    split_data_len: u64,
) -> u64 {
    let lamports_per_byte_year =
        source_rent_exempt_reserve / (source_data_len + ACCOUNT_STORAGE_OVERHEAD);
    lamports_per_byte_year * (split_data_len + ACCOUNT_STORAGE_OVERHEAD)
}

pub type RewriteStakeStatus = (&'static str, (u64, u64), (u64, u64));

// utility function, used by runtime::Stakes, tests
pub fn new_stake_history_entry<'a, I>(
    epoch: Epoch,
    stakes: I,
    history: Option<&StakeHistory>,
) -> StakeHistoryEntry
where
    I: Iterator<Item = &'a Delegation>,
{
    stakes.fold(StakeHistoryEntry::default(), |sum, stake| {
        sum + stake.stake_activating_and_deactivating(epoch, history)
    })
}

// genesis investor accounts
pub fn create_lockup_stake_account(
    authorized: &Authorized,
    lockup: &Lockup,
    rent: &Rent,
    lamports: u64,
) -> AccountSharedData {
    let mut stake_account =
        AccountSharedData::new(lamports, std::mem::size_of::<StakeState>(), &id());

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data().len());
    assert!(
        lamports >= rent_exempt_reserve,
        "lamports: {} is less than rent_exempt_reserve {}",
        lamports,
        rent_exempt_reserve
    );

    stake_account
        .set_state(&StakeState::Initialized(Meta {
            authorized: *authorized,
            lockup: *lockup,
            rent_exempt_reserve,
        }))
        .expect("set_state");

    stake_account
}

// utility function, used by Bank, tests, genesis for bootstrap
pub fn create_account(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
) -> AccountSharedData {
    do_create_account(
        authorized,
        voter_pubkey,
        vote_account,
        rent,
        lamports,
        Epoch::MAX,
    )
}

// utility function, used by tests
pub fn create_account_with_activation_epoch(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
    activation_epoch: Epoch,
) -> AccountSharedData {
    do_create_account(
        authorized,
        voter_pubkey,
        vote_account,
        rent,
        lamports,
        activation_epoch,
    )
}

fn do_create_account(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
    activation_epoch: Epoch,
) -> AccountSharedData {
    let mut stake_account =
        AccountSharedData::new(lamports, std::mem::size_of::<StakeState>(), &id());

    let vote_state = VoteState::from(vote_account).expect("vote_state");

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data().len());

    stake_account
        .set_state(&StakeState::Stake(
            Meta {
                authorized: Authorized::auto(authorized),
                rent_exempt_reserve,
                ..Meta::default()
            },
            new_stake(
                lamports - rent_exempt_reserve, // underflow is an error, is basically: assert!(lamports > rent_exempt_reserve);
                voter_pubkey,
                &vote_state,
                activation_epoch,
                &Config::default(),
            ),
        ))
        .expect("set_state");

    stake_account
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        clock::UnixTimestamp,
        native_token,
        process_instruction::MockInvokeContext,
        pubkey::Pubkey,
        system_program,
    };
    use solana_vote_program::vote_state;
    use std::{cell::RefCell, iter::FromIterator};

    #[test]
    fn test_authorized_authorize() {
        let staker = solana_sdk::pubkey::new_rand();
        let mut authorized = Authorized::auto(&staker);
        let mut signers = HashSet::new();
        assert_eq!(
            authorized.authorize(&signers, &staker, StakeAuthorize::Staker, None),
            Err(InstructionError::MissingRequiredSignature)
        );
        signers.insert(staker);
        assert_eq!(
            authorized.authorize(&signers, &staker, StakeAuthorize::Staker, None),
            Ok(())
        );
    }

    #[test]
    fn test_authorized_authorize_with_custodian() {
        let staker = solana_sdk::pubkey::new_rand();
        let custodian = solana_sdk::pubkey::new_rand();
        let invalid_custodian = solana_sdk::pubkey::new_rand();
        let mut authorized = Authorized::auto(&staker);
        let mut signers = HashSet::new();
        signers.insert(staker);

        let lockup = Lockup {
            epoch: 1,
            unix_timestamp: 1,
            custodian,
        };
        let clock = Clock {
            epoch: 0,
            unix_timestamp: 0,
            ..Clock::default()
        };

        // Legacy behaviour when the `require_custodian_for_locked_stake_authorize` feature is
        // inactive
        assert_eq!(
            authorized.authorize(&signers, &staker, StakeAuthorize::Withdrawer, None),
            Ok(())
        );

        // No lockup, no custodian
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&Lockup::default(), &clock, None))
            ),
            Ok(())
        );

        // No lockup, invalid custodian not a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&Lockup::default(), &clock, Some(&invalid_custodian)))
            ),
            Ok(()) // <== invalid custodian doesn't matter, there's no lockup
        );

        // Lockup active, invalid custodian not a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&invalid_custodian)))
            ),
            Err(StakeError::CustodianSignatureMissing.into()),
        );

        signers.insert(invalid_custodian);

        // No lockup, invalid custodian is a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&Lockup::default(), &clock, Some(&invalid_custodian)))
            ),
            Ok(()) // <== invalid custodian doesn't matter, there's no lockup
        );

        // Lockup active, invalid custodian is a signer
        signers.insert(invalid_custodian);
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&invalid_custodian)))
            ),
            Err(StakeError::LockupInForce.into()), // <== invalid custodian rejected
        );

        signers.remove(&invalid_custodian);

        // Lockup active, no custodian
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, None))
            ),
            Err(StakeError::CustodianMissing.into()),
        );

        // Lockup active, custodian not a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&custodian)))
            ),
            Err(StakeError::CustodianSignatureMissing.into()),
        );

        // Lockup active, custodian is a signer
        signers.insert(custodian);
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&custodian)))
            ),
            Ok(())
        );
    }

    #[test]
    fn test_stake_state_stake_from_fail() {
        let mut stake_account = AccountSharedData::new(0, std::mem::size_of::<StakeState>(), &id());

        stake_account
            .set_state(&StakeState::default())
            .expect("set_state");

        assert_eq!(stake_from(&stake_account), None);
    }

    #[test]
    fn test_stake_is_bootstrap() {
        assert!(Delegation {
            activation_epoch: std::u64::MAX,
            ..Delegation::default()
        }
        .is_bootstrap());
        assert!(!Delegation {
            activation_epoch: 0,
            ..Delegation::default()
        }
        .is_bootstrap());
    }

    #[test]
    fn test_stake_delegate() {
        let mut clock = Clock {
            epoch: 1,
            ..Clock::default()
        };

        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_pubkey_2 = solana_sdk::pubkey::new_rand();

        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_slot_vote_unchecked(i);
        }
        let mut vote_state_2 = VoteState::default();
        for i in 0..1000 {
            vote_state_2.process_slot_vote_unchecked(i);
        }

        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_account_2 = RefCell::new(vote_state::create_account(
            &vote_pubkey_2,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        let vote_keyed_account_2 = KeyedAccount::new(&vote_pubkey_2, false, &vote_account_2);

        let vote_state_credits = vote_state.credits();
        vote_keyed_account
            .set_state(&VoteStateVersions::new_current(vote_state))
            .unwrap();
        let vote_state_credits_2 = vote_state_2.credits();
        vote_keyed_account_2
            .set_state(&VoteStateVersions::new_current(vote_state_2))
            .unwrap();

        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta {
                authorized: Authorized {
                    staker: stake_pubkey,
                    withdrawer: stake_pubkey,
                },
                ..Meta::default()
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        // unsigned keyed account
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        let mut signers = HashSet::default();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        signers.insert(stake_pubkey);
        assert!(stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            )
            .is_ok());

        // verify that delegate() looks right, compare against hand-rolled
        let stake = stake_from(&stake_keyed_account.account.borrow()).unwrap();
        assert_eq!(
            stake,
            Stake {
                delegation: Delegation {
                    voter_pubkey: vote_pubkey,
                    stake: stake_lamports,
                    activation_epoch: clock.epoch,
                    deactivation_epoch: std::u64::MAX,
                    ..Delegation::default()
                },
                credits_observed: vote_state_credits,
            }
        );

        clock.epoch += 1;

        // verify that delegate fails as stake is active and not deactivating
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true
            ),
            Err(StakeError::TooSoonToRedelegate.into())
        );

        // deactivate
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        // verify that delegate to a different vote account fails
        // during deactivation
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account_2,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Err(StakeError::TooSoonToRedelegate.into())
        );

        // verify that delegate succeeds to same vote account
        // when stake is deactivating
        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            )
            .unwrap();

        // verify that deactivation has been cleared
        let stake = stake_from(&stake_keyed_account.account.borrow()).unwrap();
        assert_eq!(stake.delegation.deactivation_epoch, std::u64::MAX);

        // verify that delegate to a different vote account fails
        // if stake is still active
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account_2,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Err(StakeError::TooSoonToRedelegate.into())
        );

        // deactivate, so we can re-delegate
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        // without stake history, cool down is instantaneous
        clock.epoch += 1;

        // verify that delegate can be called to new vote account, 2nd is redelegate
        assert!(stake_keyed_account
            .delegate(
                &vote_keyed_account_2,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            )
            .is_ok());

        // signed but faked vote account
        let faked_vote_account = vote_account_2.clone();
        faked_vote_account
            .borrow_mut()
            .set_owner(solana_sdk::pubkey::new_rand());
        let faked_vote_keyed_account =
            KeyedAccount::new(&vote_pubkey_2, false, &faked_vote_account);
        assert_eq!(
            stake_keyed_account.delegate(
                &faked_vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Err(solana_sdk::instruction::InstructionError::IncorrectProgramId)
        );

        // verify that delegate() looks right, compare against hand-rolled
        let stake = stake_from(&stake_keyed_account.account.borrow()).unwrap();
        assert_eq!(
            stake,
            Stake {
                delegation: Delegation {
                    voter_pubkey: vote_pubkey_2,
                    stake: stake_lamports,
                    activation_epoch: clock.epoch,
                    deactivation_epoch: std::u64::MAX,
                    ..Delegation::default()
                },
                credits_observed: vote_state_credits_2,
            }
        );

        // verify that non-stakes fail delegate()
        let stake_state = StakeState::RewardsPool;

        stake_keyed_account.set_state(&stake_state).unwrap();
        assert!(stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            )
            .is_err());
    }

    fn create_stake_history_from_delegations(
        bootstrap: Option<u64>,
        epochs: std::ops::Range<Epoch>,
        delegations: &[Delegation],
    ) -> StakeHistory {
        let mut stake_history = StakeHistory::default();

        let bootstrap_delegation = if let Some(bootstrap) = bootstrap {
            vec![Delegation {
                activation_epoch: std::u64::MAX,
                stake: bootstrap,
                ..Delegation::default()
            }]
        } else {
            vec![]
        };

        for epoch in epochs {
            let entry = new_stake_history_entry(
                epoch,
                delegations.iter().chain(bootstrap_delegation.iter()),
                Some(&stake_history),
            );
            stake_history.add(epoch, entry);
        }

        stake_history
    }

    #[test]
    fn test_stake_activating_and_deactivating() {
        let stake = Delegation {
            stake: 1_000,
            activation_epoch: 0, // activating at zero
            deactivation_epoch: 5,
            ..Delegation::default()
        };

        // save this off so stake.config.warmup_rate changes don't break this test
        let increment = (1_000_f64 * stake.warmup_cooldown_rate) as u64;

        let mut stake_history = StakeHistory::default();
        // assert that this stake follows step function if there's no history
        assert_eq!(
            stake.stake_activating_and_deactivating(stake.activation_epoch, Some(&stake_history),),
            StakeActivationStatus::with_effective_and_activating(0, stake.stake),
        );
        for epoch in stake.activation_epoch + 1..stake.deactivation_epoch {
            assert_eq!(
                stake.stake_activating_and_deactivating(epoch, Some(&stake_history)),
                StakeActivationStatus::with_effective(stake.stake),
            );
        }
        // assert that this stake is full deactivating
        assert_eq!(
            stake
                .stake_activating_and_deactivating(stake.deactivation_epoch, Some(&stake_history),),
            StakeActivationStatus::with_deactivating(stake.stake),
        );
        // assert that this stake is fully deactivated if there's no history
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 1,
                Some(&stake_history),
            ),
            StakeActivationStatus::default(),
        );

        stake_history.add(
            0u64, // entry for zero doesn't have my activating amount
            StakeHistoryEntry {
                effective: 1_000,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(1, Some(&stake_history)),
            StakeActivationStatus::with_effective_and_activating(0, stake.stake),
        );

        stake_history.add(
            0u64, // entry for zero has my activating amount
            StakeHistoryEntry {
                effective: 1_000,
                activating: 1_000,
                ..StakeHistoryEntry::default()
            },
            // no entry for 1, so this stake gets shorted
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(2, Some(&stake_history)),
            StakeActivationStatus::with_effective_and_activating(
                increment,
                stake.stake - increment
            ),
        );

        // start over, test deactivation edge cases
        let mut stake_history = StakeHistory::default();

        stake_history.add(
            stake.deactivation_epoch, // entry for zero doesn't have my de-activating amount
            StakeHistoryEntry {
                effective: 1_000,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 1,
                Some(&stake_history),
            ),
            StakeActivationStatus::with_deactivating(stake.stake),
        );

        // put in my initial deactivating amount, but don't put in an entry for next
        stake_history.add(
            stake.deactivation_epoch, // entry for zero has my de-activating amount
            StakeHistoryEntry {
                effective: 1_000,
                deactivating: 1_000,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 2,
                Some(&stake_history),
            ),
            // hung, should be lower
            StakeActivationStatus::with_deactivating(stake.stake - increment),
        );
    }

    mod same_epoch_activation_then_deactivation {
        use super::*;

        enum OldDeactivationBehavior {
            Stuck,
            Slow,
        }

        fn do_test(
            old_behavior: OldDeactivationBehavior,
            expected_stakes: &[StakeActivationStatus],
        ) {
            let cluster_stake = 1_000;
            let activating_stake = 10_000;
            let some_stake = 700;
            let some_epoch = 0;

            let stake = Delegation {
                stake: some_stake,
                activation_epoch: some_epoch,
                deactivation_epoch: some_epoch,
                ..Delegation::default()
            };

            let mut stake_history = StakeHistory::default();
            let cluster_deactivation_at_stake_modified_epoch = match old_behavior {
                OldDeactivationBehavior::Stuck => 0,
                OldDeactivationBehavior::Slow => 1000,
            };

            let stake_history_entries = vec![
                (
                    cluster_stake,
                    activating_stake,
                    cluster_deactivation_at_stake_modified_epoch,
                ),
                (cluster_stake, activating_stake, 1000),
                (cluster_stake, activating_stake, 1000),
                (cluster_stake, activating_stake, 100),
                (cluster_stake, activating_stake, 100),
                (cluster_stake, activating_stake, 100),
                (cluster_stake, activating_stake, 100),
            ];

            for (epoch, (effective, activating, deactivating)) in
                stake_history_entries.into_iter().enumerate()
            {
                stake_history.add(
                    epoch as Epoch,
                    StakeHistoryEntry {
                        effective,
                        activating,
                        deactivating,
                    },
                );
            }

            assert_eq!(
                expected_stakes,
                (0..expected_stakes.len())
                    .map(|epoch| stake
                        .stake_activating_and_deactivating(epoch as u64, Some(&stake_history),))
                    .collect::<Vec<_>>()
            );
        }

        #[test]
        fn test_new_behavior_previously_slow() {
            // any stake accounts activated and deactivated at the same epoch
            // shouldn't been activated (then deactivated) at all!

            do_test(
                OldDeactivationBehavior::Slow,
                &[
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                ],
            );
        }

        #[test]
        fn test_new_behavior_previously_stuck() {
            // any stake accounts activated and deactivated at the same epoch
            // shouldn't been activated (then deactivated) at all!

            do_test(
                OldDeactivationBehavior::Stuck,
                &[
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                ],
            );
        }
    }

    #[test]
    fn test_inflation_and_slashing_with_activating_and_deactivating_stake() {
        // some really boring delegation and stake_history setup
        let (delegated_stake, mut stake, stake_history) = {
            let cluster_stake = 1_000;
            let delegated_stake = 700;

            let stake = Delegation {
                stake: delegated_stake,
                activation_epoch: 0,
                deactivation_epoch: 4,
                ..Delegation::default()
            };

            let mut stake_history = StakeHistory::default();
            stake_history.add(
                0,
                StakeHistoryEntry {
                    effective: cluster_stake,
                    activating: delegated_stake,
                    ..StakeHistoryEntry::default()
                },
            );
            let newly_effective_at_epoch1 = (cluster_stake as f64 * 0.25) as u64;
            assert_eq!(newly_effective_at_epoch1, 250);
            stake_history.add(
                1,
                StakeHistoryEntry {
                    effective: cluster_stake + newly_effective_at_epoch1,
                    activating: delegated_stake - newly_effective_at_epoch1,
                    ..StakeHistoryEntry::default()
                },
            );
            let newly_effective_at_epoch2 =
                ((cluster_stake + newly_effective_at_epoch1) as f64 * 0.25) as u64;
            assert_eq!(newly_effective_at_epoch2, 312);
            stake_history.add(
                2,
                StakeHistoryEntry {
                    effective: cluster_stake
                        + newly_effective_at_epoch1
                        + newly_effective_at_epoch2,
                    activating: delegated_stake
                        - newly_effective_at_epoch1
                        - newly_effective_at_epoch2,
                    ..StakeHistoryEntry::default()
                },
            );
            stake_history.add(
                3,
                StakeHistoryEntry {
                    effective: cluster_stake + delegated_stake,
                    ..StakeHistoryEntry::default()
                },
            );
            stake_history.add(
                4,
                StakeHistoryEntry {
                    effective: cluster_stake + delegated_stake,
                    deactivating: delegated_stake,
                    ..StakeHistoryEntry::default()
                },
            );
            let newly_not_effective_stake_at_epoch5 =
                ((cluster_stake + delegated_stake) as f64 * 0.25) as u64;
            assert_eq!(newly_not_effective_stake_at_epoch5, 425);
            stake_history.add(
                5,
                StakeHistoryEntry {
                    effective: cluster_stake + delegated_stake
                        - newly_not_effective_stake_at_epoch5,
                    deactivating: delegated_stake - newly_not_effective_stake_at_epoch5,
                    ..StakeHistoryEntry::default()
                },
            );

            (delegated_stake, stake, stake_history)
        };

        // helper closures
        let calculate_each_staking_status = |stake: &Delegation, epoch_count: usize| -> Vec<_> {
            (0..epoch_count)
                .map(|epoch| {
                    stake.stake_activating_and_deactivating(epoch as u64, Some(&stake_history))
                })
                .collect::<Vec<_>>()
        };
        let adjust_staking_status = |rate: f64, status: &[StakeActivationStatus]| {
            status
                .iter()
                .map(|entry| StakeActivationStatus {
                    effective: (entry.effective as f64 * rate) as u64,
                    activating: (entry.activating as f64 * rate) as u64,
                    deactivating: (entry.deactivating as f64 * rate) as u64,
                })
                .collect::<Vec<_>>()
        };

        let expected_staking_status_transition = vec![
            StakeActivationStatus::with_effective_and_activating(0, 700),
            StakeActivationStatus::with_effective_and_activating(250, 450),
            StakeActivationStatus::with_effective_and_activating(562, 138),
            StakeActivationStatus::with_effective(700),
            StakeActivationStatus::with_deactivating(700),
            StakeActivationStatus::with_deactivating(275),
            StakeActivationStatus::default(),
        ];
        let expected_staking_status_transition_base = vec![
            StakeActivationStatus::with_effective_and_activating(0, 700),
            StakeActivationStatus::with_effective_and_activating(250, 450),
            StakeActivationStatus::with_effective_and_activating(562, 138 + 1), // +1 is needed for rounding
            StakeActivationStatus::with_effective(700),
            StakeActivationStatus::with_deactivating(700),
            StakeActivationStatus::with_deactivating(275 + 1), // +1 is needed for rounding
            StakeActivationStatus::default(),
        ];

        // normal stake activating and deactivating transition test, just in case
        assert_eq!(
            expected_staking_status_transition,
            calculate_each_staking_status(&stake, expected_staking_status_transition.len())
        );

        // 10% inflation rewards assuming some sizable epochs passed!
        let rate = 1.10;
        stake.stake = (delegated_stake as f64 * rate) as u64;
        let expected_staking_status_transition =
            adjust_staking_status(rate, &expected_staking_status_transition_base);

        assert_eq!(
            expected_staking_status_transition,
            calculate_each_staking_status(&stake, expected_staking_status_transition_base.len()),
        );

        // 50% slashing!!!
        let rate = 0.5;
        stake.stake = (delegated_stake as f64 * rate) as u64;
        let expected_staking_status_transition =
            adjust_staking_status(rate, &expected_staking_status_transition_base);

        assert_eq!(
            expected_staking_status_transition,
            calculate_each_staking_status(&stake, expected_staking_status_transition_base.len()),
        );
    }

    #[test]
    fn test_stop_activating_after_deactivation() {
        solana_logger::setup();
        let stake = Delegation {
            stake: 1_000,
            activation_epoch: 0,
            deactivation_epoch: 3,
            ..Delegation::default()
        };

        let base_stake = 1_000;
        let mut stake_history = StakeHistory::default();
        let mut effective = base_stake;
        let other_activation = 100;
        let mut other_activations = vec![0];

        // Build a stake history where the test staker always consumes all of the available warm
        // up and cool down stake. However, simulate other stakers beginning to activate during
        // the test staker's deactivation.
        for epoch in 0..=stake.deactivation_epoch + 1 {
            let (activating, deactivating) = if epoch < stake.deactivation_epoch {
                (stake.stake + base_stake - effective, 0)
            } else {
                let other_activation_sum: u64 = other_activations.iter().sum();
                let deactivating = effective - base_stake - other_activation_sum;
                (other_activation, deactivating)
            };

            stake_history.add(
                epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );

            let effective_rate_limited = (effective as f64 * stake.warmup_cooldown_rate) as u64;
            if epoch < stake.deactivation_epoch {
                effective += effective_rate_limited.min(activating);
                other_activations.push(0);
            } else {
                effective -= effective_rate_limited.min(deactivating);
                effective += other_activation;
                other_activations.push(other_activation);
            }
        }

        for epoch in 0..=stake.deactivation_epoch + 1 {
            let history = stake_history.get(&epoch).unwrap();
            let other_activations: u64 = other_activations[..=epoch as usize].iter().sum();
            let expected_stake = history.effective - base_stake - other_activations;
            let (expected_activating, expected_deactivating) = if epoch < stake.deactivation_epoch {
                (history.activating, 0)
            } else {
                (0, history.deactivating)
            };
            assert_eq!(
                stake.stake_activating_and_deactivating(epoch, Some(&stake_history)),
                StakeActivationStatus {
                    effective: expected_stake,
                    activating: expected_activating,
                    deactivating: expected_deactivating,
                },
            );
        }
    }

    #[test]
    fn test_stake_warmup_cooldown_sub_integer_moves() {
        let delegations = [Delegation {
            stake: 2,
            activation_epoch: 0, // activating at zero
            deactivation_epoch: 5,
            ..Delegation::default()
        }];
        // give 2 epochs of cooldown
        let epochs = 7;
        // make boostrap stake smaller than warmup so warmup/cooldownn
        //  increment is always smaller than 1
        let bootstrap = (delegations[0].warmup_cooldown_rate * 100.0 / 2.0) as u64;
        let stake_history =
            create_stake_history_from_delegations(Some(bootstrap), 0..epochs, &delegations);
        let mut max_stake = 0;
        let mut min_stake = 2;

        for epoch in 0..epochs {
            let stake = delegations
                .iter()
                .map(|delegation| delegation.stake(epoch, Some(&stake_history)))
                .sum::<u64>();
            max_stake = max_stake.max(stake);
            min_stake = min_stake.min(stake);
        }
        assert_eq!(max_stake, 2);
        assert_eq!(min_stake, 0);
    }

    #[test]
    fn test_stake_warmup_cooldown() {
        let delegations = [
            Delegation {
                // never deactivates
                stake: 1_000,
                activation_epoch: std::u64::MAX,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 0,
                deactivation_epoch: 9,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 1,
                deactivation_epoch: 6,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 2,
                deactivation_epoch: 5,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 2,
                deactivation_epoch: 4,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 4,
                deactivation_epoch: 4,
                ..Delegation::default()
            },
        ];
        // chosen to ensure that the last activated stake (at 4) finishes
        //  warming up and cooling down
        //  a stake takes 2.0f64.log(1.0 + STAKE_WARMUP_RATE) epochs to warm up or cool down
        //  when all alone, but the above overlap a lot
        let epochs = 20;

        let stake_history = create_stake_history_from_delegations(None, 0..epochs, &delegations);

        let mut prev_total_effective_stake = delegations
            .iter()
            .map(|delegation| delegation.stake(0, Some(&stake_history)))
            .sum::<u64>();

        // uncomment and add ! for fun with graphing
        // eprintln("\n{:8} {:8} {:8}", "   epoch", "   total", "   delta");
        for epoch in 1..epochs {
            let total_effective_stake = delegations
                .iter()
                .map(|delegation| delegation.stake(epoch, Some(&stake_history)))
                .sum::<u64>();

            let delta = if total_effective_stake > prev_total_effective_stake {
                total_effective_stake - prev_total_effective_stake
            } else {
                prev_total_effective_stake - total_effective_stake
            };

            // uncomment and add ! for fun with graphing
            //eprint("{:8} {:8} {:8} ", epoch, total_effective_stake, delta);
            //(0..(total_effective_stake as usize / (stakes.len() * 5))).for_each(|_| eprint("#"));
            //eprintln();

            assert!(
                delta
                    <= ((prev_total_effective_stake as f64 * Config::default().warmup_cooldown_rate) as u64)
                        .max(1)
            );

            prev_total_effective_stake = total_effective_stake;
        }
    }

    #[test]
    fn test_stake_initialize() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account =
            AccountSharedData::new_ref(stake_lamports, std::mem::size_of::<StakeState>(), &id());

        // unsigned keyed account
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        let custodian = solana_sdk::pubkey::new_rand();

        // not enough balance for rent...
        assert_eq!(
            stake_keyed_account.initialize(
                &Authorized::default(),
                &Lockup::default(),
                &Rent {
                    lamports_per_byte_year: 42,
                    ..Rent::free()
                },
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // this one works, as is uninit
        assert_eq!(
            stake_keyed_account.initialize(
                &Authorized::auto(&stake_pubkey),
                &Lockup {
                    epoch: 1,
                    unix_timestamp: 0,
                    custodian
                },
                &Rent::free(),
            ),
            Ok(())
        );
        // check that we see what we expect
        assert_eq!(
            from(&stake_keyed_account.account.borrow()).unwrap(),
            StakeState::Initialized(Meta {
                lockup: Lockup {
                    unix_timestamp: 0,
                    epoch: 1,
                    custodian
                },
                ..Meta {
                    authorized: Authorized::auto(&stake_pubkey),
                    ..Meta::default()
                }
            })
        );

        // 2nd time fails, can't move it from anything other than uninit->init
        assert_eq!(
            stake_keyed_account.initialize(
                &Authorized::default(),
                &Lockup::default(),
                &Rent::free()
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_initialize_incorrect_account_sizes() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref(
            stake_lamports,
            std::mem::size_of::<StakeState>() + 1,
            &id(),
        );
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        assert_eq!(
            stake_keyed_account.initialize(
                &Authorized::default(),
                &Lockup::default(),
                &Rent {
                    lamports_per_byte_year: 42,
                    ..Rent::free()
                },
            ),
            Err(InstructionError::InvalidAccountData)
        );

        let stake_account = AccountSharedData::new_ref(
            stake_lamports,
            std::mem::size_of::<StakeState>() - 1,
            &id(),
        );
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        assert_eq!(
            stake_keyed_account.initialize(
                &Authorized::default(),
                &Lockup::default(),
                &Rent {
                    lamports_per_byte_year: 42,
                    ..Rent::free()
                },
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_deactivate() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let clock = Clock {
            epoch: 1,
            ..Clock::default()
        };

        // signed keyed account but not staked yet
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.deactivate(&clock, &signers),
            Err(InstructionError::InvalidAccountData)
        );

        // Staking
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Ok(())
        );

        // no signers fails
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        assert_eq!(
            stake_keyed_account.deactivate(&clock, &HashSet::default()),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Deactivate after staking
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(stake_keyed_account.deactivate(&clock, &signers), Ok(()));

        // verify that deactivate() only works once
        assert_eq!(
            stake_keyed_account.deactivate(&clock, &signers),
            Err(StakeError::AlreadyDeactivated.into())
        );
    }

    #[test]
    fn test_set_lockup() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        // wrong state, should fail
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        assert_eq!(
            stake_keyed_account.set_lockup(&LockupArgs::default(), &HashSet::default(), None),
            Err(InstructionError::InvalidAccountData)
        );

        // initalize the stake
        let custodian = solana_sdk::pubkey::new_rand();
        stake_keyed_account
            .initialize(
                &Authorized::auto(&stake_pubkey),
                &Lockup {
                    unix_timestamp: 1,
                    epoch: 1,
                    custodian,
                },
                &Rent::free(),
            )
            .unwrap();

        assert_eq!(
            stake_keyed_account.set_lockup(&LockupArgs::default(), &HashSet::default(), None),
            Err(InstructionError::MissingRequiredSignature)
        );

        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: Some(1),
                    epoch: Some(1),
                    custodian: Some(custodian),
                },
                &vec![custodian].into_iter().collect(),
                None
            ),
            Ok(())
        );

        // delegate stake
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();

        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &Config::default(),
                &vec![stake_pubkey].into_iter().collect(),
                true,
            )
            .unwrap();

        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: Some(1),
                    epoch: Some(1),
                    custodian: Some(custodian),
                },
                &HashSet::default(),
                None
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: Some(1),
                    epoch: Some(1),
                    custodian: Some(custodian),
                },
                &vec![custodian].into_iter().collect(),
                None
            ),
            Ok(())
        );
    }

    #[test]
    fn test_optional_lockup_for_stake_program_v3_and_earlier() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        let custodian = solana_sdk::pubkey::new_rand();
        stake_keyed_account
            .initialize(
                &Authorized::auto(&stake_pubkey),
                &Lockup {
                    unix_timestamp: 1,
                    epoch: 1,
                    custodian,
                },
                &Rent::free(),
            )
            .unwrap();

        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: None,
                    epoch: None,
                    custodian: None,
                },
                &vec![custodian].into_iter().collect(),
                None
            ),
            Ok(())
        );

        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: Some(2),
                    epoch: None,
                    custodian: None,
                },
                &vec![custodian].into_iter().collect(),
                None
            ),
            Ok(())
        );

        if let StakeState::Initialized(Meta { lockup, .. }) =
            from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(lockup.unix_timestamp, 2);
            assert_eq!(lockup.epoch, 1);
            assert_eq!(lockup.custodian, custodian);
        } else {
            panic!();
        }

        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: None,
                    epoch: Some(3),
                    custodian: None,
                },
                &vec![custodian].into_iter().collect(),
                None
            ),
            Ok(())
        );

        if let StakeState::Initialized(Meta { lockup, .. }) =
            from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(lockup.unix_timestamp, 2);
            assert_eq!(lockup.epoch, 3);
            assert_eq!(lockup.custodian, custodian);
        } else {
            panic!();
        }

        let new_custodian = solana_sdk::pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: None,
                    epoch: None,
                    custodian: Some(new_custodian),
                },
                &vec![custodian].into_iter().collect(),
                None
            ),
            Ok(())
        );

        if let StakeState::Initialized(Meta { lockup, .. }) =
            from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(lockup.unix_timestamp, 2);
            assert_eq!(lockup.epoch, 3);
            assert_eq!(lockup.custodian, new_custodian);
        } else {
            panic!();
        }

        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs::default(),
                &vec![custodian].into_iter().collect(),
                None
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
    }

    #[test]
    fn test_optional_lockup_for_stake_program_v4() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        let custodian = solana_sdk::pubkey::new_rand();
        stake_keyed_account
            .initialize(
                &Authorized::auto(&stake_pubkey),
                &Lockup {
                    unix_timestamp: 1,
                    epoch: 1,
                    custodian,
                },
                &Rent::free(),
            )
            .unwrap();

        // Lockup in force: authorized withdrawer cannot change it
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: Some(2),
                    epoch: None,
                    custodian: None
                },
                &vec![stake_pubkey].into_iter().collect(),
                Some(&Clock::default())
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Lockup in force: custodian can change it
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs {
                    unix_timestamp: Some(2),
                    epoch: None,
                    custodian: None
                },
                &vec![custodian].into_iter().collect(),
                Some(&Clock::default())
            ),
            Ok(())
        );

        // Lockup expired: custodian cannot change it
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs::default(),
                &vec![custodian].into_iter().collect(),
                Some(&Clock {
                    unix_timestamp: UnixTimestamp::MAX,
                    epoch: Epoch::MAX,
                    ..Clock::default()
                })
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Lockup expired: authorized withdrawer can change it
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs::default(),
                &vec![stake_pubkey].into_iter().collect(),
                Some(&Clock {
                    unix_timestamp: UnixTimestamp::MAX,
                    epoch: Epoch::MAX,
                    ..Clock::default()
                })
            ),
            Ok(())
        );

        // Change authorized withdrawer
        let new_withdraw_authority = solana_sdk::pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &vec![stake_pubkey].into_iter().collect(),
                &new_withdraw_authority,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );

        // Previous authorized withdrawer cannot change the lockup anymore
        assert_eq!(
            stake_keyed_account.set_lockup(
                &LockupArgs::default(),
                &vec![stake_pubkey].into_iter().collect(),
                Some(&Clock {
                    unix_timestamp: UnixTimestamp::MAX,
                    epoch: Epoch::MAX,
                    ..Clock::default()
                })
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
    }

    #[test]
    fn test_withdraw_stake() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let mut clock = Clock::default();

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        // no signers, should fail
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &to_keyed_account, // unsigned account as withdraw authority
                None,
                true,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account and uninitialized should work
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Ok(())
        );
        assert_eq!(stake_account.borrow().lamports(), 0);
        assert_eq!(stake_keyed_account.state(), Ok(StakeState::Uninitialized));

        // reset balance
        stake_account.borrow_mut().set_lamports(stake_lamports);

        // lockup
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let custodian = solana_sdk::pubkey::new_rand();
        stake_keyed_account
            .initialize(
                &Authorized::auto(&stake_pubkey),
                &Lockup {
                    unix_timestamp: 0,
                    epoch: 0,
                    custodian,
                },
                &Rent::free(),
            )
            .unwrap();

        // signed keyed account and locked up, more than available should fail
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports + 1,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Stake some lamports (available lamports for withdrawals will reduce to zero)
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Ok(())
        );

        // simulate rewards
        stake_account.borrow_mut().checked_add_lamports(10).unwrap();
        // withdrawal before deactivate works for rewards amount
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                10,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Ok(())
        );

        // simulate rewards
        stake_account.borrow_mut().checked_add_lamports(10).unwrap();
        // withdrawal of rewards fails if not in excess of stake
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                10 + 1,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // deactivate the stake before withdrawal
        assert_eq!(stake_keyed_account.deactivate(&clock, &signers), Ok(()));
        // simulate time passing
        clock.epoch += 100;

        // Try to withdraw more than what's available
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports + 10 + 1,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Try to withdraw all lamports
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports + 10,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Ok(())
        );
        assert_eq!(stake_account.borrow().lamports(), 0);
        assert_eq!(stake_keyed_account.state(), Ok(StakeState::Uninitialized));

        // overflow
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let authority_pubkey = Pubkey::new_unique();
        let stake_pubkey = Pubkey::new_unique();
        let stake_account = AccountSharedData::new_ref_data_with_space(
            1_000_000_000,
            &StakeState::Initialized(Meta {
                rent_exempt_reserve,
                authorized: Authorized {
                    staker: authority_pubkey,
                    withdrawer: authority_pubkey,
                },
                lockup: Lockup::default(),
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let stake2_pubkey = Pubkey::new_unique();
        let stake2_account = AccountSharedData::new_ref_data_with_space(
            1_000_000_000,
            &StakeState::Initialized(Meta {
                rent_exempt_reserve,
                authorized: Authorized {
                    staker: authority_pubkey,
                    withdrawer: authority_pubkey,
                },
                lockup: Lockup::default(),
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake2_account");

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let stake2_keyed_account = KeyedAccount::new(&stake2_pubkey, false, &stake2_account);
        let authority_account = AccountSharedData::new_ref(42, 0, &system_program::id());
        let authority_keyed_account =
            KeyedAccount::new(&authority_pubkey, true, &authority_account);

        assert_eq!(
            stake_keyed_account.withdraw(
                u64::MAX - 10,
                &stake2_keyed_account,
                &clock,
                &StakeHistory::default(),
                &authority_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds),
        );
    }

    #[test]
    fn test_withdraw_stake_before_warmup() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let total_lamports = 100;
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            total_lamports,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let clock = Clock::default();
        let mut future = Clock::default();
        future.epoch += 16;

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        // Stake some lamports (available lamports for withdrawals will reduce)
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &future,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            ),
            Ok(())
        );

        let stake_history = create_stake_history_from_delegations(
            None,
            0..future.epoch,
            &[stake_from(&stake_keyed_account.account.borrow())
                .unwrap()
                .delegation],
        );

        // Try to withdraw stake
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports + 1,
                &to_keyed_account,
                &clock,
                &stake_history,
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds)
        );
    }

    #[test]
    fn test_withdraw_stake_invalid_state() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let total_lamports = 100;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            total_lamports,
            &StakeState::RewardsPool,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_withdraw_lockup() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let custodian = solana_sdk::pubkey::new_rand();
        let total_lamports = 100;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            total_lamports,
            &StakeState::Initialized(Meta {
                lockup: Lockup {
                    unix_timestamp: 0,
                    epoch: 1,
                    custodian,
                },
                ..Meta::auto(&stake_pubkey)
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        let mut clock = Clock::default();

        // lockup is still in force, can't withdraw
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(StakeError::LockupInForce.into())
        );

        {
            let custodian_account = AccountSharedData::new_ref(1, 0, &system_program::id());
            let custodian_keyed_account = KeyedAccount::new(&custodian, true, &custodian_account);
            assert_eq!(
                stake_keyed_account.withdraw(
                    total_lamports,
                    &to_keyed_account,
                    &clock,
                    &StakeHistory::default(),
                    &stake_keyed_account,
                    Some(&custodian_keyed_account),
                    true,
                ),
                Ok(())
            );
        }
        // reset balance
        stake_keyed_account
            .account
            .borrow_mut()
            .set_lamports(total_lamports);

        // lockup has expired
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        clock.epoch += 1;
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Ok(())
        );
        assert_eq!(stake_keyed_account.state(), Ok(StakeState::Uninitialized));
    }

    #[test]
    fn test_withdraw_identical_authorities() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let custodian = stake_pubkey;
        let total_lamports = 100;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            total_lamports,
            &StakeState::Initialized(Meta {
                lockup: Lockup {
                    unix_timestamp: 0,
                    epoch: 1,
                    custodian,
                },
                ..Meta::auto(&stake_pubkey)
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        let clock = Clock::default();

        // lockup is still in force, even though custodian is the same as the withdraw authority
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(StakeError::LockupInForce.into())
        );

        {
            let custodian_keyed_account = KeyedAccount::new(&custodian, true, &stake_account);
            assert_eq!(
                stake_keyed_account.withdraw(
                    total_lamports,
                    &to_keyed_account,
                    &clock,
                    &StakeHistory::default(),
                    &stake_keyed_account,
                    Some(&custodian_keyed_account),
                    true,
                ),
                Ok(())
            );
            assert_eq!(stake_keyed_account.state(), Ok(StakeState::Uninitialized));
        }
    }

    #[test]
    fn test_withdraw_rent_exempt() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let clock = Clock::default();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake + rent_exempt_reserve,
            &StakeState::Initialized(Meta {
                rent_exempt_reserve,
                ..Meta::auto(&stake_pubkey)
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        // Withdrawing account down to only rent-exempt reserve should succeed before feature, and
        // fail after
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                false,
            ),
            Ok(())
        );
        stake_account
            .borrow_mut()
            .checked_add_lamports(stake)
            .unwrap(); // top up account
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Withdrawal that would leave less than rent-exempt reserve should fail
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake + 1,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                false,
            ),
            Err(InstructionError::InsufficientFunds)
        );
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake + 1,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Withdrawal of complete account should succeed
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake + rent_exempt_reserve,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
                true,
            ),
            Ok(())
        );
    }

    #[test]
    fn test_stake_state_redeem_rewards() {
        let mut vote_state = VoteState::default();
        // assume stake.stake() is right
        // bootstrap means fully-vested stake at epoch 0
        let stake_lamports = 1;
        let mut stake = new_stake(
            stake_lamports,
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
            &Config::default(),
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            redeem_stake_rewards(
                0,
                &mut stake,
                &PointValue {
                    rewards: 1_000_000_000,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0);
        vote_state.increment_credits(0);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some((stake_lamports * 2, 0)),
            redeem_stake_rewards(
                0,
                &mut stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        assert_eq!(
            stake.delegation.stake,
            stake_lamports + (stake_lamports * 2)
        );
        assert_eq!(stake.credits_observed, 2);
    }

    #[test]
    fn test_stake_state_calculate_points_with_typical_values() {
        let mut vote_state = VoteState::default();

        // bootstrap means fully-vested stake at epoch 0 with
        //  10_000_000 SOL is a big but not unreasaonable stake
        let stake = new_stake(
            native_token::sol_to_lamports(10_000_000f64),
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
            &Config::default(),
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 1_000_000_000,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        let epoch_slots: u128 = 14 * 24 * 3600 * 160;
        // put 193,536,000 credits in at epoch 0, typical for a 14-day epoch
        //  this loop takes a few seconds...
        for _ in 0..epoch_slots {
            vote_state.increment_credits(0);
        }

        // no overflow on points
        assert_eq!(
            u128::from(stake.delegation.stake) * epoch_slots,
            calculate_stake_points(&stake, &vote_state, None, null_tracer())
        );
    }

    #[test]
    fn test_stake_state_calculate_rewards() {
        let mut vote_state = VoteState::default();
        // assume stake.stake() is right
        // bootstrap means fully-vested stake at epoch 0
        let mut stake = new_stake(
            1,
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
            &Config::default(),
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 1_000_000_000,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0);
        vote_state.increment_credits(0);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some((stake.delegation.stake * 2, 0, 2)),
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 2,
                    points: 2 // all his
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        stake.credits_observed = 1;
        // this one should be able to collect exactly 1 (already observed one)
        assert_eq!(
            Some((stake.delegation.stake, 0, 2)),
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // put 1 credit in epoch 1
        vote_state.increment_credits(1);

        stake.credits_observed = 2;
        // this one should be able to collect the one just added
        assert_eq!(
            Some((stake.delegation.stake, 0, 3)),
            calculate_stake_rewards(
                1,
                &stake,
                &PointValue {
                    rewards: 2,
                    points: 2
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // put 1 credit in epoch 2
        vote_state.increment_credits(2);
        // this one should be able to collect 2 now
        assert_eq!(
            Some((stake.delegation.stake * 2, 0, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 2,
                    points: 2
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        stake.credits_observed = 0;
        // this one should be able to collect everything from t=0 a warmed up stake of 2
        // (2 credits at stake of 1) + (1 credit at a stake of 2)
        assert_eq!(
            Some((
                stake.delegation.stake * 2 // epoch 0
                    + stake.delegation.stake // epoch 1
                    + stake.delegation.stake, // epoch 2
                0,
                4
            )),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 4,
                    points: 4
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // same as above, but is a really small commission out of 32 bits,
        //  verify that None comes back on small redemptions where no one gets paid
        vote_state.commission = 1;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 4,
                    points: 4
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );
        vote_state.commission = 99;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 4,
                    points: 4
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // now one with inflation disabled. no one gets paid, but we still need
        // to advance the stake state's credits_observed field to prevent back-
        // paying rewards when inflation is turned on.
        assert_eq!(
            Some((0, 0, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 0,
                    points: 4
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // credits_observed remains at previous level when vote_state credits are
        // not advancing and inflation is disabled
        stake.credits_observed = 4;
        assert_eq!(
            Some((0, 0, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 0,
                    points: 4
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // assert the previous behavior is preserved where fix_stake_deactivate=false
        assert_eq!(
            (0, 4),
            calculate_stake_points_and_credits(&stake, &vote_state, None, null_tracer())
        );

        // get rewards and credits observed when not the activation epoch
        vote_state.commission = 0;
        stake.credits_observed = 3;
        stake.delegation.activation_epoch = 1;
        assert_eq!(
            Some((
                stake.delegation.stake, // epoch 2
                0,
                4
            )),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );

        // credits_observed is moved forward for the stake's activation epoch,
        // and no rewards are perceived
        stake.delegation.activation_epoch = 2;
        stake.credits_observed = 3;
        assert_eq!(
            Some((0, 0, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                None,
                null_tracer(),
                true,
            )
        );
    }

    #[test]
    fn test_authorize_uninit() {
        let new_authority = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::default(),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let stake_keyed_account = KeyedAccount::new(&new_authority, true, &stake_account);
        let signers = vec![new_authority].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &new_authority,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_authorize_lockup() {
        let stake_authority = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_authority)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let clock = Clock::default();
        let stake_keyed_account = KeyedAccount::new(&stake_authority, true, &stake_account);

        let stake_pubkey0 = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_authority].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &stake_pubkey0,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &stake_pubkey0,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );
        if let StakeState::Initialized(Meta { authorized, .. }) =
            from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(authorized.staker, stake_pubkey0);
            assert_eq!(authorized.withdrawer, stake_pubkey0);
        } else {
            panic!();
        }

        // A second authorization signed by the stake_keyed_account should fail
        let stake_pubkey1 = solana_sdk::pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &stake_pubkey1,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        let signers0 = vec![stake_pubkey0].into_iter().collect();

        // Test a second authorization by the newly authorized pubkey
        let stake_pubkey2 = solana_sdk::pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers0,
                &stake_pubkey2,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );
        if let StakeState::Initialized(Meta { authorized, .. }) =
            from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(authorized.staker, stake_pubkey2);
        }

        assert_eq!(
            stake_keyed_account.authorize(
                &signers0,
                &stake_pubkey2,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );
        if let StakeState::Initialized(Meta { authorized, .. }) =
            from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(authorized.staker, stake_pubkey2);
        }

        // Test that withdrawal to account fails without authorized withdrawer
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account, // old signer
                None,
                true,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        let stake_keyed_account2 = KeyedAccount::new(&stake_pubkey2, true, &stake_account);

        // Test a successful action by the currently authorized withdrawer
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account2,
                None,
                true,
            ),
            Ok(())
        );
        assert_eq!(stake_keyed_account.state(), Ok(StakeState::Uninitialized));
    }

    #[test]
    fn test_authorize_with_seed() {
        let base_pubkey = solana_sdk::pubkey::new_rand();
        let seed = "42";
        let withdrawer_pubkey = Pubkey::create_with_seed(&base_pubkey, seed, &id()).unwrap();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&withdrawer_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let base_account = AccountSharedData::new_ref(1, 0, &id());
        let base_keyed_account = KeyedAccount::new(&base_pubkey, true, &base_account);

        let stake_keyed_account = KeyedAccount::new(&withdrawer_pubkey, true, &stake_account);

        let new_authority = solana_sdk::pubkey::new_rand();

        // Wrong seed
        assert_eq!(
            stake_keyed_account.authorize_with_seed(
                &base_keyed_account,
                "",
                &id(),
                &new_authority,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Wrong base
        assert_eq!(
            stake_keyed_account.authorize_with_seed(
                &stake_keyed_account,
                seed,
                &id(),
                &new_authority,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Set stake authority
        assert_eq!(
            stake_keyed_account.authorize_with_seed(
                &base_keyed_account,
                seed,
                &id(),
                &new_authority,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None,
            ),
            Ok(())
        );

        // Set withdraw authority
        assert_eq!(
            stake_keyed_account.authorize_with_seed(
                &base_keyed_account,
                seed,
                &id(),
                &new_authority,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None,
            ),
            Ok(())
        );

        // No longer withdraw authority
        assert_eq!(
            stake_keyed_account.authorize_with_seed(
                &stake_keyed_account,
                seed,
                &id(),
                &new_authority,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
    }

    #[test]
    fn test_authorize_override() {
        let withdrawer_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&withdrawer_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let stake_keyed_account = KeyedAccount::new(&withdrawer_pubkey, true, &stake_account);

        // Authorize a staker pubkey and move the withdrawer key into cold storage.
        let new_authority = solana_sdk::pubkey::new_rand();
        let signers = vec![withdrawer_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &new_authority,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );

        // Attack! The stake key (a hot key) is stolen and used to authorize a new staker.
        let mallory_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![new_authority].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &mallory_pubkey,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );

        // Verify the original staker no longer has access.
        let new_stake_pubkey = solana_sdk::pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &new_stake_pubkey,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Verify the withdrawer (pulled from cold storage) can save the day.
        let signers = vec![withdrawer_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &new_stake_pubkey,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );

        // Attack! Verify the staker cannot be used to authorize a withdraw.
        let signers = vec![new_stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &mallory_pubkey,
                StakeAuthorize::Withdrawer,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );
    }

    #[test]
    fn test_split_source_uninitialized() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let split_stake_account = AccountSharedData::new_ref_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        let split_stake_keyed_account =
            KeyedAccount::new(&split_stake_pubkey, false, &split_stake_account);

        // no signers should fail
        assert_eq!(
            stake_keyed_account.split(
                stake_lamports / 2,
                &split_stake_keyed_account,
                &HashSet::default() // no signers
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // this should work
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
            Ok(())
        );
        assert_eq!(
            stake_keyed_account.account.borrow().lamports(),
            split_stake_keyed_account.account.borrow().lamports()
        );
    }

    #[test]
    fn test_split_split_not_uninitialized() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(Meta::auto(&stake_pubkey), just_stake(stake_lamports)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let split_stake_account = AccountSharedData::new_ref_data_with_space(
            0,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let signers = vec![stake_pubkey].into_iter().collect();
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let split_stake_keyed_account =
            KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);
        assert_eq!(
            stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
            Err(InstructionError::InvalidAccountData)
        );
    }
    fn just_stake(stake: u64) -> Stake {
        Stake {
            delegation: Delegation {
                stake,
                ..Delegation::default()
            },
            ..Stake::default()
        }
    }

    #[test]
    fn test_split_more_than_staked() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(
                Meta::auto(&stake_pubkey),
                just_stake(stake_lamports / 2 - 1),
            ),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let split_stake_account = AccountSharedData::new_ref_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let signers = vec![stake_pubkey].into_iter().collect();
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let split_stake_keyed_account =
            KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);
        assert_eq!(
            stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
            Err(StakeError::InsufficientStake.into())
        );
    }

    #[test]
    fn test_split_with_rent() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 10_000_000;
        let rent_exempt_reserve = 2_282_880;
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(meta),
            StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve)),
        ] {
            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            // not enough to make a non-zero stake account
            assert_eq!(
                stake_keyed_account.split(
                    rent_exempt_reserve,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // doesn't leave enough for initial stake to be non-zero
            assert_eq!(
                stake_keyed_account.split(
                    stake_lamports - rent_exempt_reserve,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // split account already has way enough lamports
            split_stake_keyed_account
                .account
                .borrow_mut()
                .set_lamports(10_000_000);
            assert_eq!(
                stake_keyed_account.split(
                    stake_lamports - (rent_exempt_reserve + 1), // leave rent_exempt_reserve + 1 in original account
                    &split_stake_keyed_account,
                    &signers
                ),
                Ok(())
            );

            // verify no stake leakage in the case of a stake
            if let StakeState::Stake(meta, stake) = state {
                assert_eq!(
                    split_stake_keyed_account.state(),
                    Ok(StakeState::Stake(
                        *meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports - rent_exempt_reserve - 1,
                                ..stake.delegation
                            },
                            ..*stake
                        }
                    ))
                );
                assert_eq!(
                    stake_keyed_account.account.borrow().lamports(),
                    rent_exempt_reserve + 1
                );
                assert_eq!(
                    split_stake_keyed_account.account.borrow().lamports(),
                    10_000_000 + stake_lamports - rent_exempt_reserve - 1
                );
            }
        }
    }

    #[test]
    fn test_split() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(Meta::auto(&stake_pubkey)),
            StakeState::Stake(Meta::auto(&stake_pubkey), just_stake(stake_lamports)),
        ] {
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split more than available fails
            assert_eq!(
                stake_keyed_account.split(stake_lamports + 1, &split_stake_keyed_account, &signers),
                Err(InstructionError::InsufficientFunds)
            );

            // should work
            assert_eq!(
                stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
                Ok(())
            );
            // no lamport leakage
            assert_eq!(
                stake_keyed_account.account.borrow().lamports()
                    + split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports
            );

            match state {
                StakeState::Initialized(_) => {
                    assert_eq!(Ok(*state), split_stake_keyed_account.state());
                    assert_eq!(Ok(*state), stake_keyed_account.state());
                }
                StakeState::Stake(meta, stake) => {
                    assert_eq!(
                        Ok(StakeState::Stake(
                            *meta,
                            Stake {
                                delegation: Delegation {
                                    stake: stake_lamports / 2,
                                    ..stake.delegation
                                },
                                ..*stake
                            }
                        )),
                        split_stake_keyed_account.state()
                    );
                    assert_eq!(
                        Ok(StakeState::Stake(
                            *meta,
                            Stake {
                                delegation: Delegation {
                                    stake: stake_lamports / 2,
                                    ..stake.delegation
                                },
                                ..*stake
                            }
                        )),
                        stake_keyed_account.state()
                    );
                }
                _ => unreachable!(),
            }

            // reset
            stake_keyed_account
                .account
                .borrow_mut()
                .set_lamports(stake_lamports);
        }
    }

    #[test]
    fn test_split_fake_stake_dest() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let split_stake_account = AccountSharedData::new_ref_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &solana_sdk::pubkey::new_rand(),
        )
        .expect("stake_account");

        let split_stake_keyed_account =
            KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(Meta::auto(&stake_pubkey), just_stake(stake_lamports)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        assert_eq!(
            stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
            Err(InstructionError::IncorrectProgramId),
        );
    }

    #[test]
    fn test_split_to_account_with_rent_exempt_reserve() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve * 3; // Enough to allow half to be split and remain rent-exempt

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        let state = StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve));
        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. The empty case is not covered in
        // test_split, since that test uses a Meta with rent_exempt_reserve = 0
        let split_lamport_balances = vec![0, 1, rent_exempt_reserve, rent_exempt_reserve + 1];
        for initial_balance in split_lamport_balances {
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split more than available fails
            assert_eq!(
                stake_keyed_account.split(stake_lamports + 1, &split_stake_keyed_account, &signers),
                Err(InstructionError::InsufficientFunds)
            );

            // should work
            assert_eq!(
                stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
                Ok(())
            );
            // no lamport leakage
            assert_eq!(
                stake_keyed_account.account.borrow().lamports()
                    + split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports + initial_balance
            );

            if let StakeState::Stake(meta, stake) = state {
                let expected_stake =
                    stake_lamports / 2 - (rent_exempt_reserve.saturating_sub(initial_balance));
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2
                                    - (rent_exempt_reserve.saturating_sub(initial_balance)),
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    split_stake_keyed_account.state()
                );
                assert_eq!(
                    split_stake_keyed_account.account.borrow().lamports(),
                    expected_stake
                        + rent_exempt_reserve
                        + initial_balance.saturating_sub(rent_exempt_reserve)
                );
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2 - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    stake_keyed_account.state()
                );
            }
        }
    }

    #[test]
    fn test_split_to_smaller_account_with_rent_exempt_reserve() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve * 3; // Enough to allow half to be split and remain rent-exempt

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        let state = StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve));

        let expected_rent_exempt_reserve = calculate_split_rent_exempt_reserve(
            meta.rent_exempt_reserve,
            std::mem::size_of::<StakeState>() as u64 + 100,
            std::mem::size_of::<StakeState>() as u64,
        );

        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. The empty case is not covered in
        // test_split, since that test uses a Meta with rent_exempt_reserve = 0
        let split_lamport_balances = vec![
            0,
            1,
            expected_rent_exempt_reserve,
            expected_rent_exempt_reserve + 1,
        ];
        for initial_balance in split_lamport_balances {
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>() + 100,
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split more than available fails
            assert_eq!(
                stake_keyed_account.split(stake_lamports + 1, &split_stake_keyed_account, &signers),
                Err(InstructionError::InsufficientFunds)
            );

            // should work
            assert_eq!(
                stake_keyed_account.split(stake_lamports / 2, &split_stake_keyed_account, &signers),
                Ok(())
            );
            // no lamport leakage
            assert_eq!(
                stake_keyed_account.account.borrow().lamports()
                    + split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports + initial_balance
            );

            if let StakeState::Stake(meta, stake) = state {
                let expected_split_meta = Meta {
                    authorized: Authorized::auto(&stake_pubkey),
                    rent_exempt_reserve: expected_rent_exempt_reserve,
                    ..Meta::default()
                };
                let expected_stake = stake_lamports / 2
                    - (expected_rent_exempt_reserve.saturating_sub(initial_balance));

                assert_eq!(
                    Ok(StakeState::Stake(
                        expected_split_meta,
                        Stake {
                            delegation: Delegation {
                                stake: expected_stake,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    split_stake_keyed_account.state()
                );
                assert_eq!(
                    split_stake_keyed_account.account.borrow().lamports(),
                    expected_stake
                        + expected_rent_exempt_reserve
                        + initial_balance.saturating_sub(expected_rent_exempt_reserve)
                );
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2 - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    stake_keyed_account.state()
                );
            }
        }
    }

    #[test]
    fn test_split_to_larger_account() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        let expected_rent_exempt_reserve = calculate_split_rent_exempt_reserve(
            meta.rent_exempt_reserve,
            std::mem::size_of::<StakeState>() as u64,
            std::mem::size_of::<StakeState>() as u64 + 100,
        );
        let stake_lamports = expected_rent_exempt_reserve + 1;
        let split_amount = stake_lamports - (rent_exempt_reserve + 1); // Enough so that split stake is > 0

        let state = StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve));

        let split_lamport_balances = vec![
            0,
            1,
            expected_rent_exempt_reserve,
            expected_rent_exempt_reserve + 1,
        ];
        for initial_balance in split_lamport_balances {
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>() + 100,
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // should always return error when splitting to larger account
            let split_result =
                stake_keyed_account.split(split_amount, &split_stake_keyed_account, &signers);
            assert_eq!(split_result, Err(InstructionError::InvalidAccountData));

            // Splitting 100% of source should not make a difference
            let split_result =
                stake_keyed_account.split(stake_lamports, &split_stake_keyed_account, &signers);
            assert_eq!(split_result, Err(InstructionError::InvalidAccountData));
        }
    }

    #[test]
    fn test_split_100_percent_of_source() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve * 3; // Arbitrary amount over rent_exempt_reserve

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(meta),
            StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve)),
        ] {
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split 100% over to dest
            assert_eq!(
                stake_keyed_account.split(stake_lamports, &split_stake_keyed_account, &signers),
                Ok(())
            );

            // no lamport leakage
            assert_eq!(
                stake_keyed_account.account.borrow().lamports()
                    + split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports
            );

            match state {
                StakeState::Initialized(_) => {
                    assert_eq!(Ok(*state), split_stake_keyed_account.state());
                    assert_eq!(Ok(StakeState::Uninitialized), stake_keyed_account.state());
                }
                StakeState::Stake(meta, stake) => {
                    assert_eq!(
                        Ok(StakeState::Stake(
                            *meta,
                            Stake {
                                delegation: Delegation {
                                    stake: stake_lamports - rent_exempt_reserve,
                                    ..stake.delegation
                                },
                                ..*stake
                            }
                        )),
                        split_stake_keyed_account.state()
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), stake_keyed_account.state());
                }
                _ => unreachable!(),
            }

            // reset
            stake_keyed_account
                .account
                .borrow_mut()
                .set_lamports(stake_lamports);
        }
    }

    #[test]
    fn test_split_100_percent_of_source_to_account_with_lamports() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve * 3; // Arbitrary amount over rent_exempt_reserve

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        let state = StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve));
        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. Technically, the empty case is
        // covered in test_split_100_percent_of_source, but included here as well for readability
        let split_lamport_balances = vec![0, 1, rent_exempt_reserve, rent_exempt_reserve + 1];
        for initial_balance in split_lamport_balances {
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split 100% over to dest
            assert_eq!(
                stake_keyed_account.split(stake_lamports, &split_stake_keyed_account, &signers),
                Ok(())
            );

            // no lamport leakage
            assert_eq!(
                stake_keyed_account.account.borrow().lamports()
                    + split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports + initial_balance
            );

            if let StakeState::Stake(meta, stake) = state {
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    split_stake_keyed_account.state()
                );
                assert_eq!(Ok(StakeState::Uninitialized), stake_keyed_account.state());
            }
        }
    }

    #[test]
    fn test_split_rent_exemptness() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve + 1;

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        for state in &[
            StakeState::Initialized(meta),
            StakeState::Stake(meta, just_stake(stake_lamports - rent_exempt_reserve)),
        ] {
            // Test that splitting to a larger account fails
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>() + 10000,
                &id(),
            )
            .expect("stake_account");
            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            assert_eq!(
                stake_keyed_account.split(stake_lamports, &split_stake_keyed_account, &signers),
                Err(InstructionError::InvalidAccountData)
            );

            // Test that splitting from a larger account to a smaller one works.
            // Split amount should not matter, assuming other fund criteria are met
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = AccountSharedData::new_ref_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>() + 100,
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            assert_eq!(
                stake_keyed_account.split(stake_lamports, &split_stake_keyed_account, &signers),
                Ok(())
            );

            assert_eq!(
                split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports
            );

            let expected_rent_exempt_reserve = calculate_split_rent_exempt_reserve(
                meta.rent_exempt_reserve,
                std::mem::size_of::<StakeState>() as u64 + 100,
                std::mem::size_of::<StakeState>() as u64,
            );
            let expected_split_meta = Meta {
                authorized: Authorized::auto(&stake_pubkey),
                rent_exempt_reserve: expected_rent_exempt_reserve,
                ..Meta::default()
            };

            match state {
                StakeState::Initialized(_) => {
                    assert_eq!(
                        Ok(StakeState::Initialized(expected_split_meta)),
                        split_stake_keyed_account.state()
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), stake_keyed_account.state());
                }
                StakeState::Stake(_meta, stake) => {
                    // Expected stake should reflect original stake amount so that extra lamports
                    // from the rent_exempt_reserve inequality do not magically activate
                    let expected_stake = stake_lamports - rent_exempt_reserve;

                    assert_eq!(
                        Ok(StakeState::Stake(
                            expected_split_meta,
                            Stake {
                                delegation: Delegation {
                                    stake: expected_stake,
                                    ..stake.delegation
                                },
                                ..*stake
                            }
                        )),
                        split_stake_keyed_account.state()
                    );
                    assert_eq!(
                        split_stake_keyed_account.account.borrow().lamports(),
                        expected_stake
                            + expected_rent_exempt_reserve
                            + (rent_exempt_reserve - expected_rent_exempt_reserve)
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), stake_keyed_account.state());
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn test_merge() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let signers = vec![authorized_pubkey].into_iter().collect();
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        for state in &[
            StakeState::Initialized(Meta::auto(&authorized_pubkey)),
            StakeState::Stake(Meta::auto(&authorized_pubkey), just_stake(stake_lamports)),
        ] {
            for source_state in &[
                StakeState::Initialized(Meta::auto(&authorized_pubkey)),
                StakeState::Stake(Meta::auto(&authorized_pubkey), just_stake(stake_lamports)),
            ] {
                let stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_lamports,
                    state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .expect("stake_account");
                let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

                let source_stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_lamports,
                    source_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .expect("source_stake_account");
                let source_stake_keyed_account =
                    KeyedAccount::new(&source_stake_pubkey, true, &source_stake_account);

                // Authorized staker signature required...
                assert_eq!(
                    stake_keyed_account.merge(
                        &invoke_context,
                        &source_stake_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &HashSet::new(),
                        false,
                    ),
                    Err(InstructionError::MissingRequiredSignature)
                );

                assert_eq!(
                    stake_keyed_account.merge(
                        &invoke_context,
                        &source_stake_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &signers,
                        false,
                    ),
                    Ok(())
                );

                // check lamports
                assert_eq!(
                    stake_keyed_account.account.borrow().lamports(),
                    stake_lamports * 2
                );
                assert_eq!(source_stake_keyed_account.account.borrow().lamports(), 0);

                // check state
                match state {
                    StakeState::Initialized(meta) => {
                        assert_eq!(
                            stake_keyed_account.state(),
                            Ok(StakeState::Initialized(*meta)),
                        );
                    }
                    StakeState::Stake(meta, stake) => {
                        let expected_stake = stake.delegation.stake
                            + source_state
                                .stake()
                                .map(|stake| stake.delegation.stake)
                                .unwrap_or_else(|| {
                                    stake_lamports
                                        - source_state.meta().unwrap().rent_exempt_reserve
                                });
                        assert_eq!(
                            stake_keyed_account.state(),
                            Ok(StakeState::Stake(
                                *meta,
                                Stake {
                                    delegation: Delegation {
                                        stake: expected_stake,
                                        ..stake.delegation
                                    },
                                    ..*stake
                                }
                            )),
                        );
                    }
                    _ => unreachable!(),
                }
                assert_eq!(
                    source_stake_keyed_account.state(),
                    Ok(StakeState::Uninitialized)
                );
            }
        }
    }

    #[test]
    fn test_merge_self_fails() {
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);
        let stake_address = Pubkey::new_unique();
        let authority_pubkey = Pubkey::new_unique();
        let signers = HashSet::from_iter(vec![authority_pubkey]);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_amount = 4242424242;
        let stake_lamports = rent_exempt_reserve + stake_amount;

        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&authority_pubkey)
        };
        let stake = Stake {
            delegation: Delegation {
                stake: stake_amount,
                activation_epoch: 0,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(meta, stake),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_address, true, &stake_account);

        assert_eq!(
            stake_keyed_account.merge(
                &invoke_context,
                &stake_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &signers,
                false,
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_merge_incorrect_authorized_staker() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let wrong_authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let signers = vec![authorized_pubkey].into_iter().collect();
        let wrong_signers = vec![wrong_authorized_pubkey].into_iter().collect();
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        for state in &[
            StakeState::Initialized(Meta::auto(&authorized_pubkey)),
            StakeState::Stake(Meta::auto(&authorized_pubkey), just_stake(stake_lamports)),
        ] {
            for source_state in &[
                StakeState::Initialized(Meta::auto(&wrong_authorized_pubkey)),
                StakeState::Stake(
                    Meta::auto(&wrong_authorized_pubkey),
                    just_stake(stake_lamports),
                ),
            ] {
                let stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_lamports,
                    state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .expect("stake_account");
                let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

                let source_stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_lamports,
                    source_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .expect("source_stake_account");
                let source_stake_keyed_account =
                    KeyedAccount::new(&source_stake_pubkey, true, &source_stake_account);

                assert_eq!(
                    stake_keyed_account.merge(
                        &invoke_context,
                        &source_stake_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &wrong_signers,
                        false,
                    ),
                    Err(InstructionError::MissingRequiredSignature)
                );

                assert_eq!(
                    stake_keyed_account.merge(
                        &invoke_context,
                        &source_stake_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &signers,
                        false,
                    ),
                    Err(StakeError::MergeMismatch.into())
                );
            }
        }
    }

    #[test]
    fn test_merge_invalid_account_data() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let signers = vec![authorized_pubkey].into_iter().collect();
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        for state in &[
            StakeState::Uninitialized,
            StakeState::RewardsPool,
            StakeState::Initialized(Meta::auto(&authorized_pubkey)),
            StakeState::Stake(Meta::auto(&authorized_pubkey), just_stake(stake_lamports)),
        ] {
            for source_state in &[StakeState::Uninitialized, StakeState::RewardsPool] {
                let stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_lamports,
                    state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .expect("stake_account");
                let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

                let source_stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_lamports,
                    source_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .expect("source_stake_account");
                let source_stake_keyed_account =
                    KeyedAccount::new(&source_stake_pubkey, true, &source_stake_account);

                assert_eq!(
                    stake_keyed_account.merge(
                        &invoke_context,
                        &source_stake_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &signers,
                        false,
                    ),
                    Err(InstructionError::InvalidAccountData)
                );
            }
        }
    }

    #[test]
    fn test_merge_fake_stake_source() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let signers = vec![authorized_pubkey].into_iter().collect();

        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(Meta::auto(&authorized_pubkey), just_stake(stake_lamports)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        let source_stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(Meta::auto(&authorized_pubkey), just_stake(stake_lamports)),
            std::mem::size_of::<StakeState>(),
            &solana_sdk::pubkey::new_rand(),
        )
        .expect("source_stake_account");
        let source_stake_keyed_account =
            KeyedAccount::new(&source_stake_pubkey, true, &source_stake_account);
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        assert_eq!(
            stake_keyed_account.merge(
                &invoke_context,
                &source_stake_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &signers,
                false,
            ),
            Err(InstructionError::IncorrectProgramId)
        );
    }

    #[test]
    fn test_merge_active_stake() {
        let base_lamports = 4242424242;
        let stake_address = Pubkey::new_unique();
        let source_address = Pubkey::new_unique();
        let authority_pubkey = Pubkey::new_unique();
        let signers = HashSet::from_iter(vec![authority_pubkey]);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_amount = base_lamports;
        let stake_lamports = rent_exempt_reserve + stake_amount;
        let source_amount = base_lamports;
        let source_lamports = rent_exempt_reserve + source_amount;

        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&authority_pubkey)
        };
        let mut stake = Stake {
            delegation: Delegation {
                stake: stake_amount,
                activation_epoch: 0,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(meta, stake),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_address, true, &stake_account);

        let source_activation_epoch = 2;
        let mut source_stake = Stake {
            delegation: Delegation {
                stake: source_amount,
                activation_epoch: source_activation_epoch,
                ..stake.delegation
            },
            ..stake
        };
        let source_account = AccountSharedData::new_ref_data_with_space(
            source_lamports,
            &StakeState::Stake(meta, source_stake),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("source_account");
        let source_keyed_account = KeyedAccount::new(&source_address, true, &source_account);

        let mut clock = Clock::default();
        let mut stake_history = StakeHistory::default();
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        clock.epoch = 0;
        let mut effective = base_lamports;
        let mut activating = stake_amount;
        let mut deactivating = 0;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        fn try_merge(
            invoke_context: &dyn InvokeContext,
            stake_account: &KeyedAccount,
            source_account: &KeyedAccount,
            clock: &Clock,
            stake_history: &StakeHistory,
            signers: &HashSet<Pubkey>,
        ) -> Result<(), InstructionError> {
            let test_stake_account = stake_account.account.clone();
            let test_stake_keyed =
                KeyedAccount::new(stake_account.unsigned_key(), true, &test_stake_account);
            let test_source_account = source_account.account.clone();
            let test_source_keyed =
                KeyedAccount::new(source_account.unsigned_key(), true, &test_source_account);

            let result = test_stake_keyed.merge(
                invoke_context,
                &test_source_keyed,
                clock,
                stake_history,
                signers,
                false,
            );
            if result.is_ok() {
                assert_eq!(test_source_keyed.state(), Ok(StakeState::Uninitialized),);
            }
            result
        }

        // stake activation epoch, source initialized succeeds
        assert!(try_merge(
            &invoke_context,
            &stake_keyed_account,
            &source_keyed_account,
            &clock,
            &stake_history,
            &signers
        )
        .is_ok(),);
        assert!(try_merge(
            &invoke_context,
            &source_keyed_account,
            &stake_keyed_account,
            &clock,
            &stake_history,
            &signers
        )
        .is_ok(),);

        // both activating fails
        loop {
            clock.epoch += 1;
            if clock.epoch == source_activation_epoch {
                activating += source_amount;
            }
            let delta =
                activating.min((effective as f64 * stake.delegation.warmup_cooldown_rate) as u64);
            effective += delta;
            activating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if stake_amount == stake.stake(clock.epoch, Some(&stake_history))
                && source_amount == source_stake.stake(clock.epoch, Some(&stake_history))
            {
                break;
            }
            assert_eq!(
                try_merge(
                    &invoke_context,
                    &stake_keyed_account,
                    &source_keyed_account,
                    &clock,
                    &stake_history,
                    &signers
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
            assert_eq!(
                try_merge(
                    &invoke_context,
                    &source_keyed_account,
                    &stake_keyed_account,
                    &clock,
                    &stake_history,
                    &signers
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
        }
        // Both fully activated works
        assert!(try_merge(
            &invoke_context,
            &stake_keyed_account,
            &source_keyed_account,
            &clock,
            &stake_history,
            &signers
        )
        .is_ok(),);

        // deactivate setup for deactivation
        let source_deactivation_epoch = clock.epoch + 1;
        let stake_deactivation_epoch = clock.epoch + 2;

        // active/deactivating and deactivating/inactive mismatches fail
        loop {
            clock.epoch += 1;
            let delta =
                deactivating.min((effective as f64 * stake.delegation.warmup_cooldown_rate) as u64);
            effective -= delta;
            deactivating -= delta;
            if clock.epoch == stake_deactivation_epoch {
                deactivating += stake_amount;
                stake = Stake {
                    delegation: Delegation {
                        deactivation_epoch: stake_deactivation_epoch,
                        ..stake.delegation
                    },
                    ..stake
                };
                stake_keyed_account
                    .set_state(&StakeState::Stake(meta, stake))
                    .unwrap();
            }
            if clock.epoch == source_deactivation_epoch {
                deactivating += source_amount;
                source_stake = Stake {
                    delegation: Delegation {
                        deactivation_epoch: source_deactivation_epoch,
                        ..source_stake.delegation
                    },
                    ..source_stake
                };
                source_keyed_account
                    .set_state(&StakeState::Stake(meta, source_stake))
                    .unwrap();
            }
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if 0 == stake.stake(clock.epoch, Some(&stake_history))
                && 0 == source_stake.stake(clock.epoch, Some(&stake_history))
            {
                break;
            }
            assert_eq!(
                try_merge(
                    &invoke_context,
                    &stake_keyed_account,
                    &source_keyed_account,
                    &clock,
                    &stake_history,
                    &signers
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
            assert_eq!(
                try_merge(
                    &invoke_context,
                    &source_keyed_account,
                    &stake_keyed_account,
                    &clock,
                    &stake_history,
                    &signers
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
        }

        // Both fully deactivated works
        assert!(try_merge(
            &invoke_context,
            &stake_keyed_account,
            &source_keyed_account,
            &clock,
            &stake_history,
            &signers
        )
        .is_ok(),);
    }

    #[test]
    fn test_lockup_is_expired() {
        let custodian = solana_sdk::pubkey::new_rand();
        let lockup = Lockup {
            epoch: 1,
            unix_timestamp: 1,
            custodian,
        };
        // neither time
        assert!(lockup.is_in_force(
            &Clock {
                epoch: 0,
                unix_timestamp: 0,
                ..Clock::default()
            },
            None
        ));
        // not timestamp
        assert!(lockup.is_in_force(
            &Clock {
                epoch: 2,
                unix_timestamp: 0,
                ..Clock::default()
            },
            None
        ));
        // not epoch
        assert!(lockup.is_in_force(
            &Clock {
                epoch: 0,
                unix_timestamp: 2,
                ..Clock::default()
            },
            None
        ));
        // both, no custodian
        assert!(!lockup.is_in_force(
            &Clock {
                epoch: 1,
                unix_timestamp: 1,
                ..Clock::default()
            },
            None
        ));
        // neither, but custodian
        assert!(!lockup.is_in_force(
            &Clock {
                epoch: 0,
                unix_timestamp: 0,
                ..Clock::default()
            },
            Some(&custodian),
        ));
    }

    #[test]
    #[ignore]
    #[should_panic]
    fn test_dbg_stake_minimum_balance() {
        let minimum_balance = Rent::default().minimum_balance(std::mem::size_of::<StakeState>());
        panic!(
            "stake minimum_balance: {} lamports, {} SOL",
            minimum_balance,
            minimum_balance as f64 / solana_sdk::native_token::LAMPORTS_PER_SOL as f64
        );
    }

    #[test]
    fn test_authorize_delegated_stake() {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let clock = Clock::default();

        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let signers = vec![stake_pubkey].into_iter().collect();
        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
                true,
            )
            .unwrap();

        // deactivate, so we can re-delegate
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        let new_staker_pubkey = solana_sdk::pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &signers,
                &new_staker_pubkey,
                StakeAuthorize::Staker,
                false,
                &Clock::default(),
                None
            ),
            Ok(())
        );
        let authorized = authorized_from(&stake_keyed_account.try_account_ref().unwrap()).unwrap();
        assert_eq!(authorized.staker, new_staker_pubkey);

        let other_pubkey = solana_sdk::pubkey::new_rand();
        let other_signers = vec![other_pubkey].into_iter().collect();

        // Use unsigned stake_keyed_account to test other signers
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        let new_voter_pubkey = solana_sdk::pubkey::new_rand();
        let vote_state = VoteState::default();
        let new_vote_account = RefCell::new(vote_state::create_account(
            &new_voter_pubkey,
            &solana_sdk::pubkey::new_rand(),
            0,
            100,
        ));
        let new_vote_keyed_account = KeyedAccount::new(&new_voter_pubkey, false, &new_vote_account);
        new_vote_keyed_account.set_state(&vote_state).unwrap();

        // Random other account should fail
        assert_eq!(
            stake_keyed_account.delegate(
                &new_vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &other_signers,
                true,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        let new_signers = vec![new_staker_pubkey].into_iter().collect();
        // Authorized staker should succeed
        assert_eq!(
            stake_keyed_account.delegate(
                &new_vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &new_signers,
                true,
            ),
            Ok(())
        );
        let stake = stake_from(&stake_keyed_account.try_account_ref().unwrap()).unwrap();
        assert_eq!(stake.delegation.voter_pubkey, new_voter_pubkey);

        // Test another staking action
        assert_eq!(stake_keyed_account.deactivate(&clock, &new_signers), Ok(()));
    }

    #[test]
    fn test_redelegate_consider_balance_changes() {
        let initial_lamports = 4242424242;
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let withdrawer_pubkey = Pubkey::new_unique();
        let stake_lamports = rent_exempt_reserve + initial_lamports;

        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&withdrawer_pubkey)
        };
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(meta),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&withdrawer_pubkey, true, &stake_account);

        let vote_pubkey = Pubkey::new_unique();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_unique(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);

        let signers = HashSet::from_iter(vec![withdrawer_pubkey]);
        let config = Config::default();
        let stake_history = StakeHistory::default();
        let mut clock = Clock::default();
        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &stake_history,
                &config,
                &signers,
                true,
            )
            .unwrap();

        clock.epoch += 1;
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        clock.epoch += 1;
        // Once deactivated, we withdraw stake to new keyed account
        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        let withdraw_lamports = initial_lamports / 2;
        stake_keyed_account
            .withdraw(
                withdraw_lamports,
                &to_keyed_account,
                &clock,
                &stake_history,
                &stake_keyed_account,
                None,
                true,
            )
            .unwrap();
        let expected_balance = rent_exempt_reserve + initial_lamports - withdraw_lamports;
        assert_eq!(stake_keyed_account.lamports().unwrap(), expected_balance);

        clock.epoch += 1;
        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &stake_history,
                &config,
                &signers,
                true,
            )
            .unwrap();
        let stake = stake_from(&stake_account.borrow()).unwrap();
        assert_eq!(
            stake.delegation.stake,
            stake_keyed_account.lamports().unwrap() - rent_exempt_reserve,
        );

        clock.epoch += 1;
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        // Out of band deposit
        stake_keyed_account
            .try_account_ref_mut()
            .unwrap()
            .checked_add_lamports(withdraw_lamports)
            .unwrap();

        clock.epoch += 1;
        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &stake_history,
                &config,
                &signers,
                true,
            )
            .unwrap();
        let stake = stake_from(&stake_account.borrow()).unwrap();
        assert_eq!(
            stake.delegation.stake,
            stake_keyed_account.lamports().unwrap() - rent_exempt_reserve,
        );
    }

    #[test]
    fn test_meta_rewrite_rent_exempt_reserve() {
        let right_data_len = std::mem::size_of::<StakeState>() as u64;
        let rent = Rent::default();
        let expected_rent_exempt_reserve = rent.minimum_balance(right_data_len as usize);

        let test_cases = [
            (
                right_data_len + 100,
                Some((
                    rent.minimum_balance(right_data_len as usize + 100),
                    expected_rent_exempt_reserve,
                )),
            ), // large data_len, too small rent exempt
            (right_data_len, None), // correct
            (
                right_data_len - 100,
                Some((
                    rent.minimum_balance(right_data_len as usize - 100),
                    expected_rent_exempt_reserve,
                )),
            ), // small data_len, too large rent exempt
        ];
        for (data_len, expected_rewrite) in &test_cases {
            let rent_exempt_reserve = rent.minimum_balance(*data_len as usize);
            let mut meta = Meta {
                rent_exempt_reserve,
                ..Meta::default()
            };
            let actual_rewrite = meta.rewrite_rent_exempt_reserve(&rent, right_data_len as usize);
            assert_eq!(actual_rewrite, *expected_rewrite);
            assert_eq!(meta.rent_exempt_reserve, expected_rent_exempt_reserve);
        }
    }

    #[test]
    fn test_calculate_lamports_per_byte_year() {
        let rent = Rent::default();
        let data_len = 200u64;
        let rent_exempt_reserve = rent.minimum_balance(data_len as usize);
        assert_eq!(
            calculate_split_rent_exempt_reserve(rent_exempt_reserve, data_len, data_len),
            rent_exempt_reserve
        );

        let larger_data = 4008u64;
        let larger_rent_exempt_reserve = rent.minimum_balance(larger_data as usize);
        assert_eq!(
            calculate_split_rent_exempt_reserve(rent_exempt_reserve, data_len, larger_data),
            larger_rent_exempt_reserve
        );
        assert_eq!(
            calculate_split_rent_exempt_reserve(larger_rent_exempt_reserve, larger_data, data_len),
            rent_exempt_reserve
        );

        let even_larger_data = solana_sdk::system_instruction::MAX_PERMITTED_DATA_LENGTH;
        let even_larger_rent_exempt_reserve = rent.minimum_balance(even_larger_data as usize);
        assert_eq!(
            calculate_split_rent_exempt_reserve(rent_exempt_reserve, data_len, even_larger_data),
            even_larger_rent_exempt_reserve
        );
        assert_eq!(
            calculate_split_rent_exempt_reserve(
                even_larger_rent_exempt_reserve,
                even_larger_data,
                data_len
            ),
            rent_exempt_reserve
        );
    }

    #[test]
    fn test_things_can_merge() {
        let good_stake = Stake {
            credits_observed: 4242,
            delegation: Delegation {
                voter_pubkey: Pubkey::new_unique(),
                stake: 424242424242,
                activation_epoch: 42,
                ..Delegation::default()
            },
        };
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        let identical = good_stake;
        assert!(
            MergeKind::active_stakes_can_merge(&invoke_context, &good_stake, &identical).is_ok()
        );

        let bad_credits_observed = Stake {
            credits_observed: good_stake.credits_observed + 1,
            ..good_stake
        };
        assert!(MergeKind::active_stakes_can_merge(
            &invoke_context,
            &good_stake,
            &bad_credits_observed
        )
        .is_err());

        let good_delegation = good_stake.delegation;
        let different_stake_ok = Delegation {
            stake: good_delegation.stake + 1,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &different_stake_ok
        )
        .is_ok());

        let different_activation_epoch_ok = Delegation {
            activation_epoch: good_delegation.activation_epoch + 1,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &different_activation_epoch_ok
        )
        .is_ok());

        let bad_voter = Delegation {
            voter_pubkey: Pubkey::new_unique(),
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &bad_voter
        )
        .is_err());

        let bad_warmup_cooldown_rate = Delegation {
            warmup_cooldown_rate: good_delegation.warmup_cooldown_rate + f64::EPSILON,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &bad_warmup_cooldown_rate
        )
        .is_err());
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &bad_warmup_cooldown_rate,
            &good_delegation
        )
        .is_err());

        let bad_deactivation_epoch = Delegation {
            deactivation_epoch: 43,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &bad_deactivation_epoch
        )
        .is_err());
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &bad_deactivation_epoch,
            &good_delegation
        )
        .is_err());
    }

    #[test]
    fn test_metas_can_merge_pre_v4() {
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);
        // Identical Metas can merge
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &Meta::default(),
            None,
        )
        .is_ok());

        let mismatched_rent_exempt_reserve_ok = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        assert_ne!(
            mismatched_rent_exempt_reserve_ok.rent_exempt_reserve,
            Meta::default().rent_exempt_reserve
        );
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_rent_exempt_reserve_ok,
            None,
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_rent_exempt_reserve_ok,
            &Meta::default(),
            None,
        )
        .is_ok());

        let mismatched_authorized_fails = Meta {
            authorized: Authorized {
                staker: Pubkey::new_unique(),
                withdrawer: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        assert_ne!(
            mismatched_authorized_fails.authorized,
            Meta::default().authorized
        );
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_authorized_fails,
            None,
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_authorized_fails,
            &Meta::default(),
            None,
        )
        .is_err());

        let mismatched_lockup_fails = Meta {
            lockup: Lockup {
                unix_timestamp: 424242424,
                epoch: 42,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        assert_ne!(mismatched_lockup_fails.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_lockup_fails,
            None,
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_lockup_fails,
            &Meta::default(),
            None,
        )
        .is_err());
    }

    #[test]
    fn test_metas_can_merge_v4() {
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);
        // Identical Metas can merge
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &Meta::default(),
            Some(&Clock::default())
        )
        .is_ok());

        let mismatched_rent_exempt_reserve_ok = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        assert_ne!(
            mismatched_rent_exempt_reserve_ok.rent_exempt_reserve,
            Meta::default().rent_exempt_reserve,
        );
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_rent_exempt_reserve_ok,
            Some(&Clock::default())
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_rent_exempt_reserve_ok,
            &Meta::default(),
            Some(&Clock::default())
        )
        .is_ok());

        let mismatched_authorized_fails = Meta {
            authorized: Authorized {
                staker: Pubkey::new_unique(),
                withdrawer: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        assert_ne!(
            mismatched_authorized_fails.authorized,
            Meta::default().authorized,
        );
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_authorized_fails,
            Some(&Clock::default())
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_authorized_fails,
            &Meta::default(),
            Some(&Clock::default())
        )
        .is_err());

        let lockup1_timestamp = 42;
        let lockup2_timestamp = 4242;
        let lockup1_epoch = 4;
        let lockup2_epoch = 42;
        let metas_with_lockup1 = Meta {
            lockup: Lockup {
                unix_timestamp: lockup1_timestamp,
                epoch: lockup1_epoch,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        let metas_with_lockup2 = Meta {
            lockup: Lockup {
                unix_timestamp: lockup2_timestamp,
                epoch: lockup2_epoch,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };

        // Mismatched lockups fail when both in force
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup1,
            &metas_with_lockup2,
            Some(&Clock::default())
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            Some(&Clock::default())
        )
        .is_err());

        let clock = Clock {
            epoch: lockup1_epoch + 1,
            unix_timestamp: lockup1_timestamp + 1,
            ..Clock::default()
        };

        // Mismatched lockups fail when either in force
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup1,
            &metas_with_lockup2,
            Some(&clock)
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            Some(&clock)
        )
        .is_err());

        let clock = Clock {
            epoch: lockup2_epoch + 1,
            unix_timestamp: lockup2_timestamp + 1,
            ..Clock::default()
        };

        // Mismatched lockups succeed when both expired
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup1,
            &metas_with_lockup2,
            Some(&clock)
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            Some(&clock)
        )
        .is_ok());
    }

    #[test]
    fn test_merge_kind_get_if_mergeable() {
        let authority_pubkey = Pubkey::new_unique();
        let initial_lamports = 4242424242;
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve + initial_lamports;

        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&authority_pubkey)
        };
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&authority_pubkey, true, &stake_account);
        let mut clock = Clock::default();
        let mut stake_history = StakeHistory::default();
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        // Uninitialized state fails
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_keyed_account,
                &clock,
                &stake_history
            )
            .unwrap_err(),
            InstructionError::InvalidAccountData
        );

        // RewardsPool state fails
        stake_keyed_account
            .set_state(&StakeState::RewardsPool)
            .unwrap();
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_keyed_account,
                &clock,
                &stake_history
            )
            .unwrap_err(),
            InstructionError::InvalidAccountData
        );

        // Initialized state succeeds
        stake_keyed_account
            .set_state(&StakeState::Initialized(meta))
            .unwrap();
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_keyed_account,
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::Inactive(meta, stake_lamports)
        );

        clock.epoch = 0;
        let mut effective = 2 * initial_lamports;
        let mut activating = 0;
        let mut deactivating = 0;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        clock.epoch += 1;
        activating = initial_lamports;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        let stake = Stake {
            delegation: Delegation {
                stake: initial_lamports,
                activation_epoch: 1,
                deactivation_epoch: 5,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        stake_keyed_account
            .set_state(&StakeState::Stake(meta, stake))
            .unwrap();
        // activation_epoch succeeds
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_keyed_account,
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::ActivationEpoch(meta, stake),
        );

        // all paritially activated, transient epochs fail
        loop {
            clock.epoch += 1;
            let delta =
                activating.min((effective as f64 * stake.delegation.warmup_cooldown_rate) as u64);
            effective += delta;
            activating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if activating == 0 {
                break;
            }
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &invoke_context,
                    &stake_keyed_account,
                    &clock,
                    &stake_history
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
        }

        // all epochs for which we're fully active succeed
        while clock.epoch < stake.delegation.deactivation_epoch - 1 {
            clock.epoch += 1;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &invoke_context,
                    &stake_keyed_account,
                    &clock,
                    &stake_history
                )
                .unwrap(),
                MergeKind::FullyActive(meta, stake),
            );
        }

        clock.epoch += 1;
        deactivating = stake.delegation.stake;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );
        // deactivation epoch fails, fully transient/deactivating
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_keyed_account,
                &clock,
                &stake_history
            )
            .unwrap_err(),
            InstructionError::from(StakeError::MergeTransientStake),
        );

        // all transient, deactivating epochs fail
        loop {
            clock.epoch += 1;
            let delta =
                deactivating.min((effective as f64 * stake.delegation.warmup_cooldown_rate) as u64);
            effective -= delta;
            deactivating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if deactivating == 0 {
                break;
            }
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &invoke_context,
                    &stake_keyed_account,
                    &clock,
                    &stake_history
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
        }

        // first fully-deactivated epoch succeeds
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_keyed_account,
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::Inactive(meta, stake_lamports),
        );
    }

    #[test]
    fn test_merge_kind_merge() {
        let lamports = 424242;
        let meta = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        let stake = Stake {
            delegation: Delegation {
                stake: 4242,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        let inactive = MergeKind::Inactive(Meta::default(), lamports);
        let activation_epoch = MergeKind::ActivationEpoch(meta, stake);
        let fully_active = MergeKind::FullyActive(meta, stake);
        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        assert_eq!(
            inactive
                .clone()
                .merge(&invoke_context, inactive.clone(), None)
                .unwrap(),
            None
        );
        assert_eq!(
            inactive
                .clone()
                .merge(&invoke_context, activation_epoch.clone(), None)
                .unwrap(),
            None
        );
        assert!(inactive
            .clone()
            .merge(&invoke_context, fully_active.clone(), None)
            .is_err());
        assert!(activation_epoch
            .clone()
            .merge(&invoke_context, fully_active.clone(), None)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(&invoke_context, inactive.clone(), None)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(&invoke_context, activation_epoch.clone(), None)
            .is_err());

        let new_state = activation_epoch
            .clone()
            .merge(&invoke_context, inactive, None)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, stake.delegation.stake + lamports);

        let new_state = activation_epoch
            .clone()
            .merge(&invoke_context, activation_epoch, None)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(
            delegation.stake,
            2 * stake.delegation.stake + meta.rent_exempt_reserve
        );

        let new_state = fully_active
            .clone()
            .merge(&invoke_context, fully_active, None)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, 2 * stake.delegation.stake);
    }

    #[test]
    fn test_active_stake_merge() {
        let delegation_a = 4_242_424_242u64;
        let delegation_b = 6_200_000_000u64;
        let credits_a = 124_521_000u64;
        let rent_exempt_reserve = 227_000_000u64;
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::default()
        };
        let stake_a = Stake {
            delegation: Delegation {
                stake: delegation_a,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation_b,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };

        let invoke_context = MockInvokeContext::new(&Pubkey::default(), vec![]);

        // activating stake merge, match credits observed
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a);
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b);
        let new_stake = activation_epoch_a
            .merge(&invoke_context, activation_epoch_b, None)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(new_stake.credits_observed, credits_a);
        assert_eq!(
            new_stake.delegation.stake,
            delegation_a + delegation_b + rent_exempt_reserve
        );

        // active stake merge, match credits observed
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(&invoke_context, fully_active_b, None)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(new_stake.credits_observed, credits_a);
        assert_eq!(new_stake.delegation.stake, delegation_a + delegation_b);

        // activating stake merge, unmatched credits observed
        let credits_b = 125_124_521u64;
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation_b,
                ..Delegation::default()
            },
            credits_observed: credits_b,
        };
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a);
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b);
        let new_stake = activation_epoch_a
            .merge(&invoke_context, activation_epoch_b, None)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation_a + credits_b * (delegation_b + rent_exempt_reserve))
                / (delegation_a + delegation_b + rent_exempt_reserve)
                + 1
        );
        assert_eq!(
            new_stake.delegation.stake,
            delegation_a + delegation_b + rent_exempt_reserve
        );

        // active stake merge, unmatched credits observed
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(&invoke_context, fully_active_b, None)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation_a + credits_b * delegation_b) / (delegation_a + delegation_b)
                + 1
        );
        assert_eq!(new_stake.delegation.stake, delegation_a + delegation_b);

        // active stake merge, unmatched credits observed, no need to ceiling the calculation
        let delegation = 1_000_000u64;
        let credits_a = 200_000_000u64;
        let credits_b = 100_000_000u64;
        let rent_exempt_reserve = 227_000_000u64;
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::default()
        };
        let stake_a = Stake {
            delegation: Delegation {
                stake: delegation,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation,
                ..Delegation::default()
            },
            credits_observed: credits_b,
        };
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(&invoke_context, fully_active_b, None)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation + credits_b * delegation) / (delegation + delegation)
        );
        assert_eq!(new_stake.delegation.stake, delegation * 2);
    }

    prop_compose! {
        pub fn sum_within(max: u64)(total in 1..max)
            (intermediate in 1..total, total in Just(total))
            -> (u64, u64) {
                (intermediate, total - intermediate)
        }
    }

    proptest! {
        #[test]
        fn test_stake_weighted_credits_observed(
            (credits_a, credits_b) in sum_within(u64::MAX),
            (delegation_a, delegation_b) in sum_within(u64::MAX),
        ) {
            let stake = Stake {
                delegation: Delegation {
                    stake: delegation_a,
                    ..Delegation::default()
                },
                credits_observed: credits_a
            };
            let credits_observed = stake_weighted_credits_observed(
                &stake,
                delegation_b,
                credits_b,
            ).unwrap();

            // calculated credits observed should always be between the credits of a and b
            if credits_a < credits_b {
                assert!(credits_a < credits_observed);
                assert!(credits_observed <= credits_b);
            } else {
                assert!(credits_b <= credits_observed);
                assert!(credits_observed <= credits_a);
            }

            // the difference of the combined weighted credits and the separate weighted credits
            // should be 1 or 0
            let weighted_credits_total = credits_observed as u128 * (delegation_a + delegation_b) as u128;
            let weighted_credits_a = credits_a as u128 * delegation_a as u128;
            let weighted_credits_b = credits_b as u128 * delegation_b as u128;
            let raw_diff = weighted_credits_total - (weighted_credits_a + weighted_credits_b);
            let credits_observed_diff = raw_diff / (delegation_a + delegation_b) as u128;
            assert!(credits_observed_diff <= 1);
        }
    }
}
