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
use solana_sdk::syscall;
use solana_vote_api::vote_state::VoteState;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum StakeState {
    Uninitialized,
    Stake(Stake),
    MiningPool {
        /// epoch for which this Pool will redeem rewards
        epoch: u64,

        /// the number of lamports each point is worth
        point_value: f64,
    },
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

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Stake {
    pub voter_pubkey: Pubkey,
    pub credits_observed: u64,
    pub stake: u64,      // activated stake
    pub epoch: u64,      // epoch the stake was activated
    pub prev_stake: u64, // for warmup, cooldown
}
pub const STAKE_WARMUP_EPOCHS: u64 = 3;

impl Stake {
    pub fn stake(&self, epoch: u64) -> u64 {
        // prev_stake for stuff in the past
        if epoch < self.epoch {
            return self.prev_stake;
        }
        if epoch - self.epoch >= STAKE_WARMUP_EPOCHS {
            return self.stake;
        }

        if self.stake != 0 {
            // warmup
            // 1/3rd, then 2/3rds...
            (self.stake / STAKE_WARMUP_EPOCHS) * (epoch - self.epoch + 1)
        } else if self.prev_stake != 0 {
            // cool down
            // 3/3rds, then 2/3rds...
            self.prev_stake - ((self.prev_stake / STAKE_WARMUP_EPOCHS) * (epoch - self.epoch))
        } else {
            0
        }
    }

    pub fn calculate_rewards(
        &self,
        epoch: u64,
        point_value: f64,
        vote_state: &VoteState,
    ) -> Option<(u64, u64)> {
        if self.credits_observed >= vote_state.credits() {
            return None;
        }

        let total_rewards = (self.stake(epoch) * (vote_state.credits() - self.credits_observed))
            as f64
            * point_value;

        // don't bother trying to collect fractional lamports
        if total_rewards < 1f64 {
            return None;
        }

        let (voter_rewards, staker_rewards, is_split) = vote_state.commission_split(total_rewards);

        if (voter_rewards < 1f64 || staker_rewards < 1f64) && is_split {
            // don't bother trying to collect fractional lamports
            return None;
        }

        Some((voter_rewards as u64, staker_rewards as u64))
    }

    fn delegate(
        &mut self,
        stake: u64,
        voter_pubkey: &Pubkey,
        vote_state: &VoteState,
        epoch: u64, // current: &syscall::current::Current
    ) {
        // resets the current stake's credits
        self.voter_pubkey = *voter_pubkey;
        self.credits_observed = vote_state.credits();

        // when this stake was activated
        self.epoch = epoch;
        self.stake = stake;
    }

    fn deactivate(&mut self, epoch: u64) {
        self.voter_pubkey = Pubkey::default();
        self.credits_observed = std::u64::MAX;
        self.prev_stake = self.stake(epoch);
        self.stake = 0;
        self.epoch = epoch;
    }
}

pub trait StakeAccount {
    fn delegate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        stake: u64,
        current: &syscall::current::Current,
    ) -> Result<(), InstructionError>;
    fn deactivate_stake(
        &mut self,
        current: &syscall::current::Current,
    ) -> Result<(), InstructionError>;
    fn redeem_vote_credits(
        &mut self,
        vote_account: &mut KeyedAccount,
        rewards_account: &mut KeyedAccount,
        rewards: &syscall::rewards::Rewards,
        current: &syscall::current::Current,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn delegate_stake(
        &mut self,
        vote_account: &KeyedAccount,
        new_stake: u64,
        current: &syscall::current::Current,
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
                current.epoch,
            );

            self.set_state(&StakeState::Stake(stake))
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn deactivate_stake(
        &mut self,
        current: &syscall::current::Current,
    ) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        if let StakeState::Stake(mut stake) = self.state()? {
            stake.deactivate(current.epoch);

            self.set_state(&StakeState::Stake(stake))
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
    fn redeem_vote_credits(
        &mut self,
        vote_account: &mut KeyedAccount,
        rewards_account: &mut KeyedAccount,
        rewards: &syscall::rewards::Rewards,
        current: &syscall::current::Current,
    ) -> Result<(), InstructionError> {
        if let (StakeState::Stake(mut stake), StakeState::RewardsPool) =
            (self.state()?, rewards_account.state()?)
        {
            let vote_state: VoteState = vote_account.state()?;

            if stake.voter_pubkey != *vote_account.unsigned_key() {
                return Err(InstructionError::InvalidArgument);
            }

            if stake.credits_observed > vote_state.credits() {
                return Err(InstructionError::InvalidAccountData);
            }

            if let Some((stakers_reward, voters_reward)) =
                stake.calculate_rewards(current.epoch, rewards.validator_point_value, &vote_state)
            {
                if rewards_account.account.lamports < (stakers_reward + voters_reward) {
                    return Err(InstructionError::UnbalancedInstruction);
                }
                rewards_account.account.lamports -= stakers_reward + voters_reward;

                self.account.lamports += stakers_reward;
                vote_account.account.lamports += voters_reward;

                stake.credits_observed = vote_state.credits();

                self.set_state(&StakeState::Stake(stake))
            } else {
                // not worth collecting
                Err(InstructionError::CustomError(1))
            }
        } else {
            Err(InstructionError::InvalidAccountData)
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
            epoch: 0,
            prev_stake: 0,
        }))
        .expect("set_state");

    stake_account
}

// utility function, used by Bank, tests, genesis
pub fn create_mining_pool(lamports: u64, epoch: u64, point_value: f64) -> Account {
    let mut mining_pool_account = Account::new(lamports, std::mem::size_of::<StakeState>(), &id());

    mining_pool_account
        .set_state(&StakeState::MiningPool { epoch, point_value })
        .expect("set_state");

    mining_pool_account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_vote_api::vote_state;

    #[test]
    fn test_stake_delegate_stake() {
        let current = syscall::current::Current::default();

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
            stake_keyed_account.delegate_stake(&vote_keyed_account, 0, &current),
            Err(InstructionError::MissingRequiredSignature)
        );

        // signed keyed account
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, stake_lamports, &current)
            .is_ok());

        // verify that delegate_stake() looks right, compare against hand-rolled
        let stake_state: StakeState = stake_keyed_account.state().unwrap();
        assert_eq!(
            stake_state,
            StakeState::Stake(Stake {
                voter_pubkey: vote_keypair.pubkey(),
                credits_observed: vote_state.credits(),
                stake: stake_lamports,
                epoch: 0,
                prev_stake: 0
            })
        );
        // verify that delegate_stake can't be called twice StakeState::default()
        // signed keyed account
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports, &current),
            Err(InstructionError::InvalidAccountData)
        );

        // verify can only stake up to account lamports
        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account, stake_lamports + 1, &current),
            Err(InstructionError::InsufficientFunds)
        );

        let stake_state = StakeState::MiningPool {
            epoch: 0,
            point_value: 0.0,
        };
        stake_keyed_account.set_state(&stake_state).unwrap();
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, 0, &current)
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
    fn test_stake_state_calculate_rewards() {
        //        let mut vote_state = VoteState::default();
        //        let mut vote_i = 0;
        //
        //        // put a credit in the vote_state
        //        while vote_state.credits() == 0 {
        //            vote_state.process_slot_vote_unchecked(vote_i);
        //            vote_i += 1;
        //        }
        //        // this guy can't collect now, not enough stake to get paid on 1 credit
        //        assert_eq!(None, StakeState::calculate_rewards(0, 100, &vote_state));
        //        // this guy can
        //        assert_eq!(
        //            Some((0, 1)),
        //            StakeState::calculate_rewards(0, STAKE_GETS_PAID_EVERY_VOTE, &vote_state)
        //        );
        //        // but, there's not enough to split
        //        vote_state.commission = std::u32::MAX / 2;
        //        assert_eq!(
        //            None,
        //            StakeState::calculate_rewards(0, STAKE_GETS_PAID_EVERY_VOTE, &vote_state)
        //        );
        //
        //        // put more credit in the vote_state
        //        while vote_state.credits() < 10 {
        //            vote_state.process_slot_vote_unchecked(vote_i);
        //            vote_i += 1;
        //        }
        //        vote_state.commission = 0;
        //        assert_eq!(
        //            Some((0, 10)),
        //            StakeState::calculate_rewards(0, STAKE_GETS_PAID_EVERY_VOTE, &vote_state)
        //        );
        //        vote_state.commission = std::u32::MAX;
        //        assert_eq!(
        //            Some((10, 0)),
        //            StakeState::calculate_rewards(0, STAKE_GETS_PAID_EVERY_VOTE, &vote_state)
        //        );
        //        vote_state.commission = std::u32::MAX / 2;
        //        assert_eq!(
        //            Some((5, 5)),
        //            StakeState::calculate_rewards(0, STAKE_GETS_PAID_EVERY_VOTE, &vote_state)
        //        );
        //        // not even enough stake to get paid on 10 credits...
        //        assert_eq!(None, StakeState::calculate_rewards(0, 100, &vote_state));
    }

    #[test]
    fn test_stake_redeem_vote_credits() {
        let current = syscall::current::Current::default();

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

        let pubkey = Pubkey::default();
        let mut stake_account = Account::new(
            STAKE_GETS_PAID_EVERY_VOTE,
            std::mem::size_of::<StakeState>(),
            &id(),
        );
        let mut stake_keyed_account = KeyedAccount::new(&pubkey, true, &mut stake_account);

        // delegate the stake
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, STAKE_GETS_PAID_EVERY_VOTE, &current)
            .is_ok());

        let mut mining_pool_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());
        let mut mining_pool_keyed_account =
            KeyedAccount::new(&pubkey, true, &mut mining_pool_account);

        // not a mining pool yet...
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &mut vote_keyed_account),
            Err(InstructionError::InvalidAccountData)
        );

        mining_pool_keyed_account
            .set_state(&StakeState::MiningPool {
                epoch: 0,
                point_value: 0.0,
            })
            .unwrap();

        // no movement in vote account, so no redemption needed
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &mut vote_keyed_account),
            Err(InstructionError::CustomError(1))
        );

        // move the vote account forward
        vote_state.process_slot_vote_unchecked(1000);
        vote_keyed_account.set_state(&vote_state).unwrap();

        // now, no lamports in the pool!
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &mut vote_keyed_account),
            Err(InstructionError::UnbalancedInstruction)
        );

        // add a lamport to pool
        mining_pool_keyed_account.account.lamports = 2;
        assert!(mining_pool_keyed_account
            .redeem_vote_credits(&mut stake_keyed_account, &mut vote_keyed_account)
            .is_ok()); // yay

        // lamports only shifted around, none made or lost
        assert_eq!(
            2 + 100 + STAKE_GETS_PAID_EVERY_VOTE,
            mining_pool_account.lamports + vote_account.lamports + stake_account.lamports
        );
    }

    #[test]
    fn test_stake_redeem_vote_credits_vote_errors() {
        let current = syscall::current::Current::default();

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

        let pubkey = Pubkey::default();
        let stake_lamports = 0;
        let mut stake_account =
            Account::new(stake_lamports, std::mem::size_of::<StakeState>(), &id());
        let mut stake_keyed_account = KeyedAccount::new(&pubkey, true, &mut stake_account);

        // delegate the stake
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account, stake_lamports, &current)
            .is_ok());

        let mut mining_pool_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());
        let mut mining_pool_keyed_account =
            KeyedAccount::new(&pubkey, true, &mut mining_pool_account);
        mining_pool_keyed_account
            .set_state(&StakeState::MiningPool {
                epoch: 0,
                point_value: 0.0,
            })
            .unwrap();

        let mut vote_state = VoteState::default();
        for i in 0..100 {
            // go back in time, previous state had 1000 votes
            vote_state.process_slot_vote_unchecked(i);
        }
        vote_keyed_account.set_state(&vote_state).unwrap();
        // voter credits lower than stake_delegate credits...  TODO: is this an error?
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &mut vote_keyed_account),
            Err(InstructionError::InvalidAccountData)
        );

        let vote1_keypair = Keypair::new();
        let vote1_pubkey = vote1_keypair.pubkey();
        let mut vote1_account =
            vote_state::create_account(&vote1_pubkey, &Pubkey::new_rand(), 0, 100);
        let mut vote1_keyed_account = KeyedAccount::new(&vote1_pubkey, false, &mut vote1_account);
        vote1_keyed_account.set_state(&vote_state).unwrap();

        // wrong voter_pubkey...
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &mut vote1_keyed_account),
            Err(InstructionError::InvalidArgument)
        );
    }

}
