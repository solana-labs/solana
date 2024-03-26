#![allow(clippy::arithmetic_side_effects)]

mod setup;

use {
    setup::{setup_stake, setup_vote},
    solana_program_test::ProgramTest,
    solana_sdk::{
        instruction::InstructionError,
        signature::{Keypair, Signer},
        stake::{instruction as stake_instruction, instruction::StakeError},
        transaction::{Transaction, TransactionError},
    },
    test_case::test_case,
};

#[derive(PartialEq)]
enum PendingStakeActivationTestFlag {
    MergeActive,
    MergeInactive,
    NoMerge,
}

#[test_case(PendingStakeActivationTestFlag::NoMerge; "test that redelegate stake then deactivate it then withdraw from it is not permitted")]
#[test_case(PendingStakeActivationTestFlag::MergeActive; "test that redelegate stake then merge it with another active stake then deactivate it then withdraw from it is not permitted")]
#[test_case(PendingStakeActivationTestFlag::MergeInactive; "test that redelegate stake then merge it with another inactive stake then deactivate it then withdraw from it is not permitted")]
#[tokio::test]
async fn test_stake_redelegation_pending_activation(merge_flag: PendingStakeActivationTestFlag) {
    let program_test = ProgramTest::default();
    let mut context = program_test.start_with_context().await;

    // 1. create first vote accounts
    context.warp_to_slot(100).unwrap();
    let vote_address = setup_vote(&mut context).await;

    // 1.1 advance to normal epoch
    let first_normal_slot = context.genesis_config().epoch_schedule.first_normal_slot;
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    let mut current_slot = first_normal_slot + slots_per_epoch;
    context.warp_to_slot(current_slot).unwrap();
    context.warp_forward_force_reward_interval_end().unwrap();

    // 2. create first stake account and delegate to first vote_address
    let stake_lamports = 50_000_000_000;
    let user_keypair = Keypair::new();
    let stake_address =
        setup_stake(&mut context, &user_keypair, &vote_address, stake_lamports).await;

    // 2.1 advance to new epoch so that the stake is activated.
    current_slot += slots_per_epoch;
    context.warp_to_slot(current_slot).unwrap();
    context.warp_forward_force_reward_interval_end().unwrap();

    // 2.2 stake is now activated and can't withdrawal directly
    let transaction = Transaction::new_signed_with_payer(
        &[stake_instruction::withdraw(
            &stake_address,
            &user_keypair.pubkey(),
            &solana_sdk::pubkey::new_rand(),
            1,
            None,
        )],
        Some(&context.payer.pubkey()),
        &vec![&context.payer, &user_keypair],
        context.last_blockhash,
    );
    let r = context.banks_client.process_transaction(transaction).await;
    assert_eq!(
        r.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::InsufficientFunds)
    );

    // 3. create 2nd vote account
    let vote_address2 = setup_vote(&mut context).await;

    // 3.1 relegate stake account to 2nd vote account, which creates 2nd stake account
    let stake_keypair2 = Keypair::new();
    let stake_address2 = stake_keypair2.pubkey();
    let transaction = Transaction::new_signed_with_payer(
        &stake_instruction::redelegate(
            &stake_address,
            &user_keypair.pubkey(),
            &vote_address2,
            &stake_address2,
        ),
        Some(&context.payer.pubkey()),
        &vec![&context.payer, &user_keypair, &stake_keypair2],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    if merge_flag != PendingStakeActivationTestFlag::NoMerge {
        // 3.2 create 3rd to-merge stake account
        let stake_address3 =
            setup_stake(&mut context, &user_keypair, &vote_address2, stake_lamports).await;

        // 3.2.1 deactivate merge stake account
        if merge_flag == PendingStakeActivationTestFlag::MergeInactive {
            let transaction = Transaction::new_signed_with_payer(
                &[stake_instruction::deactivate_stake(
                    &stake_address3,
                    &user_keypair.pubkey(),
                )],
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

        // 3.2.2 merge 3rd stake account to 2nd stake account. However, it should not clear the pending stake activation flags on stake_account2.
        let transaction = Transaction::new_signed_with_payer(
            &stake_instruction::merge(&stake_address2, &stake_address3, &user_keypair.pubkey()),
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

    // 3.3 deactivate 2nd stake account should fail because of pending stake activation.
    let transaction = Transaction::new_signed_with_payer(
        &[stake_instruction::deactivate_stake(
            &stake_address2,
            &user_keypair.pubkey(),
        )],
        Some(&context.payer.pubkey()),
        &vec![&context.payer, &user_keypair],
        context.last_blockhash,
    );
    let r = context.banks_client.process_transaction(transaction).await;
    assert_eq!(
        r.unwrap_err().unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(
                StakeError::RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted as u32
            )
        )
    );

    // 3.4 withdraw from 2nd stake account should also fail because of pending stake activation.
    let transaction = Transaction::new_signed_with_payer(
        &[stake_instruction::withdraw(
            &stake_address2,
            &user_keypair.pubkey(),
            &solana_sdk::pubkey::new_rand(),
            1,
            None,
        )],
        Some(&context.payer.pubkey()),
        &vec![&context.payer, &user_keypair],
        context.last_blockhash,
    );
    let r = context.banks_client.process_transaction(transaction).await;
    assert_eq!(
        r.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::InsufficientFunds)
    );

    // 4. advance to new epoch so that the 2nd stake account is fully activated
    current_slot += slots_per_epoch;
    context.warp_to_slot(current_slot).unwrap();
    context.warp_forward_force_reward_interval_end().unwrap();

    // 4.1 Now deactivate 2nd stake account should succeed because there is no pending stake activation.
    let transaction = Transaction::new_signed_with_payer(
        &[stake_instruction::deactivate_stake(
            &stake_address2,
            &user_keypair.pubkey(),
        )],
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
