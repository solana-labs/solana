#![allow(clippy::integer_arithmetic)]
use {
    assert_matches::assert_matches,
    bincode::deserialize,
    solana_program_test::{processor, ProgramTest, ProgramTestError},
    solana_sdk::{
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
            state::{Authorized, Lockup, StakeState},
        },
        system_instruction, system_program,
        sysvar::{
            clock,
            stake_history::{self, StakeHistory},
            Sysvar,
        },
        transaction::{Transaction, TransactionError},
    },
    solana_vote_program::{
        vote_instruction,
        vote_state::{VoteInit, VoteState},
    },
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
    let expected_slot = 100_000;
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

    // Try warping again to the same slot
    assert_eq!(
        context.warp_to_slot(expected_slot).unwrap_err(),
        ProgramTestError::InvalidWarpSlot,
    );
}

#[tokio::test]
async fn rent_collected_from_warp() {
    let program_id = Pubkey::new_unique();
    // Initialize and start the test network
    let program_test = ProgramTest::default();

    let mut context = program_test.start_with_context().await;
    let account_size = 100;
    let keypair = Keypair::new();
    let account_lamports = Rent::default().minimum_balance(account_size) - 100; // not rent exempt
    let instruction = system_instruction::create_account(
        &context.payer.pubkey(),
        &keypair.pubkey(),
        account_lamports,
        account_size as u64,
        &program_id,
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &keypair],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
    let account = context
        .banks_client
        .get_account(keypair.pubkey())
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, account_lamports);

    // Warp forward and see that rent has been collected
    // This test was a bit flaky with one warp, but two warps always works
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context.warp_to_slot(slots_per_epoch).unwrap();
    context.warp_to_slot(slots_per_epoch * 2).unwrap();

    let account = context
        .banks_client
        .get_account(keypair.pubkey())
        .await
        .expect("account exists")
        .unwrap();
    assert!(account.lamports < account_lamports);
}

#[tokio::test]
async fn stake_rewards_from_warp() {
    // Initialize and start the test network
    let program_test = ProgramTest::default();

    let mut context = program_test.start_with_context().await;
    // warp once to make sure stake config doesn't get rent-collected
    context.warp_to_slot(100).unwrap();
    let mut instructions = vec![];
    let validator_keypair = Keypair::new();
    instructions.push(system_instruction::create_account(
        &context.payer.pubkey(),
        &validator_keypair.pubkey(),
        42,
        0,
        &system_program::id(),
    ));
    let vote_lamports = Rent::default().minimum_balance(VoteState::size_of());
    let vote_keypair = Keypair::new();
    let user_keypair = Keypair::new();
    instructions.append(&mut vote_instruction::create_account(
        &context.payer.pubkey(),
        &vote_keypair.pubkey(),
        &VoteInit {
            node_pubkey: validator_keypair.pubkey(),
            authorized_voter: user_keypair.pubkey(),
            ..VoteInit::default()
        },
        vote_lamports,
    ));

    let stake_keypair = Keypair::new();
    let stake_lamports = 1_000_000_000_000;
    instructions.append(&mut stake_instruction::create_account_and_delegate_stake(
        &context.payer.pubkey(),
        &stake_keypair.pubkey(),
        &vote_keypair.pubkey(),
        &Authorized::auto(&user_keypair.pubkey()),
        &Lockup::default(),
        stake_lamports,
    ));
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &vec![
            &context.payer,
            &validator_keypair,
            &vote_keypair,
            &stake_keypair,
            &user_keypair,
        ],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
    let account = context
        .banks_client
        .get_account(stake_keypair.pubkey())
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    // warp one epoch forward for normal inflation, no rewards collected
    let first_normal_slot = context.genesis_config().epoch_schedule.first_normal_slot;
    context.warp_to_slot(first_normal_slot).unwrap();
    let account = context
        .banks_client
        .get_account(stake_keypair.pubkey())
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    context.increment_vote_account_credits(&vote_keypair.pubkey(), 100);

    // go forward and see that rewards have been distributed
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context
        .warp_to_slot(first_normal_slot + slots_per_epoch)
        .unwrap();

    let account = context
        .banks_client
        .get_account(stake_keypair.pubkey())
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

    let stake_state: StakeState = deserialize(&account.data).unwrap();
    let stake_history: StakeHistory = deserialize(&stake_history_account.data).unwrap();
    let clock: Clock = deserialize(&clock_account.data).unwrap();
    let stake = stake_state.stake().unwrap();
    assert_matches!(
        stake
            .delegation
            .stake_activating_and_deactivating(clock.epoch, Some(&stake_history)),
        (_, 0, 0)
    );
}
