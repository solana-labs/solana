use {
    assert_matches::assert_matches,
    common::{
        add_lookup_table_account, assert_ix_error, new_address_lookup_table, setup_test_context,
    },
    solana_program_test::*,
    solana_sdk::{
        address_lookup_table::{instruction::freeze_lookup_table, state::AddressLookupTable},
        instruction::InstructionError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
};

mod common;

#[tokio::test]
async fn test_freeze_lookup_table() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let mut initialized_table = new_address_lookup_table(Some(authority.pubkey()), 10);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(
        &mut context,
        lookup_table_address,
        initialized_table.clone(),
    )
    .await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let transaction = Transaction::new_signed_with_payer(
        &[freeze_lookup_table(
            lookup_table_address,
            authority.pubkey(),
        )],
        Some(&payer.pubkey()),
        &[payer, &authority],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
    let table_account = client
        .get_account(lookup_table_address)
        .await
        .unwrap()
        .unwrap();
    let lookup_table = AddressLookupTable::deserialize(&table_account.data).unwrap();
    assert_eq!(lookup_table.meta.authority, None);

    // Check that only the authority changed
    initialized_table.meta.authority = None;
    assert_eq!(initialized_table, lookup_table);
}

#[tokio::test]
async fn test_freeze_immutable_lookup_table() {
    let mut context = setup_test_context().await;

    let initialized_table = new_address_lookup_table(None, 10);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let authority = Keypair::new();
    let ix = freeze_lookup_table(lookup_table_address, authority.pubkey());

    assert_ix_error(
        &mut context,
        ix,
        Some(&authority),
        InstructionError::Immutable,
    )
    .await;
}

#[tokio::test]
async fn test_freeze_deactivated_lookup_table() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = {
        let mut table = new_address_lookup_table(Some(authority.pubkey()), 10);
        table.meta.deactivation_slot = 0;
        table
    };
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let ix = freeze_lookup_table(lookup_table_address, authority.pubkey());

    assert_ix_error(
        &mut context,
        ix,
        Some(&authority),
        InstructionError::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn test_freeze_lookup_table_with_wrong_authority() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let wrong_authority = Keypair::new();
    let initialized_table = new_address_lookup_table(Some(authority.pubkey()), 10);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let ix = freeze_lookup_table(lookup_table_address, wrong_authority.pubkey());

    assert_ix_error(
        &mut context,
        ix,
        Some(&wrong_authority),
        InstructionError::IncorrectAuthority,
    )
    .await;
}

#[tokio::test]
async fn test_freeze_lookup_table_without_signing() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = new_address_lookup_table(Some(authority.pubkey()), 10);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let mut ix = freeze_lookup_table(lookup_table_address, authority.pubkey());
    ix.accounts[1].is_signer = false;

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::MissingRequiredSignature,
    )
    .await;
}

#[tokio::test]
async fn test_freeze_empty_lookup_table() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = new_address_lookup_table(Some(authority.pubkey()), 0);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let ix = freeze_lookup_table(lookup_table_address, authority.pubkey());

    assert_ix_error(
        &mut context,
        ix,
        Some(&authority),
        InstructionError::InvalidInstructionData,
    )
    .await;
}
