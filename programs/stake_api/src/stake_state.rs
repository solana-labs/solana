//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

use crate::id;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::{Account, KeyedAccount};
use solana_sdk::account_utils::State;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::sysvar;
use solana_sdk::timing::Epoch;
use solana_vote_api::vote_state::VoteState;
use std::cmp;

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
    pub activated: Epoch,   // epoch the stake was activated
    pub deactivated: Epoch, // epoch the stake was deactivated, std::Epoch::MAX if not deactivated
}
pub const STAKE_WARMUP_EPOCHS: u64 = 3;

impl Default for Stake {
    fn default() -> Self {
        Stake {
            voter_pubkey: Pubkey::default(),
            credits_observed: 0,
            stake: 0,
            activated: 0,
            deactivated: std::u64::MAX,
        }
    }
}

impl Stake {
    pub fn stake(&self, epoch: u64) -> u64 {
        // before "activated" or after deactivated?
        if epoch < self.activated || epoch >= self.deactivated {
            return 0;
        }

        // curr epoch  |   0   |  1  |  2   ... | 100  | 101 | 102 | 103
        // action      | activate    |        de-activate    |     |
        //             |   |   |     |          |  |   |     |     |
        //             |   v   |     |          |  v   |     |     |
        // stake       |  1/3  | 2/3 | 3/3  ... | 3/3  | 2/3 | 1/3 | 0/3
        // -------------------------------------------------------------
        // activated   |   0   ...
        // deactivated | std::u64::MAX ...        103 ...

        // activate/deactivate can't possibly overlap
        //  (see delegate_stake() and deactivate())
        if epoch - self.activated < STAKE_WARMUP_EPOCHS {
            // warmup
            (self.stake / STAKE_WARMUP_EPOCHS) * (epoch - self.activated + 1)
        } else if self.deactivated - epoch < STAKE_WARMUP_EPOCHS {
            // cooldown
            (self.stake / STAKE_WARMUP_EPOCHS) * (self.deactivated - epoch)
        } else {
            self.stake
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

            total_rewards += (self.stake(*epoch) * epoch_credits) as f64 * point_value;

            // don't want to assume anything about order of the iterator...
            credits_observed = std::cmp::max(credits_observed, *credits);
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

    fn delegate(&mut self, stake: u64, voter_pubkey: &Pubkey, vote_state: &VoteState, epoch: u64) {
        assert!(std::u64::MAX - epoch >= (STAKE_WARMUP_EPOCHS * 2));

        // resets the current stake's credits
        self.voter_pubkey = *voter_pubkey;
        self.credits_observed = vote_state.credits();

        // when this stake was activated
        self.activated = epoch;
        self.stake = stake;
    }

    fn deactivate(&mut self, epoch: u64) {
        self.deactivated = std::cmp::max(
            epoch + STAKE_WARMUP_EPOCHS,
            self.activated + 2 * STAKE_WARMUP_EPOCHS - 1,
        );
    }
}

pub trait StakeAccount {
    fn delegate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        stake: u64,
        clock: &sysvar::clock::Clock,
    ) -> Result<(), InstructionError>;
    fn deactivate_stake(&mut self, clock: &sysvar::clock::Clock) -> Result<(), InstructionError>;
    fn redeem_vote_credits(
        &mut self,
        vote_account: &mut KeyedAccount,
        rewards_account: &mut KeyedAccount,
        rewards: &sysvar::rewards::Rewards,
    ) -> Result<(), InstructionError>;
    fn withdraw(
        &mut self,
        lamports: u64,
        to: &mut KeyedAccount,
        clock: &sysvar::clock::Clock,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn delegate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        new_stake: u64,
        clock: &sysvar::clock::Clock,
    ) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        if new_stake > self.account.lamports {
            return Err(InstructionError::InsufficientFunds);
        }

        if let StakeState::Uninitialized = self.state()? {
            let mut stake = Stake::default();

            stake.delegate(
                new_stake,
                vote_account.unsigned_key(),
                &vote_account.state()?,
                clock.epoch,
            );

            self.set_state(&StakeState::Stake(stake))
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn deactivate_stake(&mut self, clock: &sysvar::clock::Clock) -> Result<(), InstructionError> {
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
    ) -> Result<(), InstructionError> {
        if let (StakeState::Stake(mut stake), StakeState::RewardsPool) =
            (self.state()?, rewards_account.state()?)
        {
            let vote_state: VoteState = vote_account.state()?;

            if stake.voter_pubkey != *vote_account.unsigned_key() {
                return Err(InstructionError::InvalidArgument);
            }

            if let Some((stakers_reward, voters_reward, credits_observed)) =
                stake.calculate_rewards(rewards.validator_point_value, &vote_state)
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
    ) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        match self.state()? {
            StakeState::Stake(mut stake) => {
                let staked = if stake.stake(clock.epoch) == 0 {
                    0
                } else {
                    // Assume full stake if the stake is under warmup/cooldown
                    stake.stake
                };
                if lamports > self.account.lamports.saturating_sub(staked) {
                    return Err(InstructionError::InsufficientFunds);
                }
                self.account.lamports -= lamports;
                to.account.lamports += lamports;
                // Adjust the stake (in case balance dropped below stake)
                stake.stake = cmp::min(stake.stake, self.account.lamports);
                self.set_state(&StakeState::Stake(stake))
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

// utility function, used by Bank, tests, genesis
pub fn create_stake_account(
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    lamports: u64,
) -> Account {
    let mut stake_account = Account::new(lamports, std::mem::size_of::<StakeState>(), &id());

    stake_account
        .set_state(&StakeState::Stake(Stake {
            voter_pubkey: *voter_pubkey,
            credits_observed: vote_state.credits(),
            stake: lamports,
            activated: 0,
            deactivated: std::u64::MAX,
        }))
        .expect("set_state");

    stake_account
}

// utility function, used by Bank, tests, genesis
pub fn create_rewards_pool() -> Account {
    Account::new_data(std::u64::MAX, &StakeState::RewardsPool, &crate::id()).unwrap()
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

    #[test]
    fn test_stake_delegate_stake() {
        let clock = sysvar::clock::Clock::default();

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
            stake_keyed_account.delegate_stake(&vote_keyed_account, 0, &clock),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, stake_lamports, &clock)
            .is_ok());

        // verify that delegate_stake() looks right, compare against hand-rolled
        let stake_state: StakeState = stake_keyed_account.state().unwrap();
        assert_eq!(
            stake_state,
            StakeState::Stake(Stake {
                voter_pubkey: vote_keypair.pubkey(),
                credits_observed: vote_state.credits(),
                stake: stake_lamports,
                activated: 0,
                deactivated: std::u64::MAX,
            })
        );
        // verify that delegate_stake can't be called twice StakeState::default()
        // signed keyed account
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports, &clock),
            Err(InstructionError::InvalidAccountData)
        );

        // verify can only stake up to account lamports
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports + 1, &clock),
            Err(InstructionError::InsufficientFunds)
        );

        let stake_state = StakeState::RewardsPool;

        stake_keyed_account.set_state(&stake_state).unwrap();
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, 0, &clock)
            .is_err());
    }

    #[test]
    fn test_stake_stake() {
        let mut stake = Stake::default();
        assert_eq!(stake.stake(0), 0);
        let staked = STAKE_WARMUP_EPOCHS;
        stake.delegate(staked, &Pubkey::default(), &VoteState::default(), 1);
        // test warmup
        for i in 0..STAKE_WARMUP_EPOCHS {
            assert_eq!(stake.stake(i), i);
        }
        assert_eq!(stake.stake(STAKE_WARMUP_EPOCHS * 42), staked);

        stake.deactivate(STAKE_WARMUP_EPOCHS);

        // test cooldown
        for i in STAKE_WARMUP_EPOCHS..STAKE_WARMUP_EPOCHS * 2 {
            assert_eq!(
                stake.stake(i),
                staked - (staked / STAKE_WARMUP_EPOCHS) * (i - STAKE_WARMUP_EPOCHS)
            );
        }
        assert_eq!(stake.stake(STAKE_WARMUP_EPOCHS * 42), 0);
    }

    #[test]
    fn test_deactivate_stake() {
        let stake_pubkey = Pubkey::new_rand();
        let stake_lamports = 42;
        let mut stake_account =
            Account::new(stake_lamports, std::mem::size_of::<StakeState>(), &id());

        let clock = sysvar::clock::Clock::default();

        // unsigned keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &mut stake_account);
        assert_eq!(
            stake_keyed_account.deactivate_stake(&clock),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account but not staked yet
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.deactivate_stake(&clock),
            Err(InstructionError::InvalidAccountData)
        );

        // Staking
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&VoteState::default()).unwrap();
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports, &clock),
            Ok(())
        );

        // Deactivate after staking
        assert_eq!(stake_keyed_account.deactivate_stake(&clock), Ok(()));
    }

    #[test]
    fn test_withdraw_stake() {
        let stake_pubkey = Pubkey::new_rand();
        let mut total_lamports = 100;
        let stake_lamports = 42;
        let mut stake_account =
            Account::new(total_lamports, std::mem::size_of::<StakeState>(), &id());

        let clock = sysvar::clock::Clock::default();

        let to = Pubkey::new_rand();
        let mut to_account = Account::new(1, 0, &system_program::id());
        let mut to_keyed_account = KeyedAccount::new(&to, false, &mut to_account);

        // unsigned keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(total_lamports, &mut to_keyed_account, &clock),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account but uninitialized
        // try withdrawing more than balance
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(total_lamports + 1, &mut to_keyed_account, &clock),
            Err(InstructionError::InsufficientFunds)
        );

        // try withdrawing some (enough for rest of the test to carry forward)
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.withdraw(5, &mut to_keyed_account, &clock),
            Ok(())
        );
        total_lamports -= 5;

        // Stake some lamports (available lampoorts for withdrawls will reduce)
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&VoteState::default()).unwrap();
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports, &clock),
            Ok(())
        );

        // Try to withdraw more than what's available
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports + 1,
                &mut to_keyed_account,
                &clock
            ),
            Err(InstructionError::InsufficientFunds)
        );

        // Try to withdraw all unstaked lamports
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports,
                &mut to_keyed_account,
                &clock
            ),
            Ok(())
        );
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

        // Stake some lamports (available lampoorts for withdrawls will reduce)
        let vote_pubkey = Pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&VoteState::default()).unwrap();
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports, &future),
            Ok(())
        );

        // Try to withdraw including staked
        assert_eq!(
            stake_keyed_account.withdraw(
                total_lamports - stake_lamports + 1,
                &mut to_keyed_account,
                &clock
            ),
            Ok(())
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
            stake_keyed_account.withdraw(total_lamports, &mut to_keyed_account, &clock),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_stake_state_calculate_rewards() {
        let mut vote_state = VoteState::default();
        let mut stake = Stake::default();

        // warmup makes this look like zero until WARMUP_EPOCHS
        stake.stake = 1;

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(None, stake.calculate_rewards(1_000_000_000.0, &vote_state));

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0);
        vote_state.increment_credits(0);

        // this one can't collect now, no epoch credits have been saved off
        assert_eq!(None, stake.calculate_rewards(1_000_000_000.0, &vote_state));

        // put 1 credit in epoch 1, pushes the 2 above into a redeemable state
        vote_state.increment_credits(1);

        // still can't collect yet, warmup puts the kibosh on it
        assert_eq!(None, stake.calculate_rewards(1.0, &vote_state));

        stake.stake = STAKE_WARMUP_EPOCHS;
        // this one should be able to collect exactly 2
        assert_eq!(
            Some((0, 1 * 2, 2)),
            stake.calculate_rewards(1.0, &vote_state)
        );

        stake.stake = STAKE_WARMUP_EPOCHS;
        stake.credits_observed = 1;
        // this one should be able to collect exactly 1 (only observed one)
        assert_eq!(
            Some((0, 1 * 1, 2)),
            stake.calculate_rewards(1.0, &vote_state)
        );

        stake.stake = STAKE_WARMUP_EPOCHS;
        stake.credits_observed = 2;
        // this one should be able to collect none because credits_observed >= credits in a
        //  redeemable state (the 2 credits in epoch 0)
        assert_eq!(None, stake.calculate_rewards(1.0, &vote_state));

        // put 1 credit in epoch 2, pushes the 1 for epoch 1 to redeemable
        vote_state.increment_credits(2);
        // this one should be able to collect two now, one credit by a stake of 2
        assert_eq!(
            Some((0, 2 * 1, 3)),
            stake.calculate_rewards(1.0, &vote_state)
        );

        stake.credits_observed = 0;
        // this one should be able to collect everything from t=0 a warmed up stake of 2
        // (2 credits at stake of 1) + (1 credit at a stake of 2)
        assert_eq!(
            Some((0, 2 * 1 + 1 * 2, 3)),
            stake.calculate_rewards(1.0, &vote_state)
        );

        // same as above, but is a really small commission out of 32 bits,
        //  verify that None comes back on small redemptions where no one gets paid
        vote_state.commission = 1;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 3)),
            stake.calculate_rewards(1.0, &vote_state)
        );
        vote_state.commission = std::u8::MAX - 1;
        assert_eq!(
            None, // would be pSome((0, 2 * 1 + 1 * 2, 3)),
            stake.calculate_rewards(1.0, &vote_state)
        );
    }

    #[test]
    fn test_stake_redeem_vote_credits() {
        let clock = sysvar::clock::Clock::default();
        let mut rewards = sysvar::rewards::Rewards::default();
        rewards.validator_point_value = 100.0;

        let rewards_pool_pubkey = Pubkey::new_rand();
        let mut rewards_pool_account = create_rewards_pool();
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
                &rewards
            ),
            Err(InstructionError::InvalidAccountData)
        );

        // delegate the stake
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, stake_lamports, &clock)
            .is_ok());
        // no credits to claim
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards
            ),
            Err(InstructionError::CustomError(1))
        );

        // swapped rewards and vote, deserialization of rewards_pool fails
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut rewards_pool_keyed_account,
                &mut vote_keyed_account,
                &rewards
            ),
            Err(InstructionError::InvalidAccountData)
        );

        let mut vote_account =
            vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 100);

        let mut vote_state = VoteState::from(&vote_account).unwrap();
        // put in some credits in epoch 0 for which we should have a non-zero stake
        for _i in 0..100 {
            vote_state.increment_credits(0);
        }
        vote_state.increment_credits(1);

        vote_state.to(&mut vote_account).unwrap();
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);

        // some credits to claim, but rewards pool empty (shouldn't ever happen)
        rewards_pool_keyed_account.account.lamports = 1;
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards
            ),
            Err(InstructionError::UnbalancedInstruction)
        );
        rewards_pool_keyed_account.account.lamports = std::u64::MAX;

        // finally! some credits to claim
        assert_eq!(
            stake_keyed_account.redeem_vote_credits(
                &mut vote_keyed_account,
                &mut rewards_pool_keyed_account,
                &rewards
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
                &rewards
            ),
            Err(InstructionError::InvalidArgument)
        );
    }

}
