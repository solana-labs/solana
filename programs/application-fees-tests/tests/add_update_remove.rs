use solana_application_fees_program::instruction::update_fees;
use solana_sdk::native_token::LAMPORTS_PER_SOL;

use {
    assert_matches::assert_matches,
    solana_program_test::tokio,
    solana_sdk::{
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
};

mod common;
use {
    crate::common::{
        advance_slot, assert_error, create_owner_and_dummy_account, setup_test_context,
    },
    solana_sdk::instruction::InstructionError,
};

#[tokio::test]
async fn test_add_update_remove_write_lock_fees() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;

        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(100, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&context.payer.pubkey()),
            &[&context.payer, &owner],
            recent_blockhash,
        );

        let account = client.get_account(writable_account).await.unwrap().unwrap();
        assert_eq!(account.has_application_fees, false);

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
    assert_eq!(account.rent_epoch_or_application_fees, 100);

    {
        let client = &mut context.banks_client;
        let recent_blockhash = context.last_blockhash;
        // test update
        let update_ix = update_fees(10000, writable_account, owner.pubkey());

        let update_transaction = Transaction::new_signed_with_payer(
            &[update_ix.clone()],
            Some(&context.payer.pubkey()),
            &[&context.payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(update_transaction).await, Ok(()));
    }
    advance_slot(&mut context).await;

    let account2 = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account2.rent_epoch_or_application_fees, 10000);
    assert_eq!(account2.has_application_fees, true);

    {
        let client = &mut context.banks_client;
        let recent_blockhash = context.last_blockhash;
        // test remove
        let remove_ix = update_fees(0, writable_account, owner.pubkey());

        let remove_transaction = Transaction::new_signed_with_payer(
            &[remove_ix.clone()],
            Some(&context.payer.pubkey()),
            &[&context.payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(remove_transaction).await, Ok(()));
    }

    advance_slot(&mut context).await;

    let account3 = context
        .banks_client
        .get_account(writable_account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account3.rent_epoch_or_application_fees, 0);
    assert_eq!(account3.has_application_fees, false);
}

#[tokio::test]
async fn test_adding_write_lock_fees_with_wrong_owner() {
    let mut context = setup_test_context().await;

    let (_owner, writable_account) = create_owner_and_dummy_account(&mut context).await;
    let (owner2, _writable_account2) = create_owner_and_dummy_account(&mut context).await;
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(100, writable_account, owner2.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner2],
            recent_blockhash,
        );

        assert_error(
            client.process_transaction(transaction).await,
            InstructionError::IllegalOwner,
        )
        .await;
    }
}

#[tokio::test]
#[should_panic]
async fn test_adding_write_lock_fees_without_signature_owner() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(100, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        assert_error(
            client.process_transaction(transaction).await,
            InstructionError::MissingRequiredSignature,
        )
        .await;
    }
}

#[tokio::test]
#[should_panic]
async fn test_adding_write_lock_fees_without_signature_payer() {
    let mut context = setup_test_context().await;

    let (owner, writable_account) = create_owner_and_dummy_account(&mut context).await;

    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;
        let add_ix = update_fees(100, writable_account, owner.pubkey());

        let transaction = Transaction::new_signed_with_payer(
            &[add_ix.clone()],
            Some(&payer.pubkey()),
            &[&owner],
            recent_blockhash,
        );

        assert_error(
            client.process_transaction(transaction).await,
            InstructionError::MissingRequiredSignature,
        )
        .await;
    }
}

#[tokio::test]
async fn test_add_update_remove_owner_same_as_writable_account() {
    let mut context = setup_test_context().await;

    let owner = Keypair::new();
    let writable_account = owner.pubkey();

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

    // test update
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;

        let update_ix = update_fees(2 * LAMPORTS_PER_SOL, writable_account, owner.pubkey());

        let update_transaction = Transaction::new_signed_with_payer(
            &[update_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(update_transaction).await, Ok(()));
    }
    advance_slot(&mut context).await;

    // test remove
    {
        let client = &mut context.banks_client;
        let payer = &context.payer;
        let recent_blockhash = context.last_blockhash;

        let remove_ix = update_fees(0, writable_account, owner.pubkey());

        let remove_transaction = Transaction::new_signed_with_payer(
            &[remove_ix.clone()],
            Some(&payer.pubkey()),
            &[payer, &owner],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(remove_transaction).await, Ok(()));
    }
}
