//! Rewards program
//! Exchanges validation and storage proofs for lamports

use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::vote_program::{self, VoteState};

const INTEREST_PER_CREDIT_DIVISOR: u64 = 100; // Staker earns 1/INTEREST_PER_CREDIT_DIVISOR of interest per credit
const MINIMUM_CREDITS_PER_REDEMPTION: u64 = 1; // Raise this to either minimize congestion or lengthen the interest period

// Below is a temporary solution for calculating validator rewards. It targets an ROI for
// stakeholders and assumes the validator votes on every slot. It's convenient in that
// the calculation does not require knowing the size of the staking pool.
//
// TODO: Migrate to reward mechanism described by the book:
// https://github.com/solana-labs/solana/blob/master/book/src/ed_vce_state_validation_protocol_based_rewards.md
// https://github.com/solana-labs/solana/blob/master/book/src/staking-rewards.md#stake-weighted-rewards
fn calc_vote_reward(credits: u64, stake: u64) -> Result<u64, ProgramError> {
    if credits < MINIMUM_CREDITS_PER_REDEMPTION {
        error!("Credit redemption too early");
        Err(ProgramError::GenericError)?;
    }
    Ok(credits * (stake / INTEREST_PER_CREDIT_DIVISOR))
}

fn redeem_vote_credits(keyed_accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    // The owner of the vote account needs to authorize having its credits cleared.
    if keyed_accounts[1].signer_key().is_none() {
        error!("account[1] is unsigned");
        Err(ProgramError::InvalidArgument)?;
    }

    if !vote_program::check_id(&keyed_accounts[1].account.owner) {
        error!("account[1] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    let mut vote_state = VoteState::deserialize(&keyed_accounts[1].account.userdata)?;

    // TODO: This assumes the staker_id is static. If not, it should use the staker_id
    // at the time of voting, not at credit redemption.
    if vote_state.staker_id != *keyed_accounts[2].unsigned_key() {
        error!("account[2] was not the VOTE_PROGRAM's staking account");
        Err(ProgramError::InvalidArgument)?;
    }

    // TODO: This assumes the stake is static. If not, it should use the account value
    // at the time of voting, not at credit redemption.
    let stake = keyed_accounts[2].account.tokens;
    if stake == 0 {
        error!("staking account has no stake");
        Err(ProgramError::InvalidArgument)?;
    }

    let lamports = calc_vote_reward(vote_state.credits(), stake)?;

    // Transfer rewards from the rewards pool to the staking account.
    keyed_accounts[0].account.tokens -= lamports;
    keyed_accounts[2].account.tokens += lamports;

    vote_state.clear_credits();
    vote_state.serialize(&mut keyed_accounts[1].account.userdata)?;

    Ok(())
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum RewardsInstruction {
    RedeemVoteCredits,
}

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    match deserialize(data).map_err(|_| ProgramError::InvalidUserdata)? {
        RewardsInstruction::RedeemVoteCredits => redeem_vote_credits(keyed_accounts),
    }
}

//#[cfg(test)]
//mod tests {
//    use super::*;
//    use solana_sdk::account::Account;
//    use solana_sdk::signature::{Keypair, KeypairUtil};
//    use solana_sdk::vote_program;
//
//    fn create_vote_account(tokens: u64) -> Account {
//        let space = vote_program::get_max_size();
//        Account::new(tokens, space, vote_program::id())
//    }
//
//    fn register_and_deserialize(
//        from_id: &Pubkey,
//        from_account: &mut Account,
//        vote_id: &Pubkey,
//        vote_account: &mut Account,
//    ) -> Result<VoteState, ProgramError> {
//        let mut keyed_accounts = [
//            KeyedAccount::new(from_id, true, from_account),
//            KeyedAccount::new(vote_id, false, vote_account),
//        ];
//        register(&mut keyed_accounts)?;
//        let vote_state = VoteState::deserialize(&vote_account.userdata).unwrap();
//        Ok(vote_state)
//    }
//
//    fn vote_and_deserialize(
//        vote_id: &Pubkey,
//        vote_account: &mut Account,
//        vote: Vote,
//    ) -> Result<VoteState, ProgramError> {
//        let mut keyed_accounts = [KeyedAccount::new(vote_id, true, vote_account)];
//        process_vote(&mut keyed_accounts, vote)?;
//        let vote_state = VoteState::deserialize(&vote_account.userdata).unwrap();
//        Ok(vote_state)
//    }
//
//    #[test]
//    fn test_redeem_vote_credits() {
//        let from_id = Keypair::new().pubkey();
//        let mut from_account = Account::new(100, 0, Pubkey::default());
//
//        let vote_id = Keypair::new().pubkey();
//        let mut vote_account = create_vote_account(100);
//        register_and_deserialize(&from_id, &mut from_account, &vote_id, &mut vote_account).unwrap();
//
//        for _ in 0..vote_program::MAX_VOTE_HISTORY {
//            let vote = Vote::new(1);
//            let vote_state =
//                vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
//            assert_eq!(vote_state.credits(), 0);
//        }
//
//        let vote = Vote::new(1);
//        let vote_state = vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
//        assert_eq!(vote_state.credits(), 1);
//
//        let vote_state = redeem_vote_credits_and_deserialize()
//        assert_eq!(vote_state.credits(), 0);
//    }
//}
