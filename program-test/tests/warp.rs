use {
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::{Clock, DEFAULT_DEV_SLOTS_PER_EPOCH},
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction, InstructionError},
        program_error::ProgramError,
        pubkey::Pubkey,
        rent::Rent,
        system_instruction,
        sysvar::{clock, Sysvar},
    },
    solana_program_test::{processor, ProgramTest, ProgramTestError},
    solana_sdk::{
        signature::{Keypair, Signer},
        transaction::{Transaction, TransactionError},
    },
    solana_stake_program::{
        stake_instruction,
        stake_state::{Authorized, Lockup},
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
    let expected_slot = 5_000_000;
    let instruction = Instruction::new(
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
    let instruction = Instruction::new(
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

    // this test is a bit flaky because of the bank freezing non-deterministically,
    // so we do a few warps and hope that one of them gets it

    // go forward and see that rent has been collected
    context.warp_to_slot(DEFAULT_DEV_SLOTS_PER_EPOCH).unwrap();

    // go forward and see that rent has been collected
    context
        .warp_to_slot(DEFAULT_DEV_SLOTS_PER_EPOCH * 2)
        .unwrap();

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
    let staker_keypair = Keypair::new();
    let stake_keypair = Keypair::new();
    let stake_lamports = 1_000_000_000_000;
    let instructions = stake_instruction::create_account_and_delegate_stake(
        &context.payer.pubkey(),
        &stake_keypair.pubkey(),
        &context.voter.pubkey(),
        &Authorized::auto(&staker_keypair.pubkey()),
        &Lockup::default(),
        stake_lamports,
    );
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer, &stake_keypair, &staker_keypair],
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

    // warp a few slots, no rewards collected
    context.warp_to_slot(10).unwrap();
    let account = context
        .banks_client
        .get_account(stake_keypair.pubkey())
        .await
        .expect("account exists")
        .unwrap();
    assert_eq!(account.lamports, stake_lamports);

    // go forward and see that rewards have been distributed
    context
        .warp_to_slot(DEFAULT_DEV_SLOTS_PER_EPOCH * 2)
        .unwrap();

    let account = context
        .banks_client
        .get_account(stake_keypair.pubkey())
        .await
        .expect("account exists")
        .unwrap();
    assert!(account.lamports > stake_lamports);
}
