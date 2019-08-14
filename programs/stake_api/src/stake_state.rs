//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

use crate::{config::Config, id};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::{Account, KeyedAccount},
    account_utils::State,
    instruction::InstructionError,
    pubkey::Pubkey,
    sysvar::{
        self,
        stake_history::{StakeHistory, StakeHistoryEntry},
    },
    timing::Epoch,
};
use solana_vote_api::vote_state::VoteState;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum StakeState {
    Uninitialized,
    Stake(Stake),
    RewardsPool,
}

impl Default for StakeState {
    fn default() -> Self {
        StakeState::Uninitialized
    }
}

impl StakeState {
    // utility function, used by Stakes, tests
    pub fn from(account: &Account) -> Option<StakeState> {
        account.state().ok()
    }

    pub fn stake_from(account: &Account) -> Option<Stake> {
        Self::from(account).and_then(|state: Self| state.stake())
    }

    pub fn stake(&self) -> Option<Stake> {
        match self {
            StakeState::Stake(stake) => Some(stake.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Stake {
    pub voter_pubkey: Pubkey,
    pub credits_observed: u64,
    pub stake: u64,         // stake amount activated
    pub activated: Epoch, // epoch the stake was activated, std::Epoch::MAX if is a bootstrap stake
    pub deactivated: Epoch, // epoch the stake was deactivated, std::Epoch::MAX if not deactivated
    pub config: Config,
}

impl Default for Stake {
    fn default() -> Self {
        Self {
            voter_pubkey: Pubkey::default(),
            credits_observed: 0,
            stake: 0,
            activated: 0,
            deactivated: std::u64::MAX,
            config: Config::default(),
        }
    }
}

impl Stake {
    pub fn is_bootstrap(&self) -> bool {
        self.activated == std::u64::MAX
    }

    pub fn activating(&self, epoch: Epoch, history: Option<&StakeHistory>) -> u64 {
        self.stake_and_activating(epoch, history).1
    }

    pub fn stake(&self, epoch: Epoch, history: Option<&StakeHistory>) -> u64 {
        self.stake_and_activating(epoch, history).0
    }

    pub fn stake_and_activating(&self, epoch: Epoch, history: Option<&StakeHistory>) -> (u64, u64) {
        if epoch >= self.deactivated {
            (0, 0) // TODO cooldown
        } else if self.is_bootstrap() {
            (self.stake, 0)
        } else if epoch > self.activated {
            if let Some(history) = history {
                if let Some(mut entry) = history.get(&self.activated) {
                    let mut effective_stake = 0;
                    let mut next_epoch = self.activated;

                    // loop from my activation epoch until the current epoch
                    //   summing up my entitlement
                    loop {
                        if entry.activating == 0 {
                            break;
                        }
                        // how much of the growth in stake this account is
                        //  entitled to take
                        let weight =
                            (self.stake - effective_stake) as f64 / entry.activating as f64;

                        // portion of activating stake in this epoch I'm entitled to
                        effective_stake +=
                            (weight * entry.effective as f64 * self.config.warmup_rate) as u64;

                        if effective_stake >= self.stake {
                            effective_stake = self.stake;
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
                    (effective_stake, self.stake - effective_stake)
                } else {
                    // I've dropped out of warmup history, so my stake must be the full amount
                    (self.stake, 0)
                }
            } else {
                // no history, fully warmed up
                (self.stake, 0)
            }
        } else if epoch == self.activated {
            (0, self.stake)
        } else {
            (0, 0)
        }
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
                // the staker has already observed/redeemed this epoch, or activated
                //  after this epoch
                0
            };

            total_rewards +=
                (self.stake(*epoch, stake_history) * epoch_credits) as f64 * point_value;

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

    fn new_bootstrap(stake: u64, voter_pubkey: &Pubkey, vote_state: &VoteState) -> Self {
        Self {
            stake,
            activated: std::u64::MAX,
            voter_pubkey: *voter_pubkey,
            credits_observed: vote_state.credits(),
            ..Stake::default()
        }
    }

    fn new(
        stake: u64,
        voter_pubkey: &Pubkey,
        vote_state: &VoteState,
        activated: Epoch,
        config: &Config,
    ) -> Self {
        Self {
            stake,
            activated,
            voter_pubkey: *voter_pubkey,
            credits_observed: vote_state.credits(),
            config: *config,
            ..Stake::default()
        }
    }

    fn deactivate(&mut self, epoch: u64) {
        self.deactivated = epoch;
    }
}

pub trait StakeAccount {
    fn delegate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        stake: u64,
        clock: &sysvar::clock::Clock,
        config: &Config,
    ) -> Result<(), InstructionError>;
    fn deactivate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        clock: &sysvar::clock::Clock,
    ) -> Result<(), InstructionError>;
    fn redeem_vote_credits(
        &mut self,
        vote_account: &mut KeyedAccount,
        rewards_account: &mut KeyedAccount,
        rewards: &sysvar::rewards::Rewards,
        stake_history: &sysvar::stake_history::StakeHistory,
    ) -> Result<(), InstructionError>;
    fn withdraw(
        &mut self,
        lamports: u64,
        to: &mut KeyedAccount,
        clock: &sysvar::clock::Clock,
        stake_history: &sysvar::stake_history::StakeHistory,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn delegate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        new_stake: u64,
        clock: &sysvar::clock::Clock,
        config: &Config,
    ) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        if new_stake > self.account.lamports {
            return Err(InstructionError::InsufficientFunds);
        }

        if let StakeState::Uninitialized = self.state()? {
            let stake = Stake::new(
                new_stake,
                vote_account.unsigned_key(),
                &vote_account.state()?,
                clock.epoch,
                config,
            );

            self.set_state(&StakeState::Stake(stake))
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn deactivate_stake(
        &mut self,
        _vote_account: &KeyedAccount, // TODO: used in slashing
        clock: &sysvar::clock::Clock,
    ) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        if let StakeState::Stake(mut stake) = self.state()? {
            stake.deactivate(clock.epoch);

            self.set_state(&StakeState::Stake(stake))
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn redeem_vote_credits(
        &mut self,
        vote_account: &mut KeyedAccount,
        rewards_account: &mut KeyedAccount,
        rewards: &sysvar::rewards::Rewards,
        stake_history: &sysvar::stake_history::StakeHistory,
    ) -> Result<(), InstructionError> {
        if let (StakeState::Stake(mut stake), StakeState::RewardsPool) =
            (self.state()?, rewards_account.state()?)
        {
            let vote_state: VoteState = vote_account.state()?;

            if stake.voter_pubkey != *vote_account.unsigned_key() {
                return Err(InstructionError::InvalidArgument);
            }

            if let Some((stakers_reward, voters_reward, credits_observed)) = stake
                .calculate_rewards(
                    rewards.validator_point_value,
                    &vote_state,
                    Some(stake_history),
                )
            {
                if rewards_account.account.lamports < (stakers_reward + voters_reward) {
                    return Err(InstructionError::UnbalancedInstruction);
                }
                rewards_account.account.lamports -= stakers_reward + voters_reward;

                self.account.lamports += stakers_reward;
                vote_account.account.lamports += voters_reward;

                stake.credits_observed = credits_observed;

                self.set_state(&StakeState::Stake(stake))
            } else {
                // not worth collecting
                Err(InstructionError::CustomError(1))
            }
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn withdraw(
        &mut self,
        lamports: u64,
        to: &mut KeyedAccount,
        clock: &sysvar::clock::Clock,
        stake_history: &sysvar::stake_history::StakeHistory,
    ) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        match self.state()? {
            StakeState::Stake(stake) => {
                // if deactivated and in cooldown
                let staked = if clock.epoch >= stake.deactivated {
                    stake.stake(clock.epoch, Some(stake_history))
                } else {
                    // Assume full stake if the stake is under warmup, or
                    //  hasn't been de-activated
                    stake.stake
                };
                if lamports > self.account.lamports.saturating_sub(staked) {
                    return Err(InstructionError::InsufficientFunds);
                }
                self.account.lamports -= lamports;
                to.account.lamports += lamports;
                Ok(())
            }
            StakeState::Uninitialized => {
                if lamports > self.account.lamports {
                    return Err(InstructionError::InsufficientFunds);
                }
                self.account.lamports -= lamports;
                to.account.lamports += lamports;
                Ok(())
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }
}

// utility function, used by runtime::Stakes, tests
pub fn new_stake_history_entry<'a, I>(
    epoch: Epoch,
    stakes: I,
    history: Option<&StakeHistory>,
) -> StakeHistoryEntry
where
    I: Iterator<Item = &'a Stake>,
{
    // whatever the stake says they  had for the epoch
    //  and whatever the were still waiting for
    let (effective, activating): (Vec<_>, Vec<_>) = stakes
        .map(|stake| stake.stake_and_activating(epoch, history))
        .unzip();

    let effective = effective.iter().sum();
    let activating = activating.iter().sum();

    StakeHistoryEntry {
        effective,
        activating,
        ..StakeHistoryEntry::default()
    }
}

// utility function, used by Bank, tests, genesis
pub fn create_stake_account(
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    lamports: u64,
) -> Account {
    let mut stake_account = Account::new(lamports, std::mem::size_of::<StakeState>(), &id());

    stake_account
        .set_state(&StakeState::Stake(Stake::new_bootstrap(
            lamports,
            voter_pubkey,
            vote_state,
        )))
        .expect("set_state");

    stake_account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_program;
    use solana_vote_api::vote_state;

    fn create_stake_history_from_stakes(
        bootstrap: Option<u64>,
        epochs: std::ops::Range<Epoch>,
        stakes: &[Stake],
    ) -> StakeHistory {
        let mut stake_history = StakeHistory::default();

        let bootstrap_stake = if let Some(bootstrap) = bootstrap {
            vec![Stake {
                activated: std::u64::MAX,
                stake: bootstrap,
                ..Stake::default()
            }]
        } else {
            vec![]
        };

        for epoch in epochs {
            let entry = new_stake_history_entry(
                epoch,
                stakes.iter().chain(bootstrap_stake.iter()),
                Some(&stake_history),
            );
            stake_history.add(epoch, entry);
        }

        stake_history
    }

    #[test]
    fn test_stake_delegate_stake() {
        let clock = sysvar::clock::Clock {
            epoch: 1,
            ..sysvar::clock::Clock::default()
        };

        let vote_keypair = Keypair::new();
        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_slot_vote_unchecked(i);
        }

        let vote_pubkey = vote_keypair.pubkey();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&vote_state).unwrap();

        let stake_pubkey = Pubkey::default();
        let stake_lamports = 42;
        let mut stake_account =
            Account::new(stake_lamports, std::mem::size_of::<StakeState>(), &id());

        // unsigned keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &mut stake_account);

        {
            let stake_state: StakeState = stake_keyed_account.state().unwrap();
            assert_eq!(stake_state, StakeState::default());
        }

        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, 0, &clock, &Config::default()),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert!(stake_keyed_account
            .delegate_stake(
                &vote_keyed_account,
                stake_lamports,
                &clock,
                &Config::default()
            )
            .is_ok());

        // verify that delegate_stake() looks right, compare against hand-rolled
        let stake_state: StakeState = stake_keyed_account.state().unwrap();
        assert_eq!(
            stake_state,
            StakeState::Stake(Stake {
                voter_pubkey: vote_keypair.pubkey(),
                credits_observed: vote_state.credits(),
                stake: stake_lamports,
                activated: clock.epoch,
                deactivated: std::u64::MAX,
                config: Config::default()
            })
        );
        // verify that delegate_stake can't be called twice StakeState::default()
        // signed keyed account
        assert_eq!(
            stake_keyed_account.delegate_stake(
                &vote_keyed_account,
                stake_lamports,
                &clock,
                &Config::default()
            ),
            Err(InstructionError::InvalidAccountData)
        );

        // verify can only stake up to account lamports
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.delegate_stake(
                &vote_keyed_account,
                stake_lamports + 1,
                &clock,
                &Config::default()
            ),
            Err(InstructionError::InsufficientFunds)
        );

        let stake_state = StakeState::RewardsPool;

        stake_keyed_account.set_state(&stake_state).unwrap();
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, 0, &clock, &Config::default())
            .is_err());
    }

    #[test]
    fn test_stake_warmup() {
        let stakes = [
            Stake {
                stake: 1_000,
                activated: std::u64::MAX,
                ..Stake::default()
            },
            Stake {
                stake: 1_000,
                activated: 0,
                ..Stake::default()
            },
            Stake {
                stake: 1_000,
                activated: 1,
                ..Stake::default()
            },
            Stake {
                stake: 1_000,
                activated: 2,
                ..Stake::default()
            },
            Stake {
                stake: 1_000,
                activated: 2,
                ..Stake::default()
            },
            Stake {
                stake: 1_000,
                activated: 4,
                ..Stake::default()
            },
        ];
        // chosen to ensure that the last activated stake (at 4) finishes warming up
        //  a stake takes 2.0f64.log(1.0 + STAKE_WARMUP_RATE)  epochs to warm up
        //  all else equal, but the above overlap
        let epochs = 20;

        let stake_history = create_stake_history_from_stakes(None, 0..epochs, &stakes);

        let mut prev_total_effective_stake = stakes
            .iter()
            .map(|stake| stake.stake(0, Some(&stake_history)))
            .sum::<u64>();

        for epoch in 1.. {
            let total_effective_stake = stakes
                .iter()
                .map(|stake| stake.stake(epoch, Some(&stake_history)))
                .sum::<u64>();

            let delta = total_effective_stake - prev_total_effective_stake;

            if delta == 0 {
                break;
            }
            assert!(epoch < epochs); // should have warmed everything up by this time
            assert!(
                delta as f64 / prev_total_effective_stake as f64 <= Config::default().warmup_rate
            );
            prev_total_effective_stake = total_effective_stake;
        }
    }

    #[test]
    fn test_deactivate_stake() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let mut stake_account =
            Account::new(stake_lamports, std::mem::size_of::<StakeState>(), &id());

        let clock = sysvar::clock::Clock {
            epoch: 1,
            ..sysvar::clock::Clock::default()
        };

        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);

        // unsigned keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &mut stake_account);
        assert_eq!(
            stake_keyed_account.deactivate_stake(&vote_keyed_account, &clock),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account but not staked yet
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.deactivate_stake(&vote_keyed_account, &clock),
            Err(InstructionError::InvalidAccountData)
        );

        // Staking
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&VoteState::default()).unwrap();
        assert_eq!(
            stake_keyed_account.delegate_stake(
                &vote_keyed_account,
                stake_lamports,
                &clock,
                &Config::default()
            ),
            Ok(())
        );

        // Deactivate after staking
        assert_eq!(
            stake_keyed_account.deactivate_stake(&vote_keyed_account, &clock),
            Ok(())
        );
    }

    #[test]
    fn test_withdraw_stake() {
        let stake_pubkey = Pubkey::new_rand();
        let total_lamports = 100;
        let stake_lamports = 42;
        let mut stake_account =
            Account::new(total_lamports, std::mem::size_of::<StakeState>(), &id());

        let mut clock = sysvar::clock::Clock::default();

        let to = Pubkey::new_rand();
        let mut to_account = Account::new(1, 0, &system_program::id());
        let mut to_keyed_account = KeyedAccount::new(&to, false, &mut to_account);

        // unsigned keyed account should fail
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account and uninitialized should work
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Ok(())
        );
        assert_eq!(stake_account.lamports, 0);

        // reset balance
        stake_account.lamports = total_lamports;

        // signed keyed account and uninitialized, more than available should fail
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports + 1,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Stake some lamports (available lampoorts for withdrawals will reduce)
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&VoteState::default()).unwrap();
        assert_eq!(
            stake_keyed_account.delegate_stake(
                &vote_keyed_account,
                stake_lamports,
                &clock,
                &Config::default()
            ),
            Ok(())
        );

        // withdrawal before deactivate works for some portion
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Ok(())
        );
        // reset
        stake_account.lamports = total_lamports;

        // withdrawal before deactivate fails if not in excess of stake
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports + 1,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // deactivate the stake before withdrawal
        assert_eq!(
            stake_keyed_account.deactivate_stake(&vote_keyed_account, &clock),
            Ok(())
        );
        // simulate time passing
        clock.epoch += 100;

        // Try to withdraw more than what's available
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports + 1,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Try to withdraw all lamports
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Ok(())
        );
        assert_eq!(stake_account.lamports, 0);
    }

    #[test]
    fn test_withdraw_stake_before_warmup() {
        let stake_pubkey = Pubkey::new_rand();
        let total_lamports = 100;
        let stake_lamports = 42;
        let mut stake_account =
            Account::new(total_lamports, std::mem::size_of::<StakeState>(), &id());

        let clock = sysvar::clock::Clock::default();
        let mut future = sysvar::clock::Clock::default();
        future.epoch += 16;

        let to = Pubkey::new_rand();
        let mut to_account = Account::new(1, 0, &system_program::id());
        let mut to_keyed_account = KeyedAccount::new(&to, false, &mut to_account);

        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);

        // Stake some lamports (available lampoorts for withdrawals will reduce)
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&VoteState::default()).unwrap();
        assert_eq!(
            stake_keyed_account.delegate_stake(
                &vote_keyed_account,
                stake_lamports,
                &future,
                &Config::default()
            ),
            Ok(())
        );

        let stake_history = create_stake_history_from_stakes(
            None,
            0..future.epoch,
            &[StakeState::stake_from(&stake_keyed_account.account).unwrap()],
        );

        // Try to withdraw stake
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports + 1,
                &mut to_keyed_account,
                &clock,
                &stake_history
            ),
            Err(InstructionError::InsufficientFunds)
        );
    }

    #[test]
    fn test_withdraw_stake_invalid_state() {
        let stake_pubkey = Pubkey::new_rand();
        let total_lamports = 100;
        let mut stake_account =
            Account::new(total_lamports, std::mem::size_of::<StakeState>(), &id());

        let clock = sysvar::clock::Clock::default();
        let mut future = sysvar::clock::Clock::default();
        future.epoch += 16;

        let to = Pubkey::new_rand();
        let mut to_account = Account::new(1, 0, &system_program::id());
        let mut to_keyed_account = KeyedAccount::new(&to, false, &mut to_account);

        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        let stake_state = StakeState::RewardsPool;
        stake_keyed_account.set_state(&stake_state).unwrap();

        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports,
                &mut to_keyed_account,
                &clock,
                &StakeHistory::default()
            ),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_stake_state_calculate_rewards() {
        let mut vote_state = VoteState::default();
        // assume stake.stake() is right
        // bootstrap means fully-vested stake at epoch 0
        let mut stake = Stake::new_bootstrap(1, &Pubkey::default(), &vote_state);

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            stake.calculate_rewards(1_000_000_000.0, &vote_state, None)
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0);
        vote_state.increment_credits(0);

        // this one can't collect now, no epoch credits have been saved off
        //   even though point value is huuge
        assert_eq!(
            None,
            stake.calculate_rewards(1_000_000_000_000.0, &vote_state, None)
        );

        // put 1 credit in epoch 1, pushes the 2 above into a redeemable state
        vote_state.increment_credits(1);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some((0, stake.stake * 2, 2)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        stake.credits_observed = 1;
        // this one should be able to collect exactly 1 (only observed one)
        assert_eq!(
            Some((0, stake.stake * 1, 2)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        stake.credits_observed = 2;
        // this one should be able to collect none because credits_observed >= credits in a
        //  redeemable state (the 2 credits in epoch 0)
        assert_eq!(None, stake.calculate_rewards(1.0, &vote_state, None));

        // put 1 credit in epoch 2, pushes the 1 for epoch 1 to redeemable
        vote_state.increment_credits(2);
        // this one should be able to collect 1 now, one credit by a stake of 1
        assert_eq!(
            Some((0, stake.stake * 1, 3)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        stake.credits_observed = 0;
        // this one should be able to collect everything from t=0 a warmed up stake of 2
        // (2 credits at stake of 1) + (1 credit at a stake of 2)
        assert_eq!(
            Some((0, stake.stake * 1 + stake.stake * 2, 3)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );

        // same as above, but is a really small commission out of 32 bits,
        //  verify that None comes back on small redemptions where no one gets paid
        vote_state.commission = 1;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 3)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );
        vote_state.commission = std::u8::MAX - 1;
        assert_eq!(
            None, // would be pSome((0, 2 * 1 + 1 * 2, 3)),
            stake.calculate_rewards(1.0, &vote_state, None)
        );
    }

    #[test]
    fn test_stake_redeem_vote_credits() {
        let clock = sysvar::clock::Clock::default();
        let mut rewards = sysvar::rewards::Rewards::default();
        rewards.validator_point_value = 100.0;

        let rewards_pool_pubkey = Pubkey::new_rand();
        let mut rewards_pool_account = Account::new_data(
            std::u64::MAX,
            &StakeState::RewardsPool,
            &crate::rewards_pools::id(),
        )
        .unwrap();
        let mut rewards_pool_keyed_account =
            KeyedAccount::new(&rewards_pool_pubkey, false, &mut rewards_pool_account);

        let pubkey = Pubkey::default();
        let stake_lamports = 100;
        let mut stake_account =
            Account::new(stake_lamports, std::mem::size_of::<StakeState>(), &id());
        let mut stake_keyed_account = KeyedAccount::new(&pubkey, true, &mut stake_account);

        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);

        // not delegated yet, deserialization fails
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards,
                &StakeHistory::default(),
            ),
            Err(InstructionError::InvalidAccountData)
        );

        // delegate the stake
        assert!(stake_keyed_account
            .delegate_stake(
                &vote_keyed_account,
                stake_lamports,
                &clock,
                &Config::default()
            )
            .is_ok());

        let stake_history = create_stake_history_from_stakes(
            Some(100),
            0..10,
            &[StakeState::stake_from(&stake_keyed_account.account).unwrap()],
        );

        // no credits to claim
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards,
                &stake_history,
            ),
            Err(InstructionError::CustomError(1))
        );

        // in this call, we've swapped rewards and vote, deserialization of rewards_pool fails
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut rewards_pool_keyed_account,
                &mut vote_keyed_account,
                &rewards,
                &StakeHistory::default(),
            ),
            Err(InstructionError::InvalidAccountData)
        );

        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);

        let mut vote_state = VoteState::from(&vote_account).unwrap();
        // put in some credits in epoch 0 for which we should have a non-zero stake
        for _i in 0..100 {
            vote_state.increment_credits(1);
        }
        vote_state.increment_credits(2);

        vote_state.to(&mut vote_account).unwrap();
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);

        // some credits to claim, but rewards pool empty (shouldn't ever happen)
        rewards_pool_keyed_account.account.lamports = 1;
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards,
                &StakeHistory::default(),
            ),
            Err(InstructionError::UnbalancedInstruction)
        );
        rewards_pool_keyed_account.account.lamports = std::u64::MAX;

        // finally! some credits to claim
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards,
                &stake_history,
            ),
            Ok(())
        );

        let wrong_vote_pubkey = Pubkey::new_rand();
        let mut wrong_vote_keyed_account =
            KeyedAccount::new(&wrong_vote_pubkey, false, &mut vote_account);

        // wrong voter_pubkey...
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut wrong_vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards,
                &stake_history,
            ),
            Err(InstructionError::InvalidArgument)
        );
    }

}
