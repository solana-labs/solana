#![allow(clippy::arithmetic_side_effects)]

mod setup;

use {
    bincode::deserialize,
    log::debug,
    setup::{setup_stake, setup_vote},
    solana_banks_client::BanksClient,
    solana_program_test::{processor, ProgramTest, ProgramTestBanksClientExt, ProgramTestError},
    solana_sdk::{
        account::Account,
        account_info::{next_account_info, AccountInfo},
        clock::Clock,
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction, InstructionError},
        program_error::ProgramError,
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signer},
        stake::{
            instruction as stake_instruction,
            state::{StakeActivationStatus, StakeStateV2},
        },
        sysvar::{
            clock,
            stake_history::{self, StakeHistory},
            Sysvar,
        },
        transaction::{Transaction, TransactionError},
    },
    solana_stake_program::stake_state,
    solana_vote_program::vote_state,
    std::convert::TryInto,
};

// Use a big number to be sure that we get the right error
const WRONG_SLOT_ERROR: u32 = 123456;

fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let clock_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(clock_info)?;
    let expected_slot = u64::from_le_bytes(input.try_into().unwrap());
    if clock.slot == expected_slot {
        Ok(())
    } else {
        Err(ProgramError::Custom(WRONG_SLOT_ERROR))
    }
}

#[tokio::test]
async fn clock_sysvar_updated_from_warp() {
    let program_id = Pubkey::new_unique();
    // Initialize and start the test network
    let program_test = ProgramTest::new(
        "program-test-warp",
        program_id,
        processor!(process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    let mut expected_slot = 100_000;
    let instruction = Instruction::new_with_bincode(
        program_id,
        &expected_slot,
        vec![AccountMeta::new_readonly(clock::id(), false)],
    );

    // Fail transaction
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    assert_eq!(
        context
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(WRONG_SLOT_ERROR))
    );

    // Warp to success!
    context.warp_to_slot(expected_slot).unwrap();
    let instruction = Instruction::new_with_bincode(
        program_id,
        &expected_slot,
        vec![AccountMeta::new_readonly(clock::id(), false)],
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Try warping ahead one slot (corner case in warp logic)
    expected_slot += 1;
    assert!(context.warp_to_slot(expected_slot).is_ok());
    let instruction = Instruction::new_with_bincode(
        program_id,
        &expected_slot,
        vec![AccountMeta::new_readonly(clock::id(), false)],
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Try warping again to the same slot
    assert_eq!(
        context.warp_to_slot(expected_slot).unwrap_err(),
        ProgramTestError::InvalidWarpSlot,
    );
}

#[tokio::test]
async fn stake_rewards_from_warp() {
    // Initialize and start the test network
    let program_test = ProgramTest::default();
    let mut context = program_test.start_with_context().await;

    context.warp_to_slot(100).unwrap();
    let vote_address = setup_vote(&mut context).await;

    let user_keypair = Keypair::new();
    let stake_lamports = 1_000_000_000_000;
    let stake_address =
        setup_stake(&mut context, &user_keypair, &vote_address, stake_lamports).await;

    let account = context
        .banks_client
        .get_account(stake_address)
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    // warp one epoch forward for normal inflation, no rewards collected
    let first_normal_slot = context.genesis_config().epoch_schedule.first_normal_slot;
    context.warp_to_slot(first_normal_slot).unwrap();
    let account = context
        .banks_client
        .get_account(stake_address)
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    context.increment_vote_account_credits(&vote_address, 100);

    // go forward and see that rewards have been distributed
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context
        .warp_to_slot(first_normal_slot + slots_per_epoch + 1) // when partitioned rewards are enabled, the rewards are paid at 1 slot after the first slot of the epoch
        .unwrap();

    let account = context
        .banks_client
        .get_account(stake_address)
        .await
        .expect("account exists")
        .unwrap();
    assert!(account.lamports > stake_lamports);

    // check that stake is fully active
    let stake_history_account = context
        .banks_client
        .get_account(stake_history::id())
        .await
        .expect("account exists")
        .unwrap();

    let clock_account = context
        .banks_client
        .get_account(clock::id())
        .await
        .expect("account exists")
        .unwrap();

    let stake_state: StakeStateV2 = deserialize(&account.data).unwrap();
    let stake_history: StakeHistory = deserialize(&stake_history_account.data).unwrap();
    let clock: Clock = deserialize(&clock_account.data).unwrap();
    let stake = stake_state.stake().unwrap();
    assert_eq!(
        stake
            .delegation
            .stake_activating_and_deactivating(clock.epoch, Some(&stake_history), None),
        StakeActivationStatus::with_effective(stake.delegation.stake),
    );
}

#[tokio::test]
async fn stake_rewards_filter_bench_100() {
    stake_rewards_filter_bench_core(100).await;
}

async fn stake_rewards_filter_bench_core(num_stake_accounts: u64) {
    // Initialize and start the test network
    let mut program_test = ProgramTest::default();

    // create vote account
    let vote_address = Pubkey::new_unique();
    let node_address = Pubkey::new_unique();

    let vote_account = vote_state::create_account(&vote_address, &node_address, 0, 1_000_000_000);
    program_test.add_account(vote_address, vote_account.clone().into());

    // create stake accounts with 0.9 sol to test min-stake filtering
    const TEST_FILTER_STAKE: u64 = 900_000_000; // 0.9 sol
    let mut to_filter = vec![];
    for i in 0..num_stake_accounts {
        let stake_pubkey = Pubkey::new_unique();
        let stake_account = Account::from(stake_state::create_account(
            &stake_pubkey,
            &vote_address,
            &vote_account,
            &Rent::default(),
            TEST_FILTER_STAKE,
        ));
        program_test.add_account(stake_pubkey, stake_account);
        to_filter.push(stake_pubkey);
        if i % 100 == 0 {
            debug!("create stake account {} {}", i, stake_pubkey);
        }
    }

    let mut context = program_test.start_with_context().await;

    let stake_lamports = 2_000_000_000_000;

    let user_keypair = Keypair::new();
    let stake_address =
        setup_stake(&mut context, &user_keypair, &vote_address, stake_lamports).await;

    let account = context
        .banks_client
        .get_account(stake_address)
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    // warp one epoch forward for normal inflation, no rewards collected
    let first_normal_slot = context.genesis_config().epoch_schedule.first_normal_slot;
    context.warp_to_slot(first_normal_slot).unwrap();
    let account = context
        .banks_client
        .get_account(stake_address)
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    context.increment_vote_account_credits(&vote_address, 100);

    // go forward and see that rewards have been distributed
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context
        .warp_to_slot(first_normal_slot + slots_per_epoch + 1) // when partitioned rewards are enabled, the rewards are paid at 1 slot after the first slot of the epoch
        .unwrap();

    let account = context
        .banks_client
        .get_account(stake_address)
        .await
        .expect("account exists")
        .unwrap();
    assert!(account.lamports > stake_lamports);

    // check that filtered stake accounts are excluded from receiving epoch rewards
    for stake_address in to_filter {
        let account = context
            .banks_client
            .get_account(stake_address)
            .await
            .expect("account exists")
            .unwrap();
        assert_eq!(account.lamports, TEST_FILTER_STAKE);
    }

    // check that stake is fully active
    let stake_history_account = context
        .banks_client
        .get_account(stake_history::id())
        .await
        .expect("account exists")
        .unwrap();

    let clock_account = context
        .banks_client
        .get_account(clock::id())
        .await
        .expect("account exists")
        .unwrap();

    let stake_state: StakeStateV2 = deserialize(&account.data).unwrap();
    let stake_history: StakeHistory = deserialize(&stake_history_account.data).unwrap();
    let clock: Clock = deserialize(&clock_account.data).unwrap();
    let stake = stake_state.stake().unwrap();
    assert_eq!(
        stake
            .delegation
            .stake_activating_and_deactivating(clock.epoch, Some(&stake_history), None),
        StakeActivationStatus::with_effective(stake.delegation.stake),
    );
}

async fn check_credits_observed(
    banks_client: &mut BanksClient,
    stake_address: Pubkey,
    expected_credits: u64,
) {
    let stake_account = banks_client
        .get_account(stake_address)
        .await
        .unwrap()
        .unwrap();
    let stake_state: StakeStateV2 = deserialize(&stake_account.data).unwrap();
    assert_eq!(
        stake_state.stake().unwrap().credits_observed,
        expected_credits
    );
}
#[tokio::test]
async fn stake_merge_immediately_after_activation() {
    let program_test = ProgramTest::default();
    let mut context = program_test.start_with_context().await;

    context.warp_to_slot(100).unwrap();
    let vote_address = setup_vote(&mut context).await;
    context.increment_vote_account_credits(&vote_address, 100);

    let first_normal_slot = context.genesis_config().epoch_schedule.first_normal_slot;
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    let mut current_slot = first_normal_slot + slots_per_epoch;
    context.warp_to_slot(current_slot).unwrap();
    context.warp_forward_force_reward_interval_end().unwrap();

    // this is annoying, but if no stake has earned rewards, the bank won't
    // iterate through the stakes at all, which means we can only test the
    // behavior of advancing credits observed if another stake is earning rewards

    // make a base stake which receives rewards
    let user_keypair = Keypair::new();
    let stake_lamports = 1_000_000_000_000;
    let base_stake_address =
        setup_stake(&mut context, &user_keypair, &vote_address, stake_lamports).await;
    check_credits_observed(&mut context.banks_client, base_stake_address, 100).await;
    context.increment_vote_account_credits(&vote_address, 100);

    current_slot += slots_per_epoch;
    context.warp_to_slot(current_slot).unwrap();
    context.warp_forward_force_reward_interval_end().unwrap();

    // make another stake which will just have its credits observed advanced
    let absorbed_stake_address =
        setup_stake(&mut context, &user_keypair, &vote_address, stake_lamports).await;
    // the new stake is at the right value
    check_credits_observed(&mut context.banks_client, absorbed_stake_address, 200).await;
    // the base stake hasn't been moved forward because no rewards were earned
    check_credits_observed(&mut context.banks_client, base_stake_address, 100).await;

    context.increment_vote_account_credits(&vote_address, 100);
    current_slot += slots_per_epoch;
    context.warp_to_slot(current_slot).unwrap();
    context.warp_forward_force_reward_interval_end().unwrap();

    // check that base stake has earned rewards and credits moved forward
    let stake_account = context
        .banks_client
        .get_account(base_stake_address)
        .await
        .unwrap()
        .unwrap();
    let stake_state: StakeStateV2 = deserialize(&stake_account.data).unwrap();
    assert_eq!(stake_state.stake().unwrap().credits_observed, 300);
    assert!(stake_account.lamports > stake_lamports);

    // check that new stake hasn't earned rewards, but that credits_observed have been advanced
    let stake_account = context
        .banks_client
        .get_account(absorbed_stake_address)
        .await
        .unwrap()
        .unwrap();
    let stake_state: StakeStateV2 = deserialize(&stake_account.data).unwrap();
    assert_eq!(stake_state.stake().unwrap().credits_observed, 300);
    assert_eq!(stake_account.lamports, stake_lamports);

    // sanity-check that the activation epoch was actually last epoch
    let clock_account = context
        .banks_client
        .get_account(clock::id())
        .await
        .unwrap()
        .unwrap();
    let clock: Clock = deserialize(&clock_account.data).unwrap();
    assert_eq!(
        clock.epoch,
        stake_state.delegation().unwrap().activation_epoch + 1
    );

    // sanity-check that it's possible to merge the just-activated stake with the older stake!
    let transaction = Transaction::new_signed_with_payer(
        &stake_instruction::merge(
            &base_stake_address,
            &absorbed_stake_address,
            &user_keypair.pubkey(),
        ),
        Some(&context.payer.pubkey()),
        &vec![&context.payer, &user_keypair],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

#[tokio::test]
async fn get_blockhash_post_warp() {
    let program_test = ProgramTest::default();
    let mut context = program_test.start_with_context().await;

    let new_blockhash = context
        .banks_client
        .get_new_latest_blockhash(&context.last_blockhash)
        .await
        .unwrap();
    let mut tx = Transaction::new_with_payer(&[], Some(&context.payer.pubkey()));
    tx.sign(&[&context.payer], new_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();

    context.warp_to_slot(10).unwrap();

    let new_blockhash = context
        .banks_client
        .get_new_latest_blockhash(&context.last_blockhash)
        .await
        .unwrap();

    let mut tx = Transaction::new_with_payer(&[], Some(&context.payer.pubkey()));
    tx.sign(&[&context.payer], new_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();
}
