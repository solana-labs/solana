use solana_rewards_api::rewards_program;
use solana_rewards_api::rewards_transaction::RewardsTransaction;
use solana_runtime::bank::{Bank, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::vote_program::{self, VoteState};
use solana_sdk::vote_transaction::VoteTransaction;

fn create_rewards_account(
    bank: &Bank,
    from_keypair: &Keypair,
    rewards_id: Pubkey,
    lamports: u64,
) -> Result<()> {
    let last_id = bank.last_id();
    let tx = RewardsTransaction::new_account(from_keypair, rewards_id, last_id, lamports, 0);
    bank.process_transaction(&tx)
}

fn create_vote_account(
    bank: &Bank,
    from_keypair: &Keypair,
    vote_id: Pubkey,
    lamports: u64,
) -> Result<()> {
    let last_id = bank.last_id();
    let tx = VoteTransaction::new_account(from_keypair, vote_id, last_id, lamports, 0);
    bank.process_transaction(&tx)
}

fn submit_vote(bank: &Bank, vote_keypair: &Keypair, tick_height: u64) -> Result<VoteState> {
    let last_id = bank.last_id();
    let tx = VoteTransaction::new_vote(vote_keypair, tick_height, last_id, 0);
    bank.process_transaction(&tx)?;
    bank.register_tick(&hash(last_id.as_ref()));

    let vote_account = bank.get_account(&vote_keypair.pubkey()).unwrap();
    Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
}

fn redeem_credits(
    bank: &Bank,
    rewards_id: Pubkey,
    vote_keypair: &Keypair,
    to_id: Pubkey,
) -> Result<VoteState> {
    let last_id = bank.last_id();
    let tx = RewardsTransaction::new_redeem_credits(&vote_keypair, rewards_id, to_id, last_id, 0);
    bank.process_transaction(&tx)?;
    let vote_account = bank.get_account(&vote_keypair.pubkey()).unwrap();
    Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
}

#[test]
fn test_redeem_vote_credits_via_bank() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    bank.add_native_program("solana_rewards_program", &rewards_program::id());

    // Create a rewards account to hold all rewards pool tokens.
    let rewards_keypair = Keypair::new();
    let rewards_id = rewards_keypair.pubkey();
    create_rewards_account(&bank, &from_keypair, rewards_id, 100).unwrap();

    // A staker create a vote account account and delegates a validator to vote on its behalf.
    let vote_keypair = Keypair::new();
    let vote_id = vote_keypair.pubkey();
    create_vote_account(&bank, &from_keypair, vote_id, 100).unwrap();

    // The validator submits votes to accumulate credits.
    for _ in 0..vote_program::MAX_VOTE_HISTORY {
        let vote_state = submit_vote(&bank, &vote_keypair, 1).unwrap();
        assert_eq!(vote_state.credits(), 0);
    }
    let vote_state = submit_vote(&bank, &vote_keypair, 1).unwrap();
    assert_eq!(vote_state.credits(), 1);

    // TODO: Add VoteInstruction::RegisterStakerId so that we don't need to point the "to"
    // account to the "from" account.
    let to_id = from_keypair.pubkey();
    let to_tokens = bank.get_balance(&to_id);

    // Periodically, the staker sumbits its vote account to the rewards pool
    // to exchange its credits for lamports.
    let vote_state = redeem_credits(&bank, rewards_id, &vote_keypair, to_id).unwrap();
    assert!(bank.get_balance(&to_id) > to_tokens);
    assert_eq!(vote_state.credits(), 0);
}
