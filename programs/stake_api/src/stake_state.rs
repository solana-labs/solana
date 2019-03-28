//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

//use crate::{check_id, id};
//use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction_error::InstructionError;
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
    fn delegate_stake(&mut self, vote_account: &mut KeyedAccount) -> Result<(), InstructionError>;
    fn redeem_vote_credits(
        &mut self,
        stake_account: &mut KeyedAccount,
        vote_account: &mut KeyedAccount,
    ) -> Result<(), InstructionError>;
}

impl<'a> StakeAccount for KeyedAccount<'a> {
    fn delegate_stake(&mut self, vote_account: &mut KeyedAccount) -> Result<(), InstructionError> {
        if let StakeState::Delegate { .. } = self.account.state()? {
            self.set_state(&StakeState::Delegate {
                voter_id: *vote_account.unsigned_key(),
                credits_observed: vote_account.state::<VoteState>()?.credits(),
            })
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }

    fn redeem_vote_credits(
        &mut self,
        stake_account: &mut KeyedAccount,
        vote_account: &mut KeyedAccount,
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
                Err(InstructionError::InvalidArgument)
            } else if credits_observed > vote_state.credits() {
                Err(InstructionError::InvalidAccountData)
            } else {
                let credits = vote_state.credits() - credits_observed;
                credits_observed = vote_state.credits();

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
            }
        } else {
            Err(InstructionError::InvalidAccountData)
        }
    }
}

#[cfg(test)]
mod tests {
    //    use super::*;
    //  use solana_sdk::signature::{Keypair, KeypairUtil};
    //    use solana_vote_api::vote_instruction::Vote;

    #[test]
    fn test_stake_state_delegate() {
        //        let mut stake_state = StakeState::default();
        //        let keypair = Keypair::new();
        //        let mut vote_state = VoteState::default();
        //
        //        for i in 0..1000 {
        //            vote_state.process_vote(Vote::new(i));
        //        }
        //        assert!(stake_state.delegate(&keypair.pubkey(), &vote_state).is_ok());
        //
        //        assert_eq!(
        //            stake_state,
        //            StakeState::Delegate {
        //                voter_id: keypair.pubkey(),
        //                credits_observed: vote_state.credits()
        //            }
        //        );
        //        // wrong type of stake_state
        //        let mut stake_state = StakeState::MiningPool;
        //        assert!(stake_state
        //            .delegate(&keypair.pubkey(), &vote_state)
        //            .is_err());
    }
}
