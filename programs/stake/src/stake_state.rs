//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::state` or `solana_program::stake::state` instead"
)]
pub use solana_sdk::stake::state::*;
use {
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::{State, StateMut},
        clock::{Clock, Epoch},
        feature_set::{stake_merge_with_unmatched_credits_observed, stake_split_uses_rent_sysvar},
        instruction::{checked_add, InstructionError},
        keyed_account::KeyedAccount,
        pubkey::Pubkey,
        rent::{Rent, ACCOUNT_STORAGE_OVERHEAD},
        stake::{
            config::Config,
            instruction::{LockupArgs, StakeError},
            program::id,
            MINIMUM_STAKE_DELEGATION,
        },
        stake_history::{StakeHistory, StakeHistoryEntry},
    },
    solana_vote_program::vote_state::{VoteState, VoteStateVersions},
    std::{collections::HashSet, convert::TryFrom},
};

#[derive(Debug)]
pub enum SkippedReason {
    DisabledInflation,
    JustActivated,
    TooEarlyUnfairSplit,
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
) -> Result<(), StakeError> {
    // If stake is currently active:
    if stake.stake(clock.epoch, Some(stake_history)) != 0 {
        // If pubkey of new voter is the same as current,
        // and we are scheduled to start deactivating this epoch,
        // we rescind deactivation
        if stake.delegation.voter_pubkey == *voter_pubkey
            && clock.epoch == stake.delegation.deactivation_epoch
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
    let credits_in_stake = stake.credits_observed;
    let credits_in_vote = new_vote_state.credits();
    // if there is no newer credits since observed, return no point
    if credits_in_vote <= credits_in_stake {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroCreditsAndReturnCurrent.into());
        }
        return (0, credits_in_stake);
    }

    let mut points = 0;
    let mut new_credits_observed = credits_in_stake;

    for (epoch, final_epoch_credits, initial_epoch_credits) in
        new_vote_state.epoch_credits().iter().copied()
    {
        let stake_amount = u128::from(stake.delegation.stake(epoch, stake_history));

        // figure out how much this stake has seen that
        //   for which the vote account has a record
        let earned_credits = if credits_in_stake < initial_epoch_credits {
            // the staker observed the entire epoch
            final_epoch_credits - initial_epoch_credits
        } else if credits_in_stake < final_epoch_credits {
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
    _fix_activating_credits_observed: bool, // this unused flag will soon be justified by an upcoming feature pr
) -> Option<(u64, u64, u64)> {
    // ensure to run to trigger (optional) inflation_point_calc_tracer
    // this awkward flag variable will soon be justified by an upcoming feature pr
    let mut forced_credits_update_with_skipped_reward = false;
    let (points, credits_observed) = calculate_stake_points_and_credits(
        stake,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
    );

    // Drive credits_observed forward unconditionally when rewards are disabled
    // or when this is the stake's activation epoch
    if point_value.rewards == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::DisabledInflation.into());
        }
        forced_credits_update_with_skipped_reward = true;
    } else if stake.delegation.activation_epoch == rewarded_epoch {
        // not assert!()-ed; but points should be zero
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::JustActivated.into());
        }
        forced_credits_update_with_skipped_reward = true;
    }

    if forced_credits_update_with_skipped_reward {
        return Some((0, 0, credits_observed));
    }

    if points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroPoints.into());
        }
        return None;
    }
    if point_value.points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
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
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroReward.into());
        }
        return None;
    }
    let (voter_rewards, staker_rewards, is_split) = vote_state.commission_split(rewards);
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
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
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::TooEarlyUnfairSplit.into());
        }
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
    ) -> Result<(), InstructionError>;
    fn deactivate(&self, clock: &Clock, signers: &HashSet<Pubkey>) -> Result<(), InstructionError>;
    fn set_lockup(
        &self,
        lockup: &LockupArgs,
        signers: &HashSet<Pubkey>,
        clock: &Clock,
    ) -> Result<(), InstructionError>;
    fn split(
        &self,
        invoke_context: &InvokeContext,
        lamports: u64,
        split_stake: &KeyedAccount,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn merge(
        &self,
        invoke_context: &InvokeContext,
        source_stake: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn withdraw(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        withdraw_authority: &KeyedAccount,
        custodian: Option<&KeyedAccount>,
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
            let minimum_balance = rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;

            if self.lamports()? >= minimum_balance {
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
    ) -> Result<(), InstructionError> {
        if vote_account.owner()? != solana_vote_program::id() {
            return Err(InstructionError::IncorrectProgramId);
        }

        match self.state()? {
            StakeState::Initialized(meta) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                let ValidatedDelegatedInfo { stake_amount } =
                    validate_delegated_amount(self, &meta)?;
                let stake = new_stake(
                    stake_amount,
                    vote_account.unsigned_key(),
                    &State::<VoteStateVersions>::state(vote_account)?.convert_to_current(),
                    clock.epoch,
                    config,
                );
                self.set_state(&StakeState::Stake(meta, stake))
            }
            StakeState::Stake(meta, mut stake) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                let ValidatedDelegatedInfo { stake_amount } =
                    validate_delegated_amount(self, &meta)?;
                redelegate(
                    &mut stake,
                    stake_amount,
                    vote_account.unsigned_key(),
                    &State::<VoteStateVersions>::state(vote_account)?.convert_to_current(),
                    clock,
                    stake_history,
                    config,
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
        clock: &Clock,
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
        invoke_context: &InvokeContext,
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
        if !matches!(split.state()?, StakeState::Uninitialized) {
            return Err(InstructionError::InvalidAccountData);
        }
        if lamports > self.lamports()? {
            return Err(InstructionError::InsufficientFunds);
        }

        match self.state()? {
            StakeState::Stake(meta, mut stake) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                let validated_split_info = validate_split_amount(
                    invoke_context,
                    self,
                    split,
                    lamports,
                    &meta,
                    Some(&stake),
                )?;

                // split the stake, subtract rent_exempt_balance unless
                // the destination account already has those lamports
                // in place.
                // this means that the new stake account will have a stake equivalent to
                // lamports minus rent_exempt_reserve if it starts out with a zero balance
                let (remaining_stake_delta, split_stake_amount) =
                    if validated_split_info.source_remaining_balance == 0 {
                        // If split amount equals the full source stake (as implied by 0
                        // source_remaining_balance), the new split stake must equal the same
                        // amount, regardless of any current lamport balance in the split account.
                        // Since split accounts retain the state of their source account, this
                        // prevents any magic activation of stake by prefunding the split account.
                        //
                        // The new split stake also needs to ignore any positive delta between the
                        // original rent_exempt_reserve and the split_rent_exempt_reserve, in order
                        // to prevent magic activation of stake by splitting between accounts of
                        // different sizes.
                        let remaining_stake_delta =
                            lamports.saturating_sub(meta.rent_exempt_reserve);
                        (remaining_stake_delta, remaining_stake_delta)
                    } else {
                        // Otherwise, the new split stake should reflect the entire split
                        // requested, less any lamports needed to cover the split_rent_exempt_reserve.
                        (
                            lamports,
                            lamports.saturating_sub(
                                validated_split_info
                                    .destination_rent_exempt_reserve
                                    .saturating_sub(split.lamports()?),
                            ),
                        )
                    };
                let split_stake = stake.split(remaining_stake_delta, split_stake_amount)?;
                let mut split_meta = meta;
                split_meta.rent_exempt_reserve =
                    validated_split_info.destination_rent_exempt_reserve;

                self.set_state(&StakeState::Stake(meta, stake))?;
                split.set_state(&StakeState::Stake(split_meta, split_stake))?;
            }
            StakeState::Initialized(meta) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                let validated_split_info =
                    validate_split_amount(invoke_context, self, split, lamports, &meta, None)?;
                let mut split_meta = meta;
                split_meta.rent_exempt_reserve =
                    validated_split_info.destination_rent_exempt_reserve;
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
    }

    fn merge(
        &self,
        invoke_context: &InvokeContext,
        source_account: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        signers: &HashSet<Pubkey>,
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
                // stake accounts must have a balance >= rent_exempt_reserve + minimum_stake_delegation
                let reserve = checked_add(meta.rent_exempt_reserve, MINIMUM_STAKE_DELEGATION)?;

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

/// After calling `validate_delegated_amount()`, this struct contains calculated values that are used
/// by the caller.
struct ValidatedDelegatedInfo {
    stake_amount: u64,
}

/// Ensure the stake delegation amount is valid.  This checks that the account meets the minimum
/// balance requirements of delegated stake.  If not, return an error.
fn validate_delegated_amount(
    account: &KeyedAccount,
    meta: &Meta,
) -> Result<ValidatedDelegatedInfo, InstructionError> {
    let stake_amount = account.lamports()?.saturating_sub(meta.rent_exempt_reserve); // can't stake the rent
    Ok(ValidatedDelegatedInfo { stake_amount })
}

/// After calling `validate_split_amount()`, this struct contains calculated values that are used
/// by the caller.
#[derive(Copy, Clone, Debug, Default)]
struct ValidatedSplitInfo {
    source_remaining_balance: u64,
    destination_rent_exempt_reserve: u64,
}

/// Ensure the split amount is valid.  This checks the source and destination accounts meet the
/// minimum balance requirements, which is the rent exempt reserve plus the minimum stake
/// delegation, and that the source account has enough lamports for the request split amount.  If
/// not, return an error.
fn validate_split_amount(
    invoke_context: &InvokeContext,
    source_account: &KeyedAccount,
    destination_account: &KeyedAccount,
    lamports: u64,
    source_meta: &Meta,
    source_stake: Option<&Stake>,
) -> Result<ValidatedSplitInfo, InstructionError> {
    let source_lamports = source_account.lamports()?;
    let destination_lamports = destination_account.lamports()?;

    // Split amount has to be something
    if lamports == 0 {
        return Err(InstructionError::InsufficientFunds);
    }

    // Obviously cannot split more than what the source account has
    if lamports > source_lamports {
        return Err(InstructionError::InsufficientFunds);
    }

    // Verify that the source account still has enough lamports left after splitting:
    // EITHER at least the minimum balance, OR zero (in this case the source
    // account is transferring all lamports to new destination account, and the source
    // account will be closed)
    let source_minimum_balance = source_meta
        .rent_exempt_reserve
        .saturating_add(MINIMUM_STAKE_DELEGATION);
    let source_remaining_balance = source_lamports.saturating_sub(lamports);
    if source_remaining_balance == 0 {
        // full amount is a withdrawal
        // nothing to do here
    } else if source_remaining_balance < source_minimum_balance {
        // the remaining balance is too low to do the split
        return Err(InstructionError::InsufficientFunds);
    } else {
        // all clear!
        // nothing to do here
    }

    // Verify the destination account meets the minimum balance requirements
    // This must handle:
    // 1. The destination account having a different rent exempt reserve due to data size changes
    // 2. The destination account being prefunded, which would lower the minimum split amount
    let destination_rent_exempt_reserve = if invoke_context
        .feature_set
        .is_active(&stake_split_uses_rent_sysvar::ID)
    {
        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        rent.minimum_balance(destination_account.data_len()?)
    } else {
        calculate_split_rent_exempt_reserve(
            source_meta.rent_exempt_reserve,
            source_account.data_len()? as u64,
            destination_account.data_len()? as u64,
        )
    };
    let destination_minimum_balance =
        destination_rent_exempt_reserve.saturating_add(MINIMUM_STAKE_DELEGATION);
    let destination_balance_deficit =
        destination_minimum_balance.saturating_sub(destination_lamports);
    if lamports < destination_balance_deficit {
        return Err(InstructionError::InsufficientFunds);
    }

    // If the source account is already staked, the destination will end up staked as well.  Verify
    // the destination account's delegation amount is at least MINIMUM_STAKE_DELEGATION.
    //
    // The *delegation* requirements are different than the *balance* requirements.  If the
    // destination account is prefunded with a balance of `rent exempt reserve + minimum stake
    // delegation - 1`, the minimum split amount to satisfy the *balance* requirements is 1
    // lamport.  And since *only* the split amount is immediately staked in the destination
    // account, the split amount must be at least the minimum stake delegation.  So if the minimum
    // stake delegation was 10 lamports, then a split amount of 1 lamport would not meet the
    // *delegation* requirements.
    if source_stake.is_some() && lamports < MINIMUM_STAKE_DELEGATION {
        return Err(InstructionError::InsufficientFunds);
    }

    Ok(ValidatedSplitInfo {
        source_remaining_balance,
        destination_rent_exempt_reserve,
    })
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
        invoke_context: &InvokeContext,
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
        invoke_context: &InvokeContext,
        stake: &Meta,
        source: &Meta,
        clock: &Clock,
    ) -> Result<(), InstructionError> {
        // lockups may mismatch so long as both have expired
        let can_merge_lockups = stake.lockup == source.lockup
            || (!stake.lockup.is_in_force(clock, None) && !source.lockup.is_in_force(clock, None));
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
        invoke_context: &InvokeContext,
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
        invoke_context: &InvokeContext,
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
        invoke_context: &InvokeContext,
        source: Self,
        clock: &Clock,
    ) -> Result<Option<StakeState>, InstructionError> {
        Self::metas_can_merge(invoke_context, self.meta(), source.meta(), clock)?;
        self.active_stake()
            .zip(source.active_stake())
            .map(|(stake, source)| {
                if invoke_context
                    .feature_set
                    .is_active(&stake_merge_with_unmatched_credits_observed::id())
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
    invoke_context: &InvokeContext,
    stake: &mut Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Result<(), InstructionError> {
    if invoke_context
        .feature_set
        .is_active(&stake_merge_with_unmatched_credits_observed::id())
    {
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

// utility function, used by tests
pub fn create_stake_history_from_delegations(
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
    use {
        super::*,
        proptest::prelude::*,
        solana_program_runtime::invoke_context::InvokeContext,
        solana_sdk::{
            account::{create_account_shared_data_for_test, AccountSharedData, WritableAccount},
            native_token,
            pubkey::Pubkey,
            system_program,
            sysvar::SysvarId,
            transaction_context::TransactionContext,
        },
        solana_vote_program::vote_state,
        std::{cell::RefCell, iter::FromIterator},
    };

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
            let history = stake_history.get(epoch).unwrap();
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

    fn create_mock_tx_context() -> TransactionContext {
        TransactionContext::new(
            vec![(
                Rent::id(),
                create_account_shared_data_for_test(&Rent::default()),
            )],
            1,
            1,
        )
    }

    #[test]
    fn test_split_source_uninitialized() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = (rent_exempt_reserve + MINIMUM_STAKE_DELEGATION) * 2;
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
                &invoke_context,
                stake_lamports / 2,
                &split_stake_keyed_account,
                &HashSet::default() // no signers
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        let signers = HashSet::from([stake_pubkey]);

        // splitting an uninitialized account where the destination is the same as the source
        {
            // splitting should work when...
            // - when split amount is the full balance
            // - when split amount is zero
            // - when split amount is non-zero and less than the full balance
            //
            // and splitting should fail when the split amount is greater than the balance
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports,
                    &stake_keyed_account,
                    &signers
                ),
                Ok(()),
            );
            assert_eq!(
                stake_keyed_account.split(&invoke_context, 0, &stake_keyed_account, &signers),
                Ok(()),
            );
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports / 2,
                    &stake_keyed_account,
                    &signers
                ),
                Ok(()),
            );
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports + 1,
                    &stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds),
            );
        }

        // this should work
        assert_eq!(
            stake_keyed_account.split(
                &invoke_context,
                stake_lamports / 2,
                &split_stake_keyed_account,
                &signers
            ),
            Ok(())
        );
        assert_eq!(
            stake_keyed_account.account.borrow().lamports(),
            split_stake_keyed_account.account.borrow().lamports()
        );
    }

    #[test]
    fn test_split_split_not_uninitialized() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(Meta::auto(&stake_pubkey), just_stake(stake_lamports)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let signers = vec![stake_pubkey].into_iter().collect();

        for split_stake_state in &[
            StakeState::Initialized(Meta::default()),
            StakeState::Stake(Meta::default(), Stake::default()),
            StakeState::RewardsPool,
        ] {
            let split_stake_pubkey = solana_sdk::pubkey::new_rand();
            let split_stake_account = AccountSharedData::new_ref_data_with_space(
                0,
                split_stake_state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("split_stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports / 2,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InvalidAccountData)
            );
        }
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
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = (rent_exempt_reserve + MINIMUM_STAKE_DELEGATION) * 2;
        let stake_account = AccountSharedData::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(
                Meta {
                    rent_exempt_reserve,
                    ..Meta::auto(&stake_pubkey)
                },
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
            stake_keyed_account.split(
                &invoke_context,
                stake_lamports / 2,
                &split_stake_keyed_account,
                &signers
            ),
            Err(StakeError::InsufficientStake.into())
        );
    }

    #[test]
    fn test_split_with_rent() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_balance = rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = minimum_balance * 2;
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
                    &invoke_context,
                    rent_exempt_reserve,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // doesn't leave enough for initial stake to be non-zero
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
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
                .set_lamports(minimum_balance);
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports - minimum_balance,
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
                                stake: stake_lamports - minimum_balance,
                                ..stake.delegation
                            },
                            ..*stake
                        }
                    ))
                );
                assert_eq!(
                    stake_keyed_account.account.borrow().lamports(),
                    minimum_balance,
                );
                assert_eq!(
                    split_stake_keyed_account.account.borrow().lamports(),
                    stake_lamports,
                );
            }
        }
    }

    #[test]
    fn test_split_to_account_with_rent_exempt_reserve() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = (rent_exempt_reserve + MINIMUM_STAKE_DELEGATION) * 2;

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
        let split_lamport_balances = vec![
            0,
            rent_exempt_reserve - 1,
            rent_exempt_reserve,
            rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1,
            rent_exempt_reserve + MINIMUM_STAKE_DELEGATION,
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
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split more than available fails
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports + 1,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // should work
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports / 2,
                    &split_stake_keyed_account,
                    &signers
                ),
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
    fn test_split_from_larger_sized_account() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let source_larger_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>() + 100);
        let split_rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = (source_larger_rent_exempt_reserve + MINIMUM_STAKE_DELEGATION) * 2;

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve: source_larger_rent_exempt_reserve,
            ..Meta::default()
        };

        let state = StakeState::Stake(
            meta,
            just_stake(stake_lamports - source_larger_rent_exempt_reserve),
        );

        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. The empty case is not covered in
        // test_split, since that test uses a Meta with rent_exempt_reserve = 0
        let split_lamport_balances = vec![
            0,
            split_rent_exempt_reserve - 1,
            split_rent_exempt_reserve,
            split_rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1,
            split_rent_exempt_reserve + MINIMUM_STAKE_DELEGATION,
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
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports + 1,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // should work
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports / 2,
                    &split_stake_keyed_account,
                    &signers
                ),
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
                    rent_exempt_reserve: split_rent_exempt_reserve,
                    ..Meta::default()
                };
                let expected_stake = stake_lamports / 2
                    - (split_rent_exempt_reserve.saturating_sub(initial_balance));

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
                        + split_rent_exempt_reserve
                        + initial_balance.saturating_sub(split_rent_exempt_reserve)
                );
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2 - source_larger_rent_exempt_reserve,
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
    fn test_split_from_smaller_sized_account() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let source_smaller_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>());
        let split_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>() + 100);

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve: source_smaller_rent_exempt_reserve,
            ..Meta::default()
        };

        let stake_lamports = split_rent_exempt_reserve + 1;
        let split_amount = stake_lamports - (source_smaller_rent_exempt_reserve + 1); // Enough so that split stake is > 0

        let state = StakeState::Stake(
            meta,
            just_stake(stake_lamports - source_smaller_rent_exempt_reserve),
        );

        let split_lamport_balances = vec![
            0,
            1,
            split_rent_exempt_reserve,
            split_rent_exempt_reserve + 1,
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
            let split_result = stake_keyed_account.split(
                &invoke_context,
                split_amount,
                &split_stake_keyed_account,
                &signers,
            );
            assert_eq!(split_result, Err(InstructionError::InvalidAccountData));

            // Splitting 100% of source should not make a difference
            let split_result = stake_keyed_account.split(
                &invoke_context,
                stake_lamports,
                &split_stake_keyed_account,
                &signers,
            );
            assert_eq!(split_result, Err(InstructionError::InvalidAccountData));
        }
    }

    #[test]
    fn test_split_100_percent_of_source() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;

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
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports,
                    &split_stake_keyed_account,
                    &signers
                ),
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
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;

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
        let split_lamport_balances = vec![
            0,
            rent_exempt_reserve - 1,
            rent_exempt_reserve,
            rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1,
            rent_exempt_reserve + MINIMUM_STAKE_DELEGATION,
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
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            // split 100% over to dest
            assert_eq!(
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports,
                    &split_stake_keyed_account,
                    &signers
                ),
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
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let source_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>() + 100);
        let split_rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_lamports = source_rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;

        let split_stake_pubkey = solana_sdk::pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve: source_rent_exempt_reserve,
            ..Meta::default()
        };

        for state in &[
            StakeState::Initialized(meta),
            StakeState::Stake(
                meta,
                just_stake(stake_lamports - source_rent_exempt_reserve),
            ),
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
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports,
                    &split_stake_keyed_account,
                    &signers
                ),
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
                stake_keyed_account.split(
                    &invoke_context,
                    stake_lamports,
                    &split_stake_keyed_account,
                    &signers
                ),
                Ok(())
            );

            assert_eq!(
                split_stake_keyed_account.account.borrow().lamports(),
                stake_lamports
            );

            let expected_split_meta = Meta {
                authorized: Authorized::auto(&stake_pubkey),
                rent_exempt_reserve: split_rent_exempt_reserve,
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
                    let expected_stake = stake_lamports - source_rent_exempt_reserve;

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
                        expected_stake + source_rent_exempt_reserve,
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), stake_keyed_account.state());
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn test_merge() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let signers = vec![authorized_pubkey].into_iter().collect();

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
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
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
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_merge_incorrect_authorized_staker() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let wrong_authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;

        let signers = vec![authorized_pubkey].into_iter().collect();
        let wrong_signers = vec![wrong_authorized_pubkey].into_iter().collect();

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
                    ),
                    Err(StakeError::MergeMismatch.into())
                );
            }
        }
    }

    #[test]
    fn test_merge_invalid_account_data() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_pubkey = solana_sdk::pubkey::new_rand();
        let authorized_pubkey = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let signers = vec![authorized_pubkey].into_iter().collect();

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
                    ),
                    Err(InstructionError::InvalidAccountData)
                );
            }
        }
    }

    #[test]
    fn test_merge_fake_stake_source() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
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

        assert_eq!(
            stake_keyed_account.merge(
                &invoke_context,
                &source_stake_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &signers,
            ),
            Err(InstructionError::IncorrectProgramId)
        );
    }

    #[test]
    fn test_merge_active_stake() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
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
            invoke_context: &InvokeContext,
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
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let good_stake = Stake {
            credits_observed: 4242,
            delegation: Delegation {
                voter_pubkey: Pubkey::new_unique(),
                stake: 424242424242,
                activation_epoch: 42,
                ..Delegation::default()
            },
        };

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
    fn test_metas_can_merge() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        // Identical Metas can merge
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &Meta::default(),
            &Clock::default()
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
            &Clock::default()
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_rent_exempt_reserve_ok,
            &Meta::default(),
            &Clock::default()
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
            &Clock::default()
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_authorized_fails,
            &Meta::default(),
            &Clock::default()
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
            &Clock::default()
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            &Clock::default()
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
            &clock
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            &clock
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
            &clock
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            &clock
        )
        .is_ok());
    }

    #[test]
    fn test_merge_kind_get_if_mergeable() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
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
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let clock = Clock::default();
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

        assert_eq!(
            inactive
                .clone()
                .merge(&invoke_context, inactive.clone(), &clock)
                .unwrap(),
            None
        );
        assert_eq!(
            inactive
                .clone()
                .merge(&invoke_context, activation_epoch.clone(), &clock)
                .unwrap(),
            None
        );
        assert!(inactive
            .clone()
            .merge(&invoke_context, fully_active.clone(), &clock)
            .is_err());
        assert!(activation_epoch
            .clone()
            .merge(&invoke_context, fully_active.clone(), &clock)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(&invoke_context, inactive.clone(), &clock)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(&invoke_context, activation_epoch.clone(), &clock)
            .is_err());

        let new_state = activation_epoch
            .clone()
            .merge(&invoke_context, inactive, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, stake.delegation.stake + lamports);

        let new_state = activation_epoch
            .clone()
            .merge(&invoke_context, activation_epoch, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(
            delegation.stake,
            2 * stake.delegation.stake + meta.rent_exempt_reserve
        );

        let new_state = fully_active
            .clone()
            .merge(&invoke_context, fully_active, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, 2 * stake.delegation.stake);
    }

    #[test]
    fn test_active_stake_merge() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let clock = Clock::default();
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

        // activating stake merge, match credits observed
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a);
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b);
        let new_stake = activation_epoch_a
            .merge(&invoke_context, activation_epoch_b, &clock)
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
            .merge(&invoke_context, fully_active_b, &clock)
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
            .merge(&invoke_context, activation_epoch_b, &clock)
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
            .merge(&invoke_context, fully_active_b, &clock)
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
            .merge(&invoke_context, fully_active_b, &clock)
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

    /// Ensure that `initialize()` respects the MINIMUM_STAKE_DELEGATION requirements
    /// - Assert 1: accounts with a balance equal-to the minimum initialize OK
    /// - Assert 2: accounts with a balance less-than the minimum do not initialize
    #[test]
    fn test_initialize_minimum_stake_delegation() {
        for (stake_delegation, expected_result) in [
            (MINIMUM_STAKE_DELEGATION, Ok(())),
            (
                MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            let rent = Rent::default();
            let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
            let stake_pubkey = Pubkey::new_unique();
            let stake_account = AccountSharedData::new_ref(
                stake_delegation + rent_exempt_reserve,
                std::mem::size_of::<StakeState>(),
                &id(),
            );
            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

            assert_eq!(
                expected_result,
                stake_keyed_account.initialize(
                    &Authorized::auto(&stake_pubkey),
                    &Lockup::default(),
                    &rent
                ),
            );
        }
    }

    /// Ensure that `delegate()` respects the MINIMUM_STAKE_DELEGATION requirements
    /// - Assert 1: delegating an amount equal-to the minimum delegates OK
    /// - Assert 2: delegating an amount less-than the minimum delegates OK
    /// Also test both asserts above over both StakeState::{Initialized and Stake}, since the logic
    /// is slightly different for the variants.
    ///
    /// NOTE: Even though new stake accounts must have a minimum balance that is at least
    /// MINIMUM_STAKE_DELEGATION (plus rent exempt reserve), the current behavior allows
    /// withdrawing below the minimum delegation, then re-delegating successfully (see
    /// `test_behavior_withdrawal_then_redelegate_with_less_than_minimum_stake_delegation()` for
    /// more information.)
    #[test]
    fn test_delegate_minimum_stake_delegation() {
        for (stake_delegation, expected_result) in [
            (MINIMUM_STAKE_DELEGATION, Ok(())),
            (MINIMUM_STAKE_DELEGATION - 1, Ok(())),
        ] {
            let rent = Rent::default();
            let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
            let stake_pubkey = Pubkey::new_unique();
            let signers = HashSet::from([stake_pubkey]);
            let meta = Meta {
                rent_exempt_reserve,
                ..Meta::auto(&stake_pubkey)
            };

            for stake_state in &[
                StakeState::Initialized(meta),
                StakeState::Stake(meta, just_stake(stake_delegation)),
            ] {
                let stake_account = AccountSharedData::new_ref_data_with_space(
                    stake_delegation + rent_exempt_reserve,
                    stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

                let vote_pubkey = Pubkey::new_unique();
                let vote_account = RefCell::new(vote_state::create_account(
                    &vote_pubkey,
                    &Pubkey::new_unique(),
                    0,
                    100,
                ));
                let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);

                assert_eq!(
                    expected_result,
                    stake_keyed_account.delegate(
                        &vote_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &Config::default(),
                        &signers,
                    ),
                );
            }
        }
    }

    /// Ensure that `split()` respects the MINIMUM_STAKE_DELEGATION requirements.  This applies to
    /// both the source and destination acounts.  Thus, we have four permutations possible based on
    /// if each account's post-split delegation is equal-to (EQ) or less-than (LT) the minimum:
    ///
    ///  source | dest | result
    /// --------+------+--------
    ///  EQ     | EQ   | Ok
    ///  EQ     | LT   | Err
    ///  LT     | EQ   | Err
    ///  LT     | LT   | Err
    #[test]
    fn test_split_minimum_stake_delegation() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        for (source_stake_delegation, dest_stake_delegation, expected_result) in [
            (MINIMUM_STAKE_DELEGATION, MINIMUM_STAKE_DELEGATION, Ok(())),
            (
                MINIMUM_STAKE_DELEGATION,
                MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
            (
                MINIMUM_STAKE_DELEGATION - 1,
                MINIMUM_STAKE_DELEGATION,
                Err(InstructionError::InsufficientFunds),
            ),
            (
                MINIMUM_STAKE_DELEGATION - 1,
                MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            let rent = Rent::default();
            let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
            let source_pubkey = Pubkey::new_unique();
            let source_meta = Meta {
                rent_exempt_reserve,
                ..Meta::auto(&source_pubkey)
            };
            // The source account's starting balance is equal to *both* the source and dest
            // accounts' *final* balance
            let source_starting_balance =
                source_stake_delegation + dest_stake_delegation + rent_exempt_reserve * 2;

            for source_stake_state in &[
                StakeState::Initialized(source_meta),
                StakeState::Stake(
                    source_meta,
                    just_stake(source_starting_balance - rent_exempt_reserve),
                ),
            ] {
                let source_account = AccountSharedData::new_ref_data_with_space(
                    source_starting_balance,
                    source_stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let source_keyed_account = KeyedAccount::new(&source_pubkey, true, &source_account);

                let dest_pubkey = Pubkey::new_unique();
                let dest_account = AccountSharedData::new_ref_data_with_space(
                    0,
                    &StakeState::Uninitialized,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let dest_keyed_account = KeyedAccount::new(&dest_pubkey, true, &dest_account);

                assert_eq!(
                    expected_result,
                    source_keyed_account.split(
                        &invoke_context,
                        dest_stake_delegation + rent_exempt_reserve,
                        &dest_keyed_account,
                        &HashSet::from([source_pubkey]),
                    ),
                );
            }
        }
    }

    /// Ensure that splitting the full amount from an account respects the MINIMUM_STAKE_DELEGATION
    /// requirements.  This ensures that we are future-proofing/testing any raises to the minimum
    /// delegation.
    /// - Assert 1: splitting the full amount from an account that has at least the minimum
    ///             delegation is OK
    /// - Assert 2: splitting the full amount from an account that has less than the minimum
    ///             delegation is not OK
    #[test]
    fn test_split_full_amount_minimum_stake_delegation() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        for (stake_delegation, expected_result) in [
            (MINIMUM_STAKE_DELEGATION, Ok(())),
            (
                MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            let rent = Rent::default();
            let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
            let source_pubkey = Pubkey::new_unique();
            let source_meta = Meta {
                rent_exempt_reserve,
                ..Meta::auto(&source_pubkey)
            };

            for source_stake_state in &[
                StakeState::Initialized(source_meta),
                StakeState::Stake(source_meta, just_stake(stake_delegation)),
            ] {
                let source_account = AccountSharedData::new_ref_data_with_space(
                    stake_delegation + rent_exempt_reserve,
                    source_stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let source_keyed_account = KeyedAccount::new(&source_pubkey, true, &source_account);

                let dest_pubkey = Pubkey::new_unique();
                let dest_account = AccountSharedData::new_ref_data_with_space(
                    0,
                    &StakeState::Uninitialized,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let dest_keyed_account = KeyedAccount::new(&dest_pubkey, true, &dest_account);

                assert_eq!(
                    expected_result,
                    source_keyed_account.split(
                        &invoke_context,
                        source_keyed_account.lamports().unwrap(),
                        &dest_keyed_account,
                        &HashSet::from([source_pubkey]),
                    ),
                );
            }
        }
    }

    /// Ensure that `split()` correctly handles prefunded destination accounts.  When a destination
    /// account already has funds, ensure the minimum split amount reduces accordingly.
    #[test]
    fn test_split_destination_minimum_stake_delegation() {
        let mut transaction_context = create_mock_tx_context();
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());

        for (destination_starting_balance, split_amount, expected_result) in [
            // split amount must be non zero
            (
                rent_exempt_reserve + MINIMUM_STAKE_DELEGATION,
                0,
                Err(InstructionError::InsufficientFunds),
            ),
            // any split amount is OK when destination account is already fully funded
            (rent_exempt_reserve + MINIMUM_STAKE_DELEGATION, 1, Ok(())),
            // if destination is only short by 1 lamport, then split amount can be 1 lamport
            (
                rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1,
                1,
                Ok(()),
            ),
            // destination short by 2 lamports, so 1 isn't enough (non-zero split amount)
            (
                rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 2,
                1,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination is rent exempt, so split enough for minimum delegation
            (rent_exempt_reserve, MINIMUM_STAKE_DELEGATION, Ok(())),
            // destination is rent exempt, but split amount less than minimum delegation
            (
                rent_exempt_reserve,
                MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination is not rent exempt, so split enough for rent and minimum delegation
            (
                rent_exempt_reserve - 1,
                MINIMUM_STAKE_DELEGATION + 1,
                Ok(()),
            ),
            // destination is not rent exempt, but split amount only for minimum delegation
            (
                rent_exempt_reserve - 1,
                MINIMUM_STAKE_DELEGATION,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination has smallest non-zero balance, so can split the minimum balance
            // requirements minus what destination already has
            (
                1,
                rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1,
                Ok(()),
            ),
            // destination has smallest non-zero balance, but cannot split less than the minimum
            // balance requirements minus what destination already has
            (
                1,
                rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 2,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination has zero lamports, so split must be at least rent exempt reserve plus
            // minimum delegation
            (0, rent_exempt_reserve + MINIMUM_STAKE_DELEGATION, Ok(())),
            // destination has zero lamports, but split amount is less than rent exempt reserve
            // plus minimum delegation
            (
                0,
                rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            let source_pubkey = Pubkey::new_unique();
            let source_meta = Meta {
                rent_exempt_reserve,
                ..Meta::auto(&source_pubkey)
            };

            // Set the source's starting balance and stake delegation amount to something large
            // to ensure its post-split balance meets all the requirements
            let source_balance = u64::MAX;
            let source_stake_delegation = source_balance - rent_exempt_reserve;

            for source_stake_state in &[
                StakeState::Initialized(source_meta),
                StakeState::Stake(source_meta, just_stake(source_stake_delegation)),
            ] {
                let source_account = AccountSharedData::new_ref_data_with_space(
                    source_balance,
                    &source_stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let source_keyed_account = KeyedAccount::new(&source_pubkey, true, &source_account);

                let destination_pubkey = Pubkey::new_unique();
                let destination_account = AccountSharedData::new_ref_data_with_space(
                    destination_starting_balance,
                    &StakeState::Uninitialized,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let destination_keyed_account =
                    KeyedAccount::new(&destination_pubkey, true, &destination_account);

                assert_eq!(
                    expected_result,
                    source_keyed_account.split(
                        &invoke_context,
                        split_amount,
                        &destination_keyed_account,
                        &HashSet::from([source_pubkey]),
                    ),
                );

                // For the expected OK cases, when the source's StakeState is Stake, then the
                // destination's StakeState *must* also end up as Stake as well.  Additionally,
                // check to ensure the destination's delegation amount is correct.  If the
                // destination is already rent exempt, then the destination's stake delegation
                // *must* equal the split amount. Otherwise, the split amount must first be used to
                // make the destination rent exempt, and then the leftover lamports are delegated.
                if expected_result.is_ok() {
                    if let StakeState::Stake(_, _) = source_keyed_account.state().unwrap() {
                        if let StakeState::Stake(_, destination_stake) =
                            destination_keyed_account.state().unwrap()
                        {
                            let destination_initial_rent_deficit =
                                rent_exempt_reserve.saturating_sub(destination_starting_balance);
                            let expected_destination_stake_delegation =
                                split_amount - destination_initial_rent_deficit;
                            assert_eq!(
                                expected_destination_stake_delegation,
                                destination_stake.delegation.stake
                            );
                            assert!(destination_stake.delegation.stake >= MINIMUM_STAKE_DELEGATION,);
                        } else {
                            panic!("destination state must be StakeStake::Stake after successful split when source is also StakeState::Stake!");
                        }
                    }
                }
            }
        }
    }

    /// Ensure that `withdraw()` respects the MINIMUM_STAKE_DELEGATION requirements
    /// - Assert 1: withdrawing so remaining stake is equal-to the minimum is OK
    /// - Assert 2: withdrawing so remaining stake is less-than the minimum is not OK
    #[test]
    fn test_withdraw_minimum_stake_delegation() {
        let starting_stake_delegation = MINIMUM_STAKE_DELEGATION;
        for (ending_stake_delegation, expected_result) in [
            (MINIMUM_STAKE_DELEGATION, Ok(())),
            (
                MINIMUM_STAKE_DELEGATION - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            let rent = Rent::default();
            let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
            let stake_pubkey = Pubkey::new_unique();
            let meta = Meta {
                rent_exempt_reserve,
                ..Meta::auto(&stake_pubkey)
            };

            for stake_state in &[
                StakeState::Initialized(meta),
                StakeState::Stake(meta, just_stake(starting_stake_delegation)),
            ] {
                let rewards_balance = 123;
                let stake_account = AccountSharedData::new_ref_data_with_space(
                    starting_stake_delegation + rent_exempt_reserve + rewards_balance,
                    stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

                let to_pubkey = Pubkey::new_unique();
                let to_account =
                    AccountSharedData::new_ref(rent_exempt_reserve, 0, &system_program::id());
                let to_keyed_account = KeyedAccount::new(&to_pubkey, false, &to_account);

                let withdraw_amount =
                    (starting_stake_delegation + rewards_balance) - ending_stake_delegation;
                assert_eq!(
                    expected_result,
                    stake_keyed_account.withdraw(
                        withdraw_amount,
                        &to_keyed_account,
                        &Clock::default(),
                        &StakeHistory::default(),
                        &stake_keyed_account,
                        None,
                    ),
                );
            }
        }
    }

    /// The stake program currently allows delegations below the minimum stake delegation (see also
    /// `test_delegate_minimum_stake_delegation()`).  This is not the ultimate desired behavior,
    /// but this test ensures the existing behavior is not changed inadvertently.
    ///
    /// This test:
    /// 1. Initialises a stake account (with sufficient balance for both rent and minimum delegation)
    /// 2. Delegates the minimum amount
    /// 3. Deactives the delegation
    /// 4. Withdraws from the account such that the ending balance is *below* rent + minimum delegation
    /// 5. Re-delegates, now with less than the minimum delegation, but it still succeeds
    #[test]
    fn test_behavior_withdrawal_then_redelegate_with_less_than_minimum_stake_delegation() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_pubkey = Pubkey::new_unique();
        let signers = HashSet::from([stake_pubkey]);
        let stake_account = AccountSharedData::new_ref(
            rent_exempt_reserve + MINIMUM_STAKE_DELEGATION,
            std::mem::size_of::<StakeState>(),
            &id(),
        );
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        stake_keyed_account
            .initialize(&Authorized::auto(&stake_pubkey), &Lockup::default(), &rent)
            .unwrap();

        let vote_pubkey = Pubkey::new_unique();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_unique(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        let mut clock = Clock::default();
        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
            )
            .unwrap();

        clock.epoch += 1;
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        clock.epoch += 1;
        let withdraw_amount = stake_keyed_account.lamports().unwrap()
            - (rent_exempt_reserve + MINIMUM_STAKE_DELEGATION - 1);
        let withdraw_pubkey = Pubkey::new_unique();
        let withdraw_account =
            AccountSharedData::new_ref(rent_exempt_reserve, 0, &system_program::id());
        let withdraw_keyed_account = KeyedAccount::new(&withdraw_pubkey, false, &withdraw_account);
        stake_keyed_account
            .withdraw(
                withdraw_amount,
                &withdraw_keyed_account,
                &clock,
                &StakeHistory::default(),
                &stake_keyed_account,
                None,
            )
            .unwrap();

        assert!(stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
            )
            .is_ok());
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
