mod common;
use {
    crate::common::{
        advance_slot, assert_error, create_owner_and_dummy_account, setup_test_context,
    },
    assert_matches::assert_matches,
    solana_application_fees_program::instruction::{rebate, update_fees},
    solana_program_test::tokio,
    solana_sdk::{
        instruction::InstructionError, native_token::LAMPORTS_PER_SOL, signature::Keypair,
        system_instruction, system_transaction,
    },
    solana_sdk::{signature::Signer, transaction::Transaction},
};

#[tokio::test]
async fn test_application_fees_are_applied_without_rebate() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
        let account = client.get_account(writable_account).await.unwrap().unwrap();
        assert_eq!(account.has_application_fees, false);
        assert_eq!(account.rent_epoch_or_application_fees, 0);
    }
    advance_slot(&mut context).await;
    let account = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, LAMPORTS_PER_SOL);

    // check if the application fees are correcly dispatched to the correct account
    let balance_writable_account_before = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();

    // transfer 1 lamport to the writable account / but payer has to pay 1SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix = system_transaction::transfer(payer, &writable_account, 1, blockhash);
        assert_matches!(client.process_transaction(transfer_ix).await, Ok(()));

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction > LAMPORTS_PER_SOL);
    }

    advance_slot(&mut context).await;
    let balance_writable_account_after = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        LAMPORTS_PER_SOL + 1
    );
}

#[tokio::test]
async fn test_application_fees_are_applied_on_multiple_accounts() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;
    let (owner2, writable_account2) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let add_ix = update_fees(LAMPORTS_PER_SOL * 2, writable_account2, owner2.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner2],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
    }
    advance_slot(&mut context).await;

    let account = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, LAMPORTS_PER_SOL);

    let account = context
        .banks_client
        .get_account(writable_account2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, 2 * LAMPORTS_PER_SOL);

    let account = context
        .banks_client
        .get_account(writable_account2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, 2 * LAMPORTS_PER_SOL);

    let balance_writable_account_before = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    let balance_writable_account_2_before = context
        .banks_client
        .get_balance(writable_account2)
        .await
        .unwrap();

    // transfer 1 lamport to the writable account / but payer has to pay 1SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix1 = system_instruction::transfer(&payer.pubkey(), &writable_account, 1);
        let transfer_ix2 = system_instruction::transfer(&payer.pubkey(), &writable_account2, 1);
        let transaction = Transaction::new_signed_with_payer(
            &[transfer_ix1.clone(), transfer_ix2.clone()],
            Some(&payer.pubkey()),
            &[payer],
            blockhash,
        );
        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction > 3 * LAMPORTS_PER_SOL);
    }

    // check if the application fees are correcly dispatched to the correct account
    advance_slot(&mut context).await;
    let balance_writable_account_after = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    let balance_writable_account_2_after = context
        .banks_client
        .get_balance(writable_account2)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        1 * LAMPORTS_PER_SOL + 1
    );
    assert_eq!(
        balance_writable_account_2_after - balance_writable_account_2_before,
        2 * LAMPORTS_PER_SOL + 1
    );
}

#[tokio::test]
async fn test_application_fees_are_applied_without_rebate_for_failed_transactions() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;

    let payer2 = {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let payer2 = Keypair::new();
        let transfer_ix = system_transaction::transfer(
            payer,
            &payer2.pubkey(),
            10 * LAMPORTS_PER_SOL,
            recent_blockhash,
        );
        assert_matches!(client.process_transaction(transfer_ix).await, Ok(()));
        payer2
    };

    advance_slot(&mut context).await;
    let account = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, LAMPORTS_PER_SOL);

    // transfer 11 SOLs to the writable account which the payer clearly does not have / but payer has to pay 1SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &payer2;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix = system_transaction::transfer(
            payer,
            &writable_account,
            11 * LAMPORTS_PER_SOL,
            blockhash,
        );
        assert_error(
            client.process_transaction(transfer_ix).await,
            InstructionError::Custom(1),
        )
        .await;

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction > LAMPORTS_PER_SOL);
    }

    // check if the application fees are correcly dispatched to the correct account
    let balance_writable_account_before = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();

    advance_slot(&mut context).await;

    let balance_writable_account_after = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        LAMPORTS_PER_SOL
    );
}

#[tokio::test]
async fn test_application_fees_are_not_applied_if_rebated() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
    }
    advance_slot(&mut context).await;

    let account = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, LAMPORTS_PER_SOL);

    // transfer 1 lamport to the writable account / but payer has to pay 1SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix = system_instruction::transfer(&payer.pubkey(), &writable_account, 1);
        let rebate_ix = rebate(writable_account, owner.pubkey(), u64::MAX);
        let transaction = Transaction::new_signed_with_payer(
            &[transfer_ix.clone(), rebate_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            blockhash,
        );
        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction < LAMPORTS_PER_SOL);
    }

    // check if the application fees are correcly dispatched to the correct account
    let balance_writable_account_before = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();

    advance_slot(&mut context).await;

    let balance_writable_account_after = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        0
    );
}

#[tokio::test]
async fn test_application_fees_are_not_applied_on_single_rebated_account() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;
    let (owner2, writable_account2) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let add_ix = update_fees(LAMPORTS_PER_SOL * 2, writable_account2, owner2.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner2],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
    }

    advance_slot(&mut context).await;
    let account = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, LAMPORTS_PER_SOL);

    let account = context
        .banks_client
        .get_account(writable_account2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, 2 * LAMPORTS_PER_SOL);

    // transfer 1 lamport to the writable account / but payer has to pay 1SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix1 = system_instruction::transfer(&payer.pubkey(), &writable_account, 1);
        let transfer_ix2 = system_instruction::transfer(&payer.pubkey(), &writable_account2, 1);
        let rebate_ix = rebate(writable_account, owner.pubkey(), u64::MAX);
        let transaction = Transaction::new_signed_with_payer(
            &[
                transfer_ix1.clone(),
                transfer_ix2.clone(),
                rebate_ix.clone(),
            ],
            Some(&payer.pubkey()),
            &[payer, &owner],
            blockhash,
        );
        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction > 2 * LAMPORTS_PER_SOL);
        assert!(balance_before_transaction - balance_after_transaction < 3 * LAMPORTS_PER_SOL);
    }

    // check if the application fees are correcly dispatched to the correct account
    let balance_writable_account_before = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    let balance_writable_account_2_before = context
        .banks_client
        .get_balance(writable_account2)
        .await
        .unwrap();

    advance_slot(&mut context).await;

    let balance_writable_account_after = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    let balance_writable_account_2_after = context
        .banks_client
        .get_balance(writable_account2)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        0
    );
    assert_eq!(
        balance_writable_account_2_after - balance_writable_account_2_before,
        2 * LAMPORTS_PER_SOL
    );
}

#[tokio::test]
async fn test_application_fees_are_not_applied_if_rebated_owner_same_as_writable_account() {
    let mut context = setup_test_context().await;

    let owner = Keypair::new();
    let writable_account = owner.pubkey();

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let transfer_ix =
            system_instruction::transfer(&payer.pubkey(), &writable_account, LAMPORTS_PER_SOL);
        let add_ix = update_fees(LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[transfer_ix.clone(), add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
    }
    advance_slot(&mut context).await;

    let account = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, LAMPORTS_PER_SOL);

    // transfer 1 lamport to the writable account / but payer has to pay 1SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix = system_instruction::transfer(&payer.pubkey(), &writable_account, 1);
        let rebate_ix = rebate(writable_account, owner.pubkey(), u64::MAX);
        let transaction = Transaction::new_signed_with_payer(
            &[transfer_ix.clone(), rebate_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            blockhash,
        );
        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction < LAMPORTS_PER_SOL);
    }

    // check if the application fees are correcly dispatched to the correct account
    let balance_writable_account_before = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    advance_slot(&mut context).await;

    let balance_writable_account_after = context
        .banks_client
        .get_balance(writable_account)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        0
    );
}
