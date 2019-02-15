//! Rewards program
//! Exchanges validation and storage proofs for lamports

use bincode::deserialize;
use log::*;
use solana_rewards_api::rewards_instruction::RewardsInstruction;
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
// https://github.com/solana-labs/solana/blob/master/book/src/staking-rewards.md
fn calc_vote_reward(credits: u64, stake: u64) -> Result<u64, ProgramError> {
    if credits < MINIMUM_CREDITS_PER_REDEMPTION {
        error!("Credit redemption too early");
        Err(ProgramError::GenericError)?;
    }
    Ok(credits * (stake / INTEREST_PER_CREDIT_DIVISOR))
}

fn redeem_vote_credits(keyed_accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    // The owner of the vote account needs to authorize having its credits cleared.
    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] is unsigned");
        Err(ProgramError::InvalidArgument)?;
    }

    if !vote_program::check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the VOTE_PROGRAM");
        Err(ProgramError::InvalidArgument)?;
    }

    // TODO: Verify the next instruction in the transaction being processed is
    // VoteInstruction::ClearCredits and that it points to the same vote account
    // as keyed_accounts[0].

    let vote_state = VoteState::deserialize(&keyed_accounts[0].account.userdata)?;

    //// TODO: This assumes the staker_id is static. If not, it should use the staker_id
    //// at the time of voting, not at credit redemption.
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
    keyed_accounts[1].account.tokens -= lamports;
    keyed_accounts[2].account.tokens += lamports;

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use solana_rewards_api::rewards_program;
    use solana_sdk::account::Account;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::vote_program::{self, Vote};

    fn create_rewards_account(tokens: u64) -> Account {
        let space = rewards_program::get_max_size();
        Account::new(tokens, space, rewards_program::id())
    }

    fn redeem_vote_credits_(
        rewards_id: &Pubkey,
        rewards_account: &mut Account,
        vote_id: &Pubkey,
        vote_account: &mut Account,
        to_id: &Pubkey,
        to_account: &mut Account,
    ) -> Result<(), ProgramError> {
        let mut keyed_accounts = [
            KeyedAccount::new(vote_id, true, vote_account),
            KeyedAccount::new(rewards_id, false, rewards_account),
            KeyedAccount::new(to_id, false, to_account),
        ];
        redeem_vote_credits(&mut keyed_accounts)
    }

    #[test]
    fn test_redeem_vote_credits_via_program() {
        let from_id = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, Pubkey::default());

        let vote_id = Keypair::new().pubkey();
        let mut vote_account = vote_program::create_vote_account(100);
        vote_program::register_and_deserialize(
            &from_id,
            &mut from_account,
            &vote_id,
            &mut vote_account,
        )
        .unwrap();

        for _ in 0..vote_program::MAX_VOTE_HISTORY {
            let vote = Vote::new(1);
            let vote_state =
                vote_program::vote_and_deserialize(&vote_id, &mut vote_account, vote.clone())
                    .unwrap();
            assert_eq!(vote_state.credits(), 0);
        }

        let vote = Vote::new(1);
        let vote_state =
            vote_program::vote_and_deserialize(&vote_id, &mut vote_account, vote.clone()).unwrap();
        assert_eq!(vote_state.credits(), 1);

        let rewards_id = Keypair::new().pubkey();
        let mut rewards_account = create_rewards_account(100);

        // TODO: Add VoteInstruction::RegisterStakerId so that we don't need to point the "to"
        // account to the "from" account.
        let to_id = from_id;
        let mut to_account = from_account;
        let to_tokens = to_account.tokens;

        redeem_vote_credits_(
            &rewards_id,
            &mut rewards_account,
            &vote_id,
            &mut vote_account,
            &to_id,
            &mut to_account,
        )
        .unwrap();
        assert!(to_account.tokens > to_tokens);
    }
}
