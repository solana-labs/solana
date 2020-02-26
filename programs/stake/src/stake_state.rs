//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

use crate::{config::Config, id, stake_instruction::StakeError};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::{Account, KeyedAccount},
    account_utils::{State, StateMut},
    clock::{Clock, Epoch, UnixTimestamp},
    instruction::InstructionError,
    pubkey::Pubkey,
    rent::Rent,
    stake_history::{StakeHistory, StakeHistoryEntry},
};
use solana_vote_program::vote_state::{VoteState, VoteStateVersions};
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
#[allow(clippy::large_enum_variant)]
pub enum StakeState {
    Uninitialized,
    Initialized(Meta),
    Stake(Meta, Stake),
    RewardsPool,
}

impl Default for StakeState {
    fn default() -> Self {
        StakeState::Uninitialized
    }
}

impl StakeState {
    pub fn get_rent_exempt_reserve(rent: &Rent) -> u64 {
        rent.minimum_balance(std::mem::size_of::<StakeState>())
    }

    // utility function, used by Stakes, tests
    pub fn from(account: &Account) -> Option<StakeState> {
        account.state().ok()
    }

    pub fn stake_from(account: &Account) -> Option<Stake> {
        Self::from(account).and_then(|state: Self| state.stake())
    }
    pub fn stake(&self) -> Option<Stake> {
        match self {
            StakeState::Stake(_meta, stake) => Some(*stake),
            _ => None,
        }
    }

    pub fn delegation_from(account: &Account) -> Option<Delegation> {
        Self::from(account).and_then(|state: Self| state.delegation())
    }
    pub fn delegation(&self) -> Option<Delegation> {
        match self {
            StakeState::Stake(_meta, stake) => Some(stake.delegation),
            _ => None,
        }
    }

    pub fn authorized_from(account: &Account) -> Option<Authorized> {
        Self::from(account).and_then(|state: Self| state.authorized())
    }

    pub fn authorized(&self) -> Option<Authorized> {
        match self {
            StakeState::Stake(meta, _stake) => Some(meta.authorized),
            StakeState::Initialized(meta) => Some(meta.authorized),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub enum StakeAuthorize {
    Staker,
    Withdrawer,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, or
    ///   changes to authorized staker or withdrawer, unless the
    ///   transaction is signed by the custodian
    pub unix_timestamp: UnixTimestamp,
    /// epoch height at which this stake will allow withdrawal, or
    ///   changes to authorized staker or withdrawer, unless the
    ///   transaction is signed by the custodian
    ///  to the custodian
    pub epoch: Epoch,
    /// custodian signature on a transaction exempts the operation from
    ///  lockup constraints
    pub custodian: Pubkey,
}

impl Lockup {
    pub fn is_in_force(&self, clock: &Clock, signers: &HashSet<Pubkey>) -> bool {
        (self.unix_timestamp > clock.unix_timestamp || self.epoch > clock.epoch)
            && !signers.contains(&self.custodian)
    }
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Authorized {
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Meta {
    pub rent_exempt_reserve: u64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

impl Meta {
    pub fn set_lockup(
        &mut self,
        lockup: &Lockup,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        if !signers.contains(&self.lockup.custodian) {
            return Err(InstructionError::MissingRequiredSignature);
        }
        self.lockup = *lockup;
        Ok(())
    }

    pub fn authorize(
        &mut self,
        authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        signers: &HashSet<Pubkey>,
        clock: &Clock,
    ) -> Result<(), InstructionError> {
        // verify that lockup has expired or that the authorization
        //  is *also* signed by the custodian
        if self.lockup.is_in_force(clock, signers) {
            return Err(StakeError::LockupInForce.into());
        }
        self.authorized
            .authorize(signers, authority, stake_authorize)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Delegation {
    /// to whom the stake is delegated
    pub voter_pubkey: Pubkey,
    /// activated stake amount, set at delegate() time
    pub stake: u64,
    /// epoch at which this stake was activated, std::Epoch::MAX if is a bootstrap stake
    pub activation_epoch: Epoch,
    /// epoch the stake was deactivated, std::Epoch::MAX if not deactivated
    pub deactivation_epoch: Epoch,
    /// how much stake we can activate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
}

impl Default for Delegation {
    fn default() -> Self {
        Self {
            voter_pubkey: Pubkey::default(),
            stake: 0,
            activation_epoch: 0,
            deactivation_epoch: std::u64::MAX,
            warmup_cooldown_rate: Config::default().warmup_cooldown_rate,
        }
    }
}

impl Delegation {
    pub fn new(
        voter_pubkey: &Pubkey,
        stake: u64,
        activation_epoch: Epoch,
        warmup_cooldown_rate: f64,
    ) -> Self {
        Self {
            voter_pubkey: *voter_pubkey,
            stake,
            activation_epoch,
            warmup_cooldown_rate,
            ..Delegation::default()
        }
    }
    pub fn is_bootstrap(&self) -> bool {
        self.activation_epoch == std::u64::MAX
    }

    pub fn stake(&self, epoch: Epoch, history: Option<&StakeHistory>) -> u64 {
        self.stake_activating_and_deactivating(epoch, history).0
    }

    #[allow(clippy::comparison_chain)]
    fn stake_activating_and_deactivating(
        &self,
        epoch: Epoch,
        history: Option<&StakeHistory>,
    ) -> (u64, u64, u64) {
        // first, calculate an effective stake and activating number
        let (stake, activating) = self.stake_and_activating(epoch, history);

        // then de-activate some portion if necessary
        if epoch < self.deactivation_epoch {
            (stake, activating, 0) // not deactivated
        } else if epoch == self.deactivation_epoch {
            (stake, 0, stake.min(self.stake)) // can only deactivate what's activated
        } else if let Some((history, mut entry)) = history.and_then(|history| {
            history
                .get(&self.deactivation_epoch)
                .map(|entry| (history, entry))
        }) {
            // && epoch > self.deactivation_epoch
            let mut effective_stake = stake;
            let mut next_epoch = self.deactivation_epoch;

            // loop from my activation epoch until the current epoch
            //   summing up my entitlement
            loop {
                if entry.deactivating == 0 {
                    break;
                }
                // I'm trying to get to zero, how much of the deactivation in stake
                //   this account is entitled to take
                let weight = effective_stake as f64 / entry.deactivating as f64;

                // portion of activating stake in this epoch I'm entitled to
                effective_stake = effective_stake.saturating_sub(
                    ((weight * entry.effective as f64 * self.warmup_cooldown_rate) as u64).max(1),
                );

                if effective_stake == 0 {
                    break;
                }

                next_epoch += 1;
                if next_epoch >= epoch {
                    break;
                }
                if let Some(next_entry) = history.get(&next_epoch) {
                    entry = next_entry;
                } else {
                    break;
                }
            }
            (effective_stake, 0, effective_stake)
        } else {
            // no history or I've dropped out of history, so  fully deactivated
            (0, 0, 0)
        }
    }

    fn stake_and_activating(&self, epoch: Epoch, history: Option<&StakeHistory>) -> (u64, u64) {
        if self.is_bootstrap() {
            (self.stake, 0)
        } else if epoch == self.activation_epoch {
            (0, self.stake)
        } else if epoch < self.activation_epoch {
            (0, 0)
        } else if let Some((history, mut entry)) = history.and_then(|history| {
            history
                .get(&self.activation_epoch)
                .map(|entry| (history, entry))
        }) {
            // && !is_bootstrap() && epoch > self.activation_epoch
            let mut effective_stake = 0;
            let mut next_epoch = self.activation_epoch;

            // loop from my activation epoch until the current epoch
            //   summing up my entitlement
            loop {
                if entry.activating == 0 {
                    break;
                }
                // how much of the growth in stake this account is
                //  entitled to take
                let weight = (self.stake - effective_stake) as f64 / entry.activating as f64;

                // portion of activating stake in this epoch I'm entitled to
                effective_stake +=
                    ((weight * entry.effective as f64 * self.warmup_cooldown_rate) as u64).max(1);

                if effective_stake >= self.stake {
                    effective_stake = self.stake;
                    break;
                }

                next_epoch += 1;
                if next_epoch >= epoch || next_epoch >= self.deactivation_epoch {
                    break;
                }
                if let Some(next_entry) = history.get(&next_epoch) {
                    entry = next_entry;
                } else {
                    break;
                }
            }
            (effective_stake, self.stake - effective_stake)
        } else {
            // no history or I've dropped out of history, so assume fully activated
            (self.stake, 0)
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Stake {
    pub delegation: Delegation,
    /// credits observed is credits from vote account state when delegated or redeemed
    pub credits_observed: u64,
}

impl Authorized {
    pub fn auto(authorized: &Pubkey) -> Self {
        Self {
            staker: *authorized,
            withdrawer: *authorized,
        }
    }
    pub fn check(
        &self,
        signers: &HashSet<Pubkey>,
        stake_authorize: StakeAuthorize,
    ) -> Result<(), InstructionError> {
        match stake_authorize {
            StakeAuthorize::Staker if signers.contains(&self.staker) => Ok(()),
            StakeAuthorize::Withdrawer if signers.contains(&self.withdrawer) => Ok(()),
            _ => Err(InstructionError::MissingRequiredSignature),
        }
    }

    pub fn authorize(
        &mut self,
        signers: &HashSet<Pubkey>,
        new_authorized: &Pubkey,
        stake_authorize: StakeAuthorize,
    ) -> Result<(), InstructionError> {
        self.check(signers, stake_authorize)?;
        match stake_authorize {
            StakeAuthorize::Staker => self.staker = *new_authorized,
            StakeAuthorize::Withdrawer => self.withdrawer = *new_authorized,
        }
        Ok(())
    }
}

impl Stake {
    pub fn stake(&self, epoch: Epoch, history: Option<&StakeHistory>) -> u64 {
        self.delegation.stake(epoch, history)
    }

    pub fn redeem_rewards(
        &mut self,
        point_value: f64,
        vote_state: &VoteState,
        stake_history: Option<&StakeHistory>,
    ) -> Option<(u64, u64)> {
        self.calculate_rewards(point_value, vote_state, stake_history)
            .map(|(voters_reward, stakers_reward, credits_observed)| {
                self.credits_observed = credits_observed;
                self.delegation.stake += stakers_reward;
                (voters_reward, stakers_reward)
            })
    }

    /// for a given stake and vote_state, calculate what distributions and what updates should be made
    /// returns a tuple in the case of a payout of:
    ///   * voter_rewards to be distributed
    ///   * staker_rewards to be distributed
    ///   * new value for credits_observed in the stake
    //  returns None if there's no payout or if any deserved payout is < 1 lamport
    pub fn calculate_rewards(
        &self,
        point_value: f64,
        vote_state: &VoteState,
        stake_history: Option<&StakeHistory>,
    ) -> Option<(u64, u64, u64)> {
        if self.credits_observed >= vote_state.credits() {
            return None;
        }

        let mut credits_observed = self.credits_observed;
        let mut total_rewards = 0f64;
        for (epoch, credits, prev_credits) in vote_state.epoch_credits() {
            // figure out how much this stake has seen that
            //   for which the vote account has a record
            let epoch_credits = if self.credits_observed < *prev_credits {
                // the staker observed the entire epoch
                credits - prev_credits
            } else if self.credits_observed < *credits {
                // the staker registered sometime during the epoch, partial credit
                credits - credits_observed
            } else {
                // the staker has already observed or been redeemed this epoch
                //  or was activated after this epoch
                0
            };

            total_rewards +=
                (self.delegation.stake(*epoch, stake_history) * epoch_credits) as f64 * point_value;

            // don't want to assume anything about order of the iterator...
            credits_observed = credits_observed.max(*credits);
        }
        // don't bother trying to collect fractional lamports
        if total_rewards < 1f64 {
            return None;
        }

        let (voter_rewards, staker_rewards, is_split) = vote_state.commission_split(total_rewards);

        if (voter_rewards < 1f64 || staker_rewards < 1f64) && is_split {
            // don't bother trying to collect fractional lamports
            return None;
        }

        Some((
            voter_rewards as u64,
            staker_rewards as u64,
            credits_observed,
        ))
    }

    fn redelegate(
        &mut self,
        voter_pubkey: &Pubkey,
        vote_state: &VoteState,
        clock: &Clock,
        stake_history: &StakeHistory,
        config: &Config,
    ) -> Result<(), StakeError> {
        // can't redelegate if stake is active.  either the stake
        //  is freshly activated or has fully de-activated.  redelegation
        //  implies re-activation
        if self.stake(clock.epoch, Some(stake_history)) != 0 {
            return Err(StakeError::TooSoonToRedelegate);
        }
        self.delegation.activation_epoch = clock.epoch;
        self.delegation.deactivation_epoch = std::u64::MAX;
        self.delegation.voter_pubkey = *voter_pubkey;
        self.delegation.warmup_cooldown_rate = config.warmup_cooldown_rate;
        self.credits_observed = vote_state.credits();
        Ok(())
    }

    fn split(&mut self, lamports: u64) -> Result<Self, StakeError> {
        if lamports > self.delegation.stake {
            return Err(StakeError::InsufficientStake);
        }
        self.delegation.stake -= lamports;
        let new = Self {
            delegation: Delegation {
                stake: lamports,
                ..self.delegation
            },
            ..*self
        };
        Ok(new)
    }

    fn new(
        stake: u64,
        voter_pubkey: &Pubkey,
        vote_state: &VoteState,
        activation_epoch: Epoch,
        config: &Config,
    ) -> Self {
        Self {
            delegation: Delegation::new(
                voter_pubkey,
                stake,
                activation_epoch,
                config.warmup_cooldown_rate,
            ),
            credits_observed: vote_state.credits(),
        }
    }

    fn deactivate(&mut self, epoch: Epoch) -> Result<(), StakeError> {
        if self.delegation.deactivation_epoch != std::u64::MAX {
            Err(StakeError::AlreadyDeactivated)
        } else {
            self.delegation.deactivation_epoch = epoch;
            Ok(())
        }
    }
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
        authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        signers: &HashSet<Pubkey>,
        clock: &Clock,
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
        lockup: &Lockup,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn split(
        &self,
        lamports: u64,
        split_stake: &KeyedAccount,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn withdraw(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn initialize(
        &self,
        authorized: &Authorized,
        lockup: &Lockup,
        rent: &Rent,
    ) -> Result<(), InstructionError> {
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
        authority: &Pubkey,
        stake_authorize: StakeAuthorize,
        signers: &HashSet<Pubkey>,
        clock: &Clock,
    ) -> Result<(), InstructionError> {
        match self.state()? {
            StakeState::Stake(mut meta, stake) => {
                meta.authorize(authority, stake_authorize, signers, clock)?;
                self.set_state(&StakeState::Stake(meta, stake))
            }
            StakeState::Initialized(mut meta) => {
                meta.authorize(authority, stake_authorize, signers, clock)?;
                self.set_state(&StakeState::Initialized(meta))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }
    fn delegate(
        &self,
        vote_account: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        config: &Config,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        match self.state()? {
            StakeState::Initialized(meta) => {
                meta.authorized.check(signers, StakeAuthorize::Staker)?;
                let stake = Stake::new(
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
                stake.redelegate(
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
        lockup: &Lockup,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        match self.state()? {
            StakeState::Initialized(mut meta) => {
                meta.set_lockup(lockup, signers)?;
                self.set_state(&StakeState::Initialized(meta))
            }
            StakeState::Stake(mut meta, stake) => {
                meta.set_lockup(lockup, signers)?;
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
        if let StakeState::Uninitialized = split.state()? {
            // verify enough account lamports
            if lamports > self.lamports()? {
                return Err(InstructionError::InsufficientFunds);
            }

            match self.state()? {
                StakeState::Stake(meta, mut stake) => {
                    meta.authorized.check(signers, StakeAuthorize::Staker)?;

                    // verify enough lamports for rent in new stake with the split
                    if split.lamports()? + lamports < meta.rent_exempt_reserve
                     // verify enough lamports left in previous stake and not full withdrawal
                        || (lamports + meta.rent_exempt_reserve > self.lamports()? && lamports != self.lamports()?)
                    {
                        return Err(InstructionError::InsufficientFunds);
                    }
                    // split the stake, subtract rent_exempt_balance unless
                    //  the destination account already has those lamports
                    //  in place.
                    // this could represent a small loss of staked lamports
                    //  if the split account starts out with a zero balance
                    let split_stake = stake.split(
                        lamports - meta.rent_exempt_reserve.saturating_sub(split.lamports()?),
                    )?;

                    self.set_state(&StakeState::Stake(meta, stake))?;
                    split.set_state(&StakeState::Stake(meta, split_stake))?;
                }
                StakeState::Initialized(meta) => {
                    meta.authorized.check(signers, StakeAuthorize::Staker)?;

                    // enough lamports for rent in new stake
                    if lamports < meta.rent_exempt_reserve
                    // verify enough lamports left in previous stake
                        || (lamports + meta.rent_exempt_reserve > self.lamports()? && lamports != self.lamports()?)
                    {
                        return Err(InstructionError::InsufficientFunds);
                    }

                    split.set_state(&StakeState::Initialized(meta))?;
                }
                StakeState::Uninitialized => {
                    if !signers.contains(&self.unsigned_key()) {
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                }
                _ => return Err(InstructionError::InvalidAccountData),
            }

            split.try_account_ref_mut()?.lamports += lamports;
            self.try_account_ref_mut()?.lamports -= lamports;
            Ok(())
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }

    fn withdraw(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        clock: &Clock,
        stake_history: &StakeHistory,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        let (lockup, reserve, is_staked) = match self.state()? {
            StakeState::Stake(meta, stake) => {
                meta.authorized.check(signers, StakeAuthorize::Withdrawer)?;
                // if we have a deactivation epoch and we're in cooldown
                let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                    stake.delegation.stake(clock.epoch, Some(stake_history))
                } else {
                    // Assume full stake if the stake account hasn't been
                    //  de-activated, because in the future the exposed stake
                    //  might be higher than stake.stake() due to warmup
                    stake.delegation.stake
                };

                (meta.lockup, staked + meta.rent_exempt_reserve, staked != 0)
            }
            StakeState::Initialized(meta) => {
                meta.authorized.check(signers, StakeAuthorize::Withdrawer)?;

                (meta.lockup, meta.rent_exempt_reserve, false)
            }
            StakeState::Uninitialized => {
                if !signers.contains(&self.unsigned_key()) {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                (Lockup::default(), 0, false) // no lockup, no restrictions
            }
            _ => return Err(InstructionError::InvalidAccountData),
        };

        // verify that lockup has expired or that the withdrawal is signed by
        //   the custodian, both epoch and unix_timestamp must have passed
        if lockup.is_in_force(&clock, signers) {
            return Err(StakeError::LockupInForce.into());
        }

        // if the stake is active, we mustn't allow the account to go away
        if is_staked // line coverage for branch coverage
            && lamports + reserve > self.lamports()?
        {
            return Err(InstructionError::InsufficientFunds);
        }

        if lamports != self.lamports()? // not a full withdrawal
            && lamports + reserve > self.lamports()?
        {
            assert!(!is_staked);
            return Err(InstructionError::InsufficientFunds);
        }

        self.try_account_ref_mut()?.lamports -= lamports;
        to.try_account_ref_mut()?.lamports += lamports;
        Ok(())
    }
}

// utility function, used by runtime
pub fn redeem_rewards(
    stake_account: &mut Account,
    vote_account: &mut Account,
    point_value: f64,
    stake_history: Option<&StakeHistory>,
) -> Result<(u64, u64), InstructionError> {
    if let StakeState::Stake(meta, mut stake) = stake_account.state()? {
        let vote_state: VoteState =
            StateMut::<VoteStateVersions>::state(vote_account)?.convert_to_current();

        if let Some((voters_reward, stakers_reward)) =
            stake.redeem_rewards(point_value, &vote_state, stake_history)
        {
            stake_account.lamports += stakers_reward;
            vote_account.lamports += voters_reward;

            stake_account.set_state(&StakeState::Stake(meta, stake))?;

            Ok((stakers_reward, voters_reward))
        } else {
            Err(StakeError::NoCreditsToRedeem.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

// utility function, used by runtime::Stakes, tests
pub fn new_stake_history_entry<'a, I>(
    epoch: Epoch,
    stakes: I,
    history: Option<&StakeHistory>,
) -> StakeHistoryEntry
where
    I: Iterator<Item = &'a Delegation>,
{
    // whatever the stake says they  had for the epoch
    //  and whatever the were still waiting for
    fn add(a: (u64, u64, u64), b: (u64, u64, u64)) -> (u64, u64, u64) {
        (a.0 + b.0, a.1 + b.1, a.2 + b.2)
    }
    let (effective, activating, deactivating) = stakes.fold((0, 0, 0), |sum, stake| {
        add(sum, stake.stake_activating_and_deactivating(epoch, history))
    });

    StakeHistoryEntry {
        effective,
        activating,
        deactivating,
    }
}

// genesis investor accounts
pub fn create_lockup_stake_account(
    authorized: &Authorized,
    lockup: &Lockup,
    rent: &Rent,
    lamports: u64,
) -> Account {
    let mut stake_account = Account::new(lamports, std::mem::size_of::<StakeState>(), &id());

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data.len());
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
    vote_account: &Account,
    rent: &Rent,
    lamports: u64,
) -> Account {
    let mut stake_account = Account::new(lamports, std::mem::size_of::<StakeState>(), &id());

    let vote_state = VoteState::from(vote_account).expect("vote_state");

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data.len());

    stake_account
        .set_state(&StakeState::Stake(
            Meta {
                authorized: Authorized::auto(authorized),
                rent_exempt_reserve,
                ..Meta::default()
            },
            Stake::new(
                lamports - rent_exempt_reserve, // underflow is an error, is basically: assert!(lamports > rent_exempt_reserve);
                voter_pubkey,
                &vote_state,
                std::u64::MAX,
                &Config::default(),
            ),
        ))
        .expect("set_state");

    stake_account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use solana_sdk::{account::Account, pubkey::Pubkey, system_program};
    use solana_vote_program::vote_state;
    use std::cell::RefCell;

    impl Meta {
        pub fn auto(authorized: &Pubkey) -> Self {
            Self {
                authorized: Authorized::auto(authorized),
                ..Meta::default()
            }
        }
    }

    #[test]
    fn test_meta_authorize() {
        let staker = Pubkey::new_rand();
        let custodian = Pubkey::new_rand();
        let mut meta = Meta {
            authorized: Authorized::auto(&staker),
            lockup: Lockup {
                epoch: 0,
                unix_timestamp: 0,
                custodian,
            },
            ..Meta::default()
        };
        // verify sig check
        let mut signers = HashSet::new();
        let mut clock = Clock::default();

        assert_eq!(
            meta.authorize(&staker, StakeAuthorize::Staker, &signers, &clock),
            Err(InstructionError::MissingRequiredSignature)
        );
        signers.insert(staker);
        assert_eq!(
            meta.authorize(&staker, StakeAuthorize::Staker, &signers, &clock),
            Ok(())
        );
        // verify lockup check
        meta.lockup.epoch = 1;
        assert_eq!(
            meta.authorize(&staker, StakeAuthorize::Staker, &signers, &clock),
            Err(StakeError::LockupInForce.into())
        );
        // verify lockup check defeated by custodian
        signers.insert(custodian);
        assert_eq!(
            meta.authorize(&staker, StakeAuthorize::Staker, &signers, &clock),
            Ok(())
        );
        // verify lock expiry
        signers.remove(&custodian);
        clock.epoch = 1;
        assert_eq!(
            meta.authorize(&staker, StakeAuthorize::Staker, &signers, &clock),
            Ok(())
        );
    }

    #[test]
    fn test_stake_state_stake_from_fail() {
        let mut stake_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());

        stake_account
            .set_state(&StakeState::default())
            .expect("set_state");

        assert_eq!(StakeState::stake_from(&stake_account), None);
    }

    #[test]
    fn test_stake_is_bootstrap() {
        assert_eq!(
            Delegation {
                activation_epoch: std::u64::MAX,
                ..Delegation::default()
            }
            .is_bootstrap(),
            true
        );
        assert_eq!(
            Delegation {
                activation_epoch: 0,
                ..Delegation::default()
            }
            .is_bootstrap(),
            false
        );
    }

    #[test]
    fn test_stake_delegate() {
        let mut clock = Clock {
            epoch: 1,
            ..Clock::default()
        };

        let vote_pubkey = Pubkey::new_rand();
        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_slot_vote_unchecked(i);
        }

        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        let vote_state_credits = vote_state.credits();
        vote_keyed_account
            .set_state(&VoteStateVersions::Current(Box::new(vote_state)))
            .unwrap();

        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
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
            )
            .is_ok());

        // verify that delegate() looks right, compare against hand-rolled
        let stake = StakeState::stake_from(&stake_keyed_account.account.borrow()).unwrap();
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
                ..Stake::default()
            }
        );

        clock.epoch += 1;

        // verify that delegate fails if stake is still active
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers
            ),
            Err(StakeError::TooSoonToRedelegate.into())
        );

        // deactivate, so we can re-delegate
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        // without stake history, cool down is instantaneous
        clock.epoch += 1;

        // verify that delegate can be called twice, 2nd is redelegate
        assert!(stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers
            )
            .is_ok());

        // verify that delegate() looks right, compare against hand-rolled
        let stake = StakeState::stake_from(&stake_keyed_account.account.borrow()).unwrap();
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
                ..Stake::default()
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
                &signers
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
        let increment = (1_000 as f64 * stake.warmup_cooldown_rate) as u64;

        let mut stake_history = StakeHistory::default();
        // assert that this stake follows step function if there's no history
        assert_eq!(
            stake.stake_activating_and_deactivating(stake.activation_epoch, Some(&stake_history)),
            (0, stake.stake, 0)
        );
        for epoch in stake.activation_epoch + 1..stake.deactivation_epoch {
            assert_eq!(
                stake.stake_activating_and_deactivating(epoch, Some(&stake_history)),
                (stake.stake, 0, 0)
            );
        }
        // assert that this stake is full deactivating
        assert_eq!(
            stake.stake_activating_and_deactivating(stake.deactivation_epoch, Some(&stake_history)),
            (stake.stake, 0, stake.stake)
        );
        // assert that this stake is fully deactivated if there's no history
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 1,
                Some(&stake_history)
            ),
            (0, 0, 0)
        );

        stake_history.add(
            0u64, // entry for zero doesn't have my activating amount
            StakeHistoryEntry {
                effective: 1_000,
                activating: 0,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(1, Some(&stake_history)),
            (0, stake.stake, 0)
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
            (increment, stake.stake - increment, 0)
        );

        // start over, test deactivation edge cases
        let mut stake_history = StakeHistory::default();

        stake_history.add(
            stake.deactivation_epoch, // entry for zero doesn't have my de-activating amount
            StakeHistoryEntry {
                effective: 1_000,
                activating: 0,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 1,
                Some(&stake_history)
            ),
            (stake.stake, 0, stake.stake) // says "I'm still waiting for deactivation"
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
                Some(&stake_history)
            ),
            (stake.stake - increment, 0, stake.stake - increment) // hung, should be lower
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

            if epoch < stake.deactivation_epoch {
                let increase = (effective as f64 * stake.warmup_cooldown_rate) as u64;
                effective += increase.min(activating);
                other_activations.push(0);
            } else {
                let decrease = (effective as f64 * stake.warmup_cooldown_rate) as u64;
                effective -= decrease.min(deactivating);
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
                (expected_stake, expected_activating, expected_deactivating)
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
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account =
            Account::new_ref(stake_lamports, std::mem::size_of::<StakeState>(), &id());

        // unsigned keyed account
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        let custodian = Pubkey::new_rand();

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
            StakeState::from(&stake_keyed_account.account.borrow()).unwrap(),
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
    fn test_deactivate() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
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
        let vote_pubkey = Pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::Current(Box::new(VoteState::default())))
            .unwrap();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers
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
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        // wrong state, should fail
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        assert_eq!(
            stake_keyed_account.set_lockup(&Lockup::default(), &HashSet::default(),),
            Err(InstructionError::InvalidAccountData)
        );

        // initalize the stake
        let custodian = Pubkey::new_rand();
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
            stake_keyed_account.set_lockup(&Lockup::default(), &HashSet::default(),),
            Err(InstructionError::MissingRequiredSignature)
        );

        assert_eq!(
            stake_keyed_account.set_lockup(
                &Lockup {
                    unix_timestamp: 1,
                    epoch: 1,
                    custodian,
                },
                &vec![custodian].into_iter().collect()
            ),
            Ok(())
        );

        // delegate stake
        let vote_pubkey = Pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::Current(Box::new(VoteState::default())))
            .unwrap();

        stake_keyed_account
            .delegate(
                &vote_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &Config::default(),
                &vec![stake_pubkey].into_iter().collect(),
            )
            .unwrap();

        assert_eq!(
            stake_keyed_account.set_lockup(
                &Lockup {
                    unix_timestamp: 1,
                    epoch: 1,
                    custodian,
                },
                &HashSet::default(),
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
        assert_eq!(
            stake_keyed_account.set_lockup(
                &Lockup {
                    unix_timestamp: 1,
                    epoch: 1,
                    custodian,
                },
                &vec![custodian].into_iter().collect()
            ),
            Ok(())
        );
    }

    #[test]
    fn test_withdraw_stake() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let mut clock = Clock::default();

        let to = Pubkey::new_rand();
        let to_account = Account::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        // no signers, should fail
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &HashSet::default(),
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account and uninitialized should work
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers,
            ),
            Ok(())
        );
        assert_eq!(stake_account.borrow().lamports, 0);

        // reset balance
        stake_account.borrow_mut().lamports = stake_lamports;

        // lockup
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let custodian = Pubkey::new_rand();
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
                &signers,
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Stake some lamports (available lamports for withdrawals will reduce to zero)
        let vote_pubkey = Pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::Current(Box::new(VoteState::default())))
            .unwrap();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &clock,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
            ),
            Ok(())
        );

        // simulate rewards
        stake_account.borrow_mut().lamports += 10;
        // withdrawal before deactivate works for rewards amount
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                10,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers,
            ),
            Ok(())
        );

        // simulate rewards
        stake_account.borrow_mut().lamports += 10;
        // withdrawal of rewards fails if not in excess of stake
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                10 + 1,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers
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
                &signers
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
                &signers
            ),
            Ok(())
        );
        assert_eq!(stake_account.borrow().lamports, 0);
    }

    #[test]
    fn test_withdraw_stake_before_warmup() {
        let stake_pubkey = Pubkey::new_rand();
        let total_lamports = 100;
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            total_lamports,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let clock = Clock::default();
        let mut future = Clock::default();
        future.epoch += 16;

        let to = Pubkey::new_rand();
        let to_account = Account::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        // Stake some lamports (available lamports for withdrawals will reduce)
        let vote_pubkey = Pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_rand(),
            0,
            100,
        ));
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &vote_account);
        vote_keyed_account
            .set_state(&VoteStateVersions::Current(Box::new(VoteState::default())))
            .unwrap();
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.delegate(
                &vote_keyed_account,
                &future,
                &StakeHistory::default(),
                &Config::default(),
                &signers,
            ),
            Ok(())
        );

        let stake_history = create_stake_history_from_delegations(
            None,
            0..future.epoch,
            &[
                StakeState::stake_from(&stake_keyed_account.account.borrow())
                    .unwrap()
                    .delegation,
            ],
        );

        // Try to withdraw stake
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports + 1,
                &to_keyed_account,
                &clock,
                &stake_history,
                &signers,
            ),
            Err(InstructionError::InsufficientFunds)
        );
    }

    #[test]
    fn test_withdraw_stake_invalid_state() {
        let stake_pubkey = Pubkey::new_rand();
        let total_lamports = 100;
        let stake_account = Account::new_ref_data_with_space(
            total_lamports,
            &StakeState::RewardsPool,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = Pubkey::new_rand();
        let to_account = Account::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &Clock::default(),
                &StakeHistory::default(),
                &signers,
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_withdraw_lockup() {
        let stake_pubkey = Pubkey::new_rand();
        let custodian = Pubkey::new_rand();
        let total_lamports = 100;
        let stake_account = Account::new_ref_data_with_space(
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

        let to = Pubkey::new_rand();
        let to_account = Account::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        let mut clock = Clock::default();

        let signers = vec![stake_pubkey].into_iter().collect();

        // lockup is still in force, can't withdraw
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers,
            ),
            Err(StakeError::LockupInForce.into())
        );

        {
            let mut signers_with_custodian = signers.clone();
            signers_with_custodian.insert(custodian);
            assert_eq!(
                stake_keyed_account.withdraw(
                    total_lamports,
                    &to_keyed_account,
                    &clock,
                    &StakeHistory::default(),
                    &signers_with_custodian,
                ),
                Ok(())
            );
        }
        // reset balance
        stake_keyed_account.account.borrow_mut().lamports = total_lamports;

        // lockup has expired
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        clock.epoch += 1;
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers,
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
        let mut stake = Stake::new(
            stake_lamports,
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
            &Config::default(),
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            stake.redeem_rewards(1_000_000_000.0, &vote_state, None)
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0);
        vote_state.increment_credits(0);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some((0, stake_lamports * 2)),
            stake.redeem_rewards(1.0, &vote_state, None)
        );

        assert_eq!(
            stake.delegation.stake,
            stake_lamports + (stake_lamports * 2)
        );
        assert_eq!(stake.credits_observed, 2);
    }

    #[test]
    fn test_stake_state_calculate_rewards() {
        let mut vote_state = VoteState::default();
        // assume stake.stake() is right
        // bootstrap means fully-vested stake at epoch 0
        let mut stake = Stake::new(
            1,
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
            &Config::default(),
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            stake.calculate_rewards(1_000_000_000.0, &vote_state, None)
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0);
        vote_state.increment_credits(0);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some((0, stake.delegation.stake * 2, 2)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        stake.credits_observed = 1;
        // this one should be able to collect exactly 1 (already observed one)
        assert_eq!(
            Some((0, stake.delegation.stake * 1, 2)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        // put 1 credit in epoch 1
        vote_state.increment_credits(1);

        stake.credits_observed = 2;
        // this one should be able to collect the one just added
        assert_eq!(
            Some((0, stake.delegation.stake * 1, 3)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        // put 1 credit in epoch 2
        vote_state.increment_credits(2);
        // this one should be able to collect 2 now
        assert_eq!(
            Some((0, stake.delegation.stake * 2, 4)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        stake.credits_observed = 0;
        // this one should be able to collect everything from t=0 a warmed up stake of 2
        // (2 credits at stake of 1) + (1 credit at a stake of 2)
        assert_eq!(
            Some((
                0,
                stake.delegation.stake * 2 // epoch 0
                    + stake.delegation.stake * 1 // epoch 1
                    + stake.delegation.stake * 1, // epoch 2
                4
            )),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        // same as above, but is a really small commission out of 32 bits,
        //  verify that None comes back on small redemptions where no one gets paid
        vote_state.commission = 1;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 4)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );
        vote_state.commission = 99;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 4)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );
    }

    #[test]
    fn test_authorize_uninit() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::default(),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &stake_pubkey,
                StakeAuthorize::Staker,
                &signers,
                &Clock::default()
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_authorize_lockup() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let to = Pubkey::new_rand();
        let to_account = Account::new_ref(1, 0, &system_program::id());
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);

        let clock = Clock::default();
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

        let stake_pubkey0 = Pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();
        assert_eq!(
            stake_keyed_account.authorize(
                &stake_pubkey0,
                StakeAuthorize::Staker,
                &signers,
                &Clock::default()
            ),
            Ok(())
        );
        assert_eq!(
            stake_keyed_account.authorize(
                &stake_pubkey0,
                StakeAuthorize::Withdrawer,
                &signers,
                &Clock::default()
            ),
            Ok(())
        );
        if let StakeState::Initialized(Meta { authorized, .. }) =
            StakeState::from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(authorized.staker, stake_pubkey0);
            assert_eq!(authorized.withdrawer, stake_pubkey0);
        } else {
            assert!(false);
        }

        // A second authorization signed by the stake_keyed_account should fail
        let stake_pubkey1 = Pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &stake_pubkey1,
                StakeAuthorize::Staker,
                &signers,
                &Clock::default()
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        let signers0 = vec![stake_pubkey0].into_iter().collect();

        // Test a second authorization by the newly authorized pubkey
        let stake_pubkey2 = Pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &stake_pubkey2,
                StakeAuthorize::Staker,
                &signers0,
                &Clock::default()
            ),
            Ok(())
        );
        if let StakeState::Initialized(Meta { authorized, .. }) =
            StakeState::from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(authorized.staker, stake_pubkey2);
        }

        assert_eq!(
            stake_keyed_account.authorize(
                &stake_pubkey2,
                StakeAuthorize::Withdrawer,
                &signers0,
                &Clock::default()
            ),
            Ok(())
        );
        if let StakeState::Initialized(Meta { authorized, .. }) =
            StakeState::from(&stake_keyed_account.account.borrow()).unwrap()
        {
            assert_eq!(authorized.staker, stake_pubkey2);
        }

        let signers2 = vec![stake_pubkey2].into_iter().collect();

        // Test that withdrawal to account fails without authorized withdrawer
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers, // old signer
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // Test a successful action by the currently authorized withdrawer
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                stake_lamports,
                &to_keyed_account,
                &clock,
                &StakeHistory::default(),
                &signers2,
            ),
            Ok(())
        );
    }

    #[test]
    fn test_split_source_uninitialized() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let split_stake_pubkey = Pubkey::new_rand();
        let split_stake_account = Account::new_ref_data_with_space(
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
            stake_keyed_account.account.borrow().lamports,
            split_stake_keyed_account.account.borrow().lamports
        );
    }

    #[test]
    fn test_split_split_not_uninitialized() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(Meta::auto(&stake_pubkey), Stake::just_stake(stake_lamports)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let split_stake_pubkey = Pubkey::new_rand();
        let split_stake_account = Account::new_ref_data_with_space(
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
    impl Stake {
        fn just_stake(stake: u64) -> Self {
            Self {
                delegation: Delegation {
                    stake,
                    ..Delegation::default()
                },
                ..Stake::default()
            }
        }
    }

    #[test]
    fn test_split_more_than_staked() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Stake(
                Meta::auto(&stake_pubkey),
                Stake::just_stake(stake_lamports / 2 - 1),
            ),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let split_stake_pubkey = Pubkey::new_rand();
        let split_stake_account = Account::new_ref_data_with_space(
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
        let stake_pubkey = Pubkey::new_rand();
        let split_stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let rent_exempt_reserve = 10;
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(meta),
            StakeState::Stake(
                meta,
                Stake::just_stake(stake_lamports - rent_exempt_reserve),
            ),
        ] {
            let stake_account = Account::new_ref_data_with_space(
                stake_lamports,
                state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &stake_account);

            let split_stake_account = Account::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            // not enough to make a stake account
            assert_eq!(
                stake_keyed_account.split(
                    rent_exempt_reserve - 1,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // doesn't leave enough for initial stake
            assert_eq!(
                stake_keyed_account.split(
                    (stake_lamports - rent_exempt_reserve) + 1,
                    &split_stake_keyed_account,
                    &signers
                ),
                Err(InstructionError::InsufficientFunds)
            );

            // split account already has way enough lamports
            split_stake_keyed_account.account.borrow_mut().lamports = 1_000;
            assert_eq!(
                stake_keyed_account.split(
                    stake_lamports - rent_exempt_reserve,
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
                                stake: stake_lamports - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..*stake
                        }
                    ))
                );
                assert_eq!(
                    stake_keyed_account.account.borrow().lamports,
                    rent_exempt_reserve
                );
                assert_eq!(
                    split_stake_keyed_account.account.borrow().lamports,
                    1_000 + stake_lamports - rent_exempt_reserve
                );
            }
        }
    }

    #[test]
    fn test_split() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;

        let split_stake_pubkey = Pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(Meta::auto(&stake_pubkey)),
            StakeState::Stake(Meta::auto(&stake_pubkey), Stake::just_stake(stake_lamports)),
        ] {
            let split_stake_account = Account::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = Account::new_ref_data_with_space(
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
                stake_keyed_account.account.borrow().lamports
                    + split_stake_keyed_account.account.borrow().lamports,
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
            stake_keyed_account.account.borrow_mut().lamports = stake_lamports;
        }
    }

    #[test]
    fn test_split_100_percent_of_source() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let rent_exempt_reserve = 10;

        let split_stake_pubkey = Pubkey::new_rand();
        let signers = vec![stake_pubkey].into_iter().collect();

        let meta = Meta {
            authorized: Authorized::auto(&stake_pubkey),
            rent_exempt_reserve,
            ..Meta::default()
        };

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(meta),
            StakeState::Stake(
                meta,
                Stake::just_stake(stake_lamports - rent_exempt_reserve),
            ),
        ] {
            let split_stake_account = Account::new_ref_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .expect("stake_account");

            let split_stake_keyed_account =
                KeyedAccount::new(&split_stake_pubkey, true, &split_stake_account);

            let stake_account = Account::new_ref_data_with_space(
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
                stake_keyed_account.account.borrow().lamports
                    + split_stake_keyed_account.account.borrow().lamports,
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
                                    stake: stake_lamports - rent_exempt_reserve,
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
                                    stake: 0,
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
            stake_keyed_account.account.borrow_mut().lamports = stake_lamports;
        }
    }

    #[test]
    fn test_lockup_is_expired() {
        let custodian = Pubkey::new_rand();
        let signers = [custodian].iter().cloned().collect::<HashSet<_>>();
        let lockup = Lockup {
            epoch: 1,
            unix_timestamp: 1,
            custodian,
        };
        // neither time
        assert_eq!(
            lockup.is_in_force(
                &Clock {
                    epoch: 0,
                    unix_timestamp: 0,
                    ..Clock::default()
                },
                &HashSet::new()
            ),
            true
        );
        // not timestamp
        assert_eq!(
            lockup.is_in_force(
                &Clock {
                    epoch: 2,
                    unix_timestamp: 0,
                    ..Clock::default()
                },
                &HashSet::new()
            ),
            true
        );
        // not epoch
        assert_eq!(
            lockup.is_in_force(
                &Clock {
                    epoch: 0,
                    unix_timestamp: 2,
                    ..Clock::default()
                },
                &HashSet::new()
            ),
            true
        );
        // both, no custodian
        assert_eq!(
            lockup.is_in_force(
                &Clock {
                    epoch: 1,
                    unix_timestamp: 1,
                    ..Clock::default()
                },
                &HashSet::new()
            ),
            false
        );
        // neither, but custodian
        assert_eq!(
            lockup.is_in_force(
                &Clock {
                    epoch: 0,
                    unix_timestamp: 0,
                    ..Clock::default()
                },
                &signers,
            ),
            false,
        );
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
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = Account::new_ref_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_pubkey)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .expect("stake_account");

        let clock = Clock::default();

        let vote_pubkey = Pubkey::new_rand();
        let vote_account = RefCell::new(vote_state::create_account(
            &vote_pubkey,
            &Pubkey::new_rand(),
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
            )
            .unwrap();

        // deactivate, so we can re-delegate
        stake_keyed_account.deactivate(&clock, &signers).unwrap();

        let new_staker_pubkey = Pubkey::new_rand();
        assert_eq!(
            stake_keyed_account.authorize(
                &new_staker_pubkey,
                StakeAuthorize::Staker,
                &signers,
                &clock,
            ),
            Ok(())
        );
        let authorized =
            StakeState::authorized_from(&stake_keyed_account.try_account_ref().unwrap()).unwrap();
        assert_eq!(authorized.staker, new_staker_pubkey);

        let other_pubkey = Pubkey::new_rand();
        let other_signers = vec![other_pubkey].into_iter().collect();

        // Use unsigned stake_keyed_account to test other signers
        let stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &stake_account);

        let new_voter_pubkey = Pubkey::new_rand();
        let vote_state = VoteState::default();
        let new_vote_account = RefCell::new(vote_state::create_account(
            &new_voter_pubkey,
            &Pubkey::new_rand(),
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
                &new_signers
            ),
            Ok(())
        );
        let stake =
            StakeState::stake_from(&stake_keyed_account.try_account_ref().unwrap()).unwrap();
        assert_eq!(stake.delegation.voter_pubkey, new_voter_pubkey);

        // Test another staking action
        assert_eq!(stake_keyed_account.deactivate(&clock, &new_signers), Ok(()));
    }
}
