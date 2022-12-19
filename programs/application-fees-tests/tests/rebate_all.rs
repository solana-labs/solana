use solana_application_fees_program::instruction::{rebate_all, update_fees};

use {
    assert_matches::assert_matches,
    solana_program_test::tokio,
    solana_sdk::{signature::Signer, transaction::Transaction},
};

mod common;
use {
    crate::common::{
        advance_slot, create_a_dummy_account, create_owner_and_dummy_account, setup_test_context,
    },
    solana_sdk::{native_token::LAMPORTS_PER_SOL, system_instruction},
};

#[tokio::test]
async fn test_application_fees_are_not_applied_on_rebate_all() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;
    let (owner2, writable_account2) = create_owner_and_dummy_account(&mut context).await;
    let writable_account3 = create_a_dummy_account(&mut context, &owner.pubkey()).await;

    {
        // update fees for account 1
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

        // update fees for account 2
        let add_ix = update_fees(LAMPORTS_PER_SOL * 2, writable_account2, owner2.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner2],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        // update fees for account 3
        let add_ix = update_fees(LAMPORTS_PER_SOL * 3, writable_account3, owner.pubkey());

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
        .get_account(writable_account3)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.has_application_fees, true);
    assert_eq!(account.rent_epoch_or_application_fees, 3 * LAMPORTS_PER_SOL);

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
    let balance_writable_account_3_before = context
        .banks_client
        .get_balance(writable_account3)
        .await
        .unwrap();

    // transfer 1 lamport to the writable accounts / rebate_all for owner (rebates 3+1 SOLs) / but payer has to pay 2SOL as application fee
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let balance_before_transaction = client.get_balance(payer.pubkey()).await.unwrap();

        let blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_ix1 = system_instruction::transfer(&payer.pubkey(), &writable_account, 1);
        let transfer_ix2 = system_instruction::transfer(&payer.pubkey(), &writable_account2, 1);
        let transfer_ix3 = system_instruction::transfer(&payer.pubkey(), &writable_account3, 1);
        let rebate_all = rebate_all(owner.pubkey());
        let transaction = Transaction::new_signed_with_payer(
            &[
                transfer_ix1.clone(),
                transfer_ix2.clone(),
                transfer_ix3.clone(),
                rebate_all.clone(),
            ],
            Some(&payer.pubkey()),
            &[payer, &owner],
            blockhash,
        );
        assert_matches!(client.process_transaction(transaction).await, Ok(()));

        let balance_after_transaction = client.get_balance(payer.pubkey()).await.unwrap();
        assert!(balance_before_transaction - balance_after_transaction < 3 * LAMPORTS_PER_SOL);
    }

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
    let balance_writable_account_3_after = context
        .banks_client
        .get_balance(writable_account3)
        .await
        .unwrap();
    assert_eq!(
        balance_writable_account_after - balance_writable_account_before,
        1
    );
    assert_eq!(
        balance_writable_account_2_after - balance_writable_account_2_before,
        2 * LAMPORTS_PER_SOL + 1
    );
    assert_eq!(
        balance_writable_account_3_after - balance_writable_account_3_before,
        1
    );
}
