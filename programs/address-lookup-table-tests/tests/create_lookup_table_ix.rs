use {
    assert_matches::assert_matches,
    common::{assert_ix_error, overwrite_slot_hashes_with_slots, setup_test_context},
    solana_program_test::*,
    solana_sdk::{
        address_lookup_table::{
            instruction::{create_lookup_table, create_lookup_table_signed},
            program::id,
            state::{AddressLookupTable, LOOKUP_TABLE_META_SIZE},
        },
        clock::Slot,
        feature_set,
        instruction::InstructionError,
        pubkey::Pubkey,
        rent::Rent,
        signature::Signer,
        signer::keypair::Keypair,
        transaction::Transaction,
    },
};

mod common;

pub async fn setup_test_context_without_authority_feature() -> ProgramTestContext {
    let mut program_test = ProgramTest::new(
        "",
        id(),
        Some(solana_address_lookup_table_program::processor::Entrypoint::vm),
    );
    program_test.deactivate_feature(
        feature_set::relax_authority_signer_check_for_lookup_table_creation::id(),
    );
    program_test.start_with_context().await
}

#[tokio::test]
async fn test_create_lookup_table_idempotent() {
    let mut context = setup_test_context().await;

    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&context, &[test_recent_slot]);

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority_address = Pubkey::new_unique();
    let (create_lookup_table_ix, lookup_table_address) =
        create_lookup_table(authority_address, payer.pubkey(), test_recent_slot);

    // First create should succeed
    {
        let transaction = Transaction::new_signed_with_payer(
            &[create_lookup_table_ix.clone()],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
        let lookup_table_account = client
            .get_account(lookup_table_address)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lookup_table_account.owner, id());
        assert_eq!(lookup_table_account.data.len(), LOOKUP_TABLE_META_SIZE);
        assert_eq!(
            lookup_table_account.lamports,
            Rent::default().minimum_balance(LOOKUP_TABLE_META_SIZE)
        );
        let lookup_table = AddressLookupTable::deserialize(&lookup_table_account.data).unwrap();
        assert_eq!(lookup_table.meta.deactivation_slot, Slot::MAX);
        assert_eq!(lookup_table.meta.authority, Some(authority_address));
        assert_eq!(lookup_table.meta.last_extended_slot, 0);
        assert_eq!(lookup_table.meta.last_extended_slot_start_index, 0);
        assert_eq!(lookup_table.addresses.len(), 0);
    }

    // Second create should succeed too
    {
        let recent_blockhash = client
            .get_new_latest_blockhash(&recent_blockhash)
            .await
            .unwrap();
        let transaction = Transaction::new_signed_with_payer(
            &[create_lookup_table_ix],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
    }
}

#[tokio::test]
async fn test_create_lookup_table_not_idempotent() {
    let mut context = setup_test_context_without_authority_feature().await;

    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&context, &[test_recent_slot]);

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority_keypair = Keypair::new();
    let authority_address = authority_keypair.pubkey();
    let (create_lookup_table_ix, ..) =
        create_lookup_table_signed(authority_address, payer.pubkey(), test_recent_slot);

    let transaction = Transaction::new_signed_with_payer(
        &[create_lookup_table_ix.clone()],
        Some(&payer.pubkey()),
        &[payer, &authority_keypair],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));

    // Second create should fail
    {
        context.last_blockhash = client
            .get_new_latest_blockhash(&recent_blockhash)
            .await
            .unwrap();
        assert_ix_error(
            &mut context,
            create_lookup_table_ix,
            Some(&authority_keypair),
            InstructionError::AccountAlreadyInitialized,
        )
        .await;
    }
}

#[tokio::test]
async fn test_create_lookup_table_use_payer_as_authority() {
    let mut context = setup_test_context().await;

    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&context, &[test_recent_slot]);

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority_address = payer.pubkey();
    let transaction = Transaction::new_signed_with_payer(
        &[create_lookup_table(authority_address, payer.pubkey(), test_recent_slot).0],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
}

#[tokio::test]
async fn test_create_lookup_table_missing_signer() {
    let mut context = setup_test_context_without_authority_feature().await;
    let unsigned_authority_address = Pubkey::new_unique();

    let mut ix = create_lookup_table_signed(
        unsigned_authority_address,
        context.payer.pubkey(),
        Slot::MAX,
    )
    .0;
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
async fn test_create_lookup_table_not_recent_slot() {
    let mut context = setup_test_context().await;
    let payer = &context.payer;
    let authority_address = Pubkey::new_unique();

    let ix = create_lookup_table(authority_address, payer.pubkey(), Slot::MAX).0;

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::InvalidInstructionData,
    )
    .await;
}

#[tokio::test]
async fn test_create_lookup_table_pda_mismatch() {
    let mut context = setup_test_context().await;
    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&context, &[test_recent_slot]);
    let payer = &context.payer;
    let authority_address = Pubkey::new_unique();

    let mut ix = create_lookup_table(authority_address, payer.pubkey(), test_recent_slot).0;
    ix.accounts[0].pubkey = Pubkey::new_unique();

    assert_ix_error(&mut context, ix, None, InstructionError::InvalidArgument).await;
}
