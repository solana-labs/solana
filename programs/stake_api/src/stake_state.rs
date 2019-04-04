//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

//use crate::{check_id, id};
//use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::State;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_state::VoteState;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum StakeState {
    Delegate {
        voter_id: Pubkey,
        credits_observed: u64,
    },
    MiningPool,
}

impl Default for StakeState {
    fn default() -> Self {
        StakeState::Delegate {
            voter_id: Pubkey::default(),
            credits_observed: 0,
        }
    }
}

pub trait StakeAccount {
    fn delegate_stake(&mut self, vote_account: &KeyedAccount) -> Result<(), InstructionError>;
    fn redeem_vote_credits(
        &mut self,
        stake_account: &mut KeyedAccount,
        vote_account: &KeyedAccount,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn delegate_stake(&mut self, vote_account: &KeyedAccount) -> Result<(), InstructionError> {
        if self.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        if let StakeState::Delegate { .. } = self.state()? {
            let vote_state: VoteState = vote_account.state()?;
            self.set_state(&StakeState::Delegate {
                voter_id: *vote_account.unsigned_key(),
                credits_observed: vote_state.credits(),
            })
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }

    fn redeem_vote_credits(
        &mut self,
        stake_account: &mut KeyedAccount,
        vote_account: &KeyedAccount,
    ) -> Result<(), InstructionError> {
        if let (
            StakeState::MiningPool,
            StakeState::Delegate {
                voter_id,
                mut credits_observed,
            },
        ) = (self.state()?, stake_account.state()?)
        {
            let vote_state: VoteState = vote_account.state()?;

            if voter_id != *vote_account.unsigned_key() {
                return Err(InstructionError::InvalidArgument);
            }

            if credits_observed > vote_state.credits() {
                return Err(InstructionError::InvalidAccountData);
            }

            let credits = vote_state.credits() - credits_observed;
            credits_observed = vote_state.credits();

            if self.account.lamports < credits {
                return Err(InstructionError::UnbalancedInstruction);
            }
            // TODO: commission and network inflation parameter
            //   mining pool lamports reduced by credits * network_inflation_param
            //   stake_account and vote_account lamports up by the net
            //   split by a commission in vote_state
            self.account.lamports -= credits;
            stake_account.account.lamports += credits;

            stake_account.set_state(&StakeState::Delegate {
                voter_id,
                credits_observed,
            })
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_vote_api::vote_instruction::Vote;
    use solana_vote_api::vote_state::create_vote_account;

    #[test]
    fn test_stake_delegate_stake() {
        let vote_keypair = Keypair::new();
        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_vote(Vote::new(i));
        }

        let vote_pubkey = vote_keypair.pubkey();
        let mut vote_account = create_vote_account(100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&vote_state).unwrap();

        let stake_pubkey = Pubkey::default();
        let mut stake_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());

        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, false, &mut stake_account);

        assert_eq!(
            stake_keyed_account.delegate_stake(&vote_keyed_account),
            Err(InstructionError::MissingRequiredSignature)
        );

        let mut stake_keyed_account = KeyedAccount::new(&stake_pubkey, true, &mut stake_account);
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account)
            .is_ok());

        let stake_state: StakeState = stake_keyed_account.state().unwrap();
        assert_eq!(
            stake_state,
            StakeState::Delegate {
                voter_id: vote_keypair.pubkey(),
                credits_observed: vote_state.credits()
            }
        );
        let stake_state = StakeState::MiningPool;
        stake_keyed_account.set_state(&stake_state).unwrap();
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account)
            .is_err());
    }

    #[test]
    fn test_stake_redeem_vote_credits() {
        let vote_keypair = Keypair::new();
        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_vote(Vote::new(i));
        }

        let vote_pubkey = vote_keypair.pubkey();
        let mut vote_account = create_vote_account(100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&vote_state).unwrap();

        let pubkey = Pubkey::default();
        let mut stake_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());
        let mut stake_keyed_account = KeyedAccount::new(&pubkey, true, &mut stake_account);

        // delegate the stake
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account)
            .is_ok());

        let mut mining_pool_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());
        let mut mining_pool_keyed_account =
            KeyedAccount::new(&pubkey, true, &mut mining_pool_account);

        // no mining pool yet...
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &vote_keyed_account),
            Err(InstructionError::InvalidAccountData)
        );

        mining_pool_keyed_account
            .set_state(&StakeState::MiningPool)
            .unwrap();

        // no movement in vote account, so no redemption needed
        assert!(mining_pool_keyed_account
            .redeem_vote_credits(&mut stake_keyed_account, &vote_keyed_account)
            .is_ok());

        // move the vote account forward
        vote_state.process_vote(Vote::new(1000));
        vote_keyed_account.set_state(&vote_state).unwrap();

        // no lamports in the pool
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &vote_keyed_account),
            Err(InstructionError::UnbalancedInstruction)
        );

        // add a lamport
        mining_pool_keyed_account.account.lamports = 2;
        assert!(mining_pool_keyed_account
            .redeem_vote_credits(&mut stake_keyed_account, &vote_keyed_account)
            .is_ok());
    }

    #[test]
    fn test_stake_redeem_vote_credits_vote_errors() {
        let vote_keypair = Keypair::new();
        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_vote(Vote::new(i));
        }

        let vote_pubkey = vote_keypair.pubkey();
        let mut vote_account = create_vote_account(100);
        let mut vote_keyed_account = KeyedAccount::new(&vote_pubkey, false, &mut vote_account);
        vote_keyed_account.set_state(&vote_state).unwrap();

        let pubkey = Pubkey::default();
        let mut stake_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());
        let mut stake_keyed_account = KeyedAccount::new(&pubkey, true, &mut stake_account);

        // delegate the stake
        assert!(stake_keyed_account
            .delegate_stake(&vote_keyed_account)
            .is_ok());

        let mut mining_pool_account = Account::new(0, std::mem::size_of::<StakeState>(), &id());
        let mut mining_pool_keyed_account =
            KeyedAccount::new(&pubkey, true, &mut mining_pool_account);
        mining_pool_keyed_account
            .set_state(&StakeState::MiningPool)
            .unwrap();

        let mut vote_state = VoteState::default();
        for i in 0..100 {
            // go back in time, previous state had 1000 votes
            vote_state.process_vote(Vote::new(i));
        }
        vote_keyed_account.set_state(&vote_state).unwrap();
        // voter credits lower than stake_delegate credits...  TODO: is this an error?
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &vote_keyed_account),
            Err(InstructionError::InvalidAccountData)
        );

        let vote1_keypair = Keypair::new();
        let vote1_pubkey = vote1_keypair.pubkey();
        let mut vote1_account = create_vote_account(100);
        let mut vote1_keyed_account = KeyedAccount::new(&vote1_pubkey, false, &mut vote1_account);
        vote1_keyed_account.set_state(&vote_state).unwrap();

        // wrong voter_id...
        assert_eq!(
            mining_pool_keyed_account
                .redeem_vote_credits(&mut stake_keyed_account, &vote1_keyed_account),
            Err(InstructionError::InvalidArgument)
        );
    }

}
