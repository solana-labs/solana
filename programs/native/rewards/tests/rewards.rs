use solana_rewards_api::rewards_program;
use solana_rewards_api::rewards_transaction::RewardsTransaction;
use solana_runtime::{execute_transaction, RuntimeError};
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::native_loader::create_program_account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_sdk::vote_program::{self, VoteState};
use solana_sdk::vote_transaction::VoteTransaction;

fn create_rewards_account(
    from_keypair: &Keypair,
    from_account: &mut Account,
    rewards_id: Pubkey,
    lamports: u64,
) -> Result<Account, RuntimeError> {
    let system_program_account = create_program_account("solana_system_program");
    let loaders = &mut [vec![(system_program::id(), system_program_account)]];
    let last_id = Hash::default();
    let tx = RewardsTransaction::new_account(from_keypair, rewards_id, last_id, lamports, 0);
    let accounts = &mut [from_account.clone(), Account::new(0, 0, Pubkey::default())];
    execute_transaction(&tx, loaders, accounts, 0)?;

    // Simulate committing account after successful transaction
    from_account.tokens = accounts[0].tokens;

    Ok(accounts[1].clone())
}

fn create_vote_account(
    from_keypair: &Keypair,
    from_account: &mut Account,
    vote_id: Pubkey,
    lamports: u64,
) -> Result<Account, RuntimeError> {
    let system_program_account = create_program_account("solana_system_program");
    let vote_program_account = create_program_account("solana_vote_program");
    let loaders = &mut [
        vec![(system_program::id(), system_program_account)],
        vec![(vote_program::id(), vote_program_account)],
    ];
    let last_id = Hash::default();
    let tx = VoteTransaction::new_account(from_keypair, vote_id, last_id, lamports, 0);
    let accounts = &mut [from_account.clone(), Account::new(0, 0, Pubkey::default())];
    execute_transaction(&tx, loaders, accounts, 0)?;

    // Simulate committing account after successful transaction
    from_account.tokens = accounts[1].tokens;

    Ok(accounts[1].clone())
}

fn submit_vote(
    vote_keypair: &Keypair,
    vote_account: &mut Account,
    tick_height: u64,
) -> Result<VoteState, RuntimeError> {
    let vote_program_account = create_program_account("solana_vote_program");
    let loaders = &mut [vec![(vote_program::id(), vote_program_account)]];
    let last_id = Hash::default();
    let tx = VoteTransaction::new_vote(vote_keypair, tick_height, last_id, 0);
    let accounts = &mut [vote_account.clone()];
    execute_transaction(&tx, loaders, accounts, 0)?;

    // Simulate committing account after successful transaction
    vote_account
        .userdata
        .clone_from_slice(&accounts[0].userdata);

    Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
}

fn redeem_credits(
    rewards_id: Pubkey,
    rewards_account: &mut Account,
    vote_keypair: &Keypair,
    vote_account: &mut Account,
    to_id: Pubkey,
    to_account: &mut Account,
) -> Result<VoteState, RuntimeError> {
    let last_id = Hash::default();
    let rewards_program_account = create_program_account("solana_rewards_program");
    let loaders = &mut [vec![(rewards_program::id(), rewards_program_account)]];

    let accounts = &mut [
        vote_account.clone(),
        rewards_account.clone(),
        to_account.clone(),
    ];
    let tx = RewardsTransaction::new_redeem_credits(&vote_keypair, rewards_id, to_id, last_id, 0);
    execute_transaction(&tx, loaders, accounts, 0)?;

    // Simulate committing account after successful transaction
    vote_account
        .userdata
        .clone_from_slice(&accounts[0].userdata);
    rewards_account.tokens = accounts[1].tokens;
    to_account.tokens = accounts[2].tokens;

    Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
}

#[test]
fn test_redeem_vote_credits_via_runtime() {
    // Create a rewards account to hold all rewards pool tokens.
    let from_keypair = Keypair::new();
    let mut from_account = Account::new(10_000, 0, Pubkey::default());
    let rewards_keypair = Keypair::new();
    let rewards_id = rewards_keypair.pubkey();
    let mut rewards_account =
        create_rewards_account(&from_keypair, &mut from_account, rewards_id, 100).unwrap();

    // A staker create a vote account account and delegates a validator to vote on its behalf.
    let vote_keypair = Keypair::new();
    let vote_id = vote_keypair.pubkey();
    let mut vote_account =
        create_vote_account(&from_keypair, &mut from_account, vote_id, 100).unwrap();

    // The validator submits votes to accumulate credits.
    for _ in 0..vote_program::MAX_VOTE_HISTORY {
        let vote_state = submit_vote(&vote_keypair, &mut vote_account, 1).unwrap();
        assert_eq!(vote_state.credits(), 0);
    }
    let vote_state = submit_vote(&vote_keypair, &mut vote_account, 1).unwrap();
    assert_eq!(vote_state.credits(), 1);

    // TODO: Add VoteInstruction::RegisterStakerId so that we don't need to point the "to"
    // account to the "from" account.
    let to_id = from_keypair.pubkey();
    let mut to_account = from_account;
    let to_tokens = to_account.tokens;

    // Periodically, the staker sumbits its vote account to the rewards pool
    // to exchange its credits for lamports.
    let vote_state = redeem_credits(
        rewards_id,
        &mut rewards_account,
        &vote_keypair,
        &mut vote_account,
        to_id,
        &mut to_account,
    )
    .unwrap();
    assert!(to_account.tokens > to_tokens);
    assert_eq!(vote_state.credits(), 0);
}
